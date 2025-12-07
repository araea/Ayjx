use std::sync::OnceLock;
use std::{env, fs};

use super::{StatsConfig, default_config};
use crate::db::queries;
use crate::event::Context;
use crate::plugins::get_config;
use base64::{Engine as _, engine::general_purpose};
use chrono::{Local, TimeZone};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage, imageops::FilterType};
use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QuerySelect, QueryTrait};

const EMBEDDED_FONT_DATA: &[u8] = include_bytes!("../../../res/HarmonyOS_Sans_Regular.ttf");

// ================= 配色方案 =================

struct ColorScheme {
    background: RGBColor,
    card_background: RGBColor,
    primary: RGBColor,
    text_primary: RGBColor,
    text_secondary: RGBColor,
    grid_line: RGBColor,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            background: RGBColor(255, 255, 255),
            card_background: RGBColor(255, 255, 255),
            primary: RGBColor(59, 130, 246),
            text_primary: RGBColor(30, 41, 59),
            text_secondary: RGBColor(100, 116, 139),
            grid_line: RGBColor(226, 232, 240),
        }
    }
}

// 统一绘图数据结构
#[derive(Clone)]
struct ChartDataPoint {
    label: String,
    value: i64,
}

// 柱状图数据结构 (包含头像和主题色)
struct BarData {
    label: String,
    value: i64,
    avatar_url: Option<String>,
    avatar_img: Option<RgbaImage>,
    theme_color: RGBColor, // 从头像提取的主题色
}

// ================= 核心绘图调度逻辑 =================

#[allow(clippy::too_many_arguments)]
pub async fn generate(
    ctx: &Context,
    is_all_groups: bool,
    _scope: &str,
    data_type: &str,
    chart_type: &str,
    query_guild: Option<&str>,
    query_user: Option<i64>,
    start_time: i64,
    end_time: i64,
    title: &str,
) -> Result<String, String> {
    let db = &ctx.db;
    let config: StatsConfig = get_config(ctx, "stats_visualizer")
        .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

    // 2. 走势图 (Line/Area Chart)
    if chart_type == "走势" {
        let mut chart_data: Vec<ChartDataPoint> = Vec::new();

        // 24小时内按小时聚合
        if end_time - start_time <= 86400 {
            use crate::plugins::recorder::entity::{
                Column as RecordColumn, Entity as RecordEntity,
            };

            // 获取所有符合条件的时间戳
            let timestamps: Vec<i64> = RecordEntity::find()
                .filter(
                    Condition::all()
                        .add(RecordColumn::Time.gte(start_time))
                        .add(RecordColumn::Time.lt(end_time)),
                )
                .apply_if(query_guild, |q, g| q.filter(RecordColumn::GuildId.eq(g)))
                .apply_if(query_user, |q, u| q.filter(RecordColumn::UserId.eq(u)))
                .select_only()
                .column(RecordColumn::Time)
                .into_tuple()
                .all(db)
                .await
                .map_err(|e| e.to_string())?;

            let duration_hours = ((end_time - start_time) as f64 / 3600.0).ceil() as i64;
            let total_buckets = duration_hours.clamp(1, 24) as usize;
            let mut counts = vec![0i64; total_buckets];

            for ts in timestamps {
                let offset = ts - start_time;
                if offset >= 0 {
                    let bucket = (offset / 3600) as usize;
                    if bucket < total_buckets {
                        counts[bucket] += 1;
                    }
                }
            }

            for (i, count) in counts.iter().enumerate() {
                let label_time = Local
                    .timestamp_opt(start_time + (i as i64 * 3600), 0)
                    .unwrap();
                chart_data.push(ChartDataPoint {
                    label: label_time.format("%H:%M").to_string(),
                    value: *count,
                });
            }
        } else {
            // 超过24小时按天
            let trend = queries::get_daily_trend(db, query_guild, query_user, start_time, end_time)
                .await
                .map_err(|e| e.to_string())?;

            chart_data = trend
                .into_iter()
                .map(|t| ChartDataPoint {
                    label: t.date,
                    value: t.count,
                })
                .collect();
        }

        return draw_line_chart(&config, title, chart_data);
    }

    // 1. 消息类型统计
    if data_type == "消息类型" {
        let stats =
            queries::get_message_type_stats(db, query_guild, query_user, start_time, end_time)
                .await
                .map_err(|e| e.to_string())?;

        let raw_data = vec![
            (
                "文本".to_string(),
                stats.total - (stats.image + stats.record + stats.video + stats.face),
            ),
            ("图片".to_string(), stats.image),
            ("语音".to_string(), stats.record),
            ("视频".to_string(), stats.video),
            ("表情".to_string(), stats.face),
        ];

        let bar_data: Vec<BarData> = raw_data
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .map(|(k, v)| BarData {
                label: k,
                value: v,
                avatar_url: None,
                avatar_img: None,
                theme_color: RGBColor(59, 130, 246),
            })
            .collect();

        return draw_bar_chart(&config, title, bar_data);
    }

    // 3. 排行榜 (Bar Chart with Avatars)
    let mut bar_data: Vec<BarData> = Vec::new();

    let avatar_size = 100u32;

    let limit = 20;

    if is_all_groups {
        let ranking = queries::get_group_ranking(db, start_time, end_time, limit)
            .await
            .map_err(|e| e.to_string())?;

        for r in ranking {
            let url = format!("http://p.qlogo.cn/gh/{}/{}/640/", r.guild_id, r.guild_id);
            bar_data.push(BarData {
                label: r.guild_name,
                value: r.count,
                avatar_url: Some(url),
                avatar_img: None,
                theme_color: RGBColor(59, 130, 246),
            });
        }
    } else {
        // 用户排行 (发言 或 表情包)
        if data_type == "表情包" {
            let ranking =
                queries::get_user_emoji_ranking(db, query_guild, start_time, end_time, limit)
                    .await
                    .map_err(|e| e.to_string())?;
            for r in ranking {
                let url = format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", r.user_id);
                bar_data.push(BarData {
                    label: r.nickname,
                    value: r.count,
                    avatar_url: Some(url),
                    avatar_img: None,
                    theme_color: RGBColor(59, 130, 246),
                });
            }
        } else {
            // 发言排行
            let ranking = queries::get_user_ranking(db, query_guild, start_time, end_time, limit)
                .await
                .map_err(|e| e.to_string())?;
            for r in ranking {
                let url = format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", r.user_id);
                bar_data.push(BarData {
                    label: r.nickname,
                    value: r.count,
                    avatar_url: Some(url),
                    avatar_img: None,
                    theme_color: RGBColor(59, 130, 246),
                });
            }
        }
    }

    // 并发下载所有头像
    let default_avatar = create_default_avatar(avatar_size);
    let futures: Vec<_> = bar_data
        .iter()
        .map(|item| {
            let url = item.avatar_url.clone();
            async move {
                if let Some(url) = url {
                    download_avatar(&url, avatar_size).await
                } else {
                    None
                }
            }
        })
        .collect();

    let avatar_results = futures_util::future::join_all(futures).await;

    for (i, avatar) in avatar_results.into_iter().enumerate() {
        if let Some(img) = avatar {
            bar_data[i].theme_color = get_average_color(&img);
            bar_data[i].avatar_img = Some(img);
        } else {
            bar_data[i].avatar_img = Some(default_avatar.clone());
        }
    }

    draw_bar_chart(&config, title, bar_data)
}

// ================= 辅助函数 =================

static CACHED_FONT_PATH: OnceLock<String> = OnceLock::new();

fn get_font_family<'a>(config: &'a StatsConfig) -> &'a str {
    if let Some(path) = &config.font_path
        && std::path::Path::new(path).exists()
    {
        return path.as_str();
    }

    CACHED_FONT_PATH.get_or_init(|| {
        let mut temp_path = env::temp_dir();
        temp_path.push("bot_embedded_harmony_sans.ttf");

        if let Err(e) = fs::write(&temp_path, EMBEDDED_FONT_DATA) {
            warn!(target: "Plugin/Stats", "无法释放内置字体到临时目录: {}, 将回退到系统字体", e);
            return "sans-serif".to_string();
        }

        temp_path.to_string_lossy().to_string()
    }).as_str()
}

fn get_font<'a>(config: &'a StatsConfig, size: u32) -> TextStyle<'a> {
    let colors = ColorScheme::default();
    let family = get_font_family(config);

    (family, size).into_font().color(&colors.text_primary)
}

fn get_font_with_color<'a>(
    config: &'a StatsConfig,
    size: u32,
    color: &'a RGBColor,
) -> TextStyle<'a> {
    let family = get_font_family(config);

    (family, size).into_font().color(color)
}

fn get_contrast_color(bg_color: RGBColor) -> RGBColor {
    let (r, g, b) = (bg_color.0 as u32, bg_color.1 as u32, bg_color.2 as u32);
    // YIQ brightness formula
    let yiq = (r * 299 + g * 587 + b * 114) / 1000;
    if yiq >= 128 {
        RGBColor(0, 0, 0)
    } else {
        RGBColor(255, 255, 255)
    }
}

fn mix_with_white(color: RGBColor, opacity: f32) -> RGBColor {
    let r = (color.0 as f32 * opacity + 255.0 * (1.0 - opacity)) as u8;
    let g = (color.1 as f32 * opacity + 255.0 * (1.0 - opacity)) as u8;
    let b = (color.2 as f32 * opacity + 255.0 * (1.0 - opacity)) as u8;
    RGBColor(r, g, b)
}

fn save_rgba_to_base64(img: RgbaImage) -> Result<String, String> {
    let dynamic_image = DynamicImage::ImageRgba8(img);
    let mut cursor = std::io::Cursor::new(Vec::new());
    dynamic_image
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("图片编码失败: {}", e))?;
    let b64 = general_purpose::STANDARD.encode(cursor.into_inner());
    Ok(format!("base64://{}", b64))
}

fn truncate_text_to_fit(font: &plotters::style::FontDesc, text: &str, max_width: u32) -> String {
    let (w, _) = font.box_size(text).unwrap_or((0, 0));
    if w <= max_width {
        return text.to_string();
    }

    let mut s = text.to_string();
    while !s.is_empty() {
        s.pop();
        let candidate = format!("{}...", s);
        let (w, _) = font.box_size(&candidate).unwrap_or((0, 0));
        if w <= max_width {
            return candidate;
        }
    }
    "...".to_string()
}

fn get_average_color(img: &RgbaImage) -> RGBColor {
    let mut r_sum = 0u64;
    let mut g_sum = 0u64;
    let mut b_sum = 0u64;
    let count = (img.width() * img.height()) as u64;

    if count == 0 {
        return RGBColor(59, 130, 246);
    }

    for p in img.pixels() {
        r_sum += p[0] as u64;
        g_sum += p[1] as u64;
        b_sum += p[2] as u64;
    }

    RGBColor(
        (r_sum / count) as u8,
        (g_sum / count) as u8,
        (b_sum / count) as u8,
    )
}

async fn download_avatar(url: &str, size: u32) -> Option<RgbaImage> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    if let Ok(resp) = client.get(url).send().await
        && let Ok(bytes) = resp.bytes().await
        && let Ok(img) = image::load_from_memory(&bytes)
    {
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);
        return Some(make_circular_avatar(&resized, size));
    }
    None
}

fn make_circular_avatar(img: &DynamicImage, size: u32) -> RgbaImage {
    let rgba = img.to_rgba8();
    let mut result = RgbaImage::new(size, size);
    let center = size as f32 / 2.0;
    let radius = center - 1.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center + 0.5;
            let dy = y as f32 - center + 0.5;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius - 0.5 {
                result.put_pixel(x, y, *rgba.get_pixel(x, y));
            } else if dist <= radius + 0.5 {
                let alpha = (radius + 0.5 - dist).clamp(0.0, 1.0);
                let mut pixel = *rgba.get_pixel(x, y);
                pixel[3] = (pixel[3] as f32 * alpha) as u8;
                result.put_pixel(x, y, pixel);
            }
        }
    }
    result
}

fn create_default_avatar(size: u32) -> RgbaImage {
    let mut result = RgbaImage::new(size, size);
    let center = size as f32 / 2.0;
    let radius = center - 1.0;
    let bg_color = Rgba([200, 200, 200, 255]);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center + 0.5;
            let dy = y as f32 - center + 0.5;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius - 0.5 {
                result.put_pixel(x, y, bg_color);
            } else if dist <= radius + 0.5 {
                let alpha = (radius + 0.5 - dist).clamp(0.0, 1.0);
                let mut pixel = bg_color;
                pixel[3] = (255.0 * alpha) as u8;
                result.put_pixel(x, y, pixel);
            }
        }
    }
    result
}

fn overlay_image(base: &mut RgbaImage, overlay: &RgbaImage, x: i32, y: i32) {
    let (base_w, base_h) = base.dimensions();
    let (overlay_w, overlay_h) = overlay.dimensions();

    for oy in 0..overlay_h {
        for ox in 0..overlay_w {
            let bx = x + ox as i32;
            let by = y + oy as i32;

            if bx >= 0 && bx < base_w as i32 && by >= 0 && by < base_h as i32 {
                let bg = base.get_pixel(bx as u32, by as u32);
                let fg = overlay.get_pixel(ox, oy);

                let alpha = fg[3] as f32 / 255.0;
                if alpha > 0.0 {
                    let blended = Rgba([
                        ((1.0 - alpha) * bg[0] as f32 + alpha * fg[0] as f32) as u8,
                        ((1.0 - alpha) * bg[1] as f32 + alpha * fg[1] as f32) as u8,
                        ((1.0 - alpha) * bg[2] as f32 + alpha * fg[2] as f32) as u8,
                        255,
                    ]);
                    base.put_pixel(bx as u32, by as u32, blended);
                }
            }
        }
    }
}

// ================= 绘图实现 =================

/// 绘制水平条形图 (排行榜)
fn draw_bar_chart(config: &StatsConfig, title: &str, data: Vec<BarData>) -> Result<String, String> {
    if data.is_empty() {
        return Err("暂无数据".to_string());
    }

    let s = 2u32; // Scale factor

    // === 1. 预计算与布局参数 (Scaling) ===
    let padding = 24 * s;

    // 内部尺寸也随之放大
    let row_height = 50 * s;
    let font_size = 30 * s;
    let avatar_width = 50 * s;
    let gap_text = 10 * s;

    // 标题区域
    let title_font_size = 32 * s;
    let header_font_size = 20 * s;

    let header_margin = 10 * s;
    let title_margin = 15 * s; // 标题和列表的间距
    let top_area_height =
        padding + header_font_size + header_margin + title_font_size + title_margin;

    let base_bar_min_width = 150.0 * (s as f64);
    let base_bar_scale_width = 700.0 * (s as f64);
    let max_possible_bar_width = (base_bar_min_width + base_bar_scale_width) as u32;

    let max_val = data.iter().map(|d| d.value).max().unwrap_or(1).max(1);
    let total_val: i64 = data.iter().map(|d| d.value).sum();

    let font_obj = if let Some(path) = &config.font_path
        && std::fs::read(path).is_ok()
    {
        (path.as_str(), font_size).into_font()
    } else {
        ("sans-serif", font_size).into_font()
    };

    let mut formatted_counts = Vec::new();
    let mut max_count_text_width = 0u32;

    for item in data.iter() {
        let mut text = item.value.to_string();
        if item.value == max_val {
            let pct = if total_val > 0 {
                item.value as f64 / total_val as f64 * 100.0
            } else {
                0.0
            };
            let pct_str = if pct < 0.01 && pct > 0.0 {
                "<0.01".to_string()
            } else if pct < 1.0 {
                format!("{:.2}", pct)
            } else {
                format!("{:.0}", pct)
            };
            text = format!("{} ( {}%)", text, pct_str);
        }

        let (w, _) = font_obj.box_size(&text).unwrap_or((0, 0));
        max_count_text_width = max_count_text_width.max(w);
        formatted_counts.push(text);
    }

    // 计算内容区域尺寸
    let content_width = avatar_width + max_possible_bar_width + gap_text + max_count_text_width;
    let content_height = data.len() as u32 * row_height + top_area_height;

    // 计算画布尺寸 (增加四周边距)
    let canvas_width = content_width + padding * 2;
    let canvas_height = content_height + padding; // 底部留白

    // === 2. 绘图 ===
    let mut buffer = vec![0u8; (canvas_width * canvas_height * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut buffer, (canvas_width, canvas_height))
            .into_drawing_area();

        root.fill(&RGBColor(255, 255, 255))
            .map_err(|e| e.to_string())?;

        let now_str = Local::now().format("%Y-%m-%d %H:%M").to_string();
        let header_style = get_font(config, header_font_size)
            .pos(Pos::new(HPos::Center, VPos::Top))
            .color(&RGBColor(100, 116, 139));
        root.draw_text(
            &now_str,
            &header_style,
            (canvas_width as i32 / 2, padding as i32),
        )
        .map_err(|e| e.to_string())?;

        // 绘制标题 (Header 下方)
        let title_y = padding + header_font_size + header_margin;
        let title_style = get_font(config, title_font_size).pos(Pos::new(HPos::Center, VPos::Top));
        root.draw_text(
            title,
            &title_style,
            (canvas_width as i32 / 2, title_y as i32),
        )
        .map_err(|e| e.to_string())?;

        // 绘制每一行
        for (i, item) in data.iter().enumerate() {
            // Y坐标向下偏移 top_area_height
            let y = top_area_height as i32 + (i as u32 * row_height) as i32;
            // X坐标向右偏移 padding + avatar_width
            let start_x = padding as i32 + avatar_width as i32;

            // 1. 计算当前条的实际宽度
            let ratio = item.value as f64 / max_val as f64;
            let current_bar_width =
                (base_bar_min_width + base_bar_scale_width * ratio).round() as i32;

            let theme_color = item.theme_color;
            let faded_color = mix_with_white(theme_color, 0.5);

            // 2. 绘制背景条 (Faded)
            let remaining_start_x = start_x + current_bar_width;
            let faded_bar_end_x = (padding + avatar_width + max_possible_bar_width) as i32;

            if remaining_start_x < faded_bar_end_x {
                root.draw(&Rectangle::new(
                    [
                        (remaining_start_x, y),
                        (faded_bar_end_x, y + row_height as i32),
                    ],
                    faded_color.filled(),
                ))
                .map_err(|e| e.to_string())?;
            }

            // 3. 绘制进度条 (Solid)
            root.draw(&Rectangle::new(
                [
                    (start_x, y),
                    (start_x + current_bar_width, y + row_height as i32),
                ],
                theme_color.filled(),
            ))
            .map_err(|e| e.to_string())?;

            // 4. 绘制昵称 (Bar 内部左侧)
            let name_color = get_contrast_color(theme_color);
            let name_style = get_font_with_color(config, font_size, &name_color)
                .pos(Pos::new(HPos::Left, VPos::Center));

            // 稍微留白
            let max_name_width = if current_bar_width > (20 * s as i32) {
                (current_bar_width - (20 * s as i32)) as u32
            } else {
                0
            };

            let display_name = truncate_text_to_fit(&font_obj, &item.label, max_name_width);

            if !display_name.is_empty() {
                root.draw_text(
                    &display_name,
                    &name_style,
                    (
                        start_x + (10 * s as i32),
                        y + (row_height / 2) as i32 + (2 * s as i32),
                    ),
                )
                .map_err(|e| e.to_string())?;
            }

            // 5. 绘制数值 (Bar 外部右侧)
            let count_text = &formatted_counts[i];
            let count_style = get_font_with_color(config, font_size, &BLACK)
                .pos(Pos::new(HPos::Left, VPos::Center));

            root.draw_text(
                count_text,
                &count_style,
                (
                    start_x + current_bar_width + (10 * s as i32),
                    y + (row_height / 2) as i32 + (2 * s as i32),
                ),
            )
            .map_err(|e| e.to_string())?;
        }

        // 6. 绘制竖线装饰
        let vertical_line_color = RGBAColor(0, 0, 0, 0.12);
        let first_line_x = padding as i32 + (200 * s as i32);
        let line_width = 3 * s as i32;
        let mut line_x = first_line_x;
        let content_end_y = top_area_height as i32 + (data.len() as u32 * row_height) as i32;

        for _ in 0..8 {
            if line_x >= (canvas_width - padding) as i32 {
                break;
            }
            root.draw(&Rectangle::new(
                [
                    (line_x, top_area_height as i32),
                    (line_x + line_width, content_end_y),
                ],
                vertical_line_color.filled(),
            ))
            .map_err(|e| e.to_string())?;
            line_x += 100 * s as i32;
        }

        root.present().map_err(|e| e.to_string())?;
    }

    // === 3. 转换并叠加头像 ===
    let mut rgba_image = RgbaImage::new(canvas_width, canvas_height);
    for y in 0..canvas_height {
        for x in 0..canvas_width {
            let idx = ((y * canvas_width + x) * 3) as usize;
            let r = buffer[idx];
            let g = buffer[idx + 1];
            let b = buffer[idx + 2];
            rgba_image.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    // 叠加头像 (注意边距偏移)
    for (i, item) in data.iter().enumerate() {
        if let Some(avatar) = &item.avatar_img {
            let y_pos = top_area_height as i32 + (i as u32 * row_height) as i32;
            let x_pos = padding as i32;
            overlay_image(&mut rgba_image, avatar, x_pos, y_pos);
        }
    }

    save_rgba_to_base64(rgba_image)
}

/// 绘制折线图 (走势 - 区域图)
fn draw_line_chart(
    config: &StatsConfig,
    title: &str,
    data: Vec<ChartDataPoint>,
) -> Result<String, String> {
    if data.is_empty() {
        return Err("暂无数据".to_string());
    }

    let s = 2u32;
    let width = config.width * s;
    let height = config.height * s;

    let colors = ColorScheme::default();
    let mut buffer = vec![0u8; (width * height * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut buffer, (width, height)).into_drawing_area();

        root.fill(&colors.background).map_err(|e| e.to_string())?;

        let max_count = data.iter().map(|d| d.value).max().unwrap_or(10);
        let y_max = (max_count as f64 * 1.15) as i64;
        let x_labels: Vec<String> = data.iter().map(|d| d.label.clone()).collect();

        let area_color = RGBAColor(colors.primary.0, colors.primary.1, colors.primary.2, 0.25);

        let padding_top = 25 * s;
        let header_font_size = 20 * s;
        let title_font_size = 28 * s;
        let gap = 10 * s;

        let now_str = Local::now().format("%Y-%m-%d %H:%M").to_string();
        let header_style = get_font(config, header_font_size)
            .pos(Pos::new(HPos::Center, VPos::Top))
            .color(&RGBColor(100, 116, 139));

        root.draw_text(
            &now_str,
            &header_style,
            (width as i32 / 2, padding_top as i32),
        )
        .map_err(|e| e.to_string())?;

        let title_y = padding_top + header_font_size + gap;
        let title_style = get_font(config, title_font_size).pos(Pos::new(HPos::Center, VPos::Top));
        root.draw_text(title, &title_style, (width as i32 / 2, title_y as i32))
            .map_err(|e| e.to_string())?;

        // 计算图表顶部边距
        let chart_margin_top = title_y + title_font_size + (20 * s);

        let mut chart = ChartBuilder::on(&root)
            .margin_top(chart_margin_top as i32)
            .margin_bottom(25 * s as i32)
            .margin_left(45 * s as i32)
            .margin_right(45 * s as i32)
            .x_label_area_size(45 * s as i32)
            .y_label_area_size(55 * s as i32)
            .build_cartesian_2d(0..data.len(), 0..y_max)
            .map_err(|e| e.to_string())?;

        chart
            .configure_mesh()
            .light_line_style(colors.grid_line.stroke_width(1 * s))
            .bold_line_style(colors.grid_line.stroke_width(1 * s))
            .x_labels(data.len().min(12))
            .x_label_formatter(&|x| {
                if *x < x_labels.len() {
                    let full = &x_labels[*x];
                    if full.len() >= 10 && full.contains('-') {
                        full[5..].to_string()
                    } else {
                        full.clone()
                    }
                } else {
                    "".to_string()
                }
            })
            // 放大坐标轴字体
            .x_label_style(get_font_with_color(config, 11 * s, &colors.text_secondary))
            .y_label_style(get_font_with_color(config, 11 * s, &colors.text_secondary))
            .y_desc("消息数")
            .draw()
            .map_err(|e| e.to_string())?;

        chart
            .draw_series(
                AreaSeries::new(
                    data.iter().enumerate().map(|(i, d)| (i, d.value)),
                    0,
                    area_color,
                )
                .border_style(colors.primary.stroke_width(2 * s)),
            )
            .map_err(|e| e.to_string())?;

        chart
            .draw_series(PointSeries::of_element(
                data.iter().enumerate().map(|(i, d)| (i, d.value)),
                5 * s, // Point size
                colors.primary.filled(),
                &|c, sz, st| {
                    EmptyElement::at(c)
                        + Circle::new((0, 0), sz, colors.card_background.filled())
                        + Circle::new((0, 0), sz.saturating_sub(2 * s), st)
                },
            ))
            .map_err(|e| e.to_string())?;

        root.present().map_err(|e| e.to_string())?;
    }

    let mut rgba_image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 3) as usize;
            let r = buffer[idx];
            let g = buffer[idx + 1];
            let b = buffer[idx + 2];
            rgba_image.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    save_rgba_to_base64(rgba_image)
}
