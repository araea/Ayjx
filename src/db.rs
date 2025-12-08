#![allow(dead_code)]

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbErr, Statement};
use std::path::Path;
use std::time::Duration;
use tokio::fs;

use crate::info;

pub mod queries;
pub mod utils;

/// 初始化数据库连接
pub async fn init() -> Result<DatabaseConnection, DbErr> {
    if !Path::new("data").exists() {
        let _ = fs::create_dir("data").await;
    }

    // mode=rwc 允许 读/写/创建
    let db_url = "sqlite:data/bot.db?mode=rwc";

    // 配置连接池选项
    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8 * 60)); // 设置连接最大生命周期，防止长时间空闲后连接失效

    // 设置日志级别（可选）
    // opt.sqlx_logging(true)
    //    .sqlx_logging_level(log::LevelFilter::Debug);

    let db = Database::connect(opt).await?;

    // 开启 WAL 模式 (Write-Ahead Logging) 以提高并发性能
    let backend = db.get_database_backend();
    db.execute(Statement::from_string(
        backend,
        "PRAGMA journal_mode=WAL;".to_owned(),
    ))
    .await?;

    // 关闭过于严格的安全检查 (Synchronous NORMAL 足够安全且快)
    db.execute(Statement::from_string(
        backend,
        "PRAGMA synchronous=NORMAL;".to_owned(),
    ))
    .await?;

    info!(target: "Database", "连接成功: {} (WAL Mode)", db_url);

    Ok(db)
}
