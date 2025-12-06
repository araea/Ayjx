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

        pub guild_id: String,
        pub guild_name: String,
        pub channel_id: String,
        pub channel_name: String,

        pub user_id: i64,
        pub user_name: String,   // 对应 nickname (用户本名)
        pub sender_nick: String, // 对应 card (群名片)

        pub message_type: String,
        pub sub_type: String,
        pub message_id: i64,

        pub content_raw: String,  // CQ or Raw
        pub content_rich: String, // 富文本摘要
        pub content_text: String, // 纯文本内容，用于分词分析
        pub raw_message_json: String,

        pub role: String,
        pub is_reply: bool,
        pub length: i32,
        pub time: i64,
        pub time_hour: i32,
        pub time_weekday: i32,

        pub has_image: bool,     // 是否包含图片
        pub image_count: i32,    // 图片数量
        pub is_anim_emoji: bool, // 是否包含动画表情/表情包

        pub has_at: bool,  // 是否包含At
        pub at_count: i32, // At数量

        pub face_count: i32, // 小表情(face/emoji)数量

        pub is_voice: bool, // 是否是语音
        pub is_video: bool, // 是否是视频
        pub is_music: bool, // 是否是音乐分享

        pub is_rps: bool,  // 是否是猜拳
        pub is_dice: bool, // 是否是骰子
        pub is_poke: bool, // 是否是戳一戳

        pub is_forward: bool, // 是否是合并转发

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

        // 1. 创建表
        let mut create_table_stmt = schema.create_table_from_entity(RecordEntity);
        create_table_stmt.if_not_exists();

        let stmt = builder.build(&create_table_stmt);
        db.execute(stmt)
            .await
            .map_err(|e| format!("Recorder Plugin DB Init Error: {}", e))?;

        // 2. 创建索引 (为了应对高频分析查询)
        // 索引定义：(列名...)
        let indexes = vec![
            // 场景：生成群聊词云、活跃榜、时间段分布
            // 查询条件通常是：WHERE guild_id = ? AND time > ?
            sea_orm::sea_query::Index::create()
                .name("idx_records_guild_time")
                .table(RecordEntity)
                .col(entity::Column::GuildId)
                .col(entity::Column::Time)
                .if_not_exists()
                .to_owned(),
            // 场景：生成个人词云、查看个人历史
            // 查询条件通常是：WHERE user_id = ? AND guild_id = ? AND time > ?
            sea_orm::sea_query::Index::create()
                .name("idx_records_user_guild_time")
                .table(RecordEntity)
                .col(entity::Column::UserId)
                .col(entity::Column::GuildId)
                .col(entity::Column::Time)
                .if_not_exists()
                .to_owned(),
            // 场景：全平台趋势分析、数据清理
            // 查询条件通常是：WHERE time > ?
            sea_orm::sea_query::Index::create()
                .name("idx_records_time")
                .table(RecordEntity)
                .col(entity::Column::Time)
                .if_not_exists()
                .to_owned(),
        ];

        for idx in indexes {
            let stmt = builder.build(&idx);
            // 索引创建失败不应阻断启动（可能是重复创建等问题）
            if let Err(e) = db.execute(stmt).await {
                warn!(target: "Plugin/Recorder", "Index creation warning: {}", e);
            }
        }

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

        let mut record = RecordActiveModel {
            platform: Set("qq".to_string()),
            // 初始化新增字段
            content_text: Set("".to_string()),
            ..Default::default()
        };

        // 用于接收计算出的纯文本长度
        let mut text_len = 0;

        let should_insert = match &ctx.event {
            // === 接收消息 ===
            EventType::Onebot(ev) => {
                let post_type = ev.get_str("post_type").unwrap_or("");
                if post_type != "message" {
                    return Ok(Some(ctx));
                }

                // 1. 基础信息
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

                // 2. 群组/频道映射 (Group -> Guild/Channel)
                let group_id = ev
                    .get_i64("group_id")
                    .or_else(|| ev.get_u64("group_id").map(|v| v as i64))
                    .unwrap_or(0);
                let group_name = ev.get_str("group_name").unwrap_or("");

                if group_id != 0 {
                    record.guild_id = Set(group_id.to_string());
                    record.channel_id = Set(group_id.to_string());
                    record.guild_name = Set(group_name.to_string());
                    record.channel_name = Set(group_name.to_string());
                } else {
                    record.guild_id = Set("".to_string());
                    record.channel_id = Set("".to_string());
                    record.guild_name = Set("".to_string());
                    record.channel_name = Set("".to_string());
                }

                // 3. 用户信息
                record.user_id = Set(ev
                    .get_i64("user_id")
                    .or_else(|| ev.get_u64("user_id").map(|v| v as i64))
                    .unwrap_or(0));

                if let Some(sender) = ev.get("sender") {
                    let nick = sender.get_str("nickname").unwrap_or("");
                    let card = sender.get_str("card").unwrap_or("");

                    record.user_name = Set(nick.to_string());
                    record.sender_nick = Set(if !card.is_empty() {
                        card.to_string()
                    } else {
                        nick.to_string()
                    });

                    record.role = Set(sender.get_str("role").unwrap_or("member").to_string());
                }

                // 4. 消息内容
                record.content_raw = Set(ev.get_str("raw_message_json").unwrap_or("").to_string());

                let msg_val = ev.get("message");
                let raw_json = if let Some(v) = msg_val {
                    simd_json::to_string(v).unwrap_or_else(|_| "null".to_string())
                } else {
                    "null".to_string()
                };
                record.raw_message_json = Set(raw_json);

                // 解析富文本，并获取纯文本长度，同时填充 content_rich 和 content_text
                text_len = parse_message_content(msg_val, &mut record);

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
                record.message_type = Set(packet.message_type().unwrap_or("unknown").to_string());
                record.sub_type = Set("normal".to_string());

                let group_id = packet.group_id().unwrap_or(0);

                if group_id != 0 {
                    record.guild_id = Set(group_id.to_string());
                    record.channel_id = Set(group_id.to_string());

                    if let Some(origin) = &packet.original_event {
                        let origin_gid = origin
                            .get_i64("group_id")
                            .or_else(|| origin.get_u64("group_id").map(|v| v as i64))
                            .unwrap_or(0);

                        if origin_gid != 0 && origin_gid == group_id {
                            let g_name = origin.get_str("group_name").unwrap_or("").to_string();
                            record.guild_name = Set(g_name.clone());
                            record.channel_name = Set(g_name);

                            if let Some(raw) = origin.get("raw") {
                                if let Some(gn) = raw.get_str("guildName") {
                                    record.guild_name = Set(gn.to_string());
                                }
                                if let Some(cn) = raw.get_str("channelName") {
                                    record.channel_name = Set(cn.to_string());
                                }
                            }
                        } else {
                            record.guild_name = Set("".to_string());
                            record.channel_name = Set("".to_string());
                        }
                    } else {
                        record.guild_name = Set("".to_string());
                        record.channel_name = Set("".to_string());
                    }
                }

                if let Ok(uid) = ctx.bot.login_user.id.parse::<i64>() {
                    record.user_id = Set(uid);
                }
                record.user_name = Set(ctx.bot.login_user.name.clone().unwrap_or_default());
                record.sender_nick = Set(ctx
                    .bot
                    .login_user
                    .nick
                    .clone()
                    .or(ctx.bot.login_user.name.clone())
                    .unwrap_or_default());
                record.role = Set("self".to_string());

                let msg_val = packet.message();
                let raw_json = if let Some(v) = msg_val {
                    simd_json::to_string(v).unwrap_or_else(|_| "null".to_string())
                } else {
                    "null".to_string()
                };
                record.raw_message_json = Set(raw_json);

                // 解析富文本，并获取纯文本长度，同时填充 content_rich 和 content_text
                text_len = parse_message_content(msg_val, &mut record);

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

            // 计算长度 (仅统计文本消息的字符数)
            record.length = Set(text_len);

            if let Err(e) = record.insert(&ctx.db).await {
                error!(target: "Plugin/Recorder", "消息记录失败: {}", e);
            }
        }

        Ok(Some(ctx))
    })
}

/// 解析消息段数组，提取富文本摘要、特征标记以及纯文本拼接，并返回纯文本字符个数
fn parse_message_content(msg_val: Option<&OwnedValue>, record: &mut RecordActiveModel) -> i32 {
    let mut rich_text = String::new();
    let mut plain_text_acc = String::new();
    // 存储分段的纯文本，用于最终拼接 content_text
    let mut text_segments: Vec<String> = Vec::new();
    let mut text_char_count = 0;

    // 统计变量
    let mut image_count = 0;
    let mut at_count = 0;
    let mut face_count = 0;

    // 标记变量
    let mut is_anim_emoji = false;
    let mut is_voice = false;
    let mut is_video = false;
    let mut is_music = false;
    let mut is_rps = false;
    let mut is_dice = false;
    let mut is_poke = false;
    let mut is_forward = false;
    let mut is_reply_flag = false;

    if let Some(val) = msg_val {
        // 情况 1: 纯字符串消息
        if let Some(s) = val.as_str() {
            rich_text.push_str(s);
            plain_text_acc.push_str(s);
            text_segments.push(s.trim().to_string());
            text_char_count += s.chars().count();
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
                            // 将文本段存入 vec，后续用空格拼接
                            let trimmed = t.trim();
                            if !trimmed.is_empty() {
                                text_segments.push(trimmed.to_string());
                            }
                            text_char_count += t.chars().count();
                        }
                    }
                    "at" => {
                        at_count += 1;
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
                    "face" => {
                        face_count += 1;
                        rich_text.push_str("[表情]");
                    }
                    "image" => {
                        image_count += 1;
                        // 检查是否为动画表情
                        if let Some(d) = data {
                            let summary = d.get_str("summary").unwrap_or("");
                            let sub_type = d
                                .get_i64("sub_type")
                                .or_else(|| d.get_u64("sub_type").map(|v| v as i64))
                                .unwrap_or(0);
                            if summary == "[动画表情]" || sub_type == 1 {
                                is_anim_emoji = true;
                            }
                        }
                        rich_text.push_str("[图片]");
                    }
                    "record" => {
                        is_voice = true;
                        rich_text.push_str("[语音]");
                    }
                    "video" => {
                        is_video = true;
                        rich_text.push_str("[视频]");
                    }
                    "music" => {
                        is_music = true;
                        rich_text.push_str("[音乐]");
                    }
                    "poke" => {
                        is_poke = true;
                        rich_text.push_str("[戳一戳]");
                    }
                    "rps" => {
                        is_rps = true;
                        rich_text.push_str("[猜拳]");
                    }
                    "dice" => {
                        is_dice = true;
                        rich_text.push_str("[骰子]");
                    }
                    "forward" | "node" => {
                        is_forward = true;
                        rich_text.push_str("[合并转发]");
                    }
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
    // 将所有文本段用空格拼接，存入 content_text
    record.content_text = Set(text_segments.join(" "));

    let is_raw_empty = match &record.content_raw {
        ActiveValue::Set(s) | ActiveValue::Unchanged(s) => s.is_empty(),
        _ => true,
    };

    if is_raw_empty {
        record.content_raw = Set(plain_text_acc);
    }

    record.has_image = Set(image_count > 0);
    record.has_at = Set(at_count > 0);
    record.is_reply = Set(is_reply_flag);
    record.image_count = Set(image_count);
    record.is_anim_emoji = Set(is_anim_emoji);
    record.at_count = Set(at_count);
    record.face_count = Set(face_count);
    record.is_voice = Set(is_voice);
    record.is_video = Set(is_video);
    record.is_music = Set(is_music);
    record.is_rps = Set(is_rps);
    record.is_dice = Set(is_dice);
    record.is_poke = Set(is_poke);
    record.is_forward = Set(is_forward);

    text_char_count as i32
}
