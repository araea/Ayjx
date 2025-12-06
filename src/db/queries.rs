use crate::plugins::recorder::entity::{self, Entity as MessageLogs};
use sea_orm::sea_query::{Alias, Expr};
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect,
};

// ================= 数据结构 =================

/// 纯文本数据（用于生成词云）
#[derive(Debug, FromQueryResult)]
pub struct TextData {
    pub content_text: String,
}

/// 用户活跃排行（龙王榜）
#[derive(Debug, FromQueryResult)]
pub struct UserRanking {
    pub user_id: i64,
    pub nickname: String, // 优先使用群名片，无名片则使用昵称
    pub count: i64,
}

/// 每日消息量走势
#[derive(Debug, FromQueryResult)]
pub struct DailyTrend {
    pub date: String, // 格式 YYYY-MM-DD
    pub count: i64,
}

/// 小时活跃分布
#[derive(Debug, FromQueryResult)]
pub struct HourlyActivity {
    pub hour: i32, // 0-23
    pub count: i64,
}

/// 星期活跃分布
#[derive(Debug, FromQueryResult)]
pub struct WeekdayActivity {
    pub weekday: i32, // 0=Sunday, 1=Monday ... 6=Saturday
    pub count: i64,
}

/// 星期×小时 热力分布数据
#[derive(Debug, FromQueryResult)]
pub struct HeatmapData {
    pub weekday: i32,
    pub hour: i32,
    pub count: i64,
}

/// 消息类型统计
#[derive(Debug, FromQueryResult)]
pub struct MessageTypeStats {
    pub total: i64,
    pub image: i64,
    pub record: i64,
    pub video: i64,
    pub at: i64,
    pub reply: i64,
    pub face: i64,
}

// ================= 查询函数 =================

/// 获取指定时间范围内的纯文本内容列表
///
/// 适用场景：生成群聊或个人的词云
/// - `guild_id`: 指定群号（可选）
/// - `user_id`: 指定用户号（可选）
pub async fn get_text_corpus(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    user_id: Option<i64>,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<String>, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column_as(entity::Column::Tokens, "content_text")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time))
        // 过滤掉空文本
        .filter(entity::Column::ContentText.ne(""));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }
    if let Some(uid) = user_id {
        query = query.filter(entity::Column::UserId.eq(uid));
    }

    // 限制最大返回条数，防止数据量过大导致卡死
    let results: Vec<TextData> = query.limit(50000).into_model().all(db).await?;
    Ok(results.into_iter().map(|d| d.content_text).collect())
}

/// 获取活跃用户排行（龙王榜）
///
/// 适用场景：今日龙王榜、昨日活跃排名、月度活跃榜
pub async fn get_user_ranking(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    start_time: i64,
    end_time: i64,
    limit: u64,
) -> Result<Vec<UserRanking>, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column(entity::Column::UserId)
        // 取最新的一条昵称作为显示名 (MAX(sender_nick))
        .column_as(Expr::col(entity::Column::SenderNick).max(), "nickname")
        .column_as(Expr::col(entity::Column::Id).count(), "count")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }

    query
        .group_by(entity::Column::UserId)
        .order_by_desc(Expr::custom_keyword(Alias::new("count")))
        .limit(limit)
        .into_model::<UserRanking>()
        .all(db)
        .await
}

/// 获取每日消息量走势
///
/// 适用场景：近7天/30天消息量走势
pub async fn get_daily_trend(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    user_id: Option<i64>,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<DailyTrend>, DbErr> {
    // SQLite 转换时间戳为日期字符串: strftime('%Y-%m-%d', datetime(time, 'unixepoch', 'localtime'))
    let date_expr = Expr::cust("strftime('%Y-%m-%d', datetime(time, 'unixepoch', 'localtime'))");

    let mut query = MessageLogs::find()
        .select_only()
        .column_as(date_expr.clone(), "date")
        .column_as(Expr::col(entity::Column::Id).count(), "count")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }
    if let Some(uid) = user_id {
        query = query.filter(entity::Column::UserId.eq(uid));
    }

    query
        .group_by(date_expr)
        .order_by_asc(Expr::custom_keyword(Alias::new("date")))
        .into_model::<DailyTrend>()
        .all(db)
        .await
}

/// 获取24小时活跃时段分布
///
/// 适用场景：分析群聊/个人最活跃的时间段
pub async fn get_hourly_activity(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    user_id: Option<i64>,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<HourlyActivity>, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column_as(entity::Column::TimeHour, "hour")
        .column_as(Expr::col(entity::Column::Id).count(), "count")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }
    if let Some(uid) = user_id {
        query = query.filter(entity::Column::UserId.eq(uid));
    }

    query
        .group_by(entity::Column::TimeHour)
        .order_by_asc(entity::Column::TimeHour)
        .into_model::<HourlyActivity>()
        .all(db)
        .await
}

/// 获取星期活跃分布
///
/// 适用场景：分析一周中哪天最活跃
pub async fn get_weekday_activity(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<WeekdayActivity>, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column_as(entity::Column::TimeWeekday, "weekday")
        .column_as(Expr::col(entity::Column::Id).count(), "count")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }

    query
        .group_by(entity::Column::TimeWeekday)
        .order_by_asc(entity::Column::TimeWeekday)
        .into_model::<WeekdayActivity>()
        .all(db)
        .await
}

/// 获取 星期×小时 的热力分布数据
///
/// 适用场景：生成活跃热力图
pub async fn get_heatmap_data(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    start_time: i64,
    end_time: i64,
) -> Result<Vec<HeatmapData>, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column_as(entity::Column::TimeWeekday, "weekday")
        .column_as(entity::Column::TimeHour, "hour")
        .column_as(Expr::col(entity::Column::Id).count(), "count")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }

    query
        .group_by(entity::Column::TimeWeekday)
        .group_by(entity::Column::TimeHour)
        .into_model::<HeatmapData>()
        .all(db)
        .await
}

/// 获取消息类型统计（纯文、图片、语音等比例）
///
/// 适用场景：统计消息成分
pub async fn get_message_type_stats(
    db: &DatabaseConnection,
    guild_id: Option<&str>,
    user_id: Option<i64>,
    start_time: i64,
    end_time: i64,
) -> Result<MessageTypeStats, DbErr> {
    let mut query = MessageLogs::find()
        .select_only()
        .column_as(Expr::col(entity::Column::Id).count(), "total")
        // sum(image_count)
        .column_as(Expr::col(entity::Column::ImageCount).sum(), "image")
        // sum(is_voice) -> sum(cast(is_voice as integer)) if needed, but simple sum usually works for booleans in sqlite
        .column_as(Expr::col(entity::Column::IsVoice).sum(), "record")
        .column_as(Expr::col(entity::Column::IsVideo).sum(), "video")
        .column_as(Expr::col(entity::Column::AtCount).sum(), "at")
        .column_as(Expr::col(entity::Column::IsReply).sum(), "reply")
        .column_as(Expr::col(entity::Column::FaceCount).sum(), "face")
        .filter(entity::Column::Time.gte(start_time))
        .filter(entity::Column::Time.lt(end_time));

    if let Some(gid) = guild_id {
        query = query.filter(entity::Column::GuildId.eq(gid));
    }
    if let Some(uid) = user_id {
        query = query.filter(entity::Column::UserId.eq(uid));
    }

    // into_model expects a Vec, we just need one result
    let result = query.into_model::<MessageTypeStats>().one(db).await?;

    Ok(result.unwrap_or(MessageTypeStats {
        total: 0,
        image: 0,
        record: 0,
        video: 0,
        at: 0,
        reply: 0,
        face: 0,
    }))
}
