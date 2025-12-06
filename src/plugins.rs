#![allow(dead_code)]

// 引用重构后的 OneBot 适配器逻辑
use crate::adapters::onebot::{LockedWriter, send_frame_raw};
use crate::event::{Context, Event, EventType};
use crate::matcher::Matcher;
use futures_util::future::BoxFuture;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::fs;
use toml::Value;

pub mod echo;
pub mod filter_meta_event;
pub mod group_self_title;
pub mod logger;
pub mod ping_pong;
pub mod recall;
pub mod repeater;

pub type PluginError = Box<dyn std::error::Error + Send + Sync>;

pub type PluginHandler =
    fn(Context, LockedWriter) -> BoxFuture<'static, Result<Option<Context>, PluginError>>;

pub type PluginInitHandler = fn(Context) -> BoxFuture<'static, Result<(), PluginError>>;

pub struct Plugin {
    pub name: &'static str,
    pub handler: PluginHandler,
    pub on_init: Option<PluginInitHandler>,
    pub default_config: fn() -> Value,
}

static PLUGINS: OnceLock<Vec<Plugin>> = OnceLock::new();

/// 获取全局插件列表
pub fn get_plugins() -> &'static [Plugin] {
    PLUGINS.get_or_init(|| {
        vec![
            Plugin {
                name: "filter_meta_event",
                handler: filter_meta_event::handle,
                on_init: None,
                default_config: filter_meta_event::default_config,
            },
            Plugin {
                name: "logger",
                handler: logger::handle,
                on_init: None,
                default_config: logger::default_config,
            },
            Plugin {
                name: "group_self_title",
                handler: group_self_title::handle,
                on_init: None,
                default_config: group_self_title::default_config,
            },
            Plugin {
                name: "ping_pong",
                handler: ping_pong::handle,
                on_init: Some(ping_pong::init),
                default_config: ping_pong::default_config,
            },
            Plugin {
                name: "recall",
                handler: recall::handle,
                on_init: None,
                default_config: recall::default_config,
            },
            Plugin {
                name: "echo",
                handler: echo::handle,
                on_init: None,
                default_config: echo::default_config,
            },
            Plugin {
                name: "repeater",
                handler: repeater::handle,
                on_init: None,
                default_config: repeater::default_config,
            },
        ]
    })
}

pub fn register_plugins() -> &'static [Plugin] {
    get_plugins()
}

/// 执行所有插件的初始化逻辑
pub async fn do_init(ctx: Context) -> Result<(), PluginError> {
    let plugins = get_plugins();

    // 先计算启用的插件，以便输出统计信息
    let enabled_plugins: HashSet<String> = {
        let guard = ctx.config.read().unwrap();
        guard
            .plugins
            .iter()
            .filter(|(_, v)| v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect()
    };

    info!(
        target: "System",
        "正在加载插件系统 (已启用 {}/{})",
        enabled_plugins.len(),
        plugins.len()
    );

    for plugin in plugins {
        if !enabled_plugins.contains(plugin.name) {
            continue;
        }

        if let Some(init_fn) = plugin.on_init {
            let init_ctx = Context {
                event: EventType::Init,
                config: ctx.config.clone(),
                config_save_lock: ctx.config_save_lock.clone(),
                db: ctx.db.clone(),
                scheduler: ctx.scheduler.clone(),
                matcher: Arc::new(Matcher::new()),
                config_path: ctx.config_path.clone(),
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

/// 运行插件流水线
pub async fn run(mut ctx: Context, writer: LockedWriter) -> Result<(), PluginError> {
    let plugins = get_plugins();

    let enabled_plugins: HashSet<String> = {
        let config_guard = ctx.config.read().unwrap();
        config_guard
            .plugins
            .iter()
            .filter(|(_, v)| v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect()
    };

    for plugin in plugins {
        if !enabled_plugins.contains(plugin.name) {
            continue;
        }

        match (plugin.handler)(ctx, writer.clone()).await? {
            Some(next_ctx) => {
                ctx = next_ctx;
            }
            None => return Ok(()),
        }
    }

    match ctx.event {
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
