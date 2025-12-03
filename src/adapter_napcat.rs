//! NapCat (OneBot V11) 适配器
//!
//! 基于 OneBot V11 协议，增加了 NapCat 平台特有的 API 支持。

#![allow(dead_code)]

use ayjx::prelude::*;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
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

#[derive(Debug, Deserialize, Clone, Default)]
pub struct NapCatConfig {
    #[serde(default)]
    pub bots: Vec<BotConfig>,
}

#[derive(Debug, Deserialize, Clone)]
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
    /// HTTP 模式
    Http {
        api_url: String,
        #[serde(default)]
        event_port: Option<u16>,
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default)]
        secret: Option<String>,
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
    Http {
        client: reqwest::Client,
        api_url: String,
        access_token: Option<String>,
    },
}

/// 适配器内部状态
struct AdapterInner {
    connections: RwLock<HashMap<String, BotSender>>,
    pending_responses: RwLock<HashMap<String, oneshot::Sender<Value>>>,
}

impl AdapterInner {
    async fn register_connection(&self, self_id: String, sender: BotSender) {
        let mut map = self.connections.write().await;
        map.insert(self_id.clone(), sender);
        ayjx_info!("[NapCat] Bot {} 已连接", self_id);
        ayjx_info!("--------------------------------------------------");
    }

    async fn remove_connection(&self, self_id: &str) {
        let mut map = self.connections.write().await;
        if map.remove(self_id).is_some() {
            ayjx_info!("[NapCat] Bot {} 已断开", self_id);
        }
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
            }),
        }
    }

    /// 获取当前所有已连接 Bot 的 ID 和连接类型
    /// 用于测试和监控
    pub async fn get_connected_bots(&self) -> Vec<(String, String)> {
        let map = self.inner.connections.read().await;
        map.iter()
            .map(|(id, sender)| {
                let type_name = match sender {
                    BotSender::Ws(_) => "WebSocket",
                    BotSender::Http { .. } => "HTTP",
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
        .ok_or_else(|| AyjxError::Adapter("No active bot connection".into()))?;

        match sender {
            BotSender::Http {
                client,
                api_url,
                access_token,
            } => {
                let url = format!("{}/{}", api_url.trim_end_matches('/'), action);
                let mut req = client.post(&url).json(&params);
                if let Some(token) = &access_token {
                    req = req.header("Authorization", format!("Bearer {}", token));
                }

                let resp = req
                    .send()
                    .await
                    .map_err(|e| AyjxError::Io(std::io::Error::other(e)))?;

                let json: Value = resp
                    .json()
                    .await
                    .map_err(|e| AyjxError::Serde(e.to_string()))?;

                Self::check_api_response(json)
            }
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
                    .map_err(|_| AyjxError::Adapter("WebSocket channel closed".into()))?;

                match tokio::time::timeout(Duration::from_secs(60), resp_rx).await {
                    Ok(Ok(json)) => Self::check_api_response(json),
                    Ok(Err(_)) => Err(AyjxError::Adapter("Response channel closed".into())),
                    Err(_) => {
                        let mut pending = self.inner.pending_responses.write().await;
                        pending.remove(&echo);
                        Err(AyjxError::Adapter("API request timeout".into()))
                    }
                }
            }
        }
    }

    fn check_api_response(json: Value) -> AyjxResult<Value> {
        match json["status"].as_str() {
            Some("ok") => Ok(json["data"].clone()),
            Some("async") => Ok(json["data"].clone()),
            Some("failed") => Err(AyjxError::Adapter(format!(
                "API failed: {} (retcode: {})",
                json["msg"].as_str().unwrap_or("unknown"),
                json["retcode"]
            ))),
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
        env!("CARGO_PKG_VERSION")
    }
    fn platforms(&self) -> Vec<&str> {
        vec!["qq", "onebot", "napcat"]
    }

    async fn start(&self, ctx: AdapterContext) -> AyjxResult<()> {
        let app_cfg = ctx.config.get().await;
        let config: NapCatConfig = app_cfg
            .plugins
            .get("napcat")
            .map(|v| v.clone().try_into())
            .transpose()?
            .unwrap_or_default();

        if config.bots.is_empty() {
            ayjx_warn!(
                "[NapCat] 未配置任何 Bot，请在 config.toml 的 [napcat] 下添加 [[napcat.bots]]"
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
                    self.start_ws_forward(url, access_token, reconnect_interval_ms, ctx.clone());
                }
                BotConfig::WsReverse {
                    host,
                    port,
                    access_token,
                } => {
                    self.start_ws_reverse(host, port, access_token, ctx.clone());
                }
                BotConfig::Http {
                    api_url,
                    event_port,
                    access_token,
                    secret,
                } => {
                    self.start_http(api_url, event_port, access_token, secret, ctx.clone());
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
            let mut login = Login::new("qq", "napcat");
            login.user = Some(User::new(id.clone()));
            login.status = LoginStatus::Online;
            Ok(login)
        } else {
            Ok(Login::new("qq", "napcat"))
        }
    }

    // ----- 消息 API -----

    // ----- 消息 API (修改后) -----

    async fn send_message(&self, channel_id: &str, content: &str) -> AyjxResult<Vec<Message>> {
        let (msg_type, target_id) = parse_channel_id(channel_id);

        // 统一逻辑：将 Satori XML 转换为 OneBot 消息数组
        // 无论是普通消息、合并转发(node)还是引用转发(forward id)，都由转换函数处理
        let ob_message_val = satori_to_onebot(content);
        let ob_message_vec = ob_message_val
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![ob_message_val]);

        // 直接调用发送接口
        // NapCat/OneBot 接收到包含 "type": "node" 的数组时，会自动处理为合并转发消息
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

        Ok(vec![Message {
            id: msg_id,
            content: content.to_string(),
            ..Default::default()
        }])
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

        // Satori 的 next 通常是分页 token，这里映射为 OneBot 的 message_seq (起始序号)
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
                // 简单的转换，实际可能需要复用 convert_message_event 的逻辑
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
        // get_friend_list 返回的是 Vec<Value>
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
        // message_id 在好友申请事件中对应 flag
        self.set_friend_add_request(message_id, approve, comment.unwrap_or(""), None)
            .await
    }

    // ----- 频道 API (映射为群组) -----

    async fn get_channel(&self, channel_id: &str) -> AyjxResult<Channel> {
        let (ctype, target_id) = parse_channel_id(channel_id);
        if ctype == "private" {
            // 私聊视作 Direct 频道
            return Ok(Channel::new(channel_id, ChannelType::Direct));
        }

        // 群组视作 Text 频道
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
        // 在 OneBot 模型中，Guild ID (Group ID) 本身就是一个 Channel
        // 如果这里的 guild_id 是指平台层面的集合（比如频道所在的服务器），OneBot 没有这个概念
        // 但如果这里的 context 是获取某群的信息作为 channel：
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
        // duration_ms > 0 开启全体禁言，否则关闭
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
            Err(AyjxError::PermissionDenied("Unsupported role".into()))
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
        let inner = self.inner.clone();

        tokio::spawn(async move {
            loop {
                match Self::ws_forward_connect(&url, &access_token, &inner, &ctx).await {
                    Ok(self_id) => {
                        ayjx_warn!("[NapCat] WS 连接断开 (bot: {})", self_id);
                        inner.remove_connection(&self_id).await;
                    }
                    Err(e) => {
                        ayjx_error!("[NapCat] WS 连接失败: {}", e);
                    }
                }
                ayjx_info!("[NapCat] {}ms 后重连...", reconnect_interval_ms);
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
            .map_err(|e| AyjxError::Config(format!("Invalid URL: {}", e)))?
            .into_client_request()
            .map_err(|e| AyjxError::Config(format!("Failed to create request: {}", e)))?;

        if let Some(token) = access_token {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", token).parse().unwrap(),
            );
        }

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| AyjxError::Adapter(format!("WebSocket connect failed: {}", e)))?;

        ayjx_info!("[NapCat] 正向 WS 连接成功: {}", url);

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
                    }
                }

                // 处理事件
                if json.get("post_type").is_some()
                    && let Some(event) = onebot_event_to_satori(json, Some(self_id_cache.clone()))
                {
                    let _ = ctx.event_tx.send(event).await;
                }
            }
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
                    ayjx_error!("[NapCat] 反向 WS 监听失败: {}", e);
                    return;
                }
            };

            ayjx_info!("[NapCat] 反向 WS 服务器监听于 {}", addr);

            while let Ok((stream, peer_addr)) = listener.accept().await {
                let inner = inner.clone();
                let ctx = ctx.clone();
                let token = access_token.clone();

                tokio::spawn(async move {
                    // 验证 access_token 的回调函数
                    let callback = |req: &Request,
                                    mut res: Response|
                     -> Result<Response, ErrorResponse> {
                        if let Some(token) = &token {
                            let headers = req.headers();
                            // 1. 检查 HTTP Header: Authorization: Bearer <token>
                            let auth_ok = headers
                                .get("Authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(|v| v == format!("Bearer {}", token))
                                .unwrap_or(false);

                            // 2. 检查 Query Parameter: access_token=<token>
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

                    // 使用 accept_hdr_async 进行带回调的握手
                    let ws_stream = match accept_hdr_async(stream, callback).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            ayjx_warn!("[NapCat] WS 握手失败 {}: {}", peer_addr, e);
                            return;
                        }
                    };

                    ayjx_info!("[NapCat] 反向 WS 客户端连接: {}", peer_addr);

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
                                }
                            }

                            // 处理事件
                            if json.get("post_type").is_some()
                                && let Some(event) =
                                    onebot_event_to_satori(json, Some(self_id.clone()))
                            {
                                let _ = ctx.event_tx.send(event).await;
                            }
                        }
                    }

                    if !self_id.is_empty() {
                        inner.remove_connection(&self_id).await;
                    }
                    ayjx_info!("[NapCat] 反向 WS 客户端断开: {}", peer_addr);
                });
            }
        });
    }

    /// HTTP 模式
    fn start_http(
        &self,
        api_url: String,
        event_port: Option<u16>,
        access_token: Option<String>,
        secret: Option<String>,
        ctx: AdapterContext,
    ) {
        let inner = self.inner.clone();

        // 注册 HTTP 发送端
        let sender = BotSender::Http {
            client: reqwest::Client::new(),
            api_url: api_url.clone(),
            access_token: access_token.clone(),
        };

        tokio::spawn({
            let inner = inner.clone();
            async move {
                inner.register_connection("http".to_string(), sender).await;
            }
        });

        // 启动事件接收服务器
        if let Some(port) = event_port {
            tokio::spawn(async move {
                use axum::{
                    Router,
                    body::Bytes,
                    extract::State,
                    http::{HeaderMap, StatusCode},
                    routing::post,
                };
                use hmac::{Hmac, Mac};
                use sha1::Sha1;

                #[derive(Clone)]
                struct AppState {
                    event_tx: mpsc::Sender<Event>,
                    secret: Option<String>,
                }

                async fn handle_event(
                    State(state): State<AppState>,
                    headers: HeaderMap,
                    body_bytes: Bytes,
                ) -> Result<&'static str, StatusCode> {
                    // 验证 HMAC-SHA1 签名
                    if let Some(secret) = &state.secret {
                        let signature = headers.get("X-Signature").and_then(|v| v.to_str().ok());

                        // 获取 sha1=... 中的 hash 部分
                        let provided_sig = match signature {
                            Some(s) => {
                                if let Some(sig) = s.strip_prefix("sha1=") {
                                    sig
                                } else {
                                    return Err(StatusCode::BAD_REQUEST);
                                }
                            }
                            None => return Err(StatusCode::UNAUTHORIZED),
                        };

                        type HmacSha1 = Hmac<Sha1>;
                        let mut mac = HmacSha1::new_from_slice(secret.as_bytes())
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                        mac.update(&body_bytes);
                        let result = mac.finalize().into_bytes();
                        let expected_sig = hex::encode(result);

                        if provided_sig != expected_sig {
                            ayjx_warn!("[NapCat] HTTP 事件签名验证失败");
                            return Err(StatusCode::FORBIDDEN);
                        }
                    }

                    // 解析 JSON
                    let json: Value =
                        serde_json::from_slice(&body_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;

                    let self_id = headers
                        .get("X-Self-ID")
                        .and_then(|v| v.to_str().ok())
                        .map(String::from)
                        .or_else(|| json["self_id"].as_i64().map(|i| i.to_string()));

                    if let Some(event) = onebot_event_to_satori(json, self_id) {
                        let _ = state.event_tx.send(event).await;
                    }

                    Ok("")
                }

                let state = AppState {
                    event_tx: ctx.event_tx.clone(),
                    secret,
                };

                let app = Router::new()
                    .route("/", post(handle_event))
                    .with_state(state);

                let addr = format!("0.0.0.0:{}", port);
                ayjx_info!("[NapCat] HTTP 事件服务器监听于 {}", addr);

                match tokio::net::TcpListener::bind(&addr).await {
                    Ok(listener) => {
                        if let Err(e) = axum::serve(listener, app).await {
                            ayjx_error!("[NapCat] HTTP 事件服务器运行错误: {}", e);
                        }
                    }
                    Err(e) => {
                        ayjx_error!("[NapCat] HTTP 事件服务器启动失败: {}", e);
                    }
                }
            });
        }
    }
}

// ============================================================================
// 5. 事件转换 (OneBot -> Satori)
// ============================================================================

/// 将 OneBot 事件转换为 Satori 事件
///
/// 策略：
/// 1. 尽可能映射到 Satori 标准事件类型 (`type`) 以保证通用性。
/// 2. 将 OneBot 原始事件名保留在 `_type` (`platform_type`) 中，解决"语义模糊"问题。
/// 3. 将完整原始 JSON 保留在 `platform_data` 中，解决"字段缺失"问题。
pub fn onebot_event_to_satori(json: Value, self_id: Option<String>) -> Option<Event> {
    let post_type = json["post_type"].as_str()?;
    let time = json["time"].as_i64().unwrap_or(0) * 1000;

    // 优先使用事件中的 self_id
    let self_id = json["self_id"]
        .as_i64()
        .map(|i| i.to_string())
        .or(self_id)?;

    // 构建当前的 Login 状态信息
    let login = Login {
        platform: Some("qq".to_string()),
        adapter: Some("napcat".to_string()),
        status: LoginStatus::Online,
        user: Some(User::new(self_id.clone())),
        ..Default::default()
    };

    // 执行转换
    let mut event = match post_type {
        "message" | "message_sent" => convert_message_event(&json, time),
        "notice" => convert_notice_event(&json, time),
        "request" => convert_request_event(&json, time),
        "meta_event" => convert_meta_event(&json, time),
        _ => None,
    }?;

    // 统一善后处理
    event.login = Some(login);
    // 将原始 JSON 注入 platform_data，解决字段缺失问题 (如 duration, file 详情等)
    // 开发者可在业务层通过 downcast 获取原始数据
    event.platform_data = Some(Arc::new(json.clone()));

    Some(event)
}

/// 处理消息事件
fn convert_message_event(json: &Value, time: i64) -> Option<Event> {
    let msg_type = json["message_type"].as_str()?;

    // 处理 message_id
    let message_id = json["message_id"]
        .as_i64()
        .map(|i| i.to_string())
        .or_else(|| json["message_id"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "0".to_string());

    let content = parse_onebot_message(&json["message"]);

    // --- 用户信息解析 ---
    let mut user_id = String::new();
    let mut nickname = String::new();
    let mut avatar = None;
    let mut is_anonymous = false;

    // 1. 检查是否为匿名消息
    if let Some(anon) = json.get("anonymous").and_then(|v| v.as_object()) {
        is_anonymous = true;
        if let Some(anon_id) = anon.get("id").and_then(|v| v.as_i64()) {
            user_id = anon_id.to_string();
        }
        if let Some(anon_name) = anon.get("name").and_then(|v| v.as_str()) {
            nickname = anon_name.to_string();
        }
    } else {
        // 2. 正常消息
        user_id = json["user_id"]
            .as_i64()
            .map(|i| i.to_string())
            .or_else(|| json["sender"]["user_id"].as_i64().map(|i| i.to_string()))
            .unwrap_or_else(|| "0".to_string());

        if let Some(sender) = json.get("sender") {
            nickname = sender["nickname"].as_str().unwrap_or("").to_string();
        }
    }

    // 生成头像 URL
    if !is_anonymous && user_id != "0" && !user_id.is_empty() {
        avatar = Some(format!("https://q1.qlogo.cn/g?b=qq&nk={}&s=640", user_id));
    }

    // 构建 Satori User
    let mut user = User::new(user_id.clone());
    if !nickname.is_empty() {
        user.name = Some(nickname.clone());
        user.nick = Some(nickname.clone());
    }
    user.avatar = avatar.clone();
    user.is_bot = Some(false);

    // 构建 Satori Message
    let mut msg = Message::new(message_id, content);
    msg.user = Some(user.clone());
    msg.created_at = Some(time);

    // --- 频道/群组信息解析 ---
    if msg_type == "group" {
        let group_id = json["group_id"]
            .as_i64()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "0".to_string());

        msg.guild = Some(Guild::new(group_id.clone()));
        msg.channel = Some(Channel::new(
            format!("group:{}", group_id),
            ChannelType::Text,
        ));

        let mut member = GuildMember {
            user: Some(user.clone()),
            avatar: avatar.clone(),
            ..Default::default()
        };

        if let Some(sender) = json.get("sender")
            && let Some(card) = sender["card"].as_str()
            && !card.is_empty()
        {
            member.nick = Some(card.to_string());
        }

        if is_anonymous {
            member.nick = Some(nickname.clone());
        }

        msg.member = Some(member);
    } else {
        // 私聊
        msg.channel = Some(Channel::new(
            format!("private:{}", user_id),
            ChannelType::Direct,
        ));
    }

    let mut event = Event::message_created(msg);
    // 消息事件的 platform_type 也是 message
    event.platform_type = Some(msg_type.to_string());
    event.timestamp = time;
    Some(event)
}

/// 处理通知事件 (Notice Event)
///
/// 这里的关键改动是设置 `platform_type`，使上层能识别出具体的 OneBot 事件类型。
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

        // 消息撤回
        "group_recall" | "friend_recall" => (event_types::MESSAGE_DELETED, notice_type),

        // 好友增加
        "friend_add" => ("friend-added", "friend_add"),

        // 群禁言 -> 成员更新
        // 注意：通过 platform_type="group_ban" 和 platform_data["duration"] 来区分和处理
        "group_ban" => (event_types::GUILD_MEMBER_UPDATED, "group_ban"),

        // 群管理员变动 -> 成员更新
        "group_admin" => (event_types::GUILD_MEMBER_UPDATED, "group_admin"),

        // 群名片变更 -> 成员更新
        "group_card" => (event_types::GUILD_MEMBER_UPDATED, "group_card"),

        // 戳一戳等 Notify
        "notify" => match sub_type {
            Some("poke") => ("interaction/poke", "notify/poke"),
            Some("lucky_king") => ("interaction/lucky_king", "notify/lucky_king"),
            Some("honor") => (event_types::GUILD_MEMBER_UPDATED, "notify/honor"),
            _ => return None, // 未知通知忽略
        },

        // 表情回应
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
        event.guild = Some(Guild::new(gid_str.clone()));
        event.channel = Some(Channel::new(
            format!("group:{}", gid_str),
            ChannelType::Text,
        ));
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
        if main_user_id.is_none() {
            main_user_id = operator_id; // 兜底
        }
        // user_id 在 poke 里其实是 operator
        operator_id = json["user_id"].as_i64();
    }

    // 设置 User
    if let Some(uid) = main_user_id {
        event.user = Some(User::new(uid.to_string()));
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
        if !is_self || notice_type == "group_admin" || notice_type == "group_ban" {
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

    // 这里简单处理生命周期，心跳通常忽略
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

/// 构建合并转发节点列表 (Helper)
fn build_forward_nodes(elements: &[Element]) -> Vec<Value> {
    let mut nodes = Vec::new();

    for elem in elements {
        match elem {
            // 情况1: 引用已有消息进行转发
            // <message id="123456" /> 或 <message id="123456" forward />
            Element::Message {
                id: Some(msg_id), ..
            } => {
                nodes.push(json!({
                    "type": "node",
                    "data": {
                        "id": msg_id
                    }
                }));
            }
            // 情况2: 自定义伪造消息 (嵌套内容)
            // <message><author id="..." name="..." /> content...</message>
            Element::Message {
                id: None, children, ..
            } => {
                // 默认 Author 信息
                let mut user_id = "0".to_string();
                let mut nickname = "Unknown".to_string();
                let mut content_elems = Vec::new();

                // 遍历子元素，分离 Author 和 实际消息内容
                for child in children {
                    if let Element::Author { id, name, .. } = child {
                        if let Some(uid) = id {
                            user_id = uid.clone();
                        }
                        if let Some(nm) = name {
                            nickname = nm.clone();
                        }
                        // author 标签可能包含 avatar，但 OneBot node 节点主要关注 id 和 name
                    } else {
                        content_elems.push(child.clone());
                    }
                }

                // 将内容元素转换为 OneBot 消息段数组
                let mut content_arr = Vec::new();
                process_elements_to_onebot(&content_elems, &mut content_arr);

                nodes.push(json!({
                    "type": "node",
                    "data": {
                        "user_id": user_id, // NapCat 兼容字符串格式的数字 ID
                        "nickname": nickname,
                        "content": content_arr
                    }
                }));
            }
            // 忽略其他非 message 标签 (Satori 规范中合并转发应包裹在 message 元素内)
            _ => {}
        }
    }
    nodes
}

/// Satori XML -> OneBot 消息数组 (入口)
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
    let mut arr = Vec::new();

    // 递归处理
    process_elements_to_onebot(&elements, &mut arr);

    // 二次防护：检测解析器吞字现象 (保持原有逻辑)
    if arr.len() == 1
        && let Some(obj) = arr[0].as_object()
        && let Some(type_val) = obj.get("type").and_then(|v| v.as_str())
        && type_val == "text"
        && let Some(data) = obj.get("data")
        && let Some(text_val) = data.get("text").and_then(|v| v.as_str())
        && text_val.len() < content.len() * 7 / 10
        && !content.is_empty()
    {
        ayjx_debug!("[NapCat] 检测到潜在的 XML 解析文本丢失，回退为纯文本模式");
        return json!([
            {
                "type": "text",
                "data": {
                    "text": content
                }
            }
        ]);
    }

    // 二次防护：空结果回退
    if arr.is_empty() && !content.is_empty() {
        return json!([
            {
                "type": "text",
                "data": {
                    "text": content
                }
            }
        ]);
    }

    // ayjx_debug!("[NapCat] 转换后的 OneBot 消息数组: {:?}", arr);
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
                    // Case A: 引用已有的转发消息 ID (forward by id)
                    if let Some(forward_id) = id
                        && !forward_id.is_empty()
                    {
                        arr.push(json!({
                            "type": "forward",
                            "data": {
                                "id": forward_id
                            }
                        }));
                        // 引用转发通常是单独的消息体，但也允许和其他消息混合，这里继续处理后续元素
                        continue;
                    }

                    // Case B: 自定义内容的合并转发 (custom nodes)
                    // Satori 结构: <message forward><message>content...</message><message>...</message></message>
                    for child in children {
                        // 每一个子 message 对应一个 OneBot node
                        if let Element::Message {
                            children: sub_children,
                            ..
                        } = child
                        {
                            // 提取 <author> 信息 和 实际内容
                            let mut nickname = "Unknown".to_string();
                            let mut user_id = "10000".to_string(); // 默认兜底ID
                            let mut node_content_elems = Vec::new();

                            for sub in sub_children {
                                if let Element::Author {
                                    id: auth_id,
                                    name: auth_name,
                                    ..
                                } = sub
                                {
                                    if let Some(nid) = auth_id {
                                        user_id = nid.clone();
                                    }
                                    if let Some(nname) = auth_name {
                                        nickname = nname.clone();
                                    }
                                } else {
                                    node_content_elems.push(sub.clone());
                                }
                            }

                            // 递归解析节点内的内容
                            let mut node_content_arr = Vec::new();
                            process_elements_to_onebot(&node_content_elems, &mut node_content_arr);

                            // 仅当内容不为空时添加节点
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
                } else {
                    // 2. 普通消息容器 (forward=false)
                    // 仅作为分组容器，直接展平处理子元素
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

            // 忽略 Author 标签 (它应该在 Element::Message 的处理逻辑中被消耗掉，如果出现在这里说明位置不对或无需处理)
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

/// 辅助函数：转义 XML 特殊字符 (用于文件消息构建)
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
