use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::config::build_config;
use crate::db::utils::get_time_range;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use futures_util::future::BoxFuture;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use toml::Value;

mod chart;

// ================= 配置定义 =================

#[derive(Serialize, Deserialize, Clone)]
pub struct StatsConfig {
    pub enabled: bool,
    #[serde(default = "default_font_path")]
    pub font_path: Option<String>,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,

    // === 每日推送 ===
    #[serde(default)]
    pub daily_push_enabled: bool,
    #[serde(default = "default_daily_push_time")]
    pub daily_push_time: String, // "HH:MM:SS"
    #[serde(default)]
    pub daily_push_scope: String, // "本群" (默认，推送到各自群)
}

fn default_font_path() -> Option<String> {
    None
}

fn default_width() -> u32 {
    960
}

fn default_height() -> u32 {
    800
}

fn default_daily_push_time() -> String {
    "23:30:00".to_string()
}

pub fn default_config() -> Value {
    build_config(StatsConfig {
        enabled: true,
        font_path: None,
        width: 960,
        height: 800,
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
        Regex::new(
            r"^所有群(今日|昨日|本周|上周|近7天|近30天|本月|上月|今年|去年|总)发言(排行榜|走势)$",
        )
        .unwrap()
    })
}

fn get_regex_normal() -> &'static Regex {
    REGEX_NORMAL.get_or_init(|| {
        Regex::new(r"^(?:(本群|跨群|我的))?(今日|昨日|本周|上周|近7天|近30天|本月|上月|今年|去年|总)(发言|表情包|消息类型)(排行榜|走势)$")
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
                r#"请在群聊中使用"本群"相关指令。"#,
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

        let result_img = chart::generate(
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
                // 仅发送图片，不带文字标题，防止变成缩略图
                let reply = Message::new().image(b64);
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

        scheduler.schedule_daily_push(
            ctx.clone(),
            writer.clone(),
            "Stats",
            config.daily_push_time.clone(),
            move |c, w, gid| async move {
                let title = "本群 今日 发言 排行榜 (每日推送)".to_string();
                let (start, end) = get_time_range("今日");

                let res = chart::generate(
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
                    let msg = Message::new().image(b64);
                    let _ = send_msg(&c, w, Some(gid), None, msg).await;
                } else if let Err(e) = res {
                    warn!(target: "Plugin/Stats", "群 {} 推送生成失败: {}", gid, e);
                }
            },
        );

        Ok(Some(ctx))
    })
}
