use crate::adapters::onebot::api;
use crate::adapters::onebot::{LockedWriter, send_msg};
use crate::command::get_prefixes;
use crate::db::queries::get_text_corpus;
use crate::db::utils::get_time_range;
use crate::event::Context;
use crate::message::Message;
use crate::plugins::{PluginError, get_config};
use crate::{error, info, warn};
use futures_util::future::BoxFuture;
use regex::Regex;
use std::sync::OnceLock;

pub mod config;
pub mod image;
pub mod stopwords;

use config::WordCloudConfig;
pub use config::default_config;

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

            let (query_group_id, query_user_id) = match scope_str {
                "本群" => {
                    if let Some(gid) = msg.group_id() {
                        (Some(gid), None)
                    } else {
                        (None, Some(msg.user_id()))
                    }
                }
                "跨群" => (None, None),
                "我的" => (None, Some(msg.user_id())),
                _ => (None, None),
            };

            if scope_str == "本群" && query_group_id.is_none() && msg.group_id().is_none() {
                let reply =
                    Message::new().text("请在群聊中使用“本群”指令，或使用“我的”查看个人词云。");
                send_msg(&ctx, writer, msg.group_id(), Some(msg.user_id()), reply).await?;
                return Ok(None);
            }

            match generate_and_send(
                &ctx,
                writer,
                query_group_id,
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

        // 调试模式
        if config.debug_push_interval > 0 {
            info!(target: "Plugin/WordCloud", "已开启词云调试推送，间隔: {}秒", config.debug_push_interval);
            let ctx_debug = ctx.clone();
            let writer_debug = writer.clone();
            scheduler.add_interval(
                std::time::Duration::from_secs(config.debug_push_interval),
                move || {
                    let c = ctx_debug.clone();
                    let w = writer_debug.clone();
                    async move {
                        let cfg = WordCloudConfig {
                            enabled: true,
                            limit: 50,
                            width: 800,
                            height: 600,
                            font_path: None,
                            max_msg: 50000,
                            daily_push_enabled: true,
                            daily_push_time: "23:30:00".to_string(),
                            debug_push_interval: 0,
                        };
                        do_daily_push_logic(c, w, cfg).await;
                    }
                },
            );
        } else {
            // 正常每日推送
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
                        Some(gid),
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
            Some(gid),
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

#[allow(clippy::too_many_arguments)]
async fn generate_and_send(
    ctx: &Context,
    writer: LockedWriter,
    query_group_id: Option<i64>,
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
        get_text_corpus(db, query_group_id, query_user_id, start_time, end_time).await;

    let mut corpus = match corpus_result {
        Ok(c) if c.is_empty() => {
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
        image::generate_word_cloud(corpus, font_path, limit, width, height)
    })
    .await;

    match final_msg {
        Ok(Ok(base64_image)) => {
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
