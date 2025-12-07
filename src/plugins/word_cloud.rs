use crate::adapters::onebot::api;
use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::config::build_config;
use crate::db::queries::get_text_corpus;
use crate::db::utils::get_time_range;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use araea_wordcloud::{WordCloudBuilder, WordInput};
use base64::{Engine as _, engine::general_purpose};
use futures_util::future::BoxFuture;
use image::{GenericImageView, ImageFormat};
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Instant;
use toml::Value;

mod stopwords;
use stopwords::get_stop_words;

#[derive(Serialize, Deserialize, Clone)]
struct WordCloudConfig {
    enabled: bool,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default)]
    font_path: Option<String>,
    #[serde(default = "default_max_msg")]
    max_msg: usize,

    // === 每日推送 ===
    #[serde(default)]
    daily_push_enabled: bool,
    #[serde(default = "default_daily_push_time")]
    daily_push_time: String, // 格式 "HH:MM:SS"
    #[serde(default)]
    debug_push_interval: u64, // 调试用：如果不为0，则按此秒数间隔推送
}

fn default_limit() -> usize {
    50
}

fn default_width() -> u32 {
    800
}

fn default_height() -> u32 {
    600
}

fn default_max_msg() -> usize {
    50000
}

fn default_daily_push_time() -> String {
    "23:00:00".to_string()
}

pub fn default_config() -> Value {
    build_config(WordCloudConfig {
        enabled: true,
        limit: 50,
        width: 800,
        height: 600,
        font_path: None,
        max_msg: 50000,
        daily_push_enabled: false,
        daily_push_time: "23:00:00".to_string(),
        debug_push_interval: 0,
    })
}

static COMMAND_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_regex() -> &'static Regex {
    COMMAND_REGEX.get_or_init(|| {
        Regex::new(
            r"^(本群|跨群|我的)(今日|昨日|本周|上周|近7天|近30天|本月|上月|今年|去年|总)词云$",
        )
        .unwrap()
    })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let msg = match ctx.as_message() {
            Some(m) => m,
            None => return Ok(Some(ctx)),
        };
        let text = msg.text();
        let trimmed_text = text.trim();

        let prefixes = get_prefixes(&ctx);
        let mut matched_content = None;

        if prefixes.is_empty() {
            matched_content = Some(trimmed_text);
        } else {
            for prefix in &prefixes {
                if trimmed_text.starts_with(prefix) {
                    matched_content = Some(trimmed_text[prefix.len()..].trim_start());
                    break;
                }
            }
        }

        let content_to_match = match matched_content {
            Some(c) => c,
            None => return Ok(Some(ctx)),
        };

        let regex = get_regex();
        if let Some(caps) = regex.captures(content_to_match) {
            let scope_str = caps.get(1).map_or("", |m| m.as_str());
            let time_str = caps.get(2).map_or("", |m| m.as_str());

            info!(target: "Plugin/WordCloud", "收到词云请求: Scope={}, Time={}", scope_str, time_str);

            let (start_time, end_time) = get_time_range(time_str);

            let (query_guild_id, query_user_id) = match scope_str {
                "本群" => {
                    if let Some(gid) = msg.group_id() {
                        (Some(gid.to_string()), None)
                    } else {
                        (None, Some(msg.user_id()))
                    }
                }
                "跨群" => (None, None),
                "我的" => (None, Some(msg.user_id())),
                _ => (None, None),
            };

            if scope_str == "本群" && query_guild_id.is_none() && msg.group_id().is_none() {
                let reply =
                    Message::new().text("请在群聊中使用“本群”指令，或使用“我的”查看个人词云。");
                send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                return Ok(None);
            }

            match generate_and_send(
                &ctx,
                writer,
                query_guild_id.as_deref(),
                query_user_id,
                start_time,
                end_time,
                msg.group_id(),
                Some(msg.user_id()),
                Some(msg.message_id()),
                format!("{} 的 {} 词云", scope_str, time_str),
            )
            .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!(target: "Plugin/WordCloud", "Handler logic error: {}", e);
                }
            }

            return Ok(None);
        }

        Ok(Some(ctx))
    })
}

/// Bot 连接成功后的钩子
pub fn on_connected(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let config: WordCloudConfig = get_config(&ctx, "word_cloud")
            .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

        if !config.daily_push_enabled {
            return Ok(Some(ctx));
        }

        let scheduler = ctx.scheduler.clone();

        // 调试模式单独处理 (Interval)
        if config.debug_push_interval > 0 {
            info!(target: "Plugin/WordCloud", "已开启词云调试推送，间隔: {}秒", config.debug_push_interval);
            let ctx_debug = ctx.clone();
            let writer_debug = writer.clone();
            scheduler.add_interval(
                std::time::Duration::from_secs(config.debug_push_interval),
                move || {
                    // 复用每日推送的逻辑函数
                    let c = ctx_debug.clone();
                    let w = writer_debug.clone();
                    async move {
                        // 临时构造配置用于调试
                        let cfg = WordCloudConfig {
                            enabled: true,
                            limit: 50,
                            width: 800,
                            height: 600,
                            font_path: None,
                            max_msg: 50000,
                            daily_push_enabled: true,
                            daily_push_time: "".to_string(),
                            debug_push_interval: 0,
                        };
                        do_daily_push_logic(c, w, cfg).await;
                    }
                },
            );
        } else {
            // 正常每日推送使用 Scheduler 通用逻辑
            scheduler.schedule_daily_push(
                ctx.clone(),
                writer.clone(),
                "WordCloud",
                config.daily_push_time.clone(),
                move |c, w, gid| async move {
                    let (start_time, end_time) = get_time_range("今日");
                    let result = generate_and_send(
                        &c,
                        w,
                        Some(&gid.to_string()),
                        None,
                        start_time,
                        end_time,
                        Some(gid),
                        None,
                        None,
                        "本群今日词云 (每日推送)".to_string(),
                    )
                    .await;

                    if let Err(e) = result {
                        warn!(target: "Plugin/WordCloud", "群 {} 推送失败: {}", gid, e);
                    }
                },
            );
        }

        Ok(Some(ctx))
    })
}

// 保留原有的逻辑函数供 debug 调用
async fn do_daily_push_logic(ctx: Context, writer: LockedWriter, _config: WordCloudConfig) {
    let group_list = match api::get_group_list(&ctx, writer.clone(), false).await {
        Ok(list) => list,
        Err(e) => {
            warn!(target: "Plugin/WordCloud", "获取群列表失败: {}", e);
            return;
        }
    };

    let (start_time, end_time) = get_time_range("今日");

    for g in group_list {
        let gid = g.group_id;
        // 简单过滤逻辑 (正式逻辑已移至 Scheduler)
        let should_skip = {
            let guard = ctx.config.read().unwrap();
            if guard.global_filter.enable_whitelist {
                !guard.global_filter.whitelist.contains(&gid)
            } else {
                guard.global_filter.blacklist.contains(&gid)
            }
        };
        if should_skip {
            continue;
        }

        let _ = generate_and_send(
            &ctx,
            writer.clone(),
            Some(&gid.to_string()),
            None,
            start_time,
            end_time,
            Some(gid),
            None,
            None,
            "本群今日词云 (调试推送)".to_string(),
        )
        .await;
    }
}

// === 核心生成逻辑 ===

#[allow(clippy::too_many_arguments)]
async fn generate_and_send(
    ctx: &Context,
    writer: LockedWriter,
    query_guild_id: Option<&str>,
    query_user_id: Option<i64>,
    start_time: i64,
    end_time: i64,
    target_group_id: Option<i64>,
    target_user_id: Option<i64>,
    reply_msg_id: Option<i64>,
    title: String,
) -> Result<(), String> {
    let config: WordCloudConfig = get_config(ctx, "word_cloud")
        .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

    let db = &ctx.db;
    let corpus_result =
        get_text_corpus(db, query_guild_id, query_user_id, start_time, end_time).await;

    let mut corpus = match corpus_result {
        Ok(c) if c.is_empty() => {
            // 如果是主动推送，没有数据则静默跳过
            if reply_msg_id.is_none() {
                return Ok(());
            }
            let reply =
                Message::new().text(format!("生成失败：{} 范围内没有足够的消息记录。", title));
            let _ = send_msg(ctx, writer, target_group_id, target_user_id, reply).await;
            return Ok(());
        }
        Ok(c) => c,
        Err(e) => return Err(format!("DB Error: {}", e)),
    };

    if config.max_msg > 0 && corpus.len() > config.max_msg {
        let start = corpus.len().saturating_sub(config.max_msg);
        corpus = corpus.split_off(start);
    }

    // 如果是指令触发，发送提示；主动推送则略过
    if let Some(msg_id) = reply_msg_id {
        let _reply_prefix = format!("正在生成 {}，样本数: {}...", title, corpus.len());
        let _ = send_msg(
            ctx,
            writer.clone(),
            target_group_id,
            target_user_id,
            Message::new().reply(msg_id).text(_reply_prefix),
        )
        .await;
    }

    let font_path = config.font_path.clone();
    let limit = config.limit;
    let width = config.width;
    let height = config.height;

    let final_msg = tokio::task::spawn_blocking(move || {
        generate_word_cloud(corpus, font_path, limit, width, height)
    })
    .await;

    match final_msg {
        Ok(Ok(base64_image)) => {
            // 1. 发送标题文本
            let info_text = format!("📊 {}", title);
            let mut text_msg = Message::new().text(&info_text);
            if let Some(mid) = reply_msg_id {
                text_msg = text_msg.reply(mid);
            }
            let _ = send_msg(
                ctx,
                writer.clone(),
                target_group_id,
                target_user_id,
                text_msg,
            )
            .await;

            // 2. 发送纯图片消息
            let img_msg = Message::new().image(base64_image);
            let _ = send_msg(ctx, writer, target_group_id, target_user_id, img_msg).await;

            Ok(())
        }
        Ok(Err(e)) => {
            if reply_msg_id.is_some() {
                let reply = Message::new().text(format!("生成词云出错: {}", e));
                let _ = send_msg(ctx, writer, target_group_id, target_user_id, reply).await;
            }
            Err(e)
        }
        Err(e) => Err(format!("Task Join Error: {}", e)),
    }
}

// === 辅助逻辑 ===

fn generate_word_cloud(
    corpus: Vec<String>,
    font_path: Option<String>,
    limit: usize,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let start = Instant::now();

    let stop_words = get_stop_words();
    let mut freq_map: HashMap<String, f64> = HashMap::new();

    for line in corpus {
        let words = line.split_whitespace();
        for w in words {
            let w_trim = w.trim();
            // 过滤规则：长度>1，不在停用词表，非纯数字
            if w_trim.chars().count() > 1
                && !stop_words.contains(w_trim)
                && !w_trim
                    .chars()
                    .all(|c| c.is_numeric() || c.is_ascii_punctuation())
            {
                *freq_map.entry(w_trim.to_string()).or_insert(0.0) += 1.0;
            }
        }
    }

    if freq_map.is_empty() {
        return Err("有效词汇为空（可能被过滤）".to_string());
    }

    let mut word_vec: Vec<(String, f64)> = freq_map.into_iter().collect();
    word_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let top_words: Vec<WordInput> = word_vec
        .into_iter()
        .take(limit)
        .map(|(text, size)| WordInput::new(text, size as f32))
        .collect();

    let mut rng = rand::rng();
    let mut builder = WordCloudBuilder::new()
        .size(width, height)
        .seed(rng.random());

    if let Some(path) = font_path {
        match std::fs::read(&path) {
            Ok(font_data) => {
                builder = builder.font(font_data);
            }
            Err(e) => {
                return Err(format!("加载字体文件失败: {} - {}", path, e));
            }
        }
    }

    let wordcloud = builder
        .build(&top_words)
        .map_err(|e| format!("Build Error: {}", e))?;

    let png_data = wordcloud
        .to_png(2.0)
        .map_err(|e| format!("PNG Encode Error: {}", e))?;

    // 自动裁剪
    let img = image::load_from_memory(&png_data).map_err(|e| format!("Image Load Error: {}", e))?;

    let (img_w, img_h) = img.dimensions();
    let mut min_x = img_w;
    let mut min_y = img_h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found_content = false;

    // 扫描非白像素
    for y in 0..img_h {
        for x in 0..img_w {
            let pixel = img.get_pixel(x, y);
            // 假设背景纯白 (255, 255, 255)，允许少量误差
            if pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250 {
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
                found_content = true;
            }
        }
    }

    let final_data = if found_content {
        let padding = 20;
        let crop_min_x = min_x.saturating_sub(padding);
        let crop_min_y = min_y.saturating_sub(padding);
        let crop_max_x = (max_x + padding).min(img_w - 1);
        let crop_max_y = (max_y + padding).min(img_h - 1);

        let crop_width = crop_max_x - crop_min_x + 1;
        let crop_height = crop_max_y - crop_min_y + 1;

        let cropped_img = img.crop_imm(crop_min_x, crop_min_y, crop_width, crop_height);

        let mut buffer = Cursor::new(Vec::new());
        cropped_img
            .write_to(&mut buffer, ImageFormat::Png)
            .map_err(|e| format!("Image Write Error: {}", e))?;
        buffer.into_inner()
    } else {
        png_data
    };

    let b64_str = general_purpose::STANDARD.encode(&final_data);

    info!(target: "Plugin/WordCloud", "Generated & Cropped in {:?}", start.elapsed());

    Ok(format!("base64://{}", b64_str))
}
