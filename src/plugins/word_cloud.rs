use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::config::build_config;
use crate::db::queries::get_text_corpus;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use araea_wordcloud::{WordCloudBuilder, WordInput};
use base64::{Engine as _, engine::general_purpose};
use chrono::{Datelike, Duration, Local};
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

#[derive(Serialize, Deserialize)]
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

pub fn default_config() -> Value {
    build_config(WordCloudConfig {
        enabled: true,
        limit: 50,
        width: 800,
        height: 600,
        font_path: None,
        max_msg: 50000,
    })
}

static COMMAND_REGEX: OnceLock<Regex> = OnceLock::new();

fn get_regex() -> &'static Regex {
    COMMAND_REGEX.get_or_init(|| {
        Regex::new(r"^(本群|跨群|我的)(今日|昨日|本周|近7天|本月|今年|总)词云$").unwrap()
    })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let config: WordCloudConfig = get_config(&ctx, "word_cloud").unwrap_or(WordCloudConfig {
            enabled: true,
            limit: 50,
            width: 800,
            height: 600,
            font_path: None,
            max_msg: 50000,
        });

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
                "发送者" => (None, Some(msg.user_id())),
                _ => (None, None),
            };

            if scope_str == "本群" && query_guild_id.is_none() && msg.group_id().is_none() {
                let reply =
                    Message::new().text("请在群聊中使用“本群”指令，或使用“发送者”查看个人词云。");
                send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                return Ok(None);
            }

            let db = &ctx.db;
            let corpus_result = get_text_corpus(
                db,
                query_guild_id.as_deref(),
                query_user_id,
                start_time,
                end_time,
            )
            .await;

            let mut corpus = match corpus_result {
                Ok(c) if c.is_empty() => {
                    let reply = Message::new().text(format!(
                        "生成失败：{} 在 {} 范围内没有足够的消息记录。",
                        scope_str, time_str
                    ));
                    send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                    return Ok(None);
                }
                Ok(c) => c,
                Err(e) => {
                    error!(target: "Plugin/WordCloud", "DB Error: {}", e);
                    return Ok(None);
                }
            };

            if config.max_msg > 0 && corpus.len() > config.max_msg {
                let start = corpus.len().saturating_sub(config.max_msg);
                corpus = corpus.split_off(start);
            }

            let _reply_prefix = format!(
                "正在生成 {} 的 {} 词云，样本数: {}...",
                scope_str,
                time_str,
                corpus.len()
            );
            send_msg(
                &ctx,
                writer.clone(),
                msg.group_id(),
                Some(msg.user_id()),
                Message::new().text(_reply_prefix),
            )
            .await?;

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
                    let reply = Message::new().reply(msg.message_id()).image(base64_image);
                    send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                }
                Ok(Err(e)) => {
                    let reply = Message::new().text(format!("生成词云出错: {}", e));
                    send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                }
                Err(e) => {
                    error!(target: "Plugin/WordCloud", "Task Join Error: {}", e);
                }
            }

            return Ok(None);
        }

        Ok(Some(ctx))
    })
}

// === 辅助逻辑 ===

fn get_time_range(time_str: &str) -> (i64, i64) {
    let now = Local::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .unwrap();

    match time_str {
        "今日" => (today_start.timestamp(), now.timestamp()),
        "昨日" => {
            let yest_start = today_start - Duration::days(1);
            (yest_start.timestamp(), today_start.timestamp())
        }
        "本周" => {
            let weekday = now.weekday().num_days_from_monday();
            let week_start = today_start - Duration::days(weekday as i64);
            (week_start.timestamp(), now.timestamp())
        }
        "近7天" => {
            let start = now - Duration::days(7);
            (start.timestamp(), now.timestamp())
        }
        "本月" => {
            let month_start = now
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();
            (month_start.timestamp(), now.timestamp())
        }
        "今年" => {
            let year_start = now
                .date_naive()
                .with_month(1)
                .unwrap()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap();
            (year_start.timestamp(), now.timestamp())
        }
        "总" => (0, now.timestamp()),
        _ => (today_start.timestamp(), now.timestamp()),
    }
}

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
