use crate::bot::WsWriter;
use crate::event::{Context, EventType};
use crate::plugins::{PluginError, build_config};
use futures_util::future::BoxFuture;
use serde::Serialize;
use toml::Value;

#[derive(Serialize)]
struct LoggerConfig {
    enabled: bool,
}

pub fn default_config() -> Value {
    build_config(LoggerConfig { enabled: true })
}

pub fn handle<'a>(
    ctx: Context,
    _writer: &'a mut WsWriter,
) -> BoxFuture<'a, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        match &ctx.event {
            EventType::Onebot(_) => {
                if let Some(msg) = ctx.as_message() {
                    let source = if let Some(gid) = msg.group_id() {
                        format!("Group({})", gid)
                    } else {
                        format!("Private({})", msg.user_id())
                    };

                    println!(
                        "-> [Log] [Message] [{}] {}: {}",
                        source,
                        msg.sender_name(),
                        msg.text()
                    );
                } else if let Some(post_type) = ctx.post_type() {
                    println!("-> [Log] [Event] Type: {}", post_type);
                }
            }
            EventType::BeforeSend(packet) => {
                let target = if let Some(gid) = packet.group_id() {
                    format!("Group({})", gid)
                } else {
                    "Private/Unknown".to_string()
                };

                let content = packet
                    .message()
                    .and_then(|v| simd_json::to_string(v).ok())
                    .unwrap_or_else(|| "[Complex Message]".to_string());

                println!(
                    "<- [Log] [Send] To [{}]: {} -> {}",
                    target, packet.action, content
                );
            }
            EventType::Init => {
                println!("-> [Log] [System] Plugin Init Phase");
            }
        }

        Ok(Some(ctx))
    })
}
