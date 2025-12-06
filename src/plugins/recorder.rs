use crate::adapters::onebot::LockedWriter;
use crate::config::build_config;
use crate::event::{Context, EventType};
use crate::plugins::{PluginError, get_config};
use chrono::{Datelike, Local, TimeZone, Timelike};
use futures_util::future::BoxFuture;
use sea_orm::{ActiveModelTrait, ActiveValue, ConnectionTrait, Schema, Set};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use simd_json::base::{ValueAsArray, ValueAsScalar};
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsScalar};
use toml::Value;

mod entity {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "message_records")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub platform: String,
        pub group_id: i64,
        pub group_name: String,
        pub guild_id: String,
        pub guild_name: String,
        pub channel_id: String,
        pub channel_name: String,
        pub user_id: i64,
        pub user_name: String,
        pub sender_nick: String,
        pub message_type: String,
        pub sub_type: String,
        pub message_id: i64,
        pub content_raw: String,
        pub content_rich: String,
        pub raw_struct: String,
        pub role: String,
        pub has_image: bool,
        pub has_at: bool,
        pub is_reply: bool,
        pub length: i32,
        pub time: i64,
        pub time_hour: i32,
        pub time_weekday: i32,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use entity::ActiveModel as RecordActiveModel;
use entity::Entity as RecordEntity;

#[derive(Serialize, Deserialize)]
struct RecorderConfig {
    enabled: bool,
    #[serde(default = "default_true")]
    record_self: bool,
}

fn default_true() -> bool {
    true
}

pub fn default_config() -> Value {
    build_config(RecorderConfig {
        enabled: true,
        record_self: true,
    })
}

pub fn init(ctx: Context) -> BoxFuture<'static, Result<(), PluginError>> {
    Box::pin(async move {
        let db = &ctx.db;
        let builder = db.get_database_backend();
        let schema = Schema::new(builder);

        let mut create_table_stmt = schema.create_table_from_entity(RecordEntity);
        create_table_stmt.if_not_exists();

        let stmt = builder.build(&create_table_stmt);
        db.execute(stmt)
            .await
            .map_err(|e| format!("Recorder Plugin DB Init Error: {}", e))?;

        Ok(())
    })
}

pub fn handle(
    ctx: Context,
    _writer: LockedWriter,
) -> BoxFuture<'static, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let config: RecorderConfig = get_config(&ctx, "recorder").unwrap_or(RecorderConfig {
            enabled: true,
            record_self: true,
        });

        if !config.enabled {
            return Ok(Some(ctx));
        }

        let mut record = RecordActiveModel {
            platform: Set("qq".to_string()),
            ..Default::default()
        };

        let should_insert = match &ctx.event {
            // === 接收消息 ===
            EventType::Onebot(ev) => {
                let post_type = ev.get_str("post_type").unwrap_or("");
                if post_type != "message" {
                    return Ok(Some(ctx));
                }

                // 基础信息
                record.time = Set(ev
                    .get_i64("time")
                    .or_else(|| ev.get_u64("time").map(|v| v as i64))
                    .unwrap_or(0));
                record.message_type =
                    Set(ev.get_str("message_type").unwrap_or("unknown").to_string());
                record.sub_type = Set(ev.get_str("sub_type").unwrap_or("normal").to_string());
                record.message_id = Set(ev
                    .get_i64("message_id")
                    .or_else(|| ev.get_u64("message_id").map(|v| v as i64))
                    .unwrap_or(0));

                // 群组与用户
                record.group_id = Set(ev
                    .get_i64("group_id")
                    .or_else(|| ev.get_u64("group_id").map(|v| v as i64))
                    .unwrap_or(0));
                record.user_id = Set(ev
                    .get_i64("user_id")
                    .or_else(|| ev.get_u64("user_id").map(|v| v as i64))
                    .unwrap_or(0));
                record.group_name = Set(ev.get_str("group_name").unwrap_or("").to_string());

                // 发送者详细信息
                if let Some(sender) = ev.get("sender") {
                    let nick = sender.get_str("nickname").unwrap_or("");
                    let card = sender.get_str("card").unwrap_or("");
                    record.sender_nick = Set(nick.to_string());
                    record.user_name = Set(if !card.is_empty() {
                        card.to_string()
                    } else {
                        nick.to_string()
                    });
                    record.role = Set(sender.get_str("role").unwrap_or("member").to_string());
                }

                // 扩展信息 (频道等)
                if let Some(raw) = ev.get("raw") {
                    record.guild_id = Set(raw.get_str("guildId").unwrap_or("").to_string());
                    record.channel_id = Set(raw.get_str("channelId").unwrap_or("").to_string());
                    record.guild_name = Set(raw.get_str("guildName").unwrap_or("").to_string());
                    record.channel_name = Set(raw.get_str("channelName").unwrap_or("").to_string());
                }

                // 消息内容
                record.content_raw = Set(ev.get_str("raw_message").unwrap_or("").to_string());
                record.raw_struct = Set(simd_json::to_string(ev).unwrap_or_default());

                // 解析富文本结构
                parse_message_content(ev.get("message"), &mut record);

                true
            }
            // === 发送消息 (Bot 自身) ===
            EventType::BeforeSend(packet) => {
                if !config.record_self {
                    return Ok(Some(ctx));
                }

                let now = Local::now().timestamp();
                record.time = Set(now);
                record.message_id = Set(0);

                let params = &packet.params;
                record.message_type = Set(params
                    .get_str("message_type")
                    .unwrap_or("unknown")
                    .to_string());
                record.sub_type = Set("normal".to_string());
                record.group_id = Set(params
                    .get_i64("group_id")
                    .or_else(|| params.get_u64("group_id").map(|v| v as i64))
                    .unwrap_or(0));

                // 填充 Bot 自身信息
                if let Ok(uid) = ctx.bot.bot.id.parse::<i64>() {
                    record.user_id = Set(uid);
                }
                record.user_name = Set(ctx.bot.bot.name.clone().unwrap_or_default());
                record.sender_nick = Set(ctx.bot.bot.nick.clone().unwrap_or_default());
                record.role = Set("self".to_string());

                // 记录原始请求
                record.raw_struct = Set(simd_json::to_string(params).unwrap_or_default());

                // 解析消息内容
                parse_message_content(params.get("message"), &mut record);

                // 如果 content_raw 为空（通常发送时未直接提供 raw_message），则使用解析出的富文本填充
                let is_raw_empty = match &record.content_raw {
                    ActiveValue::Set(s) | ActiveValue::Unchanged(s) => s.is_empty(),
                    _ => true,
                };

                if is_raw_empty && let ActiveValue::Set(rich) = &record.content_rich {
                    record.content_raw = Set(rich.clone());
                }

                true
            }
            _ => false,
        };

        if should_insert {
            // 计算时间衍生字段
            let ts = match record.time {
                ActiveValue::Set(t) | ActiveValue::Unchanged(t) => t,
                _ => 0,
            };

            if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
                record.time_hour = Set(dt.hour() as i32);
                record.time_weekday = Set(dt.weekday().num_days_from_sunday() as i32);
            }

            // 计算长度
            let len = match &record.content_raw {
                ActiveValue::Set(s) | ActiveValue::Unchanged(s) => s.len(),
                _ => 0,
            } as i32;
            record.length = Set(len);

            if let Err(e) = record.insert(&ctx.db).await {
                error!(target: "Plugin/Recorder", "消息记录失败: {}", e);
            }
        }

        Ok(Some(ctx))
    })
}

/// 解析消息段数组，提取富文本摘要及特征标记
fn parse_message_content(msg_val: Option<&OwnedValue>, record: &mut RecordActiveModel) {
    let mut rich_text = String::new();
    let mut plain_text_acc = String::new();
    let mut has_img = false;
    let mut has_at_flag = false;
    let mut is_reply_flag = false;

    if let Some(val) = msg_val {
        // 情况 1: 纯字符串消息
        if let Some(s) = val.as_str() {
            rich_text.push_str(s);
            plain_text_acc.push_str(s);
        }
        // 情况 2: 消息段数组
        else if let Some(arr) = val.as_array() {
            for seg in arr {
                let type_ = seg.get_str("type").unwrap_or("unknown");
                let data = seg.get("data");

                match type_ {
                    "text" => {
                        if let Some(t) = data.and_then(|d| d.get_str("text")) {
                            rich_text.push_str(t);
                            plain_text_acc.push_str(t);
                        }
                    }
                    "at" => {
                        has_at_flag = true;
                        let qq = data
                            .and_then(|d| {
                                d.get_str("qq")
                                    .map(|s| s.to_string())
                                    .or_else(|| d.get_i64("qq").map(|i| i.to_string()))
                                    .or_else(|| d.get_u64("qq").map(|i| i.to_string()))
                            })
                            .unwrap_or_default();
                        rich_text.push_str(&format!("[@param={}]", qq));
                    }
                    "face" => rich_text.push_str("[表情]"),
                    "image" => {
                        has_img = true;
                        rich_text.push_str("[图片]");
                    }
                    "record" => rich_text.push_str("[语音]"),
                    "video" => rich_text.push_str("[视频]"),
                    "reply" => {
                        is_reply_flag = true;
                        rich_text.push_str("[回复]");
                    }
                    "json" => rich_text.push_str("[卡片]"),
                    "file" => rich_text.push_str("[文件]"),
                    other => rich_text.push_str(&format!("[{}]", other)),
                }
            }
        }
    }

    record.content_rich = Set(rich_text);

    // 如果 content_raw 为空，暂时用纯文本累积填充
    let is_raw_empty = match &record.content_raw {
        ActiveValue::Set(s) | ActiveValue::Unchanged(s) => s.is_empty(),
        _ => true,
    };

    if is_raw_empty {
        record.content_raw = Set(plain_text_acc);
    }

    record.has_image = Set(has_img);
    record.has_at = Set(has_at_flag);
    record.is_reply = Set(is_reply_flag);
}
