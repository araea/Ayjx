use crate::bot::{WsWriter, send_msg};
use crate::event::{Context, EventType};
use crate::message::Message;
use crate::plugins::{PluginError, build_config, get_prefix};
use futures_util::future::BoxFuture;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use simd_json::derived::ValueObjectAccessAsScalar;
use toml::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PingConfig {
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

pub fn default_config() -> Value {
    build_config(PingConfig { enabled: true })
}

/// 初始化钩子：只在启动时执行一次，用于建表
pub fn init(ctx: Context) -> BoxFuture<'static, Result<(), PluginError>> {
    Box::pin(async move {
        let db = &ctx.db;
        let create_table_sql = r#"
            CREATE TABLE IF NOT EXISTS plugin_ping_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
        "#;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            create_table_sql.to_owned(),
        ))
        .await
        .map_err(|e| format!("PingPong Plugin Init DB Error: {}", e))?;

        Ok(())
    })
}

pub fn handle<'a>(
    ctx: Context,
    writer: &'a mut WsWriter,
) -> BoxFuture<'a, Result<Option<Context>, PluginError>> {
    Box::pin(async move {
        let event = match &ctx.event {
            EventType::Onebot(e) => e,
            _ => return Ok(Some(ctx)),
        };

        if let Some("message") = event.get_str("post_type")
            && let Some(raw_msg) = event.get_str("raw_message")
        {
            let prefix = get_prefix(&ctx);
            let target_cmd = format!("{}ping", prefix);

            if raw_msg == target_cmd {
                println!("-> [Plugin] 收到 ping");
                let db = &ctx.db;

                let user_id = event.get_i64("user_id").unwrap_or(0);
                let group_id = event.get_i64("group_id");
                let message_id = event.get_i64("message_id").unwrap_or(0);

                // 插入数据
                let insert_sql = format!(
                    "INSERT INTO plugin_ping_stats (user_id) VALUES ({})",
                    user_id
                );
                db.execute(Statement::from_string(DbBackend::Sqlite, insert_sql))
                    .await
                    .map_err(|e| format!("DB Insert Error: {}", e))?;

                // 查询统计
                let count_sql = "SELECT COUNT(*) as count FROM plugin_ping_stats";
                let query_res = db
                    .query_one(Statement::from_string(
                        DbBackend::Sqlite,
                        count_sql.to_owned(),
                    ))
                    .await
                    .map_err(|e| format!("DB Query Error: {}", e))?;

                let count: i64 = match query_res {
                    Some(res) => res.try_get("", "count").unwrap_or(0),
                    None => 0,
                };

                let reply_msg = Message::new()
                    .reply(message_id)
                    .text(format!(" Pong! 全服累计 Ping 次数: {}", count));

                send_msg(&ctx, writer, group_id, Some(user_id), reply_msg).await?;
                return Ok(None);
            }
        }

        Ok(Some(ctx))
    })
}
