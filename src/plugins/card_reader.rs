use crate::adapters::onebot::{LockedWriter, api, send_msg};
use crate::command::match_command;
use crate::config::build_config;
use crate::event::Context;
use crate::plugins::{PluginError, get_config, get_data_dir};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use simd_json::derived::ValueObjectAccess;
use simd_json::prelude::ValueAsScalar;
use simd_json::prelude::ValueObjectAccessAsScalar;
use std::fs::File;
use std::io::Write;
use toml::Value;

pub mod parser;

// =============================
//          Config
// =============================

#[derive(Serialize, Deserialize)]
struct Config {
    enabled: bool,
    commands: Vec<String>,
}

pub fn default_config() -> Value {
    build_config(Config {
        enabled: true,
        commands: vec!["读卡".into(), "解析卡".into(), "看卡".into(), "card".into()],
    })
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// =============================
//          Helpers
// =============================

/// 从参数列表或引用回复中获取图片 URL
async fn get_image_url(
    ctx: &Context,
    writer: LockedWriter,
    args: &[OwnedValue],
    reply_id: Option<&String>,
) -> Option<String> {
    // 1. 检查指令参数中是否包含图片
    for seg in args {
        if seg.get_str("type") == Some("image")
            && let Some(data) = seg.get("data")
            && let Some(url) = data.get_str("url")
        {
            return Some(url.to_string());
        }
    }

    // 2. 检查引用回复
    if let Some(rid_str) = reply_id {
        let rid = rid_str
            .parse::<i32>()
            .ok()
            .or_else(|| rid_str.parse::<i64>().map(|v| v as i32).ok())?;

        if let Ok(res) = api::get_msg(ctx, writer, rid).await {
            for seg in res.message.0 {
                if seg.type_ == "image"
                    && let Some(url) = seg.data.get("url").and_then(|v| v.as_str())
                {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

// =============================
//          Main Logic
// =============================

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, std::result::Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let msg = match ctx.as_message() {
            Some(m) => m,
            None => return Ok(Some(ctx)),
        };

        let config: Config = get_config(&ctx, "card_reader")
            .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

        for cmd in &config.commands {
            if let Some(matched) = match_command(&ctx, cmd) {
                let img_url = match get_image_url(
                    &ctx,
                    writer.clone(),
                    &matched.args,
                    matched.reply_id.as_ref(),
                )
                .await
                {
                    Some(u) => u,
                    None => {
                        let _ = send_msg(
                            &ctx,
                            writer,
                            msg.group_id(),
                            Some(msg.user_id()),
                            "⚠️ 请附带角色卡图片或引用图片消息",
                        )
                        .await;
                        return Ok(None);
                    }
                };

                let _ = send_msg(
                    &ctx,
                    writer.clone(),
                    msg.group_id(),
                    Some(msg.user_id()),
                    "🔍 正在读取角色卡...",
                )
                .await;

                // 下载图片
                let img_bytes = match reqwest::get(&img_url).await {
                    Ok(resp) => match resp.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = send_msg(
                                &ctx,
                                writer,
                                msg.group_id(),
                                Some(msg.user_id()),
                                format!("❌ 图片下载失败: {}", e),
                            )
                            .await;
                            return Ok(None);
                        }
                    },
                    Err(e) => {
                        let _ = send_msg(
                            &ctx,
                            writer,
                            msg.group_id(),
                            Some(msg.user_id()),
                            format!("❌ 网络请求失败: {}", e),
                        )
                        .await;
                        return Ok(None);
                    }
                };

                // 解析与导出
                match parser::parse_png(&img_bytes) {
                    Ok((name, json_str)) => {
                        let safe_name =
                            name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
                        let safe_name = if safe_name.trim().is_empty() {
                            "character".to_string()
                        } else {
                            safe_name
                        };

                        let timestamp = chrono::Local::now().format("%H%M%S").to_string();

                        let data_dir = get_data_dir("card_reader").await?;
                        // 生成 JSON 文件，内容为完整的 JSON
                        let json_file = format!("{}_{}.json", safe_name, timestamp);
                        let json_path = data_dir.join(&json_file);

                        // 写入文件
                        if let Ok(mut f) = File::create(&json_path) {
                            let _ = f.write_all(json_str.as_bytes());
                        }

                        // 上传文件
                        let gid = msg.group_id();
                        let uid = msg.user_id();

                        // 上传 JSON
                        if let Err(e) = api::upload_file(
                            &ctx,
                            writer.clone(),
                            gid,
                            Some(uid),
                            &json_path.to_string_lossy(),
                            &json_file,
                        )
                        .await
                        {
                            error!(target: "Plugin/CardReader", "Failed to upload JSON: {}", e);
                            let _ = send_msg(
                                &ctx,
                                writer.clone(),
                                gid,
                                Some(uid),
                                format!("❌ 文件上传失败: {}", e),
                            )
                            .await;
                        }

                        // 清理临时文件
                        let _ = std::fs::remove_file(json_path);
                    }
                    Err(e) => {
                        let _ = send_msg(
                            &ctx,
                            writer,
                            msg.group_id(),
                            Some(msg.user_id()),
                            format!("❌ 解析失败: {}", e),
                        )
                        .await;
                    }
                }

                return Ok(None);
            }
        }

        Ok(Some(ctx))
    })
}
