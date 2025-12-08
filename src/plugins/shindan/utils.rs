use crate::adapters::onebot::{LockedWriter, api, send_msg};
use crate::event::Context;
use crate::message::Message;
use anyhow::Result;
use simd_json::OwnedValue;
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsScalar};

/// 将 match_command 解析出的参数（消息段列表）提取为纯文本参数列表
/// 这里主要提取文本段内容，并按空格分割，模拟原本的 split_whitespace 行为
pub fn extract_args(args: &[OwnedValue]) -> Vec<String> {
    let mut full_text = String::new();
    for arg in args {
        if arg.get_str("type") == Some("text")
            && let Some(data) = arg.get("data")
                && let Some(text) = data.get_str("text") {
                    full_text.push_str(text);
                }
    }
    if full_text.trim().is_empty() {
        return vec![];
    }
    full_text
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// 快捷回复文本消息
pub async fn reply_text(ctx: &Context, writer: LockedWriter, text: &str) -> Result<()> {
    let msg = Message::new().text(text);
    if let Some(m) = ctx.as_message() {
        send_msg(ctx, writer, m.group_id(), Some(m.user_id()), msg)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    Ok(())
}

/// 获取消息中的 AT 目标 ID
/// 使用 match_command 返回的 args (剩余消息段) 进行查找
pub fn get_at_target(args: &[OwnedValue]) -> Option<i64> {
    for seg in args {
        if seg.get_str("type") == Some("at") {
            return seg
                .get("data")
                .and_then(|d| d.get_str("qq"))
                .and_then(|s| s.parse().ok())
                .or_else(|| seg.get("data").and_then(|d| d.get_i64("qq")));
        }
    }
    None
}

/// 获取测定对象的名字和ID
pub async fn get_target_name_and_id(
    ctx: &Context,
    writer: LockedWriter,
    params: &[&str],
    args: &[OwnedValue],
) -> (String, String) {
    let msg = ctx.as_message().unwrap();

    // 1. Check AT (using args)
    if let Some(at_id) = get_at_target(args) {
        // 尝试获取群昵称
        if let Some(gid) = msg.group_id()
            && let Ok(info) = api::get_group_member_info(ctx, writer, gid, at_id, false).await {
                let name = if !info.card.is_empty() {
                    info.card
                } else {
                    info.nickname
                };
                return (name, at_id.to_string());
            }
        return (at_id.to_string(), at_id.to_string());
    }

    // 2. Check Params (exclude flags)
    let clean_params: Vec<&str> = params
        .iter()
        .filter(|&&p| !p.starts_with('-'))
        .copied()
        .collect();
    if !clean_params.is_empty() {
        return (clean_params.join(" "), "".to_string());
    }

    // 3. Self
    (msg.sender_name().to_string(), msg.user_id().to_string())
}
