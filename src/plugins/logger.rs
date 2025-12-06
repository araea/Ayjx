use crate::adapters::onebot::LockedWriter;
use crate::config::build_config;
use crate::event::{Context, EventType};
use crate::plugins::{PluginError, get_config};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use simd_json::base::{ValueAsArray, ValueAsScalar};
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsScalar};
use toml::Value;

#[derive(Serialize, Deserialize)]
struct LoggerConfig {
    enabled: bool,
    #[serde(default)]
    debug: bool,
}

pub fn default_config() -> Value {
    build_config(LoggerConfig {
        enabled: true,
        debug: false,
    })
}

pub fn handle(
    ctx: Context,
    _writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        // 获取配置
        let config: LoggerConfig = get_config(&ctx, "logger").unwrap_or(LoggerConfig {
            enabled: true,
            debug: false,
        });

        match &ctx.event {
            EventType::Onebot(ev) => {
                if config.debug {
                    debug!(target: "Logger", "ev: {:?}", ev);
                }

                if let Some(msg) = ctx.as_message() {
                    let content = format_message(ev.get("message"));
                    let sender = format!("{}({})", msg.sender_name(), msg.user_id());

                    if let Some(gid) = msg.group_id() {
                        // 格式: 接收 <- 群聊 [Group(ID)] [Sender(ID)] Content
                        info!(
                            target: "Chat",
                            "接收 <- 群聊 [Group({})] [{}] {}",
                            gid, sender, content
                        );
                    } else {
                        // 格式: 接收 <- 私聊 [Sender(ID)] Content
                        info!(
                            target: "Chat",
                            "接收 <- 私聊 [{}] {}",
                            sender, content
                        );
                    }
                } else if let Some(post_type) = ctx.post_type() {
                    // 过滤心跳日志，减少干扰
                    if post_type != "meta_event" {
                        debug!(target: "Event", "Type: {}", post_type);
                    }
                }
            }
            EventType::BeforeSend(packet) => {
                if config.debug {
                    debug!(target: "Logger", "packet: {:?}", packet);
                }
                if packet.action == "send_msg" {
                    let params = &packet.params;
                    let msg_type = params.get_str("message_type").unwrap_or("unknown");
                    let content = format_message(params.get("message"));

                    if msg_type == "group" {
                        let gid = params
                            .get_i64("group_id")
                            .or_else(|| params.get_u64("group_id").map(|v| v as i64))
                            .unwrap_or(0);
                        info!(
                            target: "Chat",
                            "发送 -> 群聊 [Group({})] {}",
                            gid, content
                        );
                    } else if msg_type == "private" {
                        let uid = params
                            .get_i64("user_id")
                            .or_else(|| params.get_u64("user_id").map(|v| v as i64))
                            .unwrap_or(0);
                        info!(
                            target: "Chat",
                            "发送 -> 私聊 [User({})] {}",
                            uid, content
                        );
                    } else {
                        info!(
                            target: "Chat",
                            "发送 -> 未知 [{}] {}",
                            msg_type, content
                        );
                    }
                } else {
                    debug!(target: "Bot", "Action: {}", packet.action);
                }
            }
            EventType::Init => {
                // Init 阶段由 plugins.rs 统一输出日志，这里不再重复
            }
        }

        Ok(Some(ctx))
    })
}

/// 将 OneBot 消息链转换为人类可读的字符串
fn format_message(msg_val: Option<&OwnedValue>) -> String {
    let val = match msg_val {
        Some(v) => v,
        None => return String::new(),
    };

    // 1. 纯字符串情况
    if let Some(s) = val.as_str() {
        return s.to_string();
    }

    // 2. 消息段数组情况
    if let Some(arr) = val.as_array() {
        let mut result = String::new();
        for seg in arr {
            let type_ = seg.get_str("type").unwrap_or("unknown");
            let data = seg.get("data");

            match type_ {
                "text" => {
                    if let Some(t) = data.and_then(|d| d.get_str("text")) {
                        result.push_str(t);
                    }
                }
                "at" => {
                    let qq = data
                        .and_then(|d| {
                            d.get_str("qq")
                                .map(|s| s.to_string())
                                .or_else(|| d.get_i64("qq").map(|i| i.to_string()))
                                .or_else(|| d.get_u64("qq").map(|i| i.to_string()))
                        })
                        .unwrap_or_else(|| "Unknown".to_string());
                    result.push_str(&format!(" [@{}] ", qq));
                }
                "face" => result.push_str(" [表情] "),
                "image" => result.push_str(" [图片] "),
                "record" => result.push_str(" [语音] "),
                "video" => result.push_str(" [视频] "),
                "reply" => result.push_str(" [回复] "),
                "json" => result.push_str(" [卡片消息] "),
                "poke" => result.push_str(" [戳一戳] "),
                other => result.push_str(&format!(" [{}] ", other)),
            }
        }
        return result;
    }

    "[复杂消息]".to_string()
}
