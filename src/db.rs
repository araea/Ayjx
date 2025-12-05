use sea_orm::{Database, DatabaseConnection, DbErr};
use std::path::Path;
use tokio::fs;

use crate::info;

/// 初始化数据库连接
pub async fn init() -> Result<DatabaseConnection, DbErr> {
    if !Path::new("data").exists() {
        let _ = fs::create_dir("data").await;
    }

    // mode=rwc 允许 读/写/创建
    let db_url = "sqlite:data/bot.db?mode=rwc";

    let db = Database::connect(db_url).await?;

    info!(target: "Database", "连接成功: {}", db_url);

    Ok(db)
}
