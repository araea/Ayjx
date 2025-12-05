#![allow(dead_code)]

use crate::event::Context;
use simd_json::OwnedValue;
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsArray, ValueObjectAccessAsScalar};

pub struct CommandMatch {
    /// 匹配后的参数列表（剩余的消息段）
    pub args: Vec<OwnedValue>,
    /// 被过滤掉的引用回复 ID
    pub reply_id: Option<String>,
    /// 被过滤掉的 AT 用户 ID 列表
    pub at_ids: Vec<String>,
}

pub fn get_prefixes(ctx: &Context) -> Vec<String> {
    ctx.config.read().unwrap().command_prefix.clone()
}

/// 解析指令：自动过滤头部的 Reply/At/空白，匹配 [Prefix][Command]，返回参数及引用信息
pub fn match_command(ctx: &Context, command_name: &str) -> Option<CommandMatch> {
    let prefixes = get_prefixes(ctx);
    // 仅处理 MessageEvent
    let msg_arr = ctx.as_message()?.0.get_array("message")?;

    let mut reply_id = None;
    let mut at_ids = Vec::new();

    for (i, segment) in msg_arr.iter().enumerate() {
        let type_ = segment.get_str("type")?;
        let data = segment.get("data")?;

        match type_ {
            "reply" => {
                if reply_id.is_none() {
                    // 尝试获取 id (可能是字符串或数字)
                    let id_str = data
                        .get_str("id")
                        .map(String::from)
                        .or_else(|| data.get_i64("id").map(|v| v.to_string()))
                        .or_else(|| data.get_u64("id").map(|v| v.to_string()));
                    reply_id = id_str;
                }
            }
            "at" => {
                let qq_str = data
                    .get_str("qq")
                    .map(String::from)
                    .or_else(|| data.get_i64("qq").map(|v| v.to_string()))
                    .or_else(|| data.get_u64("qq").map(|v| v.to_string()));
                if let Some(qq) = qq_str {
                    at_ids.push(qq);
                }
            }
            "text" => {
                let raw_text = data.get_str("text").unwrap_or("");
                // 跳过首部纯空白文本
                let trimmed_start = raw_text.trim_start();
                if trimmed_start.is_empty() {
                    continue;
                }

                // 找到第一个有效文本节点，尝试匹配
                for prefix in &prefixes {
                    let target = format!("{}{}", prefix, command_name);
                    if trimmed_start.starts_with(&target) {
                        // 匹配成功
                        let mut args = Vec::new();

                        // 处理当前文本节点剩余部分
                        let rest_of_text = &trimmed_start[target.len()..];
                        // 指令后通常有空格，作为参数时去除左侧空格
                        let args_text = rest_of_text.trim_start();

                        if !args_text.is_empty() {
                            let mut new_seg = segment.clone();
                            new_seg["data"]["text"] = OwnedValue::from(args_text);
                            args.push(new_seg);
                        }

                        // 将后续所有节点加入 args
                        for seg in msg_arr.iter().skip(i + 1) {
                            args.push(seg.clone());
                        }

                        return Some(CommandMatch {
                            reply_id,
                            at_ids,
                            args,
                        });
                    }
                }
                // 如果遇到第一个有效文本但未匹配成功，则视为匹配失败
                return None;
            }
            // 遇到其他类型（如图片）且未匹配到指令，停止
            _ => return None,
        }
    }

    None
}
