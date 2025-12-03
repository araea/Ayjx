// lib.rs
//
// ================================================================================
// Ayjx Framework Core - 安、易、简、行
// Copyright (c) 2025-Present Ayjx Team
//
// 理念：Ayjx —— 安于心，简于行。
// 架构：Satori 协议抽象 | 插件化系统 | 静态编译 | 原子配置
// ================================================================================

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::too_many_arguments)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};

// ============================================================================
// 1. Error Types (统一错误处理)
// ============================================================================

/// 框架核心错误类型
pub type AyjxError = Box<dyn std::error::Error + Send + Sync>;

pub type AyjxResult<T> = Result<T, AyjxError>;

// ============================================================================
// 2. Satori Protocol Data Models (完整实现)
// 基于 satori-doc 文档实现所有核心资源类型
// ============================================================================

// ----------------------------------------------------------------------------
// 2.1 用户 (User)
// ----------------------------------------------------------------------------

/// 用户对象
/// 参考: zh-CN/resources/user.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct User {
    /// 用户 ID
    pub id: String,
    /// 用户名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 用户昵称（优先级高于 name）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<String>,
    /// 用户头像链接
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 是否为机器人
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_bot: Option<bool>,
}

impl User {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Default::default()
        }
    }

    /// 获取显示名称（优先 nick，其次 name，最后 id）
    pub fn display_name(&self) -> &str {
        self.nick
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or(&self.id)
    }
}

// ----------------------------------------------------------------------------
// 2.2 群组 (Guild)
// ----------------------------------------------------------------------------

/// 群组对象
/// 参考: zh-CN/resources/guild.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Guild {
    /// 群组 ID
    pub id: String,
    /// 群组名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 群组头像
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

impl Guild {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Default::default()
        }
    }
}

// ----------------------------------------------------------------------------
// 2.3 频道 (Channel)
// ----------------------------------------------------------------------------

/// 频道类型
/// 参考: zh-CN/resources/channel.md
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ChannelType {
    /// 文本频道
    #[default]
    #[serde(rename = "0")]
    Text = 0,
    /// 私聊频道
    #[serde(rename = "1")]
    Direct = 1,
    /// 分类频道
    #[serde(rename = "2")]
    Category = 2,
    /// 语音频道
    #[serde(rename = "3")]
    Voice = 3,
}

/// 频道对象
/// 参考: zh-CN/resources/channel.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Channel {
    /// 频道 ID
    pub id: String,
    /// 频道类型
    #[serde(rename = "type", default)]
    pub channel_type: ChannelType,
    /// 频道名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 父频道 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

impl Channel {
    pub fn new(id: impl Into<String>, channel_type: ChannelType) -> Self {
        Self {
            id: id.into(),
            channel_type,
            ..Default::default()
        }
    }

    /// 是否为私聊频道
    pub fn is_direct(&self) -> bool {
        self.channel_type == ChannelType::Direct
    }

    /// 是否为文本频道
    pub fn is_text(&self) -> bool {
        self.channel_type == ChannelType::Text
    }
}

// ----------------------------------------------------------------------------
// 2.4 群组成员 (GuildMember)
// ----------------------------------------------------------------------------

/// 群组成员对象
/// 参考: zh-CN/resources/member.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuildMember {
    /// 用户对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// 用户在群组中的名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<String>,
    /// 用户在群组中的头像
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 加入时间（毫秒时间戳）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<i64>,
}

impl GuildMember {
    /// 获取成员显示名称
    pub fn display_name(&self) -> Option<&str> {
        self.nick
            .as_deref()
            .or_else(|| self.user.as_ref().map(|u| u.display_name()))
    }
}

// ----------------------------------------------------------------------------
// 2.5 群组角色 (GuildRole)
// ----------------------------------------------------------------------------

/// 群组角色对象
/// 参考: zh-CN/resources/role.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuildRole {
    /// 角色 ID
    pub id: String,
    /// 角色名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl GuildRole {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
        }
    }
}

// ----------------------------------------------------------------------------
// 2.6 消息 (Message)
// ----------------------------------------------------------------------------

/// 消息对象
/// 参考: zh-CN/resources/message.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    /// 消息 ID
    pub id: String,
    /// 消息内容（使用 Satori 消息元素编码）
    pub content: String,
    /// 频道对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
    /// 群组对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild: Option<Guild>,
    /// 群组成员对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<GuildMember>,
    /// 用户对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// 消息发送的时间戳（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    /// 消息修改的时间戳（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

impl Message {
    pub fn new(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            ..Default::default()
        }
    }

    /// 获取发送者 ID
    pub fn sender_id(&self) -> Option<&str> {
        self.user.as_ref().map(|u| u.id.as_str())
    }

    /// 获取频道 ID
    pub fn channel_id(&self) -> Option<&str> {
        self.channel.as_ref().map(|c| c.id.as_str())
    }

    /// 获取群组 ID
    pub fn guild_id(&self) -> Option<&str> {
        self.guild.as_ref().map(|g| g.id.as_str())
    }

    /// 是否为私聊消息
    pub fn is_direct(&self) -> bool {
        self.channel
            .as_ref()
            .map(|c| c.is_direct())
            .unwrap_or(false)
    }
}

// ----------------------------------------------------------------------------
// 2.7 登录信息 (Login)
// ----------------------------------------------------------------------------

/// 登录状态
/// 参考: zh-CN/resources/login.md
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum LoginStatus {
    /// 离线
    #[default]
    #[serde(rename = "0")]
    Offline = 0,
    /// 在线
    #[serde(rename = "1")]
    Online = 1,
    /// 正在连接
    #[serde(rename = "2")]
    Connect = 2,
    /// 正在断开连接
    #[serde(rename = "3")]
    Disconnect = 3,
    /// 正在重新连接
    #[serde(rename = "4")]
    Reconnect = 4,
}

impl LoginStatus {
    /// 是否处于在线状态
    pub fn is_online(&self) -> bool {
        matches!(self, Self::Online)
    }

    /// 是否处于连接中状态
    pub fn is_connecting(&self) -> bool {
        matches!(self, Self::Connect | Self::Reconnect)
    }
}

/// 登录信息对象
/// 参考: zh-CN/resources/login.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Login {
    /// 序列号（仅用于标识）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sn: Option<i64>,
    /// 平台名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// 用户对象
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// 登录状态
    #[serde(default)]
    pub status: LoginStatus,
    /// 适配器名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    /// 平台特性列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
}

impl Login {
    pub fn new(platform: impl Into<String>, adapter: impl Into<String>) -> Self {
        Self {
            platform: Some(platform.into()),
            adapter: Some(adapter.into()),
            status: LoginStatus::Offline,
            ..Default::default()
        }
    }

    /// 设置在线状态
    pub fn set_online(&mut self, user: User) {
        self.user = Some(user);
        self.status = LoginStatus::Online;
    }

    /// 设置离线状态
    pub fn set_offline(&mut self) {
        self.status = LoginStatus::Offline;
    }
}

// ----------------------------------------------------------------------------
// 2.8 交互 (Interaction)
// ----------------------------------------------------------------------------

/// 交互指令
/// 参考: zh-CN/resources/interaction.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Argv {
    /// 指令名称
    pub name: String,
    /// 参数列表
    #[serde(default)]
    pub arguments: Vec<serde_json::Value>,
    /// 选项
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
}

/// 交互按钮
/// 参考: zh-CN/resources/interaction.md
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Button {
    /// 按钮 ID
    pub id: String,
}

// ----------------------------------------------------------------------------
// 2.9 分页列表
// ----------------------------------------------------------------------------

/// 分页列表
/// 参考: zh-CN/protocol/api.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagedList<T> {
    /// 数据列表
    pub data: Vec<T>,
    /// 下一页令牌
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl<T> PagedList<T> {
    pub fn new(data: Vec<T>) -> Self {
        Self { data, next: None }
    }

    pub fn with_next(data: Vec<T>, next: impl Into<String>) -> Self {
        Self {
            data,
            next: Some(next.into()),
        }
    }

    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
}

/// 双向分页列表
/// 参考: zh-CN/protocol/api.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidiPagedList<T> {
    /// 数据列表
    pub data: Vec<T>,
    /// 上一页令牌
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    /// 下一页令牌
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

// ----------------------------------------------------------------------------
// 2.10 事件 (Event)
// ----------------------------------------------------------------------------

/// 事件类型常量
pub mod event_types {
    // 消息事件
    pub const MESSAGE_CREATED: &str = "message-created";
    pub const MESSAGE_UPDATED: &str = "message-updated";
    pub const MESSAGE_DELETED: &str = "message-deleted";

    // 群组事件
    pub const GUILD_ADDED: &str = "guild-added";
    pub const GUILD_UPDATED: &str = "guild-updated";
    pub const GUILD_REMOVED: &str = "guild-removed";
    pub const GUILD_REQUEST: &str = "guild-request";

    // 群组成员事件
    pub const GUILD_MEMBER_ADDED: &str = "guild-member-added";
    pub const GUILD_MEMBER_UPDATED: &str = "guild-member-updated";
    pub const GUILD_MEMBER_REMOVED: &str = "guild-member-removed";
    pub const GUILD_MEMBER_REQUEST: &str = "guild-member-request";

    // 群组角色事件
    pub const GUILD_ROLE_CREATED: &str = "guild-role-created";
    pub const GUILD_ROLE_UPDATED: &str = "guild-role-updated";
    pub const GUILD_ROLE_DELETED: &str = "guild-role-deleted";

    // 登录事件
    pub const LOGIN_ADDED: &str = "login-added";
    pub const LOGIN_REMOVED: &str = "login-removed";
    pub const LOGIN_UPDATED: &str = "login-updated";

    // 好友事件
    pub const FRIEND_REQUEST: &str = "friend-request";

    // 表态事件
    pub const REACTION_ADDED: &str = "reaction-added";
    pub const REACTION_REMOVED: &str = "reaction-removed";

    // 交互事件
    pub const INTERACTION_BUTTON: &str = "interaction/button";
    pub const INTERACTION_COMMAND: &str = "interaction/command";

    // 内部事件
    pub const INTERNAL: &str = "internal";
}

/// 核心事件结构
/// 参考: zh-CN/protocol/events.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// 事件序列号
    pub sn: i64,
    /// 事件类型
    #[serde(rename = "type")]
    pub event_type: String,
    /// 事件时间戳（毫秒）
    pub timestamp: i64,
    /// 登录信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<Login>,
    /// 交互指令
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<Argv>,
    /// 交互按钮
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button: Option<Button>,
    /// 频道
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
    /// 群组
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild: Option<Guild>,
    /// 群组成员
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<GuildMember>,
    /// 消息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    /// 操作者
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<User>,
    /// 群组角色
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<GuildRole>,
    /// 目标用户
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// 平台原生事件类型
    #[serde(rename = "_type", skip_serializing_if = "Option::is_none")]
    pub platform_type: Option<String>,
    /// 平台原生事件数据（运行时使用，不序列化）
    #[serde(skip)]
    pub platform_data: Option<Arc<dyn Any + Send + Sync>>,
}

impl Default for Event {
    fn default() -> Self {
        Self {
            sn: 0,
            event_type: String::new(),
            timestamp: chrono_timestamp_millis(),
            login: None,
            argv: None,
            button: None,
            channel: None,
            guild: None,
            member: None,
            message: None,
            operator: None,
            role: None,
            user: None,
            platform_type: None,
            platform_data: None,
        }
    }
}

impl Event {
    /// 创建基础新事件
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            timestamp: chrono_timestamp_millis(),
            ..Default::default()
        }
    }

    // ========================================================================
    // 消息事件 (Message Events)
    // ========================================================================

    /// 创建消息事件通用构建器
    fn build_message_event(event_type: &str, message: Message) -> Self {
        let user = message.user.clone();
        let channel = message.channel.clone();
        let guild = message.guild.clone();
        let member = message.member.clone();

        Self {
            event_type: event_type.to_string(),
            message: Some(message),
            user,
            channel,
            guild,
            member,
            ..Default::default()
        }
    }

    /// 创建消息创建事件
    pub fn message_created(message: Message) -> Self {
        Self::build_message_event(event_types::MESSAGE_CREATED, message)
    }

    /// 创建消息更新事件
    pub fn message_updated(message: Message) -> Self {
        Self::build_message_event(event_types::MESSAGE_UPDATED, message)
    }

    /// 创建消息删除事件
    pub fn message_deleted(message: Message) -> Self {
        Self::build_message_event(event_types::MESSAGE_DELETED, message)
    }

    // ========================================================================
    // 群组事件 (Guild Events)
    // ========================================================================

    /// 创建群组增加事件
    pub fn guild_added(guild: Guild) -> Self {
        Self {
            event_type: event_types::GUILD_ADDED.to_string(),
            guild: Some(guild),
            ..Default::default()
        }
    }

    /// 创建群组更新事件
    pub fn guild_updated(guild: Guild) -> Self {
        Self {
            event_type: event_types::GUILD_UPDATED.to_string(),
            guild: Some(guild),
            ..Default::default()
        }
    }

    /// 创建群组移除事件
    pub fn guild_removed(guild: Guild) -> Self {
        Self {
            event_type: event_types::GUILD_REMOVED.to_string(),
            guild: Some(guild),
            ..Default::default()
        }
    }

    /// 创建群组请求事件
    pub fn guild_request(guild: Guild, user: User) -> Self {
        Self {
            event_type: event_types::GUILD_REQUEST.to_string(),
            guild: Some(guild),
            user: Some(user),
            ..Default::default()
        }
    }

    // ========================================================================
    // 群组成员事件 (Guild Member Events)
    // ========================================================================

    /// 创建成员增加事件
    pub fn guild_member_added(guild: Guild, member: GuildMember) -> Self {
        // 尝试从 member 中提取 user，方便上层调用
        let user = member.user.clone();
        Self {
            event_type: event_types::GUILD_MEMBER_ADDED.to_string(),
            guild: Some(guild),
            member: Some(member),
            user,
            ..Default::default()
        }
    }

    /// 创建成员更新事件
    pub fn guild_member_updated(guild: Guild, member: GuildMember) -> Self {
        let user = member.user.clone();
        Self {
            event_type: event_types::GUILD_MEMBER_UPDATED.to_string(),
            guild: Some(guild),
            member: Some(member),
            user,
            ..Default::default()
        }
    }

    /// 创建成员移除事件
    /// operator: 可选的操作者（例如踢出成员的管理员）
    pub fn guild_member_removed(guild: Guild, user: User, operator: Option<User>) -> Self {
        Self {
            event_type: event_types::GUILD_MEMBER_REMOVED.to_string(),
            guild: Some(guild),
            user: Some(user),
            operator,
            ..Default::default()
        }
    }

    /// 创建成员请求事件
    pub fn guild_member_request(guild: Guild, user: User) -> Self {
        Self {
            event_type: event_types::GUILD_MEMBER_REQUEST.to_string(),
            guild: Some(guild),
            user: Some(user),
            ..Default::default()
        }
    }

    // ========================================================================
    // 群组角色事件 (Guild Role Events)
    // ========================================================================

    /// 创建角色创建事件
    pub fn guild_role_created(guild: Guild, role: GuildRole) -> Self {
        Self {
            event_type: event_types::GUILD_ROLE_CREATED.to_string(),
            guild: Some(guild),
            role: Some(role),
            ..Default::default()
        }
    }

    /// 创建角色更新事件
    pub fn guild_role_updated(guild: Guild, role: GuildRole) -> Self {
        Self {
            event_type: event_types::GUILD_ROLE_UPDATED.to_string(),
            guild: Some(guild),
            role: Some(role),
            ..Default::default()
        }
    }

    /// 创建角色删除事件
    pub fn guild_role_deleted(guild: Guild, role: GuildRole) -> Self {
        Self {
            event_type: event_types::GUILD_ROLE_DELETED.to_string(),
            guild: Some(guild),
            role: Some(role),
            ..Default::default()
        }
    }

    // ========================================================================
    // 登录事件 (Login Events)
    // ========================================================================

    /// 创建登录增加事件
    pub fn login_added(login: Login) -> Self {
        Self {
            event_type: event_types::LOGIN_ADDED.to_string(),
            login: Some(login),
            ..Default::default()
        }
    }

    /// 创建登录移除事件
    pub fn login_removed(login: Login) -> Self {
        Self {
            event_type: event_types::LOGIN_REMOVED.to_string(),
            login: Some(login),
            ..Default::default()
        }
    }

    /// 创建登录更新事件
    pub fn login_updated(login: Login) -> Self {
        Self {
            event_type: event_types::LOGIN_UPDATED.to_string(),
            login: Some(login),
            ..Default::default()
        }
    }

    // ========================================================================
    // 其他事件 (Reaction, Friend, Interaction)
    // ========================================================================

    /// 创建好友请求事件
    pub fn friend_request(user: User) -> Self {
        Self {
            event_type: event_types::FRIEND_REQUEST.to_string(),
            user: Some(user),
            ..Default::default()
        }
    }

    /// 创建表态增加事件
    pub fn reaction_added(message: Message, user: User) -> Self {
        let channel = message.channel.clone();
        let guild = message.guild.clone();
        Self {
            event_type: event_types::REACTION_ADDED.to_string(),
            message: Some(message),
            user: Some(user),
            channel,
            guild,
            ..Default::default()
        }
    }

    /// 创建表态移除事件
    pub fn reaction_removed(message: Message, user: User) -> Self {
        let channel = message.channel.clone();
        let guild = message.guild.clone();
        Self {
            event_type: event_types::REACTION_REMOVED.to_string(),
            message: Some(message),
            user: Some(user),
            channel,
            guild,
            ..Default::default()
        }
    }

    /// 创建按钮交互事件
    pub fn interaction_button(button: Button) -> Self {
        Self {
            event_type: event_types::INTERACTION_BUTTON.to_string(),
            button: Some(button),
            ..Default::default()
        }
    }

    /// 创建指令交互事件
    pub fn interaction_command(argv: Argv) -> Self {
        Self {
            event_type: event_types::INTERACTION_COMMAND.to_string(),
            argv: Some(argv),
            ..Default::default()
        }
    }

    // ========================================================================
    // 辅助方法 (Helpers)
    // ========================================================================

    /// 设置事件序列号 (SN)
    pub fn with_sn(mut self, sn: i64) -> Self {
        self.sn = sn;
        self
    }

    /// 设置登录信息
    pub fn with_login(mut self, login: Login) -> Self {
        self.login = Some(login);
        self
    }

    /// 是否为消息事件
    pub fn is_message_event(&self) -> bool {
        self.event_type.starts_with("message-")
    }

    /// 是否为群组事件
    pub fn is_guild_event(&self) -> bool {
        self.event_type.starts_with("guild-")
    }

    /// 获取消息内容（如果是消息事件）
    pub fn content(&self) -> Option<&str> {
        self.message.as_ref().map(|m| m.content.as_str())
    }

    /// 获取发送者 ID
    pub fn sender_id(&self) -> Option<&str> {
        self.user.as_ref().map(|u| u.id.as_str())
    }

    /// 获取频道 ID
    pub fn channel_id(&self) -> Option<&str> {
        self.channel.as_ref().map(|c| c.id.as_str())
    }

    /// 获取群组 ID
    pub fn guild_id(&self) -> Option<&str> {
        self.guild.as_ref().map(|g| g.id.as_str())
    }

    /// 获取平台名称
    pub fn platform(&self) -> Option<&str> {
        self.login.as_ref().and_then(|l| l.platform.as_deref())
    }

    /// 获取适配器名称
    pub fn adapter(&self) -> Option<&str> {
        self.login.as_ref().and_then(|l| l.adapter.as_deref())
    }
}

/// 获取当前时间戳（毫秒）
fn chrono_timestamp_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ============================================================================
// 3. 消息元素解析工具
// ============================================================================

/// 消息元素解析工具
/// 提供完整的消息元素解析能力
pub mod message_elements {
    // use super::*;
    use quick_xml::events::{BytesStart, Event as XmlEvent};
    use quick_xml::reader::Reader;
    use std::collections::HashMap;
    use std::fmt;

    /// Satori 标准消息元素
    /// 参考: zh-CN/protocol/elements.md
    #[derive(Debug, Clone, PartialEq)]
    pub enum Element {
        /// 纯文本
        Text(String),

        // --- 基础元素 ---
        /// 提及用户 <at>
        At {
            id: Option<String>,
            name: Option<String>,
            role: Option<String>,
            at_type: Option<String>, // type 字段
        },
        /// 提及频道 <sharp>
        Sharp { id: String, name: Option<String> },
        /// 链接 <a>
        Link {
            href: String,
            children: Vec<Element>, // 链接内可能包含文本或其他元素
        },

        // --- 资源元素 ---
        /// 图片 <img>
        Image {
            src: String,
            title: Option<String>,
            width: Option<u32>,
            height: Option<u32>,
            cache: Option<bool>,
            timeout: Option<u32>,
        },
        /// 音频 <audio>
        Audio {
            src: String,
            title: Option<String>,
            duration: Option<f64>,
            poster: Option<String>,
        },
        /// 视频 <video>
        Video {
            src: String,
            title: Option<String>,
            width: Option<u32>,
            height: Option<u32>,
            duration: Option<f64>,
            poster: Option<String>,
        },
        /// 文件 <file>
        File {
            src: String,
            title: Option<String>,
            poster: Option<String>,
        },

        // --- 修饰元素 ---
        /// 粗体 <b>, <strong>
        Bold(Vec<Element>),
        /// 斜体 <i>, <em>
        Italic(Vec<Element>),
        /// 下划线 <u>, <ins>
        Underline(Vec<Element>),
        /// 删除线 <s>, <del>
        Strikethrough(Vec<Element>),
        /// 剧透 <spl>
        Spoiler(Vec<Element>),
        /// 代码 <code>
        Code(String),
        /// 上标 <sup>
        Superscript(Vec<Element>),
        /// 下标 <sub>
        Subscript(Vec<Element>),

        // --- 排版元素 ---
        /// 换行 <br>
        Break,
        /// 段落 <p>
        Paragraph(Vec<Element>),
        /// 消息容器 <message>
        /// 用于合并转发或独立消息段
        Message {
            id: Option<String>,
            forward: bool,
            children: Vec<Element>,
        },

        // --- 元信息元素 ---
        /// 引用 <quote>
        Quote {
            id: Option<String>,
            children: Vec<Element>,
        },
        /// 作者 <author>
        Author {
            id: Option<String>,
            name: Option<String>,
            avatar: Option<String>,
            children: Vec<Element>,
        },

        // --- 交互元素 ---
        /// 按钮 <button>
        Button {
            id: Option<String>,
            type_: Option<String>, // type
            href: Option<String>,
            text: Option<String>, // input 填充文本
            theme: Option<String>,
            children: Vec<Element>, // 按钮上的文字
        },

        /// 未知/自定义元素
        Unknown {
            tag: String,
            attrs: HashMap<String, String>,
            children: Vec<Element>,
        },
    }

    impl Element {
        /// 是否为纯文本
        pub fn is_text(&self) -> bool {
            matches!(self, Element::Text(_))
        }

        /// 获取文本内容（如果是 Text 元素）
        pub fn as_text(&self) -> Option<&str> {
            match self {
                Element::Text(s) => Some(s),
                _ => None,
            }
        }

        /// 是否为提及用户
        pub fn is_at(&self) -> bool {
            matches!(self, Element::At { .. })
        }

        /// 获取提及的用户 ID
        pub fn at_id(&self) -> Option<&str> {
            match self {
                Element::At { id, .. } => id.as_deref(),
                _ => None,
            }
        }

        /// 是否为图片
        pub fn is_image(&self) -> bool {
            matches!(self, Element::Image { .. })
        }

        /// 获取图片链接
        pub fn image_src(&self) -> Option<&str> {
            match self {
                Element::Image { src, .. } => Some(src),
                _ => None,
            }
        }

        /// 获取子元素（如果是容器类型）
        pub fn children(&self) -> Option<&[Element]> {
            match self {
                Element::Link { children, .. }
                | Element::Bold(children)
                | Element::Italic(children)
                | Element::Underline(children)
                | Element::Strikethrough(children)
                | Element::Spoiler(children)
                | Element::Superscript(children)
                | Element::Subscript(children)
                | Element::Paragraph(children)
                | Element::Message { children, .. }
                | Element::Quote { children, .. }
                | Element::Author { children, .. }
                | Element::Button { children, .. }
                | Element::Unknown { children, .. } => Some(children),
                _ => None,
            }
        }

        /// 是否为合并转发消息节点
        /// 适配器应该检查此属性，如果为 true，则应将此元素处理为 "node" 类型，而不是递归处理子元素
        pub fn is_forward(&self) -> bool {
            match self {
                Element::Message { forward, .. } => *forward,
                _ => false,
            }
        }
    }

    // 实现 Display 以便将元素转回 XML 字符串（用于指令参数重组）
    impl fmt::Display for Element {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Element::Text(t) => write!(f, "{}", escape_xml(t)),
                Element::At {
                    id,
                    name,
                    role,
                    at_type,
                } => {
                    write!(f, "<at")?;
                    if let Some(v) = id {
                        write!(f, " id=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = name {
                        write!(f, " name=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = role {
                        write!(f, " role=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = at_type {
                        write!(f, " type=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, "/>")
                }
                Element::Sharp { id, name } => {
                    write!(f, "<sharp id=\"{}\"", escape_attr(id))?;
                    if let Some(v) = name {
                        write!(f, " name=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, "/>")
                }
                Element::Link { href, children } => {
                    write!(f, "<a href=\"{}\">", escape_attr(href))?;
                    for c in children {
                        write!(f, "{}", c)?;
                    }
                    write!(f, "</a>")
                }
                Element::Image {
                    src,
                    title,
                    width,
                    height,
                    cache,
                    timeout,
                } => {
                    write!(f, "<img src=\"{}\"", escape_attr(src))?;
                    if let Some(v) = title {
                        write!(f, " title=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = width {
                        write!(f, " width=\"{}\"", v)?;
                    }
                    if let Some(v) = height {
                        write!(f, " height=\"{}\"", v)?;
                    }
                    if let Some(v) = cache {
                        write!(f, " cache=\"{}\"", v)?;
                    }
                    if let Some(v) = timeout {
                        write!(f, " timeout=\"{}\"", v)?;
                    }
                    write!(f, "/>")
                }
                Element::Audio {
                    src,
                    title,
                    duration,
                    poster,
                } => {
                    write!(f, "<audio src=\"{}\"", escape_attr(src))?;
                    if let Some(v) = title {
                        write!(f, " title=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = duration {
                        write!(f, " duration=\"{}\"", v)?;
                    }
                    if let Some(v) = poster {
                        write!(f, " poster=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, "/>")
                }
                Element::Video {
                    src,
                    title,
                    width,
                    height,
                    duration,
                    poster,
                } => {
                    write!(f, "<video src=\"{}\"", escape_attr(src))?;
                    if let Some(v) = title {
                        write!(f, " title=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = width {
                        write!(f, " width=\"{}\"", v)?;
                    }
                    if let Some(v) = height {
                        write!(f, " height=\"{}\"", v)?;
                    }
                    if let Some(v) = duration {
                        write!(f, " duration=\"{}\"", v)?;
                    }
                    if let Some(v) = poster {
                        write!(f, " poster=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, "/>")
                }
                Element::File { src, title, poster } => {
                    write!(f, "<file src=\"{}\"", escape_attr(src))?;
                    if let Some(v) = title {
                        write!(f, " title=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = poster {
                        write!(f, " poster=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, "/>")
                }
                Element::Bold(c) => {
                    write!(f, "<b>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</b>")
                }
                Element::Italic(c) => {
                    write!(f, "<i>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</i>")
                }
                Element::Underline(c) => {
                    write!(f, "<u>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</u>")
                }
                Element::Strikethrough(c) => {
                    write!(f, "<s>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</s>")
                }
                Element::Spoiler(c) => {
                    write!(f, "<spl>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</spl>")
                }
                Element::Code(t) => write!(f, "<code>{}</code>", escape_xml(t)),
                Element::Superscript(c) => {
                    write!(f, "<sup>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</sup>")
                }
                Element::Subscript(c) => {
                    write!(f, "<sub>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</sub>")
                }
                Element::Break => write!(f, "<br/>"),
                Element::Paragraph(c) => {
                    write!(f, "<p>")?;
                    for child in c {
                        write!(f, "{}", child)?;
                    }
                    write!(f, "</p>")
                }
                // Message 元素的序列化
                Element::Message {
                    id,
                    forward,
                    children,
                } => {
                    write!(f, "<message")?;
                    if let Some(v) = id {
                        write!(f, " id=\"{}\"", escape_attr(v))?;
                    }
                    if *forward {
                        // 修改：显式输出属性值，兼容严格 XML 解析
                        write!(f, " forward=\"true\"")?;
                    }
                    if children.is_empty() {
                        write!(f, "/>")
                    } else {
                        write!(f, ">")?;
                        for c in children {
                            write!(f, "{}", c)?;
                        }
                        write!(f, "</message>")
                    }
                }
                Element::Quote { id, children } => {
                    write!(f, "<quote")?;
                    if let Some(v) = id {
                        write!(f, " id=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, ">")?;
                    for c in children {
                        write!(f, "{}", c)?;
                    }
                    write!(f, "</quote>")
                }
                Element::Author {
                    id,
                    name,
                    avatar,
                    children,
                } => {
                    write!(f, "<author")?;
                    if let Some(v) = id {
                        write!(f, " id=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = name {
                        write!(f, " name=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = avatar {
                        write!(f, " avatar=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, ">")?;
                    for c in children {
                        write!(f, "{}", c)?;
                    }
                    write!(f, "</author>")
                }
                Element::Button {
                    id,
                    type_,
                    href,
                    text,
                    theme,
                    children,
                } => {
                    write!(f, "<button")?;
                    if let Some(v) = id {
                        write!(f, " id=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = type_ {
                        write!(f, " type=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = href {
                        write!(f, " href=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = text {
                        write!(f, " text=\"{}\"", escape_attr(v))?;
                    }
                    if let Some(v) = theme {
                        write!(f, " theme=\"{}\"", escape_attr(v))?;
                    }
                    write!(f, ">")?;
                    for c in children {
                        write!(f, "{}", c)?;
                    }
                    write!(f, "</button>")
                }
                Element::Unknown {
                    tag,
                    attrs,
                    children,
                } => {
                    write!(f, "<{}", tag)?;
                    for (k, v) in attrs {
                        write!(f, " {}=\"{}\"", k, escape_attr(v))?;
                    }
                    if children.is_empty() {
                        write!(f, "/>")
                    } else {
                        write!(f, ">")?;
                        for c in children {
                            write!(f, "{}", c)?;
                        }
                        write!(f, "</{}>", tag)
                    }
                }
            }
        }
    }

    /// 解析消息内容为元素列表
    /// 使用 quick-xml 进行完整解析
    pub fn parse(content: &str) -> Vec<Element> {
        // [修复] 预处理 Satori 消息中的布尔属性 (如 <message forward ...>)
        // XML 解析器通常要求 key="value"，遇到纯布尔属性可能丢弃或报错。
        // 这里进行简单替换以兼容 NapCat/OneBot 等发送的非严格 XML。
        let fixed_content = content.replace("<message forward ", "<message forward=\"true\" ");

        // 为了处理 XML 片段（可能没有根节点），我们将其包裹在一个伪根节点中
        let wrapped_content = format!("<root>{}</root>", fixed_content);
        let mut reader = Reader::from_str(&wrapped_content);
        reader.config_mut().trim_text(false); // 保留空格

        // 解析栈：存储 (标签名, 属性, 子元素列表)
        // 使用一个特殊的内部名称作为初始容器，用于区分用户可能的输入
        let mut stack: Vec<(String, HashMap<String, String>, Vec<Element>)> = Vec::new();
        // 根节点的子元素容器，标签名 "__DOCUMENT_ROOT__" 仅作占位符
        stack.push(("__DOCUMENT_ROOT__".to_string(), HashMap::new(), Vec::new()));

        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    let attrs = parse_attributes(&e);
                    stack.push((tag_name, attrs, Vec::new()));
                }
                Ok(XmlEvent::End(_)) => {
                    if stack.len() > 1 {
                        // 弹出当前栈顶
                        let (tag, attrs, children) = stack.pop().unwrap();

                        // *** 关键修复 ***
                        // 如果当前标签是 "root" 且它是我们手动添加的最外层包裹（此时栈中只剩下一个占位符节点），
                        // 我们不将其构建为 Unknown 元素，而是直接将其子元素提取出来并合并到文档根中。
                        if tag == "root" && stack.len() == 1 {
                            if let Some(last) = stack.last_mut() {
                                last.2.extend(children);
                            }
                        } else {
                            // 正常构建元素
                            let element = build_element(tag.as_str(), attrs, children);
                            // 加入到父元素的 children 中
                            if let Some(last) = stack.last_mut() {
                                last.2.push(element);
                            }
                        }
                    } else {
                        // 根节点结束，退出
                        break;
                    }
                }
                Ok(XmlEvent::Empty(e)) => {
                    // 自闭合标签 <tag/>
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    let attrs = parse_attributes(&e);
                    let element = build_element(tag_name.as_str(), attrs, Vec::new());
                    if let Some(last) = stack.last_mut() {
                        last.2.push(element);
                    }
                }
                Ok(XmlEvent::Text(e)) => {
                    // 文本节点
                    if let Ok(text) = e.decode() {
                        let content = text.to_string();
                        if !content.is_empty()
                            && let Some(last) = stack.last_mut()
                        {
                            last.2.push(Element::Text(content));
                        }
                    }
                }
                Ok(XmlEvent::Eof) => break,
                Err(_) => {
                    // 解析错误处理：如果是简单的文本错误，作为文本处理，否则忽略或中断
                    // 这里简化处理：遇到严重错误停止解析，返回已解析部分
                    break;
                }
                _ => {} // 忽略 Comment, Decl 等
            }
            buf.clear();
        }

        // 返回伪根节点的 children
        stack
            .pop()
            .map(|(_, _, children)| children)
            .unwrap_or_default()
    }

    /// 将 quick-xml 的属性转换为 HashMap
    fn parse_attributes(e: &BytesStart) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        // [修复] 使用 unescape_value 时增加容错
        // flatten() 会吞掉错误，但对于布尔属性，我们需要确保尽量解析。
        // 不过由于 parse() 中已经做了字符串替换预处理，这里主要作为兜底。
        for attr in e.attributes().flatten() {
            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
            // 尝试解析值，如果 unescape 失败（极少情况），则存入空字符串以保留键
            // 这样 build_element 中的 contains_key 检查仍然有效
            match attr.unescape_value() {
                Ok(val) => {
                    attrs.insert(key, val.to_string());
                }
                Err(_) => {
                    attrs.insert(key, String::new());
                }
            }
        }
        attrs
    }

    /// 根据标签名、属性和子元素构建 Element
    fn build_element(
        tag: &str,
        mut attrs: HashMap<String, String>,
        children: Vec<Element>,
    ) -> Element {
        match tag {
            "at" => Element::At {
                id: attrs.remove("id"),
                name: attrs.remove("name"),
                role: attrs.remove("role"),
                at_type: attrs.remove("type"),
            },
            "sharp" => Element::Sharp {
                id: attrs.remove("id").unwrap_or_default(),
                name: attrs.remove("name"),
            },
            "a" => Element::Link {
                href: attrs.remove("href").unwrap_or_default(),
                children,
            },
            "img" => Element::Image {
                src: attrs.remove("src").unwrap_or_default(),
                title: attrs.remove("title"),
                width: attrs.remove("width").and_then(|v| v.parse().ok()),
                height: attrs.remove("height").and_then(|v| v.parse().ok()),
                cache: attrs.remove("cache").and_then(|v| v.parse().ok()),
                timeout: attrs.remove("timeout").and_then(|v| v.parse().ok()),
            },
            "audio" => Element::Audio {
                src: attrs.remove("src").unwrap_or_default(),
                title: attrs.remove("title"),
                duration: attrs.remove("duration").and_then(|v| v.parse().ok()),
                poster: attrs.remove("poster"),
            },
            "video" => Element::Video {
                src: attrs.remove("src").unwrap_or_default(),
                title: attrs.remove("title"),
                width: attrs.remove("width").and_then(|v| v.parse().ok()),
                height: attrs.remove("height").and_then(|v| v.parse().ok()),
                duration: attrs.remove("duration").and_then(|v| v.parse().ok()),
                poster: attrs.remove("poster"),
            },
            "file" => Element::File {
                src: attrs.remove("src").unwrap_or_default(),
                title: attrs.remove("title"),
                poster: attrs.remove("poster"),
            },
            "b" | "strong" => Element::Bold(children),
            "i" | "em" => Element::Italic(children),
            "u" | "ins" => Element::Underline(children),
            "s" | "del" => Element::Strikethrough(children),
            "spl" => Element::Spoiler(children),
            "code" => Element::Code(to_plain_text(&children)), // code 内容通常视为纯文本
            "sup" => Element::Superscript(children),
            "sub" => Element::Subscript(children),
            "br" => Element::Break,
            "p" => Element::Paragraph(children),
            "message" => Element::Message {
                id: attrs.remove("id"),
                // 检查 forward 是否存在且不为 "false"
                forward: attrs.contains_key("forward"),
                children,
            },
            "quote" => Element::Quote {
                id: attrs.remove("id"),
                children,
            },
            "author" => Element::Author {
                id: attrs.remove("id"),
                name: attrs.remove("name"),
                avatar: attrs.remove("avatar"),
                children,
            },
            "button" => Element::Button {
                id: attrs.remove("id"),
                type_: attrs.remove("type"),
                href: attrs.remove("href"),
                text: attrs.remove("text"),
                theme: attrs.remove("theme"),
                children,
            },
            _ => Element::Unknown {
                tag: tag.to_string(),
                attrs,
                children,
            },
        }
    }

    /// 将元素列表转换为纯文本
    /// 递归提取文本内容，忽略大多数格式，转换 <at> 等为可读文本
    pub fn to_plain_text(elements: &[Element]) -> String {
        let mut result = String::new();
        for elem in elements {
            match elem {
                Element::Text(text) => result.push_str(text),
                Element::At {
                    name, id, at_type, ..
                } => {
                    result.push('@');
                    if let Some(t) = at_type {
                        if t == "all" {
                            result.push_str("全体成员");
                        } else if t == "here" {
                            result.push_str("在线成员");
                        } else {
                            result.push_str(name.as_deref().or(id.as_deref()).unwrap_or("unknown"));
                        }
                    } else {
                        result.push_str(name.as_deref().or(id.as_deref()).unwrap_or("someone"));
                    }
                }
                Element::Sharp { name, id } => {
                    result.push('#');
                    result.push_str(name.as_deref().unwrap_or(id));
                }
                Element::Link { children, href } => {
                    let text = to_plain_text(children);
                    if text.is_empty() {
                        result.push_str(href);
                    } else {
                        result.push_str(&text);
                    }
                }
                Element::Image { title, .. } => {
                    result.push_str(title.as_deref().unwrap_or("[图片]"));
                }
                Element::Audio { .. } => result.push_str("[语音]"),
                Element::Video { .. } => result.push_str("[视频]"),
                Element::File { title, .. } => {
                    result.push_str(title.as_deref().unwrap_or("[文件]"));
                }
                Element::Button { children, .. } => {
                    result.push('[');
                    result.push_str(&to_plain_text(children));
                    result.push(']');
                }
                Element::Break => result.push('\n'),
                Element::Paragraph(children) => {
                    result.push_str(&to_plain_text(children));
                    result.push('\n');
                }
                // 容器类元素，递归提取
                Element::Bold(c)
                | Element::Italic(c)
                | Element::Underline(c)
                | Element::Strikethrough(c)
                | Element::Spoiler(c)
                | Element::Superscript(c)
                | Element::Subscript(c)
                | Element::Quote { children: c, .. }
                | Element::Author { children: c, .. }
                | Element::Unknown { children: c, .. } => {
                    result.push_str(&to_plain_text(c));
                }
                Element::Message {
                    forward,
                    children: c,
                    ..
                } => {
                    // 如果是转发引用且没有子节点（例如 <message forward id="..."/>），显示占位符
                    if *forward && c.is_empty() {
                        result.push_str("[合并转发]");
                    }
                    // 递归处理内容
                    result.push_str(&to_plain_text(c));
                    // 消息节点通常意味着换行
                    result.push('\n');
                }
                Element::Code(text) => result.push_str(text),
            }
        }
        result
    }

    /// 合并转发中的单条消息节点
    /// 用于构建自定义的合并转发内容
    #[derive(Debug, Clone, Default)]
    pub struct ForwardNode {
        /// 发送者 ID
        pub sender_id: Option<String>,
        /// 发送者名称
        pub sender_name: Option<String>,
        /// 发送者头像
        pub sender_avatar: Option<String>,
        /// 消息内容 (XML 字符串)
        pub content: String,
        /// 消息时间戳 (可选)
        pub time: Option<i64>,
    }

    impl ForwardNode {
        /// 创建一个新的转发节点
        /// content 应该是已经构建好的 XML 字符串（例如通过 MessageBuilder::build() 生成）
        pub fn new(content: impl Into<String>) -> Self {
            Self {
                content: content.into(),
                ..Default::default()
            }
        }

        /// 设置发送者 ID
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self.sender_id = Some(id.into());
            self
        }

        /// 设置发送者名称
        pub fn name(mut self, name: impl Into<String>) -> Self {
            self.sender_name = Some(name.into());
            self
        }

        /// 设置发送者头像
        pub fn avatar(mut self, url: impl Into<String>) -> Self {
            self.sender_avatar = Some(url.into());
            self
        }

        /// 转换为 XML 字符串
        fn to_xml(&self) -> String {
            // 确保所有属性都有引号
            let mut xml = String::from("<message>");
            if self.sender_id.is_some()
                || self.sender_name.is_some()
                || self.sender_avatar.is_some()
            {
                xml.push_str("<author");
                if let Some(id) = &self.sender_id {
                    xml.push_str(&format!(" id=\"{}\"", escape_attr(id)));
                }
                if let Some(name) = &self.sender_name {
                    xml.push_str(&format!(" name=\"{}\"", escape_attr(name)));
                }
                if let Some(avatar) = &self.sender_avatar {
                    xml.push_str(&format!(" avatar=\"{}\"", escape_attr(avatar)));
                }
                xml.push_str("/>");
            }
            xml.push_str(&self.content);
            xml.push_str("</message>");
            xml
        }
    }

    /// 构建消息元素（用于发送）
    /// 这是一个辅助构建器，用于生成符合 Satori 规范的 XML 字符串
    pub struct MessageBuilder {
        content: String,
    }

    impl MessageBuilder {
        pub fn new() -> Self {
            Self {
                content: String::new(),
            }
        }

        /// 添加纯文本
        pub fn text(mut self, text: impl AsRef<str>) -> Self {
            self.content.push_str(&escape_xml(text.as_ref()));
            self
        }

        /// 添加原始 XML 内容 (用于组合嵌套)
        /// 注意：不会进行转义，请确保 content 是有效的 XML 片段
        pub fn raw(mut self, content: impl AsRef<str>) -> Self {
            self.content.push_str(content.as_ref());
            self
        }

        /// 添加 @用户
        pub fn at(mut self, user_id: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!(r#"<at id="{}"/>"#, escape_attr(user_id.as_ref())));
            self
        }

        /// 添加 @全体成员
        pub fn at_all(mut self) -> Self {
            self.content.push_str(r#"<at type="all"/>"#);
            self
        }

        /// 添加 @角色
        pub fn at_role(mut self, role_id: impl AsRef<str>) -> Self {
            self.content.push_str(&format!(
                r#"<at role="{}"/>"#,
                escape_attr(role_id.as_ref())
            ));
            self
        }

        /// 添加链接
        pub fn link(mut self, href: impl AsRef<str>, text: Option<&str>) -> Self {
            let href_esc = escape_attr(href.as_ref());
            if let Some(t) = text {
                self.content
                    .push_str(&format!(r#"<a href="{}">{}</a>"#, href_esc, escape_xml(t)));
            } else {
                self.content
                    .push_str(&format!(r#"<a href="{}"/>"#, href_esc));
            }
            self
        }

        /// 添加图片
        pub fn image(mut self, src: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!(r#"<img src="{}"/>"#, escape_attr(src.as_ref())));
            self
        }

        /// 添加音频
        pub fn audio(mut self, src: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!(r#"<audio src="{}"/>"#, escape_attr(src.as_ref())));
            self
        }

        /// 添加视频
        pub fn video(mut self, src: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!(r#"<video src="{}"/>"#, escape_attr(src.as_ref())));
            self
        }

        /// 添加换行
        pub fn br(mut self) -> Self {
            self.content.push_str("<br/>");
            self
        }

        /// 添加粗体
        pub fn bold(mut self, text: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!("<b>{}</b>", escape_xml(text.as_ref())));
            self
        }

        /// 添加代码
        pub fn code(mut self, text: impl AsRef<str>) -> Self {
            self.content
                .push_str(&format!("<code>{}</code>", escape_xml(text.as_ref())));
            self
        }

        /// 添加引用 (仅 ID)
        pub fn quote(mut self, message_id: impl AsRef<str>) -> Self {
            self.content.push_str(&format!(
                r#"<quote id="{}"/>"#,
                escape_attr(message_id.as_ref())
            ));
            self
        }

        /// 添加按钮 (简易版)
        pub fn button_action(mut self, id: impl AsRef<str>, text: impl AsRef<str>) -> Self {
            self.content.push_str(&format!(
                r#"<button id="{}" type="action">{}</button>"#,
                escape_attr(id.as_ref()),
                escape_xml(text.as_ref())
            ));
            self
        }

        // --- 合并转发支持 ---
        /// 添加嵌套子消息节点 (用于合并转发内部)
        /// 传入的内容将被包裹在 <message>...</message> 中
        pub fn message_child(mut self, content: impl AsRef<str>) -> Self {
            self.content.push_str("<message>");
            self.content.push_str(content.as_ref());
            self.content.push_str("</message>");
            self
        }

        pub fn forward_id(mut self, message_id: impl AsRef<str>) -> Self {
            // 使用 forward="true"
            self.content.push_str(&format!(
                r#"<message id="{}" forward="true"/>"#,
                escape_attr(message_id.as_ref())
            ));
            self
        }

        pub fn forward_node(mut self, node: &ForwardNode) -> Self {
            self.content.push_str(&node.to_xml());
            self
        }

        pub fn merge_forward_nodes(mut self, nodes: &[ForwardNode]) -> Self {
            // 使用 forward="true"
            self.content.push_str("<message forward=\"true\">");
            for node in nodes {
                self.content.push_str(&node.to_xml());
            }
            self.content.push_str("</message>");
            self
        }

        pub fn wrap_forward(mut self) -> Self {
            let inner = std::mem::take(&mut self.content);
            // 使用 forward="true"
            self.content.push_str("<message forward=\"true\">");
            self.content.push_str(&inner);
            self.content.push_str("</message>");
            self
        }

        pub fn merge_forward(mut self, content: impl AsRef<str>) -> Self {
            // 使用 forward="true"
            self.content.push_str("<message forward=\"true\">");
            self.content.push_str(content.as_ref());
            self.content.push_str("</message>");
            self
        }

        pub fn build(self) -> String {
            self.content
        }
    }

    impl Default for MessageBuilder {
        fn default() -> Self {
            Self::new()
        }
    }
    fn escape_xml(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
    fn escape_attr(text: &str) -> String {
        escape_xml(text)
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}

// ============================================================================
// 4. 指令解析工具 (模块重构)
// ============================================================================

/// 指令解析工具库
pub mod command {
    use super::message_elements::Element;

    /// 尝试匹配并剥离前缀
    ///
    /// 遍历 `prefixes` 列表，如果 `content` 以其中任意一个开头，则返回匹配到的前缀。
    /// 开发者可以使用此函数判断消息是否为指令。
    pub fn match_prefix(content: &str, prefixes: &[String]) -> Option<String> {
        let trimmed = content.trim_start();
        for prefix in prefixes {
            if trimmed.starts_with(prefix) {
                return Some(prefix.clone());
            }
        }
        None
    }

    /// 工具：跳过消息开头的引用(Quote)元素
    ///
    /// 在处理回复消息时，通常需要忽略引用的部分，只解析用户新输入的内容。
    /// 返回跳过引用后的元素切片。
    pub fn skip_quote_elements(elements: &[Element]) -> &[Element] {
        let mut start = 0;
        while start < elements.len() {
            if matches!(elements[start], Element::Quote { .. }) {
                start += 1;
            } else {
                break;
            }
        }
        &elements[start..]
    }

    /// 工具：查找第一个纯文本元素的内容
    ///
    /// 常用于获取指令主体。例如从 `[Quote] /echo hello` 中提取 `/echo hello`。
    pub fn find_first_text(elements: &[Element]) -> Option<&str> {
        elements.iter().find_map(|e| e.as_text())
    }
}

// ============================================================================
// 5. Configuration System (配置系统)
// ============================================================================

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// 核心配置
    #[serde(default)]
    pub core: CoreConfig,
    /// 插件配置（使用 flatten 支持任意插件配置）
    #[serde(flatten)]
    pub plugins: HashMap<String, toml::Value>,
}

/// 核心配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// 指令前缀
    #[serde(default = "default_cmd_prefix")]
    pub cmd_prefix: Vec<String>,
    /// 管理员用户列表
    #[serde(default)]
    pub admin_users: Vec<String>,
    /// 黑名单用户
    #[serde(default)]
    pub blacklist_users: Vec<String>,
    /// 黑名单群组
    #[serde(default)]
    pub blacklist_guilds: Vec<String>,
    /// 会话超时时间（秒）
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,
}

fn default_cmd_prefix() -> Vec<String> {
    vec!["/".to_string(), ".".to_string()]
}

fn default_session_timeout() -> u64 {
    60
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            cmd_prefix: default_cmd_prefix(),
            admin_users: Vec::new(),
            blacklist_users: Vec::new(),
            blacklist_guilds: Vec::new(),
            session_timeout_secs: default_session_timeout(),
        }
    }
}

impl AppConfig {
    /// 获取指定插件的配置
    pub fn get_plugin_config<T: for<'de> Deserialize<'de>>(&self, plugin_id: &str) -> Option<T> {
        self.plugins
            .get(plugin_id)
            .and_then(|v| v.clone().try_into().ok())
    }

    /// 检查用户是否为管理员
    pub fn is_admin(&self, user_id: &str) -> bool {
        self.core.admin_users.contains(&user_id.to_string())
    }

    /// 检查用户是否在黑名单中
    pub fn is_user_blacklisted(&self, user_id: &str) -> bool {
        self.core.blacklist_users.contains(&user_id.to_string())
    }

    /// 检查群组是否在黑名单中
    pub fn is_guild_blacklisted(&self, guild_id: &str) -> bool {
        self.core.blacklist_guilds.contains(&guild_id.to_string())
    }
}

/// 配置管理器
pub struct ConfigManager {
    path: PathBuf,
    config: RwLock<AppConfig>,
}

impl ConfigManager {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            config: RwLock::new(AppConfig::default()),
        }
    }

    /// 获取配置文件路径
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 加载配置，如果文件不存在则创建默认配置
    pub async fn load(&self) -> AyjxResult<AppConfig> {
        if !self.path.exists() {
            let default_cfg = AppConfig::default();
            self.save_atomic(&default_cfg).await?;
            return Ok(default_cfg);
        }

        let content = fs::read_to_string(&self.path)?;
        let cfg: AppConfig = toml::from_str(&content)?;

        let mut write_lock = self.config.write().await;
        *write_lock = cfg.clone();

        Ok(cfg)
    }

    /// 原子写入配置（写临时文件 -> Rename 覆盖）
    pub async fn save_atomic(&self, cfg: &AppConfig) -> AyjxResult<()> {
        let content = toml::to_string_pretty(cfg)?;
        let tmp_path = self.path.with_extension("tmp");
        let path_clone = self.path.clone();
        let tmp_clone = tmp_path.clone();

        // 在阻塞线程中执行同步 IO
        // 移除内部错误包装，直接传递错误
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            // 确保父目录存在
            if let Some(parent) = path_clone.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut file = fs::File::create(&tmp_clone)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?; // 确保落盘
            fs::rename(&tmp_clone, &path_clone)?;
            Ok(())
        })
        .await??;

        // 更新内存缓存
        let mut write_lock = self.config.write().await;
        *write_lock = cfg.clone();

        Ok(())
    }

    /// 获取当前配置（只读）
    pub async fn get(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    /// 更新配置（会自动保存）
    pub async fn update<F>(&self, f: F) -> AyjxResult<AppConfig>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut cfg = self.config.write().await;
        f(&mut cfg);
        let new_cfg = cfg.clone();
        drop(cfg); // 释放锁

        self.save_atomic(&new_cfg).await?;
        Ok(new_cfg)
    }
}

// ============================================================================
// 6. Session / Wait Mechanism (会话机制)
// ============================================================================

/// 会话匹配器类型
type SessionMatcher = Box<dyn Fn(&Event) -> bool + Send + Sync>;

/// 等待器条目
struct Waiter {
    matcher: SessionMatcher,
    sender: oneshot::Sender<Event>,
    created_at: std::time::Instant,
    timeout: Duration,
}

/// 会话管理器
/// 用于实现 Wait/Session 机制，允许插件挂起当前执行流等待特定消息
pub struct SessionManager {
    waiters: Mutex<Vec<Waiter>>,
    next_id: AtomicU64,
    default_timeout: Duration,
}

impl SessionManager {
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            waiters: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(0),
            default_timeout,
        }
    }

    /// 注册一个等待，返回接收器
    pub async fn wait_for<F>(&self, matcher: F) -> oneshot::Receiver<Event>
    where
        F: Fn(&Event) -> bool + Send + Sync + 'static,
    {
        self.wait_for_timeout(matcher, self.default_timeout).await
    }

    /// 注册一个带超时的等待
    pub async fn wait_for_timeout<F>(
        &self,
        matcher: F,
        timeout: Duration,
    ) -> oneshot::Receiver<Event>
    where
        F: Fn(&Event) -> bool + Send + Sync + 'static,
    {
        let (tx, rx) = oneshot::channel();

        let waiter = Waiter {
            matcher: Box::new(matcher),
            sender: tx,
            created_at: std::time::Instant::now(),
            timeout,
        };

        let mut waiters = self.waiters.lock().await;
        waiters.push(waiter);

        rx
    }

    /// 检查事件是否匹配等待者
    /// 返回 true 表示事件被会话捕获，不应继续传递给普通插件
    pub async fn check(&self, event: &Event) -> bool {
        let mut waiters = self.waiters.lock().await;
        let now = std::time::Instant::now();

        // 清理过期的等待者
        waiters.retain(|w| now.duration_since(w.created_at) < w.timeout);

        // 查找匹配的等待者（FIFO）
        let mut matched_index = None;
        for (i, waiter) in waiters.iter().enumerate() {
            if (waiter.matcher)(event) {
                matched_index = Some(i);
                break;
            }
        }

        if let Some(idx) = matched_index {
            let waiter = waiters.remove(idx);
            // 尝试发送，忽略错误（接收端可能已经 drop）
            let _ = waiter.sender.send(event.clone());
            return true;
        }

        false
    }

    /// 取消所有等待（框架关闭时调用）
    pub async fn cancel_all(&self) {
        let mut waiters = self.waiters.lock().await;
        waiters.clear();
    }

    /// 获取当前等待者数量
    pub async fn pending_count(&self) -> usize {
        self.waiters.lock().await.len()
    }
}

// ============================================================================
// 7. Plugin Traits (插件接口定义)
// ============================================================================

/// 适配器插件接口
/// 负责与平台通信，实现 Satori API
#[async_trait]
pub trait Adapter: Send + Sync + 'static {
    /// 获取 Any 引用，用于向下转型
    fn as_any(&self) -> &dyn Any;

    /// 适配器唯一标识
    fn id(&self) -> &str;

    /// 适配器名称
    fn name(&self) -> &str;

    fn default_config(&self) -> Option<toml::Value> {
        None
    }

    /// 适配器版本
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// 支持的平台列表
    fn platforms(&self) -> Vec<&str>;

    /// 启动适配器
    async fn start(&self, ctx: AdapterContext) -> AyjxResult<()>;

    /// 停止适配器
    async fn stop(&self) -> AyjxResult<()>;

    /// 获取登录信息
    async fn get_login(&self) -> AyjxResult<Login>;

    // ----- Satori API: 消息相关 -----

    /// 发送消息
    async fn send_message(&self, channel_id: &str, content: &str) -> AyjxResult<Vec<Message>>;

    /// 获取消息
    async fn get_message(&self, channel_id: &str, message_id: &str) -> AyjxResult<Message> {
        Err("API not implemented".into())
    }

    /// 撤回消息
    async fn delete_message(&self, channel_id: &str, message_id: &str) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 编辑消息
    async fn update_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 获取消息列表
    async fn list_messages(
        &self,
        channel_id: &str,
        next: Option<&str>,
        limit: Option<usize>,
    ) -> AyjxResult<BidiPagedList<Message>> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 用户相关 -----

    /// 获取用户信息
    async fn get_user(&self, user_id: &str) -> AyjxResult<User> {
        Err("API not implemented".into())
    }

    /// 获取好友列表
    async fn list_friends(&self, next: Option<&str>) -> AyjxResult<PagedList<User>> {
        Err("API not implemented".into())
    }

    /// 处理好友申请
    async fn handle_friend_request(
        &self,
        message_id: &str,
        approve: bool,
        comment: Option<&str>,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 频道相关 -----

    /// 获取频道信息
    async fn get_channel(&self, channel_id: &str) -> AyjxResult<Channel> {
        Err("API not implemented".into())
    }

    /// 获取频道列表
    async fn list_channels(
        &self,
        guild_id: &str,
        next: Option<&str>,
    ) -> AyjxResult<PagedList<Channel>> {
        Err("API not implemented".into())
    }

    /// 创建群组频道
    async fn create_channel(&self, guild_id: &str, data: &Channel) -> AyjxResult<Channel> {
        Err("API not implemented".into())
    }

    /// 修改群组频道
    async fn update_channel(&self, channel_id: &str, data: &Channel) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 删除群组频道
    async fn delete_channel(&self, channel_id: &str) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 禁言群组频道 (实验性)
    async fn mute_channel(&self, channel_id: &str, duration_ms: u64) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 创建私聊频道
    async fn create_direct_channel(
        &self,
        user_id: &str,
        guild_id: Option<&str>,
    ) -> AyjxResult<Channel> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 群组相关 -----

    /// 获取群组信息
    async fn get_guild(&self, guild_id: &str) -> AyjxResult<Guild> {
        Err("API not implemented".into())
    }

    /// 获取群组列表
    async fn list_guilds(&self, next: Option<&str>) -> AyjxResult<PagedList<Guild>> {
        Err("API not implemented".into())
    }

    /// 处理群组邀请
    async fn approve_guild(
        &self,
        message_id: &str,
        approve: bool,
        comment: Option<&str>,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 群组成员相关 -----

    /// 获取群组成员
    async fn get_guild_member(&self, guild_id: &str, user_id: &str) -> AyjxResult<GuildMember> {
        Err("API not implemented".into())
    }

    /// 获取群组成员列表
    async fn list_guild_members(
        &self,
        guild_id: &str,
        next: Option<&str>,
    ) -> AyjxResult<PagedList<GuildMember>> {
        Err("API not implemented".into())
    }

    /// 踢出群组成员
    async fn kick_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        permanent: bool,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 禁言群组成员
    async fn mute_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        duration_ms: u64,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 处理群组成员申请
    async fn approve_guild_member(
        &self,
        message_id: &str,
        approve: bool,
        comment: Option<&str>,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 群组角色相关 -----
    /// 获取群组角色
    async fn get_guild_role(&self, guild_id: &str, role_id: &str) -> AyjxResult<GuildRole> {
        Err("API not implemented".into())
    }

    /// 获取群组角色列表
    async fn list_guild_roles(
        &self,
        guild_id: &str,
        next: Option<&str>,
    ) -> AyjxResult<PagedList<GuildRole>> {
        Err("API not implemented".into())
    }

    /// 创建群组角色
    async fn create_guild_role(&self, guild_id: &str, role: &GuildRole) -> AyjxResult<GuildRole> {
        Err("API not implemented".into())
    }

    /// 修改群组角色
    async fn update_guild_role(
        &self,
        guild_id: &str,
        role_id: &str,
        role: &GuildRole,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 删除群组角色
    async fn delete_guild_role(&self, guild_id: &str, role_id: &str) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 设置群组成员角色
    async fn set_guild_member_role(
        &self,
        guild_id: &str,
        user_id: &str,
        role_id: &str,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 取消群组成员角色
    async fn unset_guild_member_role(
        &self,
        guild_id: &str,
        user_id: &str,
        role_id: &str,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    // ----- Satori API: 表态相关 -----

    /// 添加表态
    async fn create_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 删除表态
    async fn delete_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
        user_id: Option<&str>,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 清除表态
    async fn clear_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: Option<&str>,
    ) -> AyjxResult<()> {
        Err("API not implemented".into())
    }

    /// 获取表态列表
    async fn list_reactions(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
        next: Option<&str>,
    ) -> AyjxResult<PagedList<User>> {
        Err("API not implemented".into())
    }
}

/// 适配器上下文，传递给适配器的 start 方法
pub struct AdapterContext {
    /// 事件发送通道
    pub event_tx: mpsc::Sender<Event>,
    /// 配置管理器
    pub config: Arc<ConfigManager>,
    /// 系统信号订阅
    pub system_rx: broadcast::Receiver<SystemSignal>,
    /// 数据目录
    pub data_dir: PathBuf,
}

impl Clone for AdapterContext {
    fn clone(&self) -> Self {
        Self {
            event_tx: self.event_tx.clone(),
            config: self.config.clone(),
            system_rx: self.system_rx.resubscribe(),
            data_dir: self.data_dir.clone(),
        }
    }
}

/// 业务逻辑插件接口
#[async_trait]
pub trait Plugin: Send + Sync {
    /// 插件唯一标识
    fn id(&self) -> &str;

    /// 插件名称
    fn name(&self) -> &str;

    /// 插件描述
    fn description(&self) -> &str {
        ""
    }

    fn default_config(&self) -> Option<toml::Value> {
        None
    }

    /// 插件版本
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// 插件优先级（数字越小优先级越高）
    fn priority(&self) -> i32 {
        100
    }

    /// 插件加载时调用
    async fn on_load(&self, _ctx: &PluginContext) -> AyjxResult<()> {
        Ok(())
    }

    /// 插件卸载时调用（异步清理）
    async fn on_unload(&self, _ctx: &PluginContext) -> AyjxResult<()> {
        Ok(())
    }

    /// 插件被清理时的回调（同步清理）
    /// 用于处理无法在 on_unload 中处理的资源释放，或确保某些操作（如关闭浏览器进程）一定执行
    fn cleanup(&self) {}

    /// 接收事件
    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult>;

    /// 配置重载通知
    async fn on_config_reload(
        &self,
        _ctx: &PluginContext,
        _new_config: &AppConfig,
    ) -> AyjxResult<()> {
        Ok(())
    }

    /// 适配器连接状态变化
    async fn on_connect(&self, _ctx: &PluginContext, _login: &Login) -> AyjxResult<()> {
        Ok(())
    }

    /// 适配器断开连接
    async fn on_disconnect(&self, _ctx: &PluginContext, _login: &Login) -> AyjxResult<()> {
        Ok(())
    }
}

/// 事件处理结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventResult {
    /// 继续传递事件给后续插件
    #[default]
    Continue,
    /// 停止传递事件（事件已被处理）
    Stop,
}

/// 插件上下文
#[derive(Clone)]
pub struct PluginContext {
    inner: Arc<PluginContextInner>,
}

struct PluginContextInner {
    config: Arc<ConfigManager>,
    adapters: Arc<RwLock<HashMap<String, Arc<dyn Adapter>>>>,
    // 引用 AyjxInner 中的插件列表，用于管理插件状态
    plugins: Arc<RwLock<Vec<PluginSlot>>>,
    session_manager: Arc<SessionManager>,
    system_tx: broadcast::Sender<SystemSignal>,
    running: Arc<AtomicBool>,
    data_base_dir: PathBuf,
    plugin_id: String,
}

impl PluginContext {
    fn new(
        plugin_id: String,
        config: Arc<ConfigManager>,
        adapters: Arc<RwLock<HashMap<String, Arc<dyn Adapter>>>>,
        plugins: Arc<RwLock<Vec<PluginSlot>>>,
        session_manager: Arc<SessionManager>,
        system_tx: broadcast::Sender<SystemSignal>,
        running: Arc<AtomicBool>,
        data_base_dir: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(PluginContextInner {
                config,
                adapters,
                plugins,
                session_manager,
                system_tx,
                running,
                data_base_dir,
                plugin_id,
            }),
        }
    }

    /// 获取配置
    pub async fn config(&self) -> AppConfig {
        self.inner.config.get().await
    }

    /// 获取当前插件的配置
    pub async fn plugin_config<T: for<'de> Deserialize<'de>>(&self) -> Option<T> {
        self.config().await.get_plugin_config(&self.inner.plugin_id)
    }

    /// 获取当前插件的数据目录
    pub fn data_dir(&self) -> PathBuf {
        self.inner.data_base_dir.join(&self.inner.plugin_id)
    }

    /// 确保数据目录存在
    pub async fn ensure_data_dir(&self) -> AyjxResult<PathBuf> {
        let dir = self.data_dir();
        tokio::fs::create_dir_all(&dir).await?;
        Ok(dir)
    }

    /// 发送消息（便捷方法）
    pub async fn send_message(
        &self,
        adapter_id: &str,
        channel_id: &str,
        content: &str,
    ) -> AyjxResult<Vec<Message>> {
        let adapters = self.inner.adapters.read().await;
        if let Some(adapter) = adapters.get(adapter_id) {
            adapter.send_message(channel_id, content).await
        } else {
            Err(format!("Adapter {} not found", adapter_id).into())
        }
    }

    /// 通过事件快速回复消息
    pub async fn reply(&self, event: &Event, content: &str) -> AyjxResult<Vec<Message>> {
        let adapter_id = event
            .adapter()
            .ok_or_else(|| "Event has no adapter info".to_string())?;
        let channel_id = event
            .channel_id()
            .ok_or_else(|| "Event has no channel info".to_string())?;

        self.send_message(adapter_id, channel_id, content).await
    }

    /// 等待下一条符合条件的消息
    pub async fn wait_for<F>(&self, matcher: F) -> AyjxResult<Event>
    where
        F: Fn(&Event) -> bool + Send + Sync + 'static,
    {
        let timeout = Duration::from_secs(self.config().await.core.session_timeout_secs);
        self.wait_for_timeout(matcher, timeout).await
    }

    /// 等待下一条符合条件的消息（带超时）
    pub async fn wait_for_timeout<F>(&self, matcher: F, timeout: Duration) -> AyjxResult<Event>
    where
        F: Fn(&Event) -> bool + Send + Sync + 'static,
    {
        let rx = self
            .inner
            .session_manager
            .wait_for_timeout(matcher, timeout)
            .await;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(_)) => Err("Session closed".into()),
            Err(_) => Err("Session timeout".into()),
        }
    }

    /// 等待特定用户的下一条消息（便捷方法）
    pub async fn prompt(&self, user_id: &str, channel_id: Option<&str>) -> AyjxResult<String> {
        let user_id = user_id.to_string();
        let channel_id = channel_id.map(|s| s.to_string());

        let matcher = move |e: &Event| {
            if e.event_type != event_types::MESSAGE_CREATED {
                return false;
            }

            let user_match = e.sender_id() == Some(user_id.as_str());
            let channel_match = channel_id
                .as_ref()
                .is_none_or(|cid| e.channel_id() == Some(cid.as_str()));

            user_match && channel_match
        };

        let event = self.wait_for(matcher).await?;
        event
            .content()
            .map(|s| s.to_string())
            .ok_or_else(|| "Event has no message content".into())
    }

    /// 获取用户信息（便捷方法）
    pub async fn get_user(&self, adapter_id: &str, user_id: &str) -> AyjxResult<User> {
        let adapters = self.inner.adapters.read().await;
        if let Some(adapter) = adapters.get(adapter_id) {
            adapter.get_user(user_id).await
        } else {
            Err(format!("Adapter {} not found", adapter_id).into())
        }
    }

    /// 获取适配器（便捷方法）
    pub async fn get_adapter(&self, id: &str) -> Option<Arc<dyn Adapter>> {
        self.inner.adapters.read().await.get(id).cloned()
    }

    /// 检查用户是否为管理员
    pub async fn is_admin(&self, user_id: &str) -> bool {
        self.config().await.is_admin(user_id)
    }

    /// 检查用户是否在黑名单中
    pub async fn is_blacklisted(&self, user_id: &str) -> bool {
        self.config().await.is_user_blacklisted(user_id)
    }

    /// 框架是否在运行中
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    /// 请求关闭框架
    pub fn request_shutdown(&self) {
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.system_tx.send(SystemSignal::Shutdown);
    }

    /// 请求重启框架
    pub fn request_restart(&self) {
        // 重启也是一种关闭，但信号不同
        self.inner.running.store(false, Ordering::SeqCst);
        let _ = self.inner.system_tx.send(SystemSignal::Restart);
    }

    /// 请求重新加载配置
    pub fn request_config_reload(&self) {
        let _ = self.inner.system_tx.send(SystemSignal::ConfigReload);
    }

    // 修改 enable_plugin
    pub async fn enable_plugin(&self, plugin_id: &str) -> bool {
        // 这里只需要读锁，因为我们要修改的是 Arc 内部的 AtomicBool，而不是 Vec 结构本身
        let plugins = self.inner.plugins.read().await;

        if let Some(slot) = plugins.iter().find(|p| p.plugin.id() == plugin_id) {
            // 检查当前状态 (load)
            if !slot.enabled.load(Ordering::SeqCst) {
                // 修改状态 (store)
                slot.enabled.store(true, Ordering::SeqCst);
                println!("[Ayjx] 插件 {} 已启用", slot.plugin.name());
                return true;
            }
        }
        false
    }

    // 修改 disable_plugin
    pub async fn disable_plugin(&self, plugin_id: &str) -> bool {
        let plugins = self.inner.plugins.read().await;

        if let Some(slot) = plugins.iter().find(|p| p.plugin.id() == plugin_id) {
            // 检查当前状态
            if slot.enabled.load(Ordering::SeqCst) {
                // 修改状态
                slot.enabled.store(false, Ordering::SeqCst);
                println!("[Ayjx] 插件 {} 已禁用", slot.plugin.name());
                return true;
            }
        }
        false
    }
}

// ============================================================================
// 8. Middleware System (中间件系统)
// ============================================================================

/// 中间件处理函数类型
pub type MiddlewareFuture<'a> =
    Pin<Box<dyn Future<Output = AyjxResult<MiddlewareResult>> + Send + 'a>>;

/// 中间件结果
#[derive(Debug, Clone)]
pub enum MiddlewareResult {
    /// 继续执行后续中间件和插件
    Continue(Event),
    /// 停止执行，事件被中间件消费
    Stop,
    /// 修改事件后继续
    Modified(Event),
}

/// 中间件接口
#[async_trait]
pub trait Middleware: Send + Sync {
    /// 中间件名称
    fn name(&self) -> &str;

    /// 中间件优先级（数字越小越先执行）
    fn priority(&self) -> i32 {
        100
    }

    /// 处理事件
    async fn process(&self, ctx: &MiddlewareContext, event: Event) -> AyjxResult<MiddlewareResult>;
}

/// 中间件上下文
#[derive(Clone)]
pub struct MiddlewareContext {
    config: Arc<ConfigManager>,
}

impl MiddlewareContext {
    pub async fn config(&self) -> AppConfig {
        self.config.get().await
    }
}

// ============================================================================
// 9. System Signals (系统信号)
// ============================================================================

/// 系统信号
#[derive(Clone, Debug)]
pub enum SystemSignal {
    /// 关闭框架
    Shutdown,
    /// 重启框架
    Restart,
    /// 重新加载配置
    ConfigReload,
    /// 适配器状态变化
    AdapterStatusChanged {
        adapter_id: String,
        status: LoginStatus,
    },
}

// ============================================================================
// 10. Framework Core (框架核心)
// ============================================================================

/// 插件容器，包含插件实例和状态
#[derive(Clone)]
struct PluginSlot {
    plugin: Arc<dyn Plugin>,
    enabled: Arc<AtomicBool>,
}

/// 框架内部状态 (用于并发共享)
struct AyjxInner {
    config: Arc<ConfigManager>,
    adapters: Arc<RwLock<HashMap<String, Arc<dyn Adapter>>>>,
    // 修改：使用 RwLock 和 PluginSlot 支持运行时状态管理
    plugins: Arc<RwLock<Vec<PluginSlot>>>,
    middlewares: Arc<Vec<Box<dyn Middleware>>>,
    session_manager: Arc<SessionManager>,
    system_tx: broadcast::Sender<SystemSignal>,
    running: Arc<AtomicBool>,
    data_dir: PathBuf,
    event_tx: mpsc::Sender<Event>,
}

/// 框架构建器
pub struct AyjxBuilder {
    config_path: PathBuf,
    data_dir: PathBuf,
    adapters: Vec<Box<dyn Adapter>>,
    plugins: Vec<Box<dyn Plugin>>,
    middlewares: Vec<Box<dyn Middleware>>,
}

impl AyjxBuilder {
    /// 创建新的框架构建器
    pub fn new() -> Self {
        Self {
            config_path: PathBuf::from("config.toml"),
            data_dir: PathBuf::from("data"),
            adapters: Vec::new(),
            plugins: Vec::new(),
            middlewares: Vec::new(),
        }
    }

    /// 设置配置文件路径
    pub fn config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config_path = path.as_ref().to_path_buf();
        self
    }

    /// 设置数据目录
    pub fn data_dir<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.data_dir = path.as_ref().to_path_buf();
        self
    }

    /// 注册适配器
    pub fn adapter<A: Adapter + 'static>(mut self, adapter: A) -> Self {
        self.adapters.push(Box::new(adapter));
        self
    }

    /// 注册插件
    pub fn plugin<P: Plugin + 'static>(mut self, plugin: P) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// 注册中间件
    pub fn middleware<M: Middleware + 'static>(mut self, middleware: M) -> Self {
        self.middlewares.push(Box::new(middleware));
        self
    }

    /// 添加默认中间件
    pub fn with_default_middlewares(self) -> Self {
        // 现在黑名单是一个插件，不再作为内置中间件处理
        // 如果有其他内置中间件可保留，否则此方法可废弃或留空
        self
    }

    /// 构建并返回框架实例
    pub fn build(self) -> Ayjx {
        Ayjx::from_builder(self)
    }
}

impl Default for AyjxBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Ayjx 框架核心
pub struct Ayjx {
    /// 内部状态 (Arc 包裹，支持并发)
    inner: Arc<AyjxInner>,
    /// 事件接收端 (仅在主循环使用)
    event_rx: Option<mpsc::Receiver<Event>>,
}

impl Ayjx {
    /// 创建框架构建器
    pub fn builder() -> AyjxBuilder {
        AyjxBuilder::new()
    }

    /// 从构建器创建框架实例
    fn from_builder(builder: AyjxBuilder) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);
        let (system_tx, _) = broadcast::channel(64);

        let config = Arc::new(ConfigManager::new(&builder.config_path));

        // 初始化适配器
        let mut adapters_map = HashMap::new();
        for adapter in builder.adapters {
            adapters_map.insert(
                adapter.id().to_string(),
                Arc::from(adapter) as Arc<dyn Adapter>,
            );
        }
        let adapters = Arc::new(RwLock::new(adapters_map));

        // 排序插件并初始化 Slot
        let mut raw_plugins = builder.plugins;
        raw_plugins.sort_by_key(|p| p.priority());

        let mut plugin_slots = Vec::new();
        for p in raw_plugins {
            plugin_slots.push(PluginSlot {
                plugin: Arc::from(p),
                enabled: Arc::new(AtomicBool::new(true)),
            });
        }

        let mut middlewares = builder.middlewares;
        middlewares.sort_by_key(|m| m.priority());

        let session_manager = Arc::new(SessionManager::new(Duration::from_secs(60)));

        let inner = Arc::new(AyjxInner {
            config,
            adapters,
            plugins: Arc::new(RwLock::new(plugin_slots)),
            middlewares: Arc::new(middlewares),
            session_manager,
            system_tx,
            running: Arc::new(AtomicBool::new(false)),
            data_dir: builder.data_dir,
            event_tx,
        });

        Self {
            inner,
            event_rx: Some(event_rx),
        }
    }

    /// 启动框架
    pub async fn run(mut self) -> AyjxResult<()> {
        println!("╔═══════════════════════════════════════════════════╗");
        println!("║                                                   ║");
        println!("║     Ayjx Framework - 安、易、简、行               ║");
        println!("║     Ayjx —— 安于心，简于行。                      ║");
        println!("║                                                   ║");
        println!("╚═══════════════════════════════════════════════════╝");
        println!();

        self.inner.running.store(true, Ordering::SeqCst);

        // 1. 加载配置
        println!("[Ayjx] 正在加载配置...");
        // 这里我们需要先加载一次，获取当前磁盘上的状态
        let mut initial_config = self.inner.config.load().await?;
        let mut config_modified = false;

        // [新增] 1.5 配置自动注册与合并
        // 检查所有插件，如果配置中不存在该插件的块，则写入默认配置
        println!("[Ayjx] 正在检查配置完整性...");

        // 检查插件配置
        {
            let plugins = self.inner.plugins.read().await;
            for slot in plugins.iter() {
                let pid = slot.plugin.id();
                // 如果配置中没有这个插件的 key，且插件提供了默认配置
                if !initial_config.plugins.contains_key(pid)
                    && let Some(def_cfg) = slot.plugin.default_config() {
                        println!("[Ayjx]   + 初始化插件配置: {}", slot.plugin.name());
                        initial_config.plugins.insert(pid.to_string(), def_cfg);
                        config_modified = true;
                    }
            }
        }

        // 检查适配器配置 (逻辑同上，适配器通常也存放在 plugins 字典或单独字段，这里为了统一暂存入 plugins)
        // 注意：如果你希望适配器配置单独存放（如 [adapters.qq]），需修改 AppConfig 结构。
        // 这里假设适配器配置也混入 plugins 结构或你需要扩展 AppConfig。
        // 为了演示，我们暂时将其视为一种"组件配置"，同样存入 config.plugins (因为 AppConfig 只有 plugins 字段支持动态 Map)
        {
            let adapters = self.inner.adapters.read().await;
            for (id, adapter) in adapters.iter() {
                if !initial_config.plugins.contains_key(id)
                    && let Some(def_cfg) = adapter.default_config() {
                        println!("[Ayjx]   + 初始化适配器配置: {}", adapter.name());
                        initial_config.plugins.insert(id.to_string(), def_cfg);
                        config_modified = true;
                    }
            }
        }

        // 如果配置有更新，原子落盘
        if config_modified {
            println!("[Ayjx] 检测到新组件，正在更新配置文件...");
            self.inner.config.save_atomic(&initial_config).await?;
        }

        // 2. 确保数据目录存在
        tokio::fs::create_dir_all(&self.inner.data_dir).await?;
        println!("[Ayjx] 数据目录: {}", self.inner.data_dir.display());

        // 3. 初始化插件
        println!("[Ayjx] 正在初始化插件...");
        {
            let plugins = self.inner.plugins.read().await;
            for slot in plugins.iter() {
                if !slot.enabled.load(Ordering::SeqCst) {
                    continue;
                }
                let plugin = &slot.plugin;
                let ctx = self.create_plugin_context(plugin.id());
                if let Err(e) = plugin.on_load(&ctx).await {
                    eprintln!("[Ayjx] 插件 {} 初始化失败: {}", plugin.name(), e);
                } else {
                    println!("[Ayjx]   - {} v{} 已加载", plugin.name(), plugin.version());
                }
            }
        }

        // 4. 启动适配器
        println!("[Ayjx] 正在启动适配器...");
        let adapters = self.inner.adapters.read().await;
        for (id, adapter) in adapters.iter() {
            let adapter_clone = adapter.clone();
            let event_tx = self.inner.event_tx.clone();
            let config = self.inner.config.clone();
            let system_rx = self.inner.system_tx.subscribe();
            let data_dir = self.inner.data_dir.join(id);

            let id_clone = id.clone();
            let adapter_name = adapter.name().to_string();

            tokio::spawn(async move {
                let ctx = AdapterContext {
                    event_tx,
                    config,
                    system_rx,
                    data_dir,
                };

                if let Err(e) = adapter_clone.start(ctx).await {
                    eprintln!("[Ayjx] 适配器 {} 运行错误: {}", id_clone, e);
                }
            });

            println!("[Ayjx]   - {} ({}) 已启动", adapter_name, id);
        }
        drop(adapters);

        // 5. 进入事件循环
        println!("[Ayjx] 事件循环已启动，等待消息...");

        let mut event_rx = self.event_rx.take().expect("event_rx already taken");
        let mut system_rx = self.inner.system_tx.subscribe();
        let mut restart_requested = false;

        loop {
            tokio::select! {
                // 处理系统信号
                Ok(signal) = system_rx.recv() => {
                    match signal {
                        SystemSignal::Shutdown => {
                            println!("[Ayjx] 收到关闭信号，正在停止...");
                            break;
                        }
                        SystemSignal::Restart => {
                            println!("[Ayjx] 收到重启信号，正在准备重启...");
                            restart_requested = true;
                            break;
                        }
                        SystemSignal::ConfigReload => {
                            println!("[Ayjx] 重新加载配置...");
                            if let Ok(new_cfg) = self.inner.config.load().await {
                                let plugins = self.inner.plugins.read().await;
                                for slot in plugins.iter() {
                                    if !slot.enabled.load(Ordering::SeqCst) { continue; }
                                    let ctx = self.create_plugin_context(slot.plugin.id());
                                    let _ = slot.plugin.on_config_reload(&ctx, &new_cfg).await;
                                }
                            }
                        }
                        SystemSignal::AdapterStatusChanged { adapter_id, status } => {
                            println!("[Ayjx] 适配器 {} 状态变化: {:?}", adapter_id, status);
                        }
                    }
                }

                // 处理事件 (关键优化：使用 tokio::spawn 实现并行处理)
                Some(event) = event_rx.recv() => {
                    let inner = self.inner.clone();
                    tokio::spawn(async move {
                        if let Err(e) = inner.process_event(event).await {
                            eprintln!("[Ayjx] 事件处理错误: {}", e);
                        }
                    });
                }

                // 检查运行状态
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if !self.inner.running.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }

        // 清理
        self.shutdown().await?;

        if restart_requested {
            println!("[Ayjx] 框架已停止，请由外部进程进行重启操作。");
            // 这里返回 Ok，外部程序（如 main 函数中的 loop）可以根据需要重新调用 run 或退出
        } else {
            println!("[Ayjx] 框架已停止");
        }

        Ok(())
    }

    /// 创建插件上下文
    fn create_plugin_context(&self, plugin_id: &str) -> PluginContext {
        self.inner.create_plugin_context(plugin_id)
    }

    /// 关闭框架
    async fn shutdown(&self) -> AyjxResult<()> {
        self.inner.running.store(false, Ordering::SeqCst);

        // 停止所有适配器
        let adapters = self.inner.adapters.read().await;
        for (id, adapter) in adapters.iter() {
            if let Err(e) = adapter.stop().await {
                eprintln!("[Ayjx] 停止适配器 {} 时发生错误: {}", id, e);
            }
        }

        // 卸载所有插件 (包括调用同步的 cleanup)
        let plugins = self.inner.plugins.read().await;
        for slot in plugins.iter() {
            if !slot.enabled.load(Ordering::SeqCst) {
                continue;
            }
            // 无论是否启用，或者异步卸载是否成功，都调用 cleanup 进行最终清理
            // 修复：使用 cleanup 代替 drop 以避免 E0040 错误
            slot.plugin.cleanup();
        }

        // 取消所有等待中的会话
        self.inner.session_manager.cancel_all().await;

        Ok(())
    }

    /// 注入事件（用于测试或外部触发）
    pub async fn inject_event(&self, event: Event) -> AyjxResult<()> {
        self.inner.event_tx.send(event).await.map_err(|e| e.into())
    }
}

// 内部状态实现方法
impl AyjxInner {
    /// 处理单个事件
    async fn process_event(&self, mut event: Event) -> AyjxResult<()> {
        // 1. Session 拦截
        if self.session_manager.check(&event).await {
            return Ok(());
        }

        // 2. 执行中间件链
        let middleware_ctx = MiddlewareContext {
            config: self.config.clone(),
        };

        for middleware in self.middlewares.iter() {
            match middleware.process(&middleware_ctx, event.clone()).await? {
                MiddlewareResult::Continue(e) => {
                    event = e;
                }
                MiddlewareResult::Stop => {
                    return Ok(());
                }
                MiddlewareResult::Modified(e) => {
                    event = e;
                }
            }
        }

        // 3. 分发给插件
        // 关键优化：获取快照，避免在执行插件逻辑时持有锁
        let slots: Vec<PluginSlot> = {
            let guard = self.plugins.read().await;
            guard.clone() // <--- 现在 PluginSlot 实现了 Clone，这里可以工作了
        }; // 读锁在这里释放，防止死锁

        for slot in slots {
            // 使用 load 检查状态
            if !slot.enabled.load(Ordering::SeqCst) {
                continue;
            }

            let ctx = self.create_plugin_context(slot.plugin.id());

            // 现在调用 await 是安全的
            match slot.plugin.on_event(&ctx, &event).await {
                Ok(EventResult::Stop) => break,
                Ok(EventResult::Continue) => continue,
                Err(e) => {
                    eprintln!(
                        "[Ayjx] 插件 {} 处理事件时发生错误: {}",
                        slot.plugin.name(),
                        e
                    );
                }
            }
        }

        Ok(())
    }

    fn create_plugin_context(&self, plugin_id: &str) -> PluginContext {
        PluginContext::new(
            plugin_id.to_string(),
            self.config.clone(),
            self.adapters.clone(),
            self.plugins.clone(),
            self.session_manager.clone(),
            self.system_tx.clone(),
            self.running.clone(),
            self.data_dir.clone(),
        )
    }

    /// 注入事件（用于测试或外部触发）
    pub async fn inject_event(&self, event: Event) -> AyjxResult<()> {
        self.event_tx.send(event).await.map_err(|e| e.into())
    }
}

// ============================================================================
// 11. Re-exports (重新导出)
// ============================================================================

pub mod prelude {
    //! 常用类型的预导入模块
    //!
    //! 建议在开发插件时使用：
    //! ```rust
    //! use ayjx::prelude::*;
    //! ```

    // 1. 框架核心与错误处理
    pub use super::{Ayjx, AyjxBuilder, AyjxError, AyjxResult};

    // 2. 插件与中间件系统 (核心 API)
    pub use super::{
        Adapter, AdapterContext, EventResult, Middleware, MiddlewareContext, MiddlewareResult,
        Plugin, PluginContext, SystemSignal,
    };

    // 3. 配置对象
    pub use super::{AppConfig, CoreConfig};

    // 4. Satori 协议数据模型
    pub use super::{
        Argv, BidiPagedList, Button, Channel, ChannelType, Event, Guild, GuildMember, GuildRole,
        Login, LoginStatus, Message, PagedList, User,
    };

    // 5. 工具模块
    pub use super::{command, event_types, message_elements};

    // 6. 常用工具类型
    pub use super::message_elements::{Element, ForwardNode, MessageBuilder};

    // 7. 外部依赖
    pub use async_trait::async_trait;

    // 导出 toml 供插件序列化配置使用
    pub use toml;
}
