use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::config::build_config;
use crate::db::queries;
use crate::db::utils::get_time_range;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use base64::{Engine as _, engine::general_purpose};
use futures_util::future::BoxFuture;
use image::{DynamicImage, ImageFormat, RgbImage};
use plotters::prelude::*;
use regex::Regex;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect,
    QueryTrait,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use toml::Value;

// ================= 配置定义 =================

#[derive(Serialize, Deserialize, Clone)]
struct StatsConfig {
    enabled: bool,
    #[serde(default = "default_font_path")]
    font_path: Option<String>,
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,

    // === 每日推送 ===
    #[serde(default)]
    daily_push_enabled: bool,
    #[serde(default = "default_daily_push_time")]
    daily_push_time: String, // "HH:MM:SS"
    #[serde(default)]
    daily_push_scope: String, // "本群" (默认，推送到各自群)
}

fn default_font_path() -> Option<String> {
    None
}

fn default_width() -> u32 {
    800
}

fn default_height() -> u32 {
    600
}

fn default_daily_push_time() -> String {
    "23:30:00".to_string()
}

pub fn default_config() -> Value {
    build_config(StatsConfig {
        enabled: true,
        font_path: None,
        width: 800,
        height: 600,
        daily_push_enabled: false,
        daily_push_time: "23:30:00".to_string(),
        daily_push_scope: "本群".to_string(),
    })
}

// ================= 正则匹配 =================

static REGEX_GLOBAL: OnceLock<Regex> = OnceLock::new();
static REGEX_NORMAL: OnceLock<Regex> = OnceLock::new();

fn get_regex_global() -> &'static Regex {
    REGEX_GLOBAL.get_or_init(|| {
        Regex::new(r"^(今日|昨日|本周|上周|近7天|近30天|本月|上月|今年|去年|总)所有群发言(排行榜|走势|活跃度)$")
            .unwrap()
    })
}

fn get_regex_normal() -> &'static Regex {
    REGEX_NORMAL.get_or_init(|| {
        Regex::new(r"^(?:(本群|跨群|我的))?(今日|昨日|本周|上周|近7天|近30天|本月|上月|今年|去年|总)(发言|表情包|消息类型)(排行榜|走势|活跃度)$")
            .unwrap()
    })
}

// ================= 插件入口 =================

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

        let content = match matched_content {
            Some(c) => c,
            None => return Ok(Some(ctx)),
        };

        // 匹配并提取参数
        let (scope, time_str, data_type, chart_type, is_all_groups) =
            if let Some(caps) = get_regex_global().captures(content) {
                let t = caps.get(1).map_or("", |m| m.as_str());
                let c_type = caps.get(2).map_or("", |m| m.as_str());
                ("跨群", t, "发言", c_type, true)
            } else if let Some(caps) = get_regex_normal().captures(content) {
                let s = caps.get(1).map_or("本群", |m| m.as_str());
                let t = caps.get(2).map_or("", |m| m.as_str());
                let d = caps.get(3).map_or("", |m| m.as_str());
                let c = caps.get(4).map_or("", |m| m.as_str());
                let final_scope = if s.is_empty() { "本群" } else { s };
                (final_scope, t, d, c, false)
            } else {
                return Ok(Some(ctx));
            };

        // 校验 Context
        let group_id = msg.group_id();
        let user_id = msg.user_id();

        if scope == "本群" && group_id.is_none() {
            let _ = send_msg(
                &ctx,
                writer,
                None,
                Some(user_id),
                "请在群聊中使用“本群”相关指令。",
            )
            .await;
            return Ok(None);
        }

        info!(
            target: "Plugin/Stats",
            "Req: Scope={}, Time={}, Data={}, Chart={}, Global={}",
            scope, time_str, data_type, chart_type, is_all_groups
        );

        let (start_time, end_time) = get_time_range(time_str);

        let (query_guild, query_user) = match scope {
            "本群" => (group_id.map(|g| g.to_string()), None),
            "跨群" => (None, None),
            "我的" => (None, Some(user_id)),
            _ => (None, None),
        };

        let title = if is_all_groups {
            format!("所有群 {} {} {}", time_str, data_type, chart_type)
        } else {
            format!("{} {} {} {}", scope, time_str, data_type, chart_type)
        };

        let result_img = execute_chart_generation(
            &ctx,
            is_all_groups,
            scope,
            data_type,
            chart_type,
            query_guild.as_deref(),
            query_user,
            start_time,
            end_time,
            &title,
        )
        .await;

        match result_img {
            Ok(b64) => {
                let reply = Message::new().text(format!("📊 {}", title)).image(b64);
                let _ = send_msg(&ctx, writer, group_id, Some(user_id), reply).await;
            }
            Err(e) => {
                let _ = send_msg(
                    &ctx,
                    writer,
                    group_id,
                    Some(user_id),
                    format!("生成失败: {}", e),
                )
                .await;
            }
        }

        Ok(None)
    })
}

/// 每日推送钩子
pub fn on_connected(
    ctx: Context,
    writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let config: StatsConfig = get_config(&ctx, "stats_visualizer")
            .unwrap_or_else(|| serde::Deserialize::deserialize(default_config()).unwrap());

        if !config.daily_push_enabled {
            return Ok(Some(ctx));
        }

        let scheduler = ctx.scheduler.clone();

        // 使用通用逻辑调度
        scheduler.schedule_daily_push(
            ctx.clone(),
            writer.clone(),
            "Stats",
            config.daily_push_time.clone(),
            move |c, w, gid| async move {
                let title = "本群 今日 发言 排行榜 (每日推送)".to_string();
                let (start, end) = get_time_range("今日");

                let res = execute_chart_generation(
                    &c,
                    false,
                    "本群",
                    "发言",
                    "排行榜",
                    Some(&gid.to_string()),
                    None,
                    start,
                    end,
                    &title,
                )
                .await;

                if let Ok(b64) = res {
                    let msg = Message::new().text(format!("📅 {}", title)).image(b64);
                    let _ = send_msg(&c, w, Some(gid), None, msg).await;
                } else if let Err(e) = res {
                    warn!(target: "Plugin/Stats", "群 {} 推送生成失败: {}", gid, e);
                }
            },
        );

        Ok(Some(ctx))
    })
}

// ================= 核心绘图调度逻辑 =================

#[derive(Debug, FromQueryResult)]
struct HourlyCount {
    hour: i32,
    count: i64,
}

// 统一绘图数据结构
struct ChartDataPoint {
    label: String,
    value: i64,
}

#[allow(clippy::too_many_arguments)]
async fn execute_chart_generation(
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

    // 1. 获取数据
    if data_type == "消息类型" {
        let stats =
            queries::get_message_type_stats(db, query_guild, query_user, start_time, end_time)
                .await
                .map_err(|e| e.to_string())?;

        let data = vec![
            (
                "文本".to_string(),
                stats.total - (stats.image + stats.record + stats.video + stats.face),
            ),
            ("图片".to_string(), stats.image),
            ("语音".to_string(), stats.record),
            ("视频".to_string(), stats.video),
            ("表情".to_string(), stats.face),
        ];
        let data: Vec<(String, i64)> = data.into_iter().filter(|(_, v)| *v > 0).collect();
        return draw_bar_chart(&config, title, data);
    }

    if chart_type == "走势" || chart_type == "活跃度" {
        let mut chart_data: Vec<ChartDataPoint> = Vec::new();

        // 如果查询范围在 24 小时内，使用小时作为横坐标
        if end_time - start_time <= 86400 {
            use crate::plugins::recorder::entity::{
                Column as RecordColumn, Entity as RecordEntity,
            };

            let results = RecordEntity::find()
                .filter(
                    Condition::all()
                        .add(RecordColumn::Time.gte(start_time))
                        .add(RecordColumn::Time.lt(end_time)),
                )
                .apply_if(query_guild, |q, g| q.filter(RecordColumn::GuildId.eq(g)))
                .apply_if(query_user, |q, u| q.filter(RecordColumn::UserId.eq(u)))
                .select_only()
                .column(RecordColumn::TimeHour)
                .column_as(RecordColumn::Id.count(), "count")
                .group_by(RecordColumn::TimeHour)
                .order_by_asc(RecordColumn::TimeHour)
                .into_model::<HourlyCount>()
                .all(db)
                .await
                .map_err(|e| e.to_string())?;

            // 填充 0-23 小时
            let mut map = std::collections::HashMap::new();
            for r in results {
                map.insert(r.hour, r.count);
            }
            for h in 0..24 {
                chart_data.push(ChartDataPoint {
                    label: format!("{:02}:00", h),
                    value: *map.get(&h).unwrap_or(&0),
                });
            }
        } else {
            // 否则按天
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

    // 排行榜 (Bar Chart)
    if is_all_groups {
        let ranking = queries::get_group_ranking(db, start_time, end_time, 15)
            .await
            .map_err(|e| e.to_string())?;
        let data: Vec<(String, i64)> = ranking
            .into_iter()
            .map(|r| (format!("{} ({})", r.guild_name, r.guild_id), r.count))
            .collect();
        return draw_bar_chart(&config, title, data);
    }

    if data_type == "表情包" {
        let ranking = queries::get_user_emoji_ranking(db, query_guild, start_time, end_time, 15)
            .await
            .map_err(|e| e.to_string())?;
        let data: Vec<(String, i64)> = ranking.into_iter().map(|r| (r.nickname, r.count)).collect();
        draw_bar_chart(&config, title, data)
    } else {
        let ranking = queries::get_user_ranking(db, query_guild, start_time, end_time, 15)
            .await
            .map_err(|e| e.to_string())?;
        let data: Vec<(String, i64)> = ranking.into_iter().map(|r| (r.nickname, r.count)).collect();
        draw_bar_chart(&config, title, data)
    }
}

// ================= Plotters 绘图实现 =================

fn get_font<'a>(config: &'a StatsConfig, size: u32) -> TextStyle<'a> {
    if let Some(path) = &config.font_path {
        match std::fs::read(path) {
            Ok(_) => {
                return (path.as_str(), size)
                    .into_font()
                    .color(&query_font_color(0));
            }
            Err(_) => {}
        }
    }
    ("sans-serif", size).into_font().color(&query_font_color(0))
}

fn query_font_color(idx: usize) -> RGBColor {
    let colors = [BLACK, RED, BLUE, GREEN, MAGENTA, CYAN];
    colors[idx % colors.len()]
}

fn save_buffer_to_base64(buf: Vec<u8>, width: u32, height: u32) -> Result<String, String> {
    let img_buffer = RgbImage::from_raw(width, height, buf)
        .ok_or_else(|| "无法从原始像素数据构建图像".to_string())?;
    let dynamic_image = DynamicImage::ImageRgb8(img_buffer);
    let mut cursor = std::io::Cursor::new(Vec::new());
    dynamic_image
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("图片编码失败: {}", e))?;
    let b64 = general_purpose::STANDARD.encode(cursor.into_inner());
    Ok(format!("base64://{}", b64))
}

/// 绘制水平条形图 (排行榜)
/// 修复: 使用 Segmented 坐标轴实现居中对齐；动态计算 Label 区域宽度防止文字截断。
fn draw_bar_chart(
    config: &StatsConfig,
    title: &str,
    data: Vec<(String, i64)>,
) -> Result<String, String> {
    if data.is_empty() {
        return Err("暂无数据".to_string());
    }

    // 翻转数据，使得第一名在最上面
    let data: Vec<(String, i64)> = data.into_iter().rev().collect();

    let width = config.width;
    let height = config.height;
    let mut buffer = vec![0u8; (width * height * 3) as usize];

    // 动态计算 Y 轴 Label 宽度 (估计值：字符数 * 15px)
    let max_label_len = data
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(0);
    // 限制最小 100，最大 400
    let y_label_area_size = (max_label_len as u32 * 15).clamp(100, 400);

    {
        let root = BitMapBackend::with_buffer(&mut buffer, (width, height)).into_drawing_area();
        root.fill(&WHITE).map_err(|e| e.to_string())?;

        let max_val = data.iter().map(|(_, v)| *v).max().unwrap_or(10);

        // 使用 Segmented 坐标系
        let mut chart = ChartBuilder::on(&root)
            .caption(title, get_font(config, 30))
            .margin(20)
            .x_label_area_size(40)
            .y_label_area_size(y_label_area_size)
            .build_cartesian_2d(
                0..(max_val as f64 * 1.1) as i64,
                (0..data.len()).into_segmented(),
            )
            .map_err(|e| e.to_string())?;

        chart
            .configure_mesh()
            .y_labels(data.len())
            .y_label_formatter(&|y| match y {
                SegmentValue::Exact(i) | SegmentValue::CenterOf(i) => {
                    if *i < data.len() {
                        data[*i].0.clone()
                    } else {
                        "".to_string()
                    }
                }
                _ => "".to_string(),
            })
            .y_label_style(get_font(config, 15))
            .draw()
            .map_err(|e| e.to_string())?;

        chart
            .draw_series(data.iter().enumerate().map(|(i, (_name, count))| {
                let style = Palette99::pick(i).filled();
                // 在 Segmented 坐标系中，y 指定为 SegmentValue::Exact(i) 会自动填充该段的高度
                Rectangle::new(
                    [
                        (0, SegmentValue::Exact(i)),
                        (*count, SegmentValue::Exact(i)),
                    ],
                    style,
                )
            }))
            .map_err(|e| e.to_string())?
            .label("Count")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED));

        // 绘制数值标签 (居中显示在条形末端)
        for (i, (_, count)) in data.iter().enumerate() {
            root.draw_text(
                &count.to_string(),
                &get_font(config, 15),
                chart.backend_coord(&(*count, SegmentValue::CenterOf(i))),
            )
            .ok();
        }

        root.present().map_err(|e| e.to_string())?;
    }

    save_buffer_to_base64(buffer, width, height)
}

/// 绘制折线图 (走势)
fn draw_line_chart(
    config: &StatsConfig,
    title: &str,
    data: Vec<ChartDataPoint>,
) -> Result<String, String> {
    if data.is_empty() {
        return Err("暂无数据".to_string());
    }

    let width = config.width;
    let height = config.height;
    let mut buffer = vec![0u8; (width * height * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut buffer, (width, height)).into_drawing_area();
        root.fill(&WHITE).map_err(|e| e.to_string())?;

        let max_count = data.iter().map(|d| d.value).max().unwrap_or(10);
        let x_labels: Vec<String> = data.iter().map(|d| d.label.clone()).collect();

        let mut chart = ChartBuilder::on(&root)
            .caption(title, get_font(config, 30))
            .margin(20)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(0..data.len(), 0..(max_count as f64 * 1.1) as i64)
            .map_err(|e| e.to_string())?;

        chart
            .configure_mesh()
            .x_labels(data.len())
            .x_label_formatter(&|x| {
                if *x < x_labels.len() {
                    let full = &x_labels[*x];
                    // 简单截断长日期
                    if full.len() > 8 {
                        full[5..].to_string()
                    } else {
                        full.clone()
                    }
                } else {
                    "".to_string()
                }
            })
            .x_label_style(get_font(config, 12))
            .y_label_style(get_font(config, 15))
            .draw()
            .map_err(|e| e.to_string())?;

        chart
            .draw_series(LineSeries::new(
                data.iter().enumerate().map(|(i, d)| (i, d.value)),
                &RED,
            ))
            .map_err(|e| e.to_string())?;

        chart
            .draw_series(PointSeries::of_element(
                data.iter().enumerate().map(|(i, d)| (i, d.value)),
                3,
                &RED,
                &|c, s, st| EmptyElement::at(c) + Circle::new((0, 0), s, st.filled()),
            ))
            .map_err(|e| e.to_string())?;

        root.present().map_err(|e| e.to_string())?;
    }

    save_buffer_to_base64(buffer, width, height)
}
