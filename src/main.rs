mod bot;
mod config;
mod db;
mod event;
mod message;
mod plugins;
mod scheduler;

use crate::config::AppConfig;
use crate::event::{Context, EventType, Matcher};
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
            println!("检测到新插件 [{}]，写入默认配置...", plugin.name);
            app_config
                .plugins
                .insert(plugin.name.to_string(), (plugin.default_config)());
            config_dirty = true;
        }
    }

    if config_dirty || !Path::new(config_path).exists() {
        app_config.save(config_path).await?;
        if config_dirty {
            println!("配置文件已更新。");
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
        if bot_conf.access_token.trim().is_empty() {
            println!("Bot [{}] Token 为空，跳过。", bot_conf.url);
            continue;
        }

        active_bots += 1;
        let bot_shared_cfg = shared_config.clone();
        let bot_scheduler = scheduler.clone();
        let bot_save_lock = save_lock.clone();
        let bot_config_path = config_path.to_string();
        let bot_db = db.clone();

        tokio::spawn(async move {
            bot::run_bot_loop(
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

    println!("激活 Bot 数量: {}。按 Ctrl+C 退出。", active_bots);

    // 等待退出信号 (优雅关闭)
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("\n收到退出信号，正在清理资源...");
        }
        Err(err) => {
            eprintln!("监听信号失败: {}", err);
        }
    }

    // 执行清理工作
    scheduler.shutdown();
    let _ = db.close().await;

    // 退出前强制再保存一次配置，确保万无一失
    let config_snapshot = if let Ok(guard) = shared_config.read() {
        Some(guard.clone())
    } else {
        eprintln!("无法获取配置锁，跳过保存。");
        None
    };

    if let Some(cfg) = config_snapshot {
        if let Err(e) = cfg.save(config_path).await {
            eprintln!("退出前保存配置失败: {}", e);
        } else {
            println!("配置已保存。");
        }
    }

    println!("再见！");
    Ok(())
}
