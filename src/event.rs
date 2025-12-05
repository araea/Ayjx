use crate::config::AppConfig;
use crate::scheduler::Scheduler;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex as AsyncMutex;

pub type Event = OwnedValue;

/// 统一的上下文，包含事件数据、可变配置和任务调度器
pub struct Context {
    pub event: EventType,
    pub config: Arc<RwLock<AppConfig>>,
    pub config_save_lock: Arc<AsyncMutex<()>>,
    pub db: DatabaseConnection,
    pub scheduler: Arc<Scheduler>,
    pub config_path: String,
}

/// 事件类型
#[derive(Debug)]
pub enum EventType {
    /// 来自 OneBot 的原始事件
    Onebot(Event),
    /// 插件准备发送消息前的拦截事件
    BeforeSend(SendPacket),
    /// 系统初始化事件 (用于插件 on_init 生命周期)
    Init,
}

/// 发送包结构，用于在 BeforeSend 中传递
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SendPacket {
    pub action: String,
    pub params: serde_json::Value,
}
