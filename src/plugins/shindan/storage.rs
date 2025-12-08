use crate::error;
use crate::info;
use crate::plugins::get_data_dir;
use sea_orm::ActiveValue::Set;
use sea_orm::QuerySelect;
use sea_orm::Schema;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, QueryOrder};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::fs;

use super::config::{ShindanDefinition, ShindanList};
use super::entity::{item_stats, user_stats};

// 默认的神断列表内容，当 data/shindan/shindans.toml 不存在时写入
const DEFAULT_SHINDANS: &str = include_str!("../../../res/shindan/shindans.toml");

pub struct Storage {
    shindans: Arc<RwLock<ShindanList>>,
    config_path: Arc<RwLock<PathBuf>>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            shindans: Arc::new(RwLock::new(ShindanList::default())),
            config_path: Arc::new(RwLock::new(PathBuf::new())),
        }
    }

    /// 初始化：确定路径、加载或创建 TOML、建表
    pub async fn init(&self, db: &DatabaseConnection) {
        // 1. 获取插件数据目录 data/shindan
        let data_dir = match get_data_dir("shindan").await {
            Ok(p) => p,
            Err(e) => {
                error!(target: "Shindan", "获取数据目录失败: {}", e);
                PathBuf::from("data/shindan")
            }
        };

        // 确保目录存在
        if !data_dir.exists() {
            let _ = fs::create_dir_all(&data_dir).await;
        }

        let toml_path = data_dir.join("shindans.toml");

        // 更新 stored path
        {
            let mut guard = self.config_path.write().unwrap();
            *guard = toml_path.clone();
        }

        // 2. 如果文件不存在，写入默认内容
        if !toml_path.exists() {
            if let Err(e) = fs::write(&toml_path, DEFAULT_SHINDANS).await {
                error!(target: "Shindan", "创建默认 shindans.toml 失败: {}", e);
            } else {
                info!(target: "Shindan", "已创建默认神断列表: {:?}", toml_path);
            }
        }

        // 3. 加载 shindans.toml
        match fs::read_to_string(&toml_path).await {
            Ok(content) => match toml::from_str::<ShindanList>(&content) {
                Ok(list) => {
                    let mut guard = self.shindans.write().unwrap();
                    *guard = list;
                    info!(target: "Shindan", "已加载 {} 个神断定义", guard.shindan.len());
                }
                Err(e) => error!(target: "Shindan", "解析 shindans.toml 失败: {}", e),
            },
            Err(e) => error!(target: "Shindan", "读取 shindans.toml 失败: {}", e),
        }

        // 4. 初始化数据库表
        let builder = db.get_database_backend();
        let schema = Schema::new(builder);

        if let Err(e) = db
            .execute(
                builder.build(
                    schema
                        .create_table_from_entity(user_stats::Entity)
                        .if_not_exists(),
                ),
            )
            .await
        {
            error!(target: "Shindan", "初始化 user_stats 表失败: {}", e);
        }

        if let Err(e) = db
            .execute(
                builder.build(
                    schema
                        .create_table_from_entity(item_stats::Entity)
                        .if_not_exists(),
                ),
            )
            .await
        {
            error!(target: "Shindan", "初始化 item_stats 表失败: {}", e);
        }

        // 创建索引
        let idx_user = sea_orm::sea_query::Index::create()
            .name("idx_shindan_user_count")
            .table(user_stats::Entity)
            .col(user_stats::Column::Count)
            .if_not_exists()
            .to_owned();
        if let Err(e) = db.execute(builder.build(&idx_user)).await {
            error!(target: "Shindan", "创建索引 idx_shindan_user_count 失败: {}", e);
        }

        let idx_item = sea_orm::sea_query::Index::create()
            .name("idx_shindan_item_count")
            .table(item_stats::Entity)
            .col(item_stats::Column::Count)
            .if_not_exists()
            .to_owned();
        if let Err(e) = db.execute(builder.build(&idx_item)).await {
            error!(target: "Shindan", "创建索引 idx_shindan_item_count 失败: {}", e);
        }
    }

    async fn save_toml_internal(&self, list: &ShindanList) {
        let path = {
            let guard = self.config_path.read().unwrap();
            guard.clone()
        };
        // 如果未初始化（path为空），则不保存
        if path.as_os_str().is_empty() {
            return;
        }

        match toml::to_string_pretty(list) {
            Ok(s) => {
                if let Err(e) = fs::write(&path, s).await {
                    error!(target: "Shindan", "保存 shindans.toml 失败: {}", e);
                }
            }
            Err(e) => error!(target: "Shindan", "序列化 shindans.toml 失败: {}", e),
        }
    }

    /// 获取内存中的神断列表
    pub fn get_shindans(&self) -> Vec<ShindanDefinition> {
        self.shindans.read().unwrap().shindan.clone()
    }

    /// 添加神断并保存
    pub async fn add_shindan(&self, s: ShindanDefinition) {
        let list_to_save = {
            let mut guard = self.shindans.write().unwrap();
            guard.shindan.push(s);
            // 按命令排序
            guard.shindan.sort_by(|a, b| a.command.cmp(&b.command));
            guard.clone()
        };
        self.save_toml_internal(&list_to_save).await;
    }

    /// 删除神断并保存
    pub async fn remove_shindan(&self, command: &str) -> Option<ShindanDefinition> {
        let (removed, list_to_save) = {
            let mut guard = self.shindans.write().unwrap();
            let mut removed_item = None;

            if let Some(idx) = guard.shindan.iter().position(|s| s.command == command) {
                removed_item = Some(guard.shindan.remove(idx));
            }

            (removed_item, guard.clone())
        };

        if removed.is_some() {
            self.save_toml_internal(&list_to_save).await;
        }
        removed
    }

    /// 更新模式
    pub async fn update_mode(&self, command: &str, mode: &str) -> bool {
        let list_to_save = {
            let mut guard = self.shindans.write().unwrap();
            if let Some(s) = guard.shindan.iter_mut().find(|s| s.command == command) {
                s.mode = mode.to_string();
                Some(guard.clone())
            } else {
                None
            }
        };

        if let Some(list) = list_to_save {
            self.save_toml_internal(&list).await;
            true
        } else {
            false
        }
    }

    /// 修改命令
    pub async fn update_command(&self, old_cmd: &str, new_cmd: &str) -> bool {
        let list_to_save = {
            let mut guard = self.shindans.write().unwrap();
            if let Some(s) = guard.shindan.iter_mut().find(|s| s.command == old_cmd) {
                s.command = new_cmd.to_string();
                Some(guard.clone())
            } else {
                None
            }
        };

        if let Some(list) = list_to_save {
            self.save_toml_internal(&list).await;
            true
        } else {
            false
        }
    }

    // --- DB Operations ---

    pub async fn record_usage(
        &self,
        db: &DatabaseConnection,
        user_id: i64,
        user_name: &str,
        shindan_id: &str,
    ) {
        // Upsert User: 如果不存在则插入1，如果存在(冲突)则 count+1，并更新 name
        let user_active = user_stats::ActiveModel {
            user_id: Set(user_id),
            name: Set(user_name.to_string()),
            count: Set(1),
        };

        let user_upsert = user_stats::Entity::insert(user_active).on_conflict(
            OnConflict::column(user_stats::Column::UserId)
                .update_column(user_stats::Column::Name)
                .value(
                    user_stats::Column::Count,
                    Expr::col(user_stats::Column::Count).add(1),
                )
                .to_owned(),
        );

        if let Err(e) = user_upsert.exec(db).await {
            error!(target: "Shindan", "保存用户统计失败: {}", e);
        }

        // Upsert Item: 如果不存在则插入1，如果存在(冲突)则 count+1
        let item_active = item_stats::ActiveModel {
            shindan_id: Set(shindan_id.to_string()),
            count: Set(1),
        };

        let item_upsert = item_stats::Entity::insert(item_active).on_conflict(
            OnConflict::column(item_stats::Column::ShindanId)
                .value(
                    item_stats::Column::Count,
                    Expr::col(item_stats::Column::Count).add(1),
                )
                .to_owned(),
        );

        if let Err(e) = item_upsert.exec(db).await {
            error!(target: "Shindan", "保存神断统计失败: {}", e);
        }
    }

    pub async fn get_user_ranking(
        &self,
        db: &DatabaseConnection,
        limit: u64,
    ) -> Vec<user_stats::Model> {
        user_stats::Entity::find()
            .order_by_desc(user_stats::Column::Count)
            .limit(limit)
            .all(db)
            .await
            .unwrap_or_default()
    }

    pub async fn get_item_ranking(
        &self,
        db: &DatabaseConnection,
        limit: u64,
    ) -> Vec<item_stats::Model> {
        item_stats::Entity::find()
            .order_by_desc(item_stats::Column::Count)
            .limit(limit)
            .all(db)
            .await
            .unwrap_or_default()
    }

    pub async fn get_user_count(&self, db: &DatabaseConnection, user_id: i64) -> i32 {
        user_stats::Entity::find_by_id(user_id)
            .one(db)
            .await
            .unwrap_or(None)
            .map(|u| u.count)
            .unwrap_or(0)
    }
}
