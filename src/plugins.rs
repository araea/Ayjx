#![allow(dead_code)]

use crate::adapters::onebot::{LockedWriter, send_frame_raw};
use crate::event::{BotStatus, Context, Event, EventType};
use crate::matcher::Matcher;
use futures_util::future::BoxFuture;
use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::fs;
use toml::Value;

pub type PluginError = Box<dyn std::error::Error + Send + Sync>;

pub type PluginHandler =
    fn(Context, LockedWriter) -> BoxFuture<'static, Result<Option<Context>, PluginError>>;

pub type PluginInitHandler = fn(Context) -> BoxFuture<'static, Result<(), PluginError>>;

pub struct Plugin {
    pub name: &'static str,
    pub handler: PluginHandler,
    pub on_init: Option<PluginInitHandler>,
    /// 当 Bot 连接成功且获取到自身信息后触发 (用于注册主动推送任务等)
    pub on_connected: Option<PluginHandler>,
    pub default_config: fn() -> Value,
}

static PLUGINS: OnceLock<Vec<Plugin>> = OnceLock::new();

/// 插件注册宏
macro_rules! register_plugins {
    (
        $(
            $module:ident $( { $($key:ident : $val:expr),* } )?
        ),* $(,)?
    ) => {
        // 1. 自动生成模块声明 (无需手动 pub mod)
        $( pub mod $module; )*

        // 2. 生成获取插件列表的函数
        pub fn get_plugins() -> &'static [Plugin] {
            PLUGINS.get_or_init(|| {
                vec![
                    $(
                        {
                            // 默认构造
                            #[allow(unused)]
                            let mut p = Plugin {
                                name: stringify!($module),
                                handler: $module::handle,
                                on_init: None,
                                on_connected: None,
                                default_config: $module::default_config,
                            };
                            // 应用自定义覆盖 (如果有)
                            $(
                                $( p.$key = $val; )*
                            )?
                            p
                        }
                    ),*
                ]
            })
        }
    };
}

//在此处注册所有插件
register_plugins!(
    filter_meta_event,
    logger,
    recorder {
        on_init: Some(recorder::init)
    },
    media_transfer,
    sticker_saver,
    group_self_title,
    ping_pong {
        on_init: Some(ping_pong::init)
    },
    recall,
    echo,
    repeater,
    word_cloud,
    stats_visualizer {
        on_connected: Some(stats_visualizer::on_connected)
    },
    card_reader,
    gif_lab,
    image_splitter,
    ciyi {
        on_init: Some(ciyi::init)
    },
    web_shot,
    shindan {
        on_init: Some(shindan::init)
    },
    oai {
        on_init: Some(oai::init)
    },
);

pub fn register_plugins() -> &'static [Plugin] {
    get_plugins()
}

/// 执行所有插件的初始化逻辑
pub async fn do_init(ctx: Context) -> Result<(), PluginError> {
    let plugins = get_plugins();

    let enabled_count = {
        let guard = ctx.config.read().unwrap();
        plugins
            .iter()
            .filter(|p| {
                guard
                    .plugins
                    .get(p.name)
                    .and_then(|v| v.get("enabled"))
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
            })
            .count()
    };

    info!(
        target: "System",
        "正在加载插件系统 (已启用 {}/{})",
        enabled_count,
        plugins.len()
    );

    for plugin in plugins {
        let is_enabled = {
            let guard = ctx.config.read().unwrap();
            guard
                .plugins
                .get(plugin.name)
                .and_then(|v| v.get("enabled"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        };

        if !is_enabled {
            continue;
        }

        if let Some(init_fn) = plugin.on_init {
            let init_ctx = Context {
                event: EventType::Init, // 移除 Arc
                config: ctx.config.clone(),
                config_save_lock: ctx.config_save_lock.clone(),
                db: ctx.db.clone(),
                scheduler: ctx.scheduler.clone(),
                matcher: Arc::new(Matcher::new()),
                config_path: ctx.config_path.clone(),
                bot: BotStatus {
                    adapter: "system".to_string(),
                    platform: "internal".to_string(),
                    login_user: Default::default(),
                },
            };

            // 执行初始化
            match init_fn(init_ctx).await {
                Ok(_) => {
                    info!(target: "Plugin", "✅ [{}] 就绪 (Init Success)", plugin.name);
                }
                Err(e) => {
                    error!(target: "Plugin", "❌ [{}] 初始化失败: {}", plugin.name, e);
                }
            }
        } else {
            info!(target: "Plugin", "✅ [{}] 就绪", plugin.name);
        }
    }
    Ok(())
}

/// 当 Bot 连接建立后触发（用于注册定时任务或主动操作）
pub async fn do_connected(ctx: Context, writer: LockedWriter) -> Result<(), PluginError> {
    let plugins = get_plugins();

    for plugin in plugins {
        let is_enabled = {
            let guard = ctx.config.read().unwrap();
            guard
                .plugins
                .get(plugin.name)
                .and_then(|v| v.get("enabled"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        };

        if !is_enabled {
            continue;
        }

        if let Some(conn_fn) = plugin.on_connected {
            if let Err(e) = conn_fn(ctx.clone(), writer.clone()).await {
                error!(target: "Plugin", "❌ [{}] 连接钩子执行失败: {}", plugin.name, e);
            } else {
                info!(target: "Plugin", "🔗 [{}] 连接钩子已触发", plugin.name);
            }
        }
    }
    Ok(())
}

/// 运行插件流水线
pub async fn run(mut ctx: Context, writer: LockedWriter) -> Result<(), PluginError> {
    let plugins = get_plugins();

    for plugin in plugins {
        // 直接在循环内获取读锁进行轻量检查，避免为每个事件创建 HashSet
        let is_enabled = {
            let config_guard = ctx.config.read().unwrap();
            config_guard
                .plugins
                .get(plugin.name)
                .and_then(|v| v.get("enabled"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        };

        if !is_enabled {
            continue;
        }

        // ctx 在这里 Move 进 handler，若插件返回 Some(ctx) 则接力给下一个插件
        // 这样插件拥有 Context 的所有权，可以修改 Context.event 中的内容
        match (plugin.handler)(ctx, writer.clone()).await? {
            Some(next_ctx) => {
                ctx = next_ctx;
            }
            None => return Ok(()),
        }
    }

    // 注意：ctx.event 现在是 EventType，可以直接 match 引用
    match &ctx.event {
        EventType::Onebot(_) => {}
        EventType::BeforeSend(packet) => {
            let json_str = simd_json::to_string(&packet)?;
            send_frame_raw(writer, json_str).await?;
        }
        EventType::Init => {}
    }

    Ok(())
}

// ================= 工具函数 =================

/// 将伪造/修改过的事件推送回流水线
pub async fn send_fake_event(
    ctx: &Context,
    writer: LockedWriter,
    event: Event,
) -> Result<(), PluginError> {
    let new_ctx = Context {
        event: EventType::Onebot(event),
        config: ctx.config.clone(),
        config_save_lock: ctx.config_save_lock.clone(),
        db: ctx.db.clone(),
        scheduler: ctx.scheduler.clone(),
        matcher: ctx.matcher.clone(),
        config_path: ctx.config_path.clone(),
        bot: ctx.bot.clone(),
    };
    run(new_ctx, writer).await
}

pub async fn get_data_dir(plugin_name: &str) -> Result<PathBuf, PluginError> {
    let mut path = std::env::current_exe()?
        .parent()
        .ok_or("Cannot get parent dir")?
        .to_path_buf();
    path.push("data");
    path.push(plugin_name);
    if !path.exists() {
        fs::create_dir_all(&path).await?;
    }
    Ok(path)
}

pub fn get_config<T>(ctx: &Context, plugin_name: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    let guard = ctx.config.read().unwrap();
    guard
        .plugins
        .get(plugin_name)
        .and_then(|v| T::deserialize(v.clone()).ok())
}

/// 修改配置 (异步 & 自动持久化 & 线程安全)
pub async fn update_config<T, F>(ctx: &Context, plugin_name: &str, f: F) -> Result<(), PluginError>
where
    T: Serialize + DeserializeOwned + Clone,
    F: FnOnce(T) -> T,
{
    {
        let mut guard = ctx.config.write().unwrap();
        if let Some(v) = guard.plugins.get_mut(plugin_name)
            && let Ok(current_cfg) = T::deserialize(v.clone())
        {
            let new_cfg = f(current_cfg);
            if let Ok(new_val) = Value::try_from(new_cfg) {
                *v = new_val;
            }
        }
    }

    let _fs_guard = ctx.config_save_lock.lock().await;

    let latest_config_snapshot = {
        let guard = ctx.config.read().unwrap();
        guard.clone()
    };

    latest_config_snapshot.save(&ctx.config_path).await?;

    Ok(())
}
