use crate::api;
use crate::bot::LockedWriter;
use crate::command::match_command;
use crate::config::build_config;
use crate::event::Context;
use crate::plugins::PluginError;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use toml::Value;

#[derive(Serialize, Deserialize)]
struct Config {
    enabled: bool,
}

pub fn default_config() -> Value {
    build_config(Config { enabled: true })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        // 尝试匹配指令 "撤回"
        if let Some(cmd) = match_command(&ctx, "撤回") {
            // 检查是否存在引用回复 (Reply)
            if let Some(reply_id_str) = cmd.reply_id {
                let msg = match ctx.as_message() {
                    Some(m) => m,
                    None => return Ok(Some(ctx)),
                };

                // 获取当前指令消息的 ID (api 定义为 i32)
                let command_msg_id = msg.message_id() as i32;

                // 解析目标消息 ID
                if let Ok(target_id) = reply_id_str.parse::<i32>() {
                    // 1. 撤回被引用的那条消息
                    // 忽略错误（如权限不足或消息太旧）
                    let _ = api::delete_msg(&ctx, writer.clone(), target_id).await;

                    // 2. 撤回用户发送的这条指令消息
                    let _ = api::delete_msg(&ctx, writer, command_msg_id).await;

                    // 既然已执行撤回操作，拦截后续处理
                    return Ok(None);
                }
            }
        }

        Ok(Some(ctx))
    })
}
