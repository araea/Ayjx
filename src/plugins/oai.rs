use crate::adapters::onebot::LockedWriter;
use crate::config::build_config;
use crate::event::Context;
use crate::plugins::{PluginError, get_data_dir};
use crate::{error, info, warn};
use futures_util::future::BoxFuture;

use std::sync::Arc;
use toml::Value;

// 引入子模块
pub mod data;
pub mod logic;
pub mod parser;
pub mod types;
pub mod utils;

use data::MANAGER;

pub fn default_config() -> Value {
    build_config(serde_json::json!({
        "enabled": true
    }))
}

pub fn init(_ctx: Context) -> BoxFuture<'static, Result<(), PluginError>> {
    Box::pin(async move {
        let dir = get_data_dir("oai").await?;
        let mgr = Arc::new(data::Manager::new(dir));

        // 尝试预加载模型列表
        let mgr_clone = mgr.clone();
        tokio::spawn(async move {
            if let Err(e) = mgr_clone.fetch_models().await {
                warn!(target: "OAI", "初始化获取模型列表失败: {}", e);
            } else {
                info!(target: "OAI", "初始化获取模型列表成功");
            }
        });

        if MANAGER.set(mgr).is_err() {
            warn!(target: "OAI", "Manager 已经被初始化");
        }
        Ok(())
    })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        // 确保 Manager 已初始化
        let mgr = match MANAGER.get() {
            Some(m) => m,
            None => {
                error!(target: "OAI", "插件尚未初始化");
                return Ok(Some(ctx));
            }
        };

        // 获取纯文本内容
        let raw_text = match ctx.as_message() {
            Some(msg) => msg.text().to_string(),
            None => return Ok(Some(ctx)),
        };

        // 1. 全局指令解析
        if let Some(cmd) = parser::parse_global(&raw_text) {
            logic::execute(cmd, String::new(), vec![], &ctx, &writer, mgr).await;
            return Ok(None); // 指令被消费，不再传递
        }

        // 2. 创建指令解析
        if let Some((name, desc, model, prompt)) = parser::parse_create(&raw_text) {
            logic::handle_create(&name, &desc, &model, &prompt, &ctx, &writer, mgr).await;
            return Ok(None);
        }

        // 3. 删除指令解析
        let agents = mgr.agent_names().await;
        if let Some(name) = parser::parse_delete_agent(&raw_text, &agents) {
            let cmd = parser::Command::new(&name, parser::Action::Delete);
            logic::execute(cmd, String::new(), vec![], &ctx, &writer, mgr).await;
            return Ok(None);
        }

        // 4. 智能体指令/对话解析
        if let Some(cmd) = parser::parse_agent_cmd(&raw_text, &agents) {
            let (quote, imgs) = utils::get_full_content(&ctx, &writer, Some(&cmd.agent)).await;

            // 拼接提示词：引用 + 用户输入参数
            let prompt = if matches!(
                cmd.action,
                parser::Action::Chat | parser::Action::Regenerate
            ) {
                format!("{}{}", quote, cmd.args).trim().to_string()
            } else {
                cmd.args.clone()
            };

            logic::execute(cmd, prompt, imgs, &ctx, &writer, mgr).await;
            return Ok(None);
        }

        Ok(Some(ctx))
    })
}
