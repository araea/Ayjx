mod adapters;
mod command;
mod config;
mod db;
mod event;
#[macro_use]
mod log;
mod matcher;
mod message;
mod plugins;
mod scheduler;

use crate::config::AppConfig;
use crate::event::{Context, EventType};
use crate::matcher::Matcher;
use crate::scheduler::Scheduler;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::fs;
use tokio::signal;
use tokio::sync::Mutex as AsyncMutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config_path = "config.toml";

    let db = db::init().await.expect("数据库初始化失败");

    // 加载或创建基础配置
    let mut config_content = String::new();
    if Path::new(config_path).exists() {
        config_content = fs::read_to_string(config_path).await?;
    }

    let mut app_config: AppConfig = toml::from_str(&config_content).unwrap_or_default();

    // 动态合并插件默认配置
    let registered_plugins = plugins::get_plugins();
    let mut config_dirty = false;

    for plugin in registered_plugins {
        if !app_config.plugins.contains_key(plugin.name) {
            info!("检测到新插件 [{}]，写入默认配置...", plugin.name);
            app_config
                .plugins
                .insert(plugin.name.to_string(), (plugin.default_config)());
            config_dirty = true;
        }
    }

    if config_dirty || !Path::new(config_path).exists() {
        app_config.save(config_path).await?;
        if config_dirty {
            info!("配置文件已更新。");
        }
    }

    // 构建运行时组件
    let shared_config = Arc::new(RwLock::new(app_config.clone()));
    // 初始化调度器
    let scheduler = Arc::new(Scheduler::new());
    // 初始化文件写入锁
    let save_lock = Arc::new(AsyncMutex::new(()));

    // === 触发插件初始化钩子 (生命周期: init) ===
    let init_ctx = Context {
        event: EventType::Init,
        config: shared_config.clone(),
        config_save_lock: save_lock.clone(),
        db: db.clone(),
        scheduler: scheduler.clone(),
        matcher: Arc::new(Matcher::new()),
        config_path: config_path.to_string(),
    };
    plugins::do_init(init_ctx).await?;
    // ==========================================

    // 启动 Bots
    let mut active_bots = 0;
    for bot_conf in app_config.bots {
        // 1. 检查是否启用
        if !bot_conf.enabled {
            if bot_conf.protocol == "onebot" {
                info!(
                    "Bot (OneBot) 已禁用。若需启用请在配置文件中设置 enabled = true 并填写 access_token。"
                );
            }
            continue;
        }

        // 针对 OneBot 的特殊检查
        if bot_conf.protocol == "onebot" {
            let url = match &bot_conf.url {
                Some(u) if !u.is_empty() => u,
                _ => {
                    error!("Bot 配置错误: OneBot 协议必须指定 url");
                    continue;
                }
            };

            let token = bot_conf.access_token.as_deref().unwrap_or("");
            // 检查 Token 是否为空或占位符
            if token.trim().is_empty() || token == "YOUR_TOKEN_HERE" {
                warn!("Bot [{}] 未配置有效的 access_token，跳过连接。", url);
                continue;
            }
        }

        let adapter = if let Some(a) = adapters::find_adapter(&bot_conf.protocol) {
            a
        } else {
            error!("Bot 配置了未知的协议 '{}'，跳过。", bot_conf.protocol);
            continue;
        };

        active_bots += 1;
        let bot_shared_cfg = shared_config.clone();
        let bot_scheduler = scheduler.clone();
        let bot_save_lock = save_lock.clone();
        let bot_config_path = config_path.to_string();
        let bot_db = db.clone();
        let handler = adapter.handler;
        let protocol_name = bot_conf.protocol.clone();

        let bot_url = bot_conf
            .url
            .clone()
            .unwrap_or_else(|| "Internal".to_string());

        tokio::spawn(async move {
            info!("启动适配器 [{}] -> {}", protocol_name, bot_url);
            handler(
                bot_conf,
                bot_shared_cfg,
                bot_db,
                bot_scheduler,
                bot_save_lock,
                bot_config_path,
            )
            .await;
        });
    }

    info!("激活 Bot 数量: {}。按 Ctrl+C 退出。", active_bots);

    // 等待退出信号 (优雅关闭)
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("收到退出信号，正在清理资源...");
        }
        Err(err) => {
            error!("监听信号失败: {}", err);
        }
    }

    // 执行清理工作
    scheduler.shutdown();
    let _ = db.close().await;

    // 退出前强制再保存一次配置，确保万无一失
    let config_snapshot = if let Ok(guard) = shared_config.read() {
        Some(guard.clone())
    } else {
        error!("无法获取配置锁，跳过保存。");
        None
    };

    if let Some(cfg) = config_snapshot {
        if let Err(e) = cfg.save(config_path).await {
            error!("退出前保存配置失败: {}", e);
        } else {
            info!("配置已保存。");
        }
    }

    info!("Bye!");
    Ok(())
}
