//! NapCat (OneBot V11) 适配器
//!
//! 基于 OneBot V11 协议，增加了 NapCat 平台特有的 API 支持。

#![allow(dead_code)]

use ayjx::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, protocol::Message as WsMessage},
};

mod api;
pub use api::NapCatApi;

// ============================================================================
// 1. 配置定义
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct NapCatConfig {
    #[serde(default)]
    pub bots: Vec<BotConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "protocol", rename_all = "kebab-case")]
pub enum BotConfig {
    /// 正向 WebSocket
    WsForward {
        url: String,
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default = "default_reconnect_interval")]
        reconnect_interval_ms: u64,
    },
    /// 反向 WebSocket
    WsReverse {
        host: String,
        port: u16,
        #[serde(default)]
        access_token: Option<String>,
    },
}

fn default_reconnect_interval() -> u64 {
    3000
}

// ============================================================================
// 2. 内部结构与通信抽象
// ============================================================================

/// Bot 发送端抽象
#[derive(Clone)]
enum BotSender {
    Ws(mpsc::UnboundedSender<String>),
}

/// 适配器内部状态
struct AdapterInner {
    connections: RwLock<HashMap<String, BotSender>>,
    pending_responses: RwLock<HashMap<String, oneshot::Sender<Value>>>,
    logins: RwLock<HashMap<String, Login>>,
    // 新增：保存上下文以支持在 send_message 中推送事件
    context: RwLock<Option<AdapterContext>>,
}

impl AdapterInner {
    async fn register_connection(&self, self_id: String, sender: BotSender) {
        let mut map = self.connections.write().await;
        map.insert(self_id.clone(), sender);
    }

    async fn remove_connection(&self, self_id: &str) {
        {
            let mut map = self.connections.write().await;
            if map.remove(self_id).is_some() {}
        }
        // 连接断开时清理缓存
        let mut logins = self.logins.write().await;
        logins.remove(self_id);
    }
}

/// NapCat (OneBot V11) 适配器
pub struct NapCatAdapter {
    inner: Arc<AdapterInner>,
}

impl NapCatAdapter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AdapterInner {
                connections: RwLock::new(HashMap::new()),
                pending_responses: RwLock::new(HashMap::new()),
                logins: RwLock::new(HashMap::new()),
                context: RwLock::new(None), // 初始化为空
            }),
        }
    }

    /// 获取当前所有已连接 Bot 的 ID 和连接类型
    pub async fn get_connected_bots(&self) -> Vec<(String, String)> {
        let map = self.inner.connections.read().await;
        map.iter()
            .map(|(id, sender)| {
                let type_name = match sender {
                    BotSender::Ws(_) => "WebSocket",
                };
                (id.clone(), type_name.to_string())
            })
            .collect()
    }

    /// 调用 API
    pub async fn call_api(
        &self,
        action: &str,
        params: Value,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let sender = {
            let map = self.inner.connections.read().await;
            if let Some(id) = self_id {
                map.get(id).cloned()
            } else {
                map.values().next().cloned()
            }
        }
        .ok_or_else(|| "No active bot connection".to_string())?;

        match sender {
            BotSender::Ws(tx) => {
                let echo = uuid::Uuid::new_v4().to_string();
                let frame = json!({
                    "action": action,
                    "params": params,
                    "echo": echo
                });

                let (resp_tx, resp_rx) = oneshot::channel();
                {
                    let mut pending = self.inner.pending_responses.write().await;
                    pending.insert(echo.clone(), resp_tx);
                }

                tx.send(frame.to_string())
                    .map_err(|_| "WebSocket channel closed".to_string())?;

                match tokio::time::timeout(Duration::from_secs(60), resp_rx).await {
                    Ok(Ok(json)) => Self::check_api_response(json),
                    Ok(Err(_)) => Err("Response channel closed".into()),
                    Err(_) => {
                        let mut pending = self.inner.pending_responses.write().await;
                        pending.remove(&echo);
                        Err("API request timeout".into())
                    }
                }
            }
        }
    }

    /// 尝试更新登录信息的缓存
    async fn update_login_info(&self, self_id: String) {
        if self.inner.logins.read().await.contains_key(&self_id) {
            return;
        }

        // 构造基础 Login
        let mut login = Login::new("qq", "napcat");
        let mut user = User::new(self_id.clone());
        user.avatar = Some(format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", self_id));
        user.is_bot = Some(true);
        login.user = Some(user);
        login.status = LoginStatus::Online;

        // 尝试调用 API 获取详细信息
        match self.get_login_info(Some(&self_id)).await {
            Ok(info) => {
                if let Some(user) = login.user.as_mut()
                    && let Some(nick) = info["nickname"].as_str()
                {
                    user.name = Some(nick.to_string());
                    user.nick = Some(nick.to_string());
                }
            }
            Err(e) => {
                eprintln!("[NapCat] 获取 Bot {} 登录详情失败: {}", self_id, e);
            }
        }

        self.inner.logins.write().await.insert(self_id, login);
    }

    /// 获取缓存的 Login 信息，如果不存在则返回默认构造
    async fn get_cached_login(&self, self_id: &str) -> Login {
        if let Some(login) = self.inner.logins.read().await.get(self_id) {
            return login.clone();
        }

        let mut login = Login::new("qq", "napcat");
        login.user = Some(User::new(self_id.to_string()));
        login.status = LoginStatus::Online;
        login
    }

    /// 处理消息片段发送
    async fn send_element_slice(
        &self,
        msg_type: &str,
        target_id: &str,
        batch: &[Element],
    ) -> AyjxResult<Option<Message>> {
        let ob_message_val = elements_to_onebot(batch);

        if let Some(arr) = ob_message_val.as_array()
            && arr.is_empty()
        {
            return Ok::<Option<Message>, Box<dyn std::error::Error + Send + Sync>>(None);
        }

        let msg_id = self
            .send_onebot_msg(msg_type, target_id, ob_message_val)
            .await?;

        Ok(Some(Message {
            id: msg_id,
            content: String::new(),
            ..Default::default()
        }))
    }

    /// 统一处理 OneBot 消息发送逻辑
    async fn send_onebot_msg(
        &self,
        msg_type: &str,
        target_id: &str,
        message: Value,
    ) -> AyjxResult<String> {
        let ob_message_vec = message.as_array().cloned().unwrap_or_else(|| vec![message]);

        let resp = if msg_type == "private" {
            self.send_private_msg(target_id, ob_message_vec, None)
                .await?
        } else {
            self.send_group_msg(target_id, ob_message_vec, None).await?
        };

        // 提取消息ID (兼容不同返回结构)
        let msg_id = resp["message_id"]
            .as_str()
            .or_else(|| resp["data"]["message_id"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }

    fn check_api_response(json: Value) -> AyjxResult<Value> {
        match json["status"].as_str() {
            Some("ok") => Ok(json["data"].clone()),
            Some("async") => Ok(json["data"].clone()),
            Some("failed") => Err(format!(
                "API failed: {} (retcode: {})",
                json["msg"].as_str().unwrap_or("unknown"),
                json["retcode"]
            )
            .into()),
            _ => Ok(json),
        }
    }
}

impl Default for NapCatAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 2. NapCat 平台特有 API 实现 (通过 Trait 桥接)
// ============================================================================

#[async_trait]
impl NapCatApi for NapCatAdapter {
    async fn call_api(
        &self,
        action: &str,
        params: Value,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        NapCatAdapter::call_api(self, action, params, self_id).await
    }
}

// ============================================================================
// 3. Adapter Trait 实现
// ============================================================================

#[async_trait]
impl Adapter for NapCatAdapter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn id(&self) -> &str {
        "napcat"
    }
    fn name(&self) -> &str {
        "NapCat (OneBot V11)"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }
    fn platforms(&self) -> Vec<&str> {
        vec!["qq", "onebot", "napcat"]
    }

    fn default_config(&self) -> Option<toml::Value> {
        let config = NapCatConfig {
            bots: vec![
                // 默认 1: 正向 WebSocket (主动连接 NapCat)
                BotConfig::WsForward {
                    url: "ws://127.0.0.1:3001".to_string(),
                    access_token: Some("".to_string()),
                    reconnect_interval_ms: default_reconnect_interval(),
                },
                // 默认 2: 反向 WebSocket (监听来自 NapCat 的连接)
                BotConfig::WsReverse {
                    host: "0.0.0.0".to_string(),
                    port: 8080,
                    access_token: Some("".to_string()),
                },
            ],
        };

        match toml::Value::try_from(config) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[NapCat] 默认配置生成失败: {}", e);
                None
            }
        }
    }

    async fn start(&self, ctx: AdapterContext) -> AyjxResult<()> {
        // 保存 Context 以便在 send_message 中使用
        *self.inner.context.write().await = Some(ctx.clone());

        let app_cfg = ctx.config.get().await;

        let config: NapCatConfig = if let Some(val) = app_cfg.plugins.get("napcat") {
            val.clone()
                .try_into()
                .map_err(|e| format!("[NapCat] 配置解析失败，请检查 config.toml 格式: {}", e))?
        } else {
            NapCatConfig::default()
        };

        if config.bots.is_empty() {
            println!(
                "[NapCat] ⚠️ 警告: 未配置任何 Bot 连接。请检查 config.toml 中的 [[plugins.napcat.bots]]"
            );
            return Ok(());
        }

        for bot_cfg in config.bots {
            match bot_cfg {
                BotConfig::WsForward {
                    url,
                    access_token,
                    reconnect_interval_ms,
                } => {
                    if !url.is_empty() {
                        self.start_ws_forward(
                            url,
                            access_token,
                            reconnect_interval_ms,
                            ctx.clone(),
                        );
                    }
                }
                BotConfig::WsReverse {
                    host,
                    port,
                    access_token,
                } => {
                    self.start_ws_reverse(host, port, access_token, ctx.clone());
                }
            }
        }

        Ok(())
    }

    async fn stop(&self) -> AyjxResult<()> {
        let mut map = self.inner.connections.write().await;
        map.clear();
        Ok(())
    }

    async fn get_login(&self) -> AyjxResult<Login> {
        let map = self.inner.connections.read().await;
        if let Some((id, _)) = map.iter().next() {
            Ok(self.get_cached_login(id).await)
        } else {
            Ok(Login::new("qq", "napcat"))
        }
    }

    // ----- 消息 API -----

    async fn send_message(&self, channel_id: &str, content: &str) -> AyjxResult<Vec<Message>> {
        let (msg_type, target_id) = parse_channel_id(channel_id);
        let mut sent_messages = Vec::new();

        // 1. 如果没有 XML 特征，直接作为纯文本发送
        if !content.contains('<') && !content.contains('>') {
            let ob_message = json!([{ "type": "text", "data": { "text": content } }]);
            let msg_id = self
                .send_onebot_msg(msg_type, target_id, ob_message)
                .await?;
            sent_messages.push(Message {
                id: msg_id,
                content: content.to_string(),
                ..Default::default()
            });
        } else {
            // 2. 解析并遍历元素
            let elements = message_elements::parse(content);
            let mut buffer: Vec<Element> = Vec::new();

            for elem in elements {
                if matches!(elem, Element::Message { .. }) {
                    // 遇到 <message> 标签：

                    // A. 立即发送之前缓冲区的内容（如果有）
                    if !buffer.is_empty() {
                        if let Some(msg) = self
                            .send_element_slice(msg_type, target_id, &buffer)
                            .await?
                        {
                            sent_messages.push(msg);
                        }
                        buffer.clear();
                    }

                    // B. 处理当前的 <message> 元素
                    if let Some(msg) = self
                        .send_element_slice(msg_type, target_id, &[elem])
                        .await?
                    {
                        sent_messages.push(msg);
                    }
                } else {
                    // 普通元素加入缓冲区
                    buffer.push(elem);
                }
            }

            // 3. 发送剩余的缓冲区内容
            if !buffer.is_empty()
                && let Some(msg) = self
                    .send_element_slice(msg_type, target_id, &buffer)
                    .await?
            {
                sent_messages.push(msg);
            }

            // 兜底：如果没有任何消息生成但内容不为空（防止静默失败）
            if sent_messages.is_empty() && !content.is_empty() {
                let ob_message = json!([{ "type": "text", "data": { "text": content } }]);
                let msg_id = self
                    .send_onebot_msg(msg_type, target_id, ob_message)
                    .await?;
                sent_messages.push(Message {
                    id: msg_id,
                    content: content.to_string(),
                    ..Default::default()
                });
            }
        }

        // --- 主动推送 message-created 事件 ---
        // 获取 Context (需要 clone 避免持有读锁)
        let ctx_opt = self.inner.context.read().await.clone();
        if let Some(ctx) = ctx_opt {
            // 获取当前 Bot ID (取第一个活跃连接，如果没有则默认)
            let self_id = self
                .inner
                .connections
                .read()
                .await
                .keys()
                .next()
                .cloned()
                .unwrap_or_default();

            let login = self.get_cached_login(&self_id).await;

            for mut msg in sent_messages.clone() {
                // 补全 Message 内部的时间 (如果 API 没返回，用当前时间)
                if msg.created_at.is_none() {
                    msg.created_at = Some(chrono::Utc::now().timestamp_millis());
                }
                // 补全 Message 的 User
                if let Some(u) = &login.user {
                    msg.user = Some(u.clone());
                }

                // 补全 Guild/Channel 信息
                if msg_type == "group" {
                    let mut guild = Guild::new(target_id.to_string());
                    guild.avatar = Some(format!(
                        "http://p.qlogo.cn/gh/{}/{}/640/",
                        target_id, target_id
                    ));
                    msg.guild = Some(guild);
                    msg.channel = Some(Channel::new(target_id.to_string(), ChannelType::Text));
                } else {
                    msg.channel = Some(Channel::new(
                        format!("private:{}", target_id),
                        ChannelType::Direct,
                    ));
                }

                let mut event = Event::message_created(msg);
                event.login = Some(login.clone());
                event.platform_type = Some(msg_type.to_string());
                event.timestamp = chrono::Utc::now().timestamp_millis();

                // 设置 User (Sender)
                if let Some(u) = &login.user {
                    event.user = Some(u.clone());
                }

                let _ = ctx.event_tx.send(event).await;
            }
        }

        Ok(sent_messages)
    }

    async fn delete_message(&self, _channel_id: &str, message_id: &str) -> AyjxResult<()> {
        self.delete_msg(message_id, None).await
    }

    async fn get_message(&self, _channel_id: &str, message_id: &str) -> AyjxResult<Message> {
        let resp = self.get_msg(message_id, None).await?;

        Ok(Message {
            id: resp["message_id"].to_string(),
            content: parse_onebot_message(&resp["message"]),
            created_at: resp["time"].as_i64().map(|t| t * 1000),
            ..Default::default()
        })
    }

    async fn list_messages(
        &self,
        channel_id: &str,
        next: Option<&str>,
        _limit: Option<usize>,
    ) -> AyjxResult<BidiPagedList<Message>> {
        let (msg_type, target_id) = parse_channel_id(channel_id);
        let resp = if msg_type == "private" {
            self.get_friend_msg_history(target_id, next, Some(20), None, None)
                .await?
        } else {
            self.get_group_msg_history(target_id, next, Some(20), None, None)
                .await?
        };

        let mut messages = Vec::new();
        if let Some(list) = resp["messages"].as_array() {
            for item in list {
                let msg = Message {
                    id: item["message_id"].to_string(),
                    content: parse_onebot_message(&item["message"]),
                    created_at: item["time"].as_i64().map(|t| t * 1000),
                    user: Some(User::new(item["sender"]["user_id"].to_string())),
                    ..Default::default()
                };
                messages.push(msg);
            }
        }

        Ok(BidiPagedList {
            data: messages,
            prev: None,
            next: None,
        })
    }

    // ----- 用户 API -----

    async fn get_user(&self, user_id: &str) -> AyjxResult<User> {
        let resp = self.get_stranger_info(user_id, None).await?;

        Ok(User {
            id: resp["user_id"].to_string(),
            name: resp["nickname"].as_str().map(String::from),
            nick: resp["nickname"].as_str().map(String::from),
            avatar: Some(format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", user_id)),
            is_bot: Some(false),
        })
    }

    async fn list_friends(&self, _next: Option<&str>) -> AyjxResult<PagedList<User>> {
        let resp = self.get_friend_list(None, None).await?;
        let mut users = Vec::new();
        for item in resp {
            users.push(User {
                id: item["user_id"].to_string(),
                name: item["nickname"].as_str().map(String::from),
                nick: item["remark"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                avatar: Some(format!(
                    "https://q1.qlogo.cn/g?b=qq&nk={}&s=640",
                    item["user_id"]
                )),
                is_bot: Some(false),
            });
        }
        Ok(PagedList::new(users))
    }

    async fn handle_friend_request(
        &self,
        message_id: &str,
        approve: bool,
        comment: Option<&str>,
    ) -> AyjxResult<()> {
        self.set_friend_add_request(message_id, approve, comment.unwrap_or(""), None)
            .await
    }

    // ----- 频道 API (映射为群组) -----

    async fn get_channel(&self, channel_id: &str) -> AyjxResult<Channel> {
        let (ctype, target_id) = parse_channel_id(channel_id);
        if ctype == "private" {
            return Ok(Channel::new(channel_id, ChannelType::Direct));
        }

        let resp = self.get_group_info(target_id, None).await?;
        Ok(Channel {
            id: channel_id.to_string(),
            channel_type: ChannelType::Text,
            name: resp["group_name"].as_str().map(String::from),
            parent_id: None,
        })
    }

    async fn list_channels(
        &self,
        guild_id: &str,
        _next: Option<&str>,
    ) -> AyjxResult<PagedList<Channel>> {
        let channel = self.get_channel(&format!("group:{}", guild_id)).await?;
        Ok(PagedList::new(vec![channel]))
    }

    async fn update_channel(&self, channel_id: &str, data: &Channel) -> AyjxResult<()> {
        let (_, target_id) = parse_channel_id(channel_id);
        if let Some(name) = &data.name {
            self.set_group_name(target_id, name, None).await?;
        }
        Ok(())
    }

    async fn delete_channel(&self, channel_id: &str) -> AyjxResult<()> {
        let (_, target_id) = parse_channel_id(channel_id);
        self.set_group_leave(target_id, Some(false), None).await
    }

    async fn mute_channel(&self, channel_id: &str, duration_ms: u64) -> AyjxResult<()> {
        let (_, target_id) = parse_channel_id(channel_id);
        self.set_group_whole_ban(target_id, duration_ms > 0, None)
            .await
    }

    // ----- 群组 API -----

    async fn get_guild(&self, guild_id: &str) -> AyjxResult<Guild> {
        let resp = self.get_group_info(guild_id, None).await?;

        Ok(Guild {
            id: resp["group_id"].to_string(),
            name: resp["group_name"].as_str().map(String::from),
            avatar: Some(format!(
                "https://p.qlogo.cn/gh/{}/{}/100",
                guild_id, guild_id
            )),
        })
    }

    async fn list_guilds(&self, _next: Option<&str>) -> AyjxResult<PagedList<Guild>> {
        let resp = self.get_group_list(None, None).await?;
        let mut guilds = Vec::new();
        for item in resp {
            let gid = item["group_id"].to_string();
            guilds.push(Guild {
                id: gid.clone(),
                name: item["group_name"].as_str().map(String::from),
                avatar: Some(format!("https://p.qlogo.cn/gh/{}/{}/100", gid, gid)),
            });
        }
        Ok(PagedList::new(guilds))
    }

    // ----- 群组成员 API -----

    async fn get_guild_member(&self, guild_id: &str, user_id: &str) -> AyjxResult<GuildMember> {
        let resp = self
            .get_group_member_info(guild_id, user_id, None, None)
            .await?;

        Ok(GuildMember {
            user: Some(User {
                id: resp["user_id"].to_string(),
                name: resp["nickname"].as_str().map(String::from),
                nick: resp["card"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                ..Default::default()
            }),
            nick: resp["card"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from),
            avatar: None,
            joined_at: resp["join_time"].as_i64().map(|t| t * 1000),
        })
    }

    async fn list_guild_members(
        &self,
        guild_id: &str,
        _next: Option<&str>,
    ) -> AyjxResult<PagedList<GuildMember>> {
        let resp = self.get_group_member_list(guild_id, None, None).await?;

        let mut members = Vec::new();
        for item in resp {
            members.push(GuildMember {
                user: Some(User {
                    id: item["user_id"].to_string(),
                    name: item["nickname"].as_str().map(String::from),
                    nick: item["card"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from),
                    ..Default::default()
                }),
                nick: item["card"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                avatar: None,
                joined_at: item["join_time"].as_i64().map(|t| t * 1000),
            });
        }
        Ok(PagedList::new(members))
    }

    async fn kick_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        permanent: bool,
    ) -> AyjxResult<()> {
        self.set_group_kick(guild_id, user_id, Some(permanent), None)
            .await
    }

    async fn mute_guild_member(
        &self,
        guild_id: &str,
        user_id: &str,
        duration_ms: u64,
    ) -> AyjxResult<()> {
        self.set_group_ban(guild_id, user_id, (duration_ms / 1000) as u32, None)
            .await
    }

    async fn approve_guild_member(
        &self,
        message_id: &str,
        approve: bool,
        comment: Option<&str>,
    ) -> AyjxResult<()> {
        // message_id 对应 flag
        self.set_group_add_request(message_id, approve, comment, None)
            .await
    }

    async fn set_guild_member_role(
        &self,
        guild_id: &str,
        user_id: &str,
        role_id: &str,
    ) -> AyjxResult<()> {
        // OneBot 仅支持设置管理员
        if role_id == "admin" {
            self.set_group_admin(guild_id, user_id, true, None).await
        } else {
            Err("Unsupported role".into())
        }
    }

    async fn unset_guild_member_role(
        &self,
        guild_id: &str,
        user_id: &str,
        role_id: &str,
    ) -> AyjxResult<()> {
        if role_id == "admin" {
            self.set_group_admin(guild_id, user_id, false, None).await
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// 4. 连接实现
// ============================================================================

impl NapCatAdapter {
    /// 正向 WebSocket
    fn start_ws_forward(
        &self,
        url: String,
        access_token: Option<String>,
        reconnect_interval_ms: u64,
        ctx: AdapterContext,
    ) {
        // 如果 access_token 为 None 或空字符串，则不启动连接任务
        let has_token = access_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);

        if !has_token {
            return;
        }

        let inner = self.inner.clone();

        tokio::spawn(async move {
            loop {
                match Self::ws_forward_connect(&url, &access_token, &inner, &ctx).await {
                    Ok(self_id) => {
                        inner.remove_connection(&self_id).await;
                    }
                    Err(e) => {
                        eprintln!("[NapCat] WS 连接失败: {}", e);
                    }
                }
                println!("[NapCat] {}ms 后重连...", reconnect_interval_ms);
                tokio::time::sleep(Duration::from_millis(reconnect_interval_ms)).await;
            }
        });
    }

    async fn ws_forward_connect(
        url: &str,
        access_token: &Option<String>,
        inner: &Arc<AdapterInner>,
        ctx: &AdapterContext,
    ) -> AyjxResult<String> {
        let mut request = url
            .parse::<http::Uri>()
            .map_err(|e| format!("Invalid URL: {}", e))?
            .into_client_request()
            .map_err(|e| format!("Failed to create request: {}", e))?;

        if let Some(token) = access_token {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", token).parse().unwrap(),
            );
        }

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| format!("WebSocket connect failed: {}", e))?;

        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        // 发送任务
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(WsMessage::Text(msg)).await.is_err() {
                    break;
                }
            }
        });

        let mut self_id_cache = String::new();
        let temp_adapter = NapCatAdapter {
            inner: inner.clone(),
        };

        // 接收循环
        while let Some(Ok(msg)) = read.next().await {
            if let WsMessage::Text(text) = msg
                && let Ok(json) = serde_json::from_str::<Value>(&text)
            {
                // 处理 API 响应
                if let Some(echo) = json["echo"].as_str() {
                    let mut pending = inner.pending_responses.write().await;
                    if let Some(sender) = pending.remove(echo) {
                        let _ = sender.send(json);
                        continue;
                    }
                }

                // 获取 self_id 并注册连接
                if let Some(sid) = json["self_id"].as_i64() {
                    let sid_str = sid.to_string();
                    if self_id_cache != sid_str {
                        self_id_cache = sid_str.clone();
                        inner
                            .register_connection(self_id_cache.clone(), BotSender::Ws(tx.clone()))
                            .await;

                        // --- 生命周期：登录创建 ---
                        let mut login = Login::new("qq", "napcat");
                        login.user = Some(User::new(self_id_cache.clone()));
                        login.status = LoginStatus::Online;

                        let _ = ctx.event_tx.send(Event::login_added(login.clone())).await;
                        let _ = ctx.event_tx.send(Event::login_updated(login)).await;

                        let adapter_clone = NapCatAdapter {
                            inner: inner.clone(),
                        };
                        let sid_clone = self_id_cache.clone();

                        tokio::spawn(async move {
                            adapter_clone.update_login_info(sid_clone.clone()).await;
                        });
                    }
                }

                if json.get("post_type").is_some() {
                    let login = temp_adapter.get_cached_login(&self_id_cache).await;
                    if let Some(event) = onebot_event_to_satori(json, login) {
                        let _ = ctx.event_tx.send(event).await;
                    }
                }
            }
        }

        // --- 生命周期：连接断开 ---
        if !self_id_cache.is_empty() {
            let mut login = temp_adapter.get_cached_login(&self_id_cache).await;
            login.status = LoginStatus::Offline;

            let _ = ctx.event_tx.send(Event::login_updated(login.clone())).await;
            let _ = ctx.event_tx.send(Event::login_removed(login)).await;
        }

        Ok(self_id_cache)
    }

    /// 反向 WebSocket
    fn start_ws_reverse(
        &self,
        host: String,
        port: u16,
        access_token: Option<String>,
        ctx: AdapterContext,
    ) {
        // 如果 access_token 为 None 或空字符串，则不启动监听任务
        let has_token = access_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);

        if !has_token {
            return;
        }

        let inner = self.inner.clone();

        tokio::spawn(async move {
            use tokio::net::TcpListener;
            use tokio_tungstenite::accept_hdr_async;
            use tokio_tungstenite::tungstenite::handshake::server::{
                ErrorResponse, Request, Response,
            };

            let addr = format!("{}:{}", host, port);
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[NapCat] 反向 WS 监听失败: {}", e);
                    return;
                }
            };

            while let Ok((stream, peer_addr)) = listener.accept().await {
                let inner = inner.clone();
                let ctx = ctx.clone();
                let token = access_token.clone();

                tokio::spawn(async move {
                    let callback = |req: &Request,
                                    mut res: Response|
                     -> Result<Response, ErrorResponse> {
                        if let Some(token) = &token {
                            let headers = req.headers();
                            let auth_ok = headers
                                .get("Authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(|v| v == format!("Bearer {}", token))
                                .unwrap_or(false);

                            let query_ok = req
                                .uri()
                                .query()
                                .map(|q| q.contains(&format!("access_token={}", token)))
                                .unwrap_or(false);

                            if !auth_ok && !query_ok {
                                *res.status_mut() = http::StatusCode::UNAUTHORIZED;
                                return Err(ErrorResponse::new(Some("Unauthorized".to_string())));
                            }
                        }
                        Ok(res)
                    };

                    let ws_stream = match accept_hdr_async(stream, callback).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            eprintln!("[NapCat] WS 握手失败 {}: {}", peer_addr, e);
                            return;
                        }
                    };

                    let (mut write, mut read) = ws_stream.split();
                    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

                    // 发送任务
                    tokio::spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            if write.send(WsMessage::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                    });

                    let mut self_id = String::new();
                    let temp_adapter = NapCatAdapter {
                        inner: inner.clone(),
                    };

                    while let Some(Ok(msg)) = read.next().await {
                        if let WsMessage::Text(text) = msg
                            && let Ok(json) = serde_json::from_str::<Value>(&text)
                        {
                            // 处理 API 响应
                            if let Some(echo) = json["echo"].as_str() {
                                let mut pending = inner.pending_responses.write().await;
                                if let Some(sender) = pending.remove(echo) {
                                    let _ = sender.send(json);
                                    continue;
                                }
                            }

                            // 注册连接
                            if let Some(sid) = json["self_id"].as_i64() {
                                let sid_str = sid.to_string();
                                if self_id != sid_str {
                                    self_id = sid_str.clone();
                                    inner
                                        .register_connection(
                                            self_id.clone(),
                                            BotSender::Ws(tx.clone()),
                                        )
                                        .await;

                                    // --- 生命周期：登录创建 ---
                                    let mut login = Login::new("qq", "napcat");
                                    login.user = Some(User::new(self_id.clone()));
                                    login.status = LoginStatus::Online;

                                    let _ =
                                        ctx.event_tx.send(Event::login_added(login.clone())).await;
                                    let _ = ctx.event_tx.send(Event::login_updated(login)).await;

                                    let adapter_clone = NapCatAdapter {
                                        inner: inner.clone(),
                                    };
                                    let sid_clone = self_id.clone();

                                    tokio::spawn(async move {
                                        adapter_clone.update_login_info(sid_clone.clone()).await;
                                    });
                                }
                            }

                            // 处理事件
                            if json.get("post_type").is_some() {
                                let login = temp_adapter.get_cached_login(&self_id).await;
                                if let Some(event) = onebot_event_to_satori(json, login) {
                                    let _ = ctx.event_tx.send(event).await;
                                }
                            }
                        }
                    }

                    // --- 生命周期：连接断开 ---
                    if !self_id.is_empty() {
                        let mut login = temp_adapter.get_cached_login(&self_id).await;
                        login.status = LoginStatus::Offline;

                        let _ = ctx.event_tx.send(Event::login_updated(login.clone())).await;
                        let _ = ctx.event_tx.send(Event::login_removed(login)).await;

                        inner.remove_connection(&self_id).await;
                    }
                });
            }
        });
    }
}

// ============================================================================
// 5. 事件转换 (OneBot -> Satori)
// ============================================================================

/// 将 OneBot 事件转换为 Satori 事件
pub fn onebot_event_to_satori(json: Value, login: Login) -> Option<Event> {
    let post_type = json["post_type"].as_str()?;
    let time = json["time"].as_i64().unwrap_or(0) * 1000;

    let self_id = login.user.as_ref()?.id.clone();

    // 执行转换
    let mut event = match post_type {
        // 标准消息事件 -> message-created
        "message" => convert_message_event(&json, time, &self_id, event_types::MESSAGE_CREATED),
        // 自身发送/回显事件 -> message-sent (自定义类型，区别于 message-created)
        // 这样既保留了服务端同步过来的自身消息，又不会与 Adapter 层主动推送的事件 ID 冲突或语义混淆
        "message_sent" => convert_message_event(&json, time, &self_id, "message-sent"),
        "notice" => convert_notice_event(&json, time),
        "request" => convert_request_event(&json, time),
        "meta_event" => convert_meta_event(&json, time),
        _ => None,
    }?;

    // 统一善后处理
    event.login = Some(login);
    event.platform_data = Some(Arc::new(json.clone()));

    Some(event)
}

/// 尝试从 JSON 中获取字符串或数字并转为 String
fn get_id_string(v: &Value) -> Option<String> {
    v.as_str()
        .map(|s| s.to_string())
        .or_else(|| v.as_i64().map(|i| i.to_string()))
}

/// 处理消息事件 (包含 message 和 message_sent)
/// 增加 event_type 参数以区分标准消息和自身发送的消息
fn convert_message_event(
    json: &Value,
    time: i64,
    self_id: &str,
    event_type: &str,
) -> Option<Event> {
    let mut event = Event::new(event_type);
    // 1. 基础字段校验
    let msg_type = json.get("message_type")?.as_str()?;

    // 解析 message_id，默认 "0"
    let message_id = json
        .get("message_id")
        .and_then(get_id_string)
        .unwrap_or_else(|| "0".to_string());

    let content = parse_onebot_message(&json["message"]);

    // --- 2. 用户信息解析 ---
    // 返回元组: (user_id, nickname, role, is_anonymous)
    let (user_id, nickname, role_id, is_anonymous) =
        if let Some(anon) = json.get("anonymous").and_then(|v| v.as_object()) {
            // 匿名消息处理
            let uid = anon
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default();
            let name = anon
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            (uid, name.to_string(), String::new(), true)
        } else {
            // 正常消息处理
            let uid = get_id_string(&json["user_id"])
                .or_else(|| get_id_string(&json["sender"]["user_id"]))
                .unwrap_or_else(|| "0".to_string());

            let sender = json.get("sender");
            let name = sender
                .and_then(|s| s.get("nickname").and_then(|v| v.as_str()))
                .unwrap_or("");
            let role = sender
                .and_then(|s| s.get("role").and_then(|v| v.as_str()))
                .unwrap_or("");

            (uid, name.to_string(), role.to_string(), false)
        };

    // 头像生成
    let avatar = if !is_anonymous && user_id != "0" && !user_id.is_empty() {
        Some(format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", user_id))
    } else {
        None
    };

    // 构建 User 对象
    let mut user = User::new(user_id.clone());
    user.is_bot = Some(user_id == self_id);
    user.avatar = avatar.clone();
    if !nickname.is_empty() {
        user.name = Some(nickname.clone());
        user.nick = Some(nickname.clone());
    }

    // 构建 GuildMember 对象
    let member = GuildMember {
        user: Some(user.clone()),
        avatar: avatar.clone(),
        nick: if !nickname.is_empty() {
            Some(nickname)
        } else {
            None
        },
        ..Default::default()
    };

    // --- 3. 消息与频道构建 ---
    let mut msg = Message::new(message_id, content);
    msg.user = Some(user.clone());
    msg.created_at = Some(time);
    event.user = Some(user);

    // 根据消息类型处理 Guild/Channel 信息
    if msg_type == "group" {
        let group_id = json
            .get("group_id")
            .and_then(get_id_string)
            .unwrap_or_else(|| "0".to_string());

        let group_name = json
            .get("group_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // 构建 Guild
        let mut guild = Guild::new(group_id.clone());
        guild.name = group_name.clone();
        guild.avatar = Some(format!(
            "http://p.qlogo.cn/gh/{}/{}/640/",
            group_id, group_id
        ));

        msg.guild = Some(guild.clone());
        event.guild = Some(guild);

        // 构建 Channel
        let mut channel = Channel::new(group_id, ChannelType::Text);
        channel.name = group_name;
        msg.channel = Some(channel.clone());
        event.channel = Some(channel);
        msg.member = Some(member);
    } else {
        // 私聊
        msg.channel = Some(Channel::new(
            format!("private:{}", user_id),
            ChannelType::Direct,
        ));
    }

    // --- 4. 补充 Event ---
    event.message = Some(msg);
    event.platform_type = Some(msg_type.to_string());
    event.timestamp = time;

    if !role_id.is_empty() && msg_type == "group" {
        event.role = Some(GuildRole::new(role_id));
    }

    Some(event)
}

/// 处理通知事件 (Notice Event)
fn convert_notice_event(json: &Value, time: i64) -> Option<Event> {
    let notice_type = json["notice_type"].as_str()?;
    let sub_type = json["sub_type"].as_str();

    // 1. 确定 Satori 标准类型 (Type)
    // 2. 确定平台原生类型 (Platform Type / _type)

    let (satori_type, platform_type) = match notice_type {
        // 群文件上传 -> 消息事件
        "group_upload" => (event_types::MESSAGE_CREATED, "group_upload"),

        // 群成员增加
        "group_increase" => (event_types::GUILD_MEMBER_ADDED, "group_increase"),

        // 群成员减少
        "group_decrease" => {
            if sub_type == Some("kick_me") {
                (event_types::GUILD_REMOVED, "group_decrease/kick_me")
            } else {
                (event_types::GUILD_MEMBER_REMOVED, "group_decrease")
            }
        }

        // 消息撤回 (好友/群)
        "group_recall" | "friend_recall" => (event_types::MESSAGE_DELETED, notice_type),

        // 好友增加
        "friend_add" => ("friend-added", "friend_add"),

        // 群禁言 -> 成员更新
        "group_ban" => (event_types::GUILD_MEMBER_UPDATED, "group_ban"),

        // 群管理员变动 -> 成员更新
        "group_admin" => (event_types::GUILD_MEMBER_UPDATED, "group_admin"),

        // 群名片变更 -> 成员更新
        "group_card" => (event_types::GUILD_MEMBER_UPDATED, "group_card"),

        // 扩展通知 (戳一戳, 荣誉, 头衔, 点赞, 输入状态)
        "notify" => match sub_type {
            Some("poke") => ("interaction/poke", "notify/poke"),
            Some("lucky_king") => ("interaction/lucky_king", "notify/lucky_king"),
            Some("honor") => (event_types::GUILD_MEMBER_UPDATED, "notify/honor"),
            // NapCat 补充事件：群成员头衔变更
            Some("title") => (event_types::GUILD_MEMBER_UPDATED, "notify/title"),
            // NapCat 补充事件：点赞
            Some("profile_like") => ("interaction/like", "notify/profile_like"),
            // NapCat 补充事件：输入状态更新
            Some("input_status") => ("interaction/typing", "notify/input_status"),
            _ => return None, // 未知通知忽略
        },

        // 表情回应 (NapCat 仅收自己的，其余扩展接口拉取)
        "group_msg_emoji_like" => (event_types::REACTION_ADDED, "group_msg_emoji_like"),

        // 精华消息
        "essence" => ("interaction/essence", "essence"),

        _ => return None,
    };

    // 创建事件
    let mut event = Event::new(satori_type);
    event.platform_type = Some(platform_type.to_string());
    event.timestamp = time;

    // --- 填充特定事件数据 ---

    // 特殊处理：群文件上传需要构造消息内容
    if notice_type == "group_upload"
        && let Some(file) = json.get("file")
    {
        let name = file["name"].as_str().unwrap_or("file");
        let size = file["size"].as_u64().unwrap_or(0);
        let busid = file["busid"].as_i64().unwrap_or(0);

        event.message = Some(Message {
            id: time.to_string(),
            content: format!(
                r#"<file name="{}" size="{}" busid="{}"/>"#,
                escape_xml(name),
                size,
                busid
            ),
            created_at: Some(time),
            ..Default::default()
        });
    }

    // 特殊处理：消息撤回需要 ID
    if notice_type.contains("recall")
        && let Some(mid) = json["message_id"].as_i64()
    {
        event.message = Some(Message::new(mid.to_string(), ""));
    }

    // --- 填充通用实体 (Guild, Channel, User, Operator) ---

    // 1. 填充 Guild 和 Channel
    if let Some(gid) = json["group_id"].as_i64() {
        let gid_str = gid.to_string();

        let mut guild = Guild::new(gid_str.clone());
        guild.avatar = Some(format!("http://p.qlogo.cn/gh/{}/{}/640/", gid_str, gid_str));
        event.guild = Some(guild);

        event.channel = Some(Channel::new(gid_str, ChannelType::Text));
    }

    // 2. 填充 User (事件主体) 和 Operator (操作者)
    // 逻辑：识别 user_id, target_id, operator_id 并正确映射

    // 默认情况：user_id 是主体
    let mut main_user_id = json["user_id"].as_i64();
    let mut operator_id = json["operator_id"].as_i64();

    // 戳一戳特殊逻辑：target_id 是被戳的人(User), user_id 是戳的人(Operator)
    if notice_type == "notify" && sub_type == Some("poke") {
        if let Some(target) = json["target_id"].as_i64() {
            main_user_id = Some(target); // 被戳者
        }
        // user_id 在 poke 里其实是 operator
        operator_id = json["user_id"].as_i64();
    }

    if notice_type == "group_msg_emoji_like" {
        operator_id = json["user_id"].as_i64();
        event.message = Some(Message {
            id: json["message_id"].as_i64().unwrap_or(0).to_string(),
            content: match json["likes"].as_array() {
                Some(arr) if !arr.is_empty() => {
                    let emoji_id = arr[0]
                        .get("emoji_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0");
                    format!(r#"<face id="{}"/>"#, emoji_id)
                }
                _ => String::new(),
            },
            ..Default::default()
        });
        main_user_id = None;
    }

    // 设置 User
    if let Some(uid) = main_user_id {
        let mut user = User::new(uid.to_string());
        user.is_bot = Some(user.id == json["self_id"].as_i64().unwrap_or(0).to_string());
        event.user = Some(user.clone());
        event.member = Some(GuildMember {
            user: Some(user.clone()),
            ..Default::default()
        });
    }

    // 设置 Operator
    // 如果显式有 operator_id，或者 poke 里的 user_id
    if let Some(oid) = operator_id {
        let oid_str = oid.to_string();
        // 避免自身操作自身时重复 (除非业务需要)
        let is_self = event
            .user
            .as_ref()
            .map(|u| u.id == oid_str)
            .unwrap_or(false);
        // 对于管理变动、禁言等管理操作，即使是自己操作自己，记录 operator 也有意义
        if !is_self
            || notice_type == "group_admin"
            || notice_type == "group_ban"
            || sub_type == Some("poke")
        {
            event.operator = Some(User::new(oid_str));
        }
    }

    Some(event)
}

/// 处理请求事件 (Request Event)
fn convert_request_event(json: &Value, time: i64) -> Option<Event> {
    let request_type = json["request_type"].as_str()?;
    let sub_type = json["sub_type"].as_str();

    let (satori_type, platform_type) = match request_type {
        "friend" => (event_types::FRIEND_REQUEST, "request/friend"),
        "group" => match sub_type {
            Some("invite") => (event_types::GUILD_REQUEST, "request/group/invite"),
            Some("add") => (event_types::GUILD_MEMBER_REQUEST, "request/group/add"),
            _ => return None,
        },
        _ => return None,
    };

    let mut event = Event::new(satori_type);
    event.platform_type = Some(platform_type.to_string());
    event.timestamp = time;

    if let Some(gid) = json["group_id"].as_i64() {
        let gid_str = gid.to_string();
        event.guild = Some(Guild::new(gid_str));
    }

    if let Some(uid) = json["user_id"].as_i64() {
        let mut user = User::new(uid.to_string());
        if let Some(nick) = json["nickname"].as_str() {
            user.nick = Some(nick.to_string());
        }
        event.user = Some(user);
    }

    // flag 用于处理请求 (approve/reject)，映射为 message.id
    if let Some(flag) = json["flag"].as_str() {
        let msg = Message {
            id: flag.to_string(),
            content: json["comment"].as_str().unwrap_or("").to_string(),
            ..Default::default()
        };
        event.message = Some(msg);
    }

    Some(event)
}

/// 处理元事件 (Meta Event)
fn convert_meta_event(json: &Value, time: i64) -> Option<Event> {
    let meta_type = json["meta_event_type"].as_str()?;

    match meta_type {
        "lifecycle" => {
            let sub_type = json["sub_type"].as_str().unwrap_or("connect");
            let event_type = match sub_type {
                "connect" | "enable" => event_types::LOGIN_ADDED,
                "disable" => event_types::LOGIN_REMOVED,
                _ => return None,
            };
            let mut event = Event::new(event_type);
            event.platform_type = Some(format!("lifecycle/{}", sub_type));
            event.timestamp = time;
            Some(event)
        }
        // 心跳事件通常用于保活，不转换为 Satori 业务事件
        "heartbeat" => None,
        _ => None,
    }
}

// ============================================================================
// 6. 消息转换
// ============================================================================

/// 解析 channel_id
fn parse_channel_id(channel_id: &str) -> (&str, &str) {
    if let Some(id) = channel_id.strip_prefix("private:") {
        ("private", id)
    } else if let Some(id) = channel_id.strip_prefix("group:") {
        ("group", id)
    } else {
        ("group", channel_id)
    }
}

/// OneBot 消息 -> Satori XML
fn parse_onebot_message(message: &Value) -> String {
    let mut result = String::new();

    if let Some(arr) = message.as_array() {
        for seg in arr {
            let seg_type = seg["type"].as_str().unwrap_or("text");
            let data = &seg["data"];

            match seg_type {
                "text" => {
                    let text = data["text"].as_str().unwrap_or("");
                    result.push_str(&escape_xml(text));
                }
                "at" => {
                    let qq = data["qq"]
                        .as_str()
                        .or_else(|| data["qq"].as_i64().map(|_| ""))
                        .unwrap_or("");
                    if qq == "all" {
                        result.push_str(r#"<at type="all"/>"#);
                    } else {
                        let qq_str = data["qq"]
                            .as_i64()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| qq.to_string());
                        result.push_str(&format!(r#"<at id="{}"/>"#, qq_str));
                    }
                }
                "face" => {
                    let id = data["id"]
                        .as_str()
                        .or_else(|| data["id"].as_i64().map(|_| ""))
                        .unwrap_or("0");
                    let id_str = data["id"]
                        .as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| id.to_string());
                    result.push_str(&format!(r#"<face id="{}"/>"#, id_str));
                }
                "image" => {
                    let url = data["url"]
                        .as_str()
                        .or_else(|| data["file"].as_str())
                        .unwrap_or("");
                    result.push_str(&format!(r#"<img src="{}"/>"#, escape_xml(url)));
                }
                "record" => {
                    let url = data["url"]
                        .as_str()
                        .or_else(|| data["file"].as_str())
                        .unwrap_or("");
                    result.push_str(&format!(r#"<audio src="{}"/>"#, escape_xml(url)));
                }
                "video" => {
                    let url = data["url"]
                        .as_str()
                        .or_else(|| data["file"].as_str())
                        .unwrap_or("");
                    result.push_str(&format!(r#"<video src="{}"/>"#, escape_xml(url)));
                }
                "reply" => {
                    let id = data["id"]
                        .as_str()
                        .or_else(|| data["id"].as_i64().map(|_| ""))
                        .unwrap_or("");
                    let id_str = data["id"]
                        .as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| id.to_string());
                    result.push_str(&format!(r#"<quote id="{}"/>"#, id_str));
                }
                "forward" => {
                    let id = data["id"].as_str().unwrap_or("");
                    result.push_str(&format!(r#"<message forward id="{}"/>"#, id));
                }
                "share" => {
                    let url = data["url"].as_str().unwrap_or("");
                    let title = data["title"].as_str().unwrap_or("");
                    result.push_str(&format!(
                        r#"<a href="{}">{}</a>"#,
                        escape_xml(url),
                        escape_xml(title)
                    ));
                }
                _ => {
                    // 未知类型，尝试提取文本
                    if let Some(text) = data["text"].as_str() {
                        result.push_str(&escape_xml(text));
                    }
                }
            }
        }
    } else if let Some(s) = message.as_str() {
        // CQ 码字符串格式 (简化处理，直接返回)
        result.push_str(s);
    }

    result
}

/// Satori XML -> OneBot 消息数组 (入口 - 处理纯字符串)
fn satori_to_onebot(content: &str) -> Value {
    // 如果内容不包含 XML 标签特征字符，直接作为纯文本处理。
    if !content.contains('<') && !content.contains('>') {
        return json!([
            {
                "type": "text",
                "data": {
                    "text": content
                }
            }
        ]);
    }

    let elements = message_elements::parse(content);
    let result = elements_to_onebot(&elements);

    // 二次防护：检测解析器吞字现象
    if let Some(arr) = result.as_array()
        && arr.len() == 1
        && let Some(obj) = arr[0].as_object()
        && let Some(type_val) = obj.get("type").and_then(|v| v.as_str())
        && type_val == "text"
        && let Some(data) = obj.get("data")
        && let Some(text_val) = data.get("text").and_then(|v| v.as_str())
        && text_val.len() < content.len() * 7 / 10
        && !content.is_empty()
    {
        return json!([
            {
                "type": "text",
                "data": {
                    "text": content
                }
            }
        ]);
    }

    result
}

/// Satori Elements -> OneBot 消息数组 (供 send_message 分段调用)
fn elements_to_onebot(elements: &[Element]) -> Value {
    let mut arr = Vec::new();
    process_elements_to_onebot(elements, &mut arr);
    json!(arr)
}

/// 递归处理消息元素并填充到 OneBot 数组
fn process_elements_to_onebot(elements: &[Element], arr: &mut Vec<Value>) {
    for elem in elements {
        match elem {
            // ----- 基础叶子节点 -----
            Element::Text(t) => {
                if !t.is_empty() {
                    arr.push(json!({"type": "text", "data": {"text": t}}));
                }
            }
            Element::At { id, at_type, .. } => {
                if at_type.as_deref() == Some("all") {
                    arr.push(json!({"type": "at", "data": {"qq": "all"}}));
                } else if let Some(uid) = id {
                    arr.push(json!({"type": "at", "data": {"qq": uid}}));
                }
            }
            Element::Image { src, .. } => {
                arr.push(json!({"type": "image", "data": {"file": src}}));
            }
            Element::Audio { src, .. } => {
                arr.push(json!({"type": "record", "data": {"file": src}}));
            }
            Element::Video { src, .. } => {
                arr.push(json!({"type": "video", "data": {"file": src}}));
            }
            Element::Quote { id, .. } => {
                if let Some(mid) = id {
                    arr.push(json!({"type": "reply", "data": {"id": mid}}));
                }
            }
            Element::Break => {
                arr.push(json!({"type": "text", "data": {"text": "\n"}}));
            }

            // ----- 容器节点 (特殊处理 Message) -----
            Element::Message {
                forward,
                id,
                children,
                ..
            } => {
                // 1. 处理合并转发 (forward=true)
                if *forward {
                    // 情况 A: 引用已有的转发消息 ID (Forward by ID)
                    // <message forward id="..."/>
                    if let Some(forward_id) = id
                        && !forward_id.is_empty()
                    {
                        arr.push(json!({
                            "type": "forward",
                            "data": {
                                "id": forward_id
                            }
                        }));
                    }
                    // 情况 B: 自定义合并转发内容 (Custom Merged Forward)
                    // <message forward><message>...</message><message>...</message></message>
                    else {
                        for child in children {
                            // Satori 规范：合并转发中的每一条消息都由 <message> 包裹
                            if let Element::Message {
                                id: node_msg_id,
                                children: node_children,
                                ..
                            } = child
                            {
                                // 子节点类型 1: 引用消息 (通过 ID)
                                // <message id="123456"/>
                                if let Some(nid) = node_msg_id
                                    && !nid.is_empty()
                                {
                                    arr.push(json!({
                                        "type": "node",
                                        "data": {
                                            "id": nid
                                        }
                                    }));
                                }
                                // 子节点类型 2: 自定义/伪造消息
                                // <message><author .../> content...</message>
                                else {
                                    let mut user_id = "0".to_string(); // 默认 ID
                                    let mut nickname = "匿名".to_string(); // 默认昵称
                                    let mut content_elems = Vec::new();

                                    // 分离 <author> 和实际内容
                                    for sub in node_children {
                                        if let Element::Author {
                                            id: auth_id,
                                            name: auth_name,
                                            ..
                                        } = sub
                                        {
                                            if let Some(uid) = auth_id {
                                                user_id = uid.clone();
                                            }
                                            if let Some(nm) = auth_name {
                                                nickname = nm.clone();
                                            }
                                        } else {
                                            content_elems.push(sub.clone());
                                        }
                                    }

                                    // 递归解析节点内部的内容
                                    let mut node_content_arr = Vec::new();
                                    process_elements_to_onebot(
                                        &content_elems,
                                        &mut node_content_arr,
                                    );

                                    // 只有内容不为空时才生成节点
                                    if !node_content_arr.is_empty() {
                                        arr.push(json!({
                                            "type": "node",
                                            "data": {
                                                "user_id": user_id,
                                                "nickname": nickname,
                                                "content": node_content_arr
                                            }
                                        }));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // 2. 普通消息容器 (forward=false)
                    // <message>content</message>
                    // 在 OneBot 消息链中，我们将其子元素平铺 (Flatten)。
                    // 这样 <message>A</message><message>B</message> 如果在 send_message 中被分开处理，
                    // 则各自调用此函数；如果在递归中遇到嵌套的无属性 <message>，则解包。
                    process_elements_to_onebot(children, arr);
                }
            }

            // ----- 容器节点 (Struct Variants) -----
            Element::Unknown {
                tag,
                attrs,
                children,
            } => {
                if tag == "face"
                    && let Some(id) = attrs.get("id")
                {
                    arr.push(json!({"type": "face", "data": {"id": id}}));
                }
                process_elements_to_onebot(children, arr);
            }

            // ----- 容器节点 (Tuple Variants) -----
            Element::Bold(children)
            | Element::Italic(children)
            | Element::Underline(children)
            | Element::Strikethrough(children)
            | Element::Spoiler(children)
            | Element::Superscript(children)
            | Element::Subscript(children)
            | Element::Paragraph(children) => {
                process_elements_to_onebot(children, arr);
            }

            // ----- 特殊处理 -----
            Element::Link { href, children } => {
                let text = message_elements::to_plain_text(children);
                let display = if text.is_empty() {
                    href.clone()
                } else {
                    format!("{} ({})", text, href)
                };
                arr.push(json!({"type": "text", "data": {"text": display}}));
            }

            Element::Code(text) => {
                arr.push(json!({"type": "text", "data": {"text": text}}));
            }

            // Author 标签由父级 Message 节点处理，如果单独出现则忽略
            Element::Author { .. } => {}

            _ => {
                let text = message_elements::to_plain_text(std::slice::from_ref(elem));
                if !text.is_empty() {
                    arr.push(json!({"type": "text", "data": {"text": text}}));
                }
            }
        }
    }
}

/// 转义 XML 特殊字符 (用于文件消息构建)
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
