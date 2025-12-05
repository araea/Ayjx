use crate::config::{AppConfig, BotConfig};
use crate::event::{Context, Event, EventType, Matcher, SendPacket};
use crate::plugins;
use crate::scheduler::Scheduler;
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{client::IntoClientRequest, protocol::Message as WsMessage},
};

pub type BotError = Box<dyn std::error::Error + Send + Sync>;

pub type WsWriter =
    futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;

#[derive(Serialize)]
struct SendParamsInner<T> {
    message_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<i64>,
    message: T,
}

pub async fn run_bot_loop(
    bot_config: BotConfig,
    global_config: Arc<RwLock<AppConfig>>,
    db: DatabaseConnection,
    scheduler: Arc<Scheduler>,
    save_lock: Arc<AsyncMutex<()>>,
    config_path: String,
) {
    loop {
        match connect_and_listen(
            &bot_config,
            global_config.clone(),
            db.clone(),
            scheduler.clone(),
            save_lock.clone(),
            config_path.clone(),
        )
        .await
        {
            Ok(()) => eprintln!("Bot [{}] 连接断开，3秒后重连...", bot_config.url),
            Err(e) => eprintln!("Bot [{}] 连接失败: {}。3秒后重试...", bot_config.url, e),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn connect_and_listen(
    config: &BotConfig,
    global_config: Arc<RwLock<AppConfig>>,
    db: DatabaseConnection,
    scheduler: Arc<Scheduler>,
    save_lock: Arc<AsyncMutex<()>>,
    config_path: String,
) -> Result<(), BotError> {
    let mut request = config.url.as_str().into_client_request()?;
    let token_header = format!("Bearer {}", config.access_token);
    request
        .headers_mut()
        .insert("Authorization", HeaderValue::from_str(&token_header)?);

    let (ws_stream, _) = connect_async(request).await?;
    println!("Bot [{}] 连接成功！", config.url);

    let (mut write_half, mut read_half) = ws_stream.split();

    // 每个连接初始化一个 Matcher，用于处理该 Bot 的交互式等待
    let matcher = Arc::new(Matcher::new());

    while let Some(message) = read_half.next().await {
        match message {
            Ok(WsMessage::Text(text)) => {
                let mut data = text.as_bytes().to_vec();
                if let Err(e) = process_frame(
                    &mut data,
                    &mut write_half,
                    global_config.clone(),
                    db.clone(),
                    scheduler.clone(),
                    save_lock.clone(),
                    config_path.clone(),
                    matcher.clone(),
                )
                .await
                {
                    eprintln!("Event processing error: {}", e);
                }
            }
            Ok(WsMessage::Close(_)) => return Ok(()),
            Err(e) => return Err(Box::new(e)),
            _ => {}
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_frame(
    data: &mut [u8],
    writer: &mut WsWriter,
    config: Arc<RwLock<AppConfig>>,
    db: DatabaseConnection,
    scheduler: Arc<Scheduler>,
    save_lock: Arc<AsyncMutex<()>>,
    config_path: String,
    matcher: Arc<Matcher>,
) -> Result<(), BotError> {
    let event: Event = match simd_json::to_owned_value(data) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    // 优先尝试分发给等待者 (交互式输入)
    // 如果 dispatch 返回 None，说明消息被等待者消费了，不再走常规插件流程
    let event = match matcher.dispatch(event).await {
        Some(e) => e,
        None => return Ok(()),
    };

    let ctx = Context {
        event: EventType::Onebot(event),
        config,
        config_save_lock: save_lock,
        db,
        scheduler,
        matcher,
        config_path,
    };

    plugins::run(ctx, writer).await?;
    Ok(())
}

pub async fn send_msg<M>(
    ctx: &Context,
    writer: &mut WsWriter,
    group_id: Option<i64>,
    user_id: Option<i64>,
    message: M,
) -> Result<(), BotError>
where
    M: Serialize,
{
    let (msg_type, target_group, target_user) = if let Some(gid) = group_id.filter(|&id| id != 0) {
        ("group", Some(gid), None)
    } else if let Some(uid) = user_id.filter(|&id| id != 0) {
        ("private", None, Some(uid))
    } else {
        return Ok(());
    };

    let params = SendParamsInner {
        message_type: msg_type,
        group_id: target_group,
        user_id: target_user,
        message,
    };

    let json_str = simd_json::to_string(&params)?;
    let mut json_bytes = json_str.into_bytes();
    let params_val =
        simd_json::to_owned_value(&mut json_bytes).map_err(|e| Box::new(e) as BotError)?;

    let packet = SendPacket {
        action: "send_msg".to_string(),
        params: params_val,
    };

    let new_ctx = Context {
        event: EventType::BeforeSend(packet),
        config: ctx.config.clone(),
        config_save_lock: ctx.config_save_lock.clone(),
        db: ctx.db.clone(),
        scheduler: ctx.scheduler.clone(),
        matcher: ctx.matcher.clone(),
        config_path: ctx.config_path.clone(),
    };

    plugins::run(new_ctx, writer).await?;
    Ok(())
}

pub async fn send_frame_raw(writer: &mut WsWriter, json_str: String) -> Result<(), BotError> {
    writer.send(WsMessage::Text(json_str.into())).await?;
    Ok(())
}
