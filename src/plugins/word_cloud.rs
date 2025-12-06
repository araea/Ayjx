use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::config::build_config;
use crate::db::queries::get_text_corpus;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use araea_wordcloud::{ColorScheme, WordCloudBuilder, WordInput};
use base64::{Engine as _, engine::general_purpose};
use chrono::{Datelike, Duration, Local};
use futures_util::future::BoxFuture;
use image::{GenericImageView, ImageFormat};
use jieba_rs::Jieba;
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Instant;
use toml::Value;

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
}

fn default_limit() -> usize {
    200
}

fn default_width() -> u32 {
    1200
}

fn default_height() -> u32 {
    800
}

pub fn default_config() -> Value {
    build_config(WordCloudConfig {
        enabled: true,
        limit: 200,
        width: 1200,
        height: 800,
        font_path: None,
    })
}

static JIEBA: OnceLock<Jieba> = OnceLock::new();
static STOP_WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
static COMMAND_REGEX: OnceLock<Regex> = OnceLock::new();

fn init_jieba() -> Jieba {
    Jieba::new()
}

fn get_stop_words() -> &'static HashSet<&'static str> {
    STOP_WORDS.get_or_init(|| {
        let list = vec![
            // --- 代词 & 称谓 ---
            "我",
            "你",
            "他",
            "她",
            "它",
            "我们",
            "你们",
            "他们",
            "咱们",
            "大家",
            "自己",
            "别人",
            "人家",
            "这里",
            "那里",
            "哪里",
            // --- 常见语气词/填充词 ---
            "这个",
            "那个",
            "这种",
            "那种",
            "这样",
            "那样",
            "有点",
            "有简",
            "有些",
            "有的",
            "其实",
            "确实",
            "就是",
            "算是",
            "感觉",
            "觉得",
            "认为",
            "以为",
            "可能",
            "大概",
            "应该",
            "好像",
            "似乎",
            "也许",
            "然后",
            "接着",
            "结果",
            "后来",
            "之前",
            "之后",
            "反正",
            "总之",
            "毕竟",
            "原来",
            "根本",
            "简直",
            "真是",
            "真的",
            "非常",
            "特别",
            "相当",
            "比较",
            "一般",
            "一直",
            "一定",
            "已经",
            "依然",
            "仍然",
            "只是",
            "不过",
            "但是",
            "可是",
            "而且",
            "虽然",
            "因为",
            "所以",
            "如果",
            "假如",
            "比如",
            "例如",
            "顺便",
            "马上",
            "现在",
            "刚才",
            "最近",
            "平时",
            "意思",
            "样子",
            "东西",
            "事情",
            "情况",
            "问题",
            "一下",
            "一点",
            "一些",
            "一次",
            "一会儿",
            "哈哈",
            "哈哈哈",
            "呵呵",
            "嘿嘿",
            "呜呜",
            "啧啧",
            // --- 单字虚词/介词/助词 ---
            "的",
            "了",
            "在",
            "是",
            "有",
            "和",
            "与",
            "或",
            "及",
            "去",
            "来",
            "做",
            "干",
            "弄",
            "搞",
            "说",
            "看",
            "想",
            "这",
            "那",
            "就",
            "也",
            "都",
            "而",
            "着",
            "吧",
            "呢",
            "啊",
            "嘛",
            "呀",
            "哦",
            "噢",
            "嗯",
            "呗",
            "啦",
            "咯",
            "被",
            "给",
            "把",
            "让",
            "对",
            "向",
            "往",
            "自",
            "从",
            "不",
            "没",
            "别",
            "非",
            "无",
            // --- 否定与判断组合 ---
            "没有",
            "不是",
            "不行",
            "不能",
            "不会",
            "不要",
            "不用",
            "可以",
            "能够",
            "需要",
            "愿意",
            "喜欢",
            "知道",
            "明白",
            "出来",
            "进去",
            "起来",
            "下去",
            "回来",
            "回去",
            // --- 特殊标记 ---
            "[图片]",
            "[表情]",
            "[语音]",
            "[视频]",
            "[引用]",
            "truncated",
            // --- 保留原列表中的其他词 ---
            "人",
            "一",
            "一个",
            "上",
            "很",
            "到",
            "要",
            "会",
            "好",
            "吗",
            "哈",
            "什么",
            "怎么",
            "还是",
            "或者",
            "http",
            "https",
            "com",
            "cn",
            "www",
            "img",
            "image",
            "CQ",
            "cq",
            "face",
            "url",
            "video",
            "record",
            "reply",
            "at",
            "file",
            "json",
            "xml",
            "今天",
            "昨天",
            "明天",
            "时候",
        ];
        list.into_iter().collect()
    })
}

fn get_regex() -> &'static Regex {
    COMMAND_REGEX.get_or_init(|| {
        Regex::new(r"^(本群|跨群|发送者)(今日|昨日|本周|近7天|本月|今年|总)词云$").unwrap()
    })
}

pub fn handle(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let config: WordCloudConfig = get_config(&ctx, "word_cloud").unwrap_or(WordCloudConfig {
            enabled: true,
            limit: 200,
            width: 1200,
            height: 800,
            font_path: None,
        });

        let msg = match ctx.as_message() {
            Some(m) => m,
            None => return Ok(Some(ctx)),
        };
        let text = msg.text();
        let trimmed_text = text.trim();

        // 1. 处理指令前缀
        let prefixes = get_prefixes(&ctx);
        let mut content_to_match = trimmed_text;

        // 尝试去除配置中的指令前缀
        for prefix in prefixes {
            if trimmed_text.starts_with(&prefix) {
                content_to_match = trimmed_text[prefix.len()..].trim_start();
                break;
            }
        }

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

            let corpus = match corpus_result {
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

/// 计算时间戳范围 (start, end)
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

/// 生成词云图片，自动裁剪并返回 Base64 URI
fn generate_word_cloud(
    corpus: Vec<String>,
    _font_path: Option<String>,
    limit: usize,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let start = Instant::now();

    // 1. 分词与统计
    let jieba = JIEBA.get_or_init(init_jieba);
    let stop_words = get_stop_words();
    let mut freq_map: HashMap<String, f64> = HashMap::new();

    for line in corpus {
        let words = jieba.cut(&line, false);
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

    // 2. 排序并截取 Top N
    let mut word_vec: Vec<(String, f64)> = freq_map.into_iter().collect();
    word_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let top_words: Vec<WordInput> = word_vec
        .into_iter()
        .take(limit)
        .map(|(text, size)| WordInput::new(text, size as f32))
        .collect();

    // 3. 构建词云
    let mut rng = rand::rng();
    let builder = WordCloudBuilder::new()
        .size(width, height)
        .color_scheme(ColorScheme::Ocean)
        .background("#FFFFFF") // 使用白色背景以便于裁剪
        .padding(2)
        .word_spacing(3.0)
        .angles(vec![0.0])
        .font_size_range(20.0, 150.0)
        .seed(rng.random());

    // if let Some(path) = font_path {
    //     builder = builder.font(&path);
    // }

    let wordcloud = builder
        .build(&top_words)
        .map_err(|e| format!("Build Error: {}", e))?;

    // 4. 获取原始 PNG 数据
    let png_data = wordcloud
        .to_png(1.0)
        .map_err(|e| format!("PNG Encode Error: {}", e))?;

    // 5. 自动裁剪 (Auto Crop)
    // 加载图片
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
            // pixel[0]=R, pixel[1]=G, pixel[2]=B
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
        // 添加适量 padding
        let padding = 20;
        let crop_min_x = min_x.saturating_sub(padding);
        let crop_min_y = min_y.saturating_sub(padding);
        let crop_max_x = (max_x + padding).min(img_w - 1);
        let crop_max_y = (max_y + padding).min(img_h - 1);

        let crop_width = crop_max_x - crop_min_x + 1;
        let crop_height = crop_max_y - crop_min_y + 1;

        // 执行裁剪
        let cropped_img = img.crop_imm(crop_min_x, crop_min_y, crop_width, crop_height);

        // 重新编码为 PNG
        let mut buffer = Cursor::new(Vec::new());
        cropped_img
            .write_to(&mut buffer, ImageFormat::Png)
            .map_err(|e| format!("Image Write Error: {}", e))?;
        buffer.into_inner()
    } else {
        // 未发现内容（全白），返回原图
        png_data
    };

    // 6. Base64 编码
    let b64_str = general_purpose::STANDARD.encode(&final_data);

    info!(target: "Plugin/WordCloud", "Generated & Cropped in {:?}", start.elapsed());

    // 7. 返回 Base64 URI
    Ok(format!("base64://{}", b64_str))
}
