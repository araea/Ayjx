#![allow(dead_code)]
use anyhow::{Result, anyhow};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value as JsonValue; // Used for constructing requests
use simd_json::OwnedValue; // Used for parsing responses (faster)
use simd_json::derived::{ValueObjectAccess, ValueObjectAccessAsScalar};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

// Global ID counter for JSON-RPC messages
static GLOBAL_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn next_id() -> usize {
    GLOBAL_ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

#[derive(Debug)]
pub enum TransportMessage {
    Request(JsonValue, oneshot::Sender<Result<TransportResponse>>),
    ListenTargetMessage(u64, oneshot::Sender<Result<TransportResponse>>),
    WaitForEvent(String, String, oneshot::Sender<()>),
    Shutdown,
}

#[derive(Debug)]
pub enum TransportResponse {
    Response(u64, OwnedValue),
    Target(OwnedValue),
}

struct TransportActor {
    pending_requests: HashMap<u64, oneshot::Sender<Result<TransportResponse>>>,
    event_listeners: HashMap<(String, String), Vec<oneshot::Sender<()>>>,
    ws_sink: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    command_rx: mpsc::Receiver<TransportMessage>,
}

impl TransportActor {
    async fn run(mut self, mut ws_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) {
        loop {
            tokio::select! {
                Some(msg) = ws_stream.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // 使用 simd_json 进行高性能解析
                            // transform String -> bytes for simd_json
                            let mut bytes = text.as_bytes().to_vec();
                            if let Ok(val) = simd_json::to_owned_value(&mut bytes) { self.handle_incoming_json(val) }
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
                Some(msg) = self.command_rx.recv() => {
                    if !self.handle_command(msg).await {
                        break;
                    }
                }
                else => break,
            }
        }
    }

    fn handle_incoming_json(&mut self, val: OwnedValue) {
        // 1. Check if it is a direct response to a command
        if let Some(id) = val.get_u64("id") {
            if let Some(sender) = self.pending_requests.remove(&id) {
                // Return the whole value, let the caller extract result
                let _ = sender.send(Ok(TransportResponse::Response(id, val)));
            }
            return;
        }

        // 2. Check if it is a "Target.receivedMessageFromTarget" (nested JSON)
        if let Some(method) = val.get_str("method")
            && method == "Target.receivedMessageFromTarget"
                && let Some(params) = val.get("params") {
                    // The inner message is a string that needs another parse
                    if let Some(inner_str) = params.get_str("message") {
                        let mut inner_bytes = inner_str.as_bytes().to_vec();
                        if let Ok(inner_json) = simd_json::to_owned_value(&mut inner_bytes) {
                            self.handle_target_message(inner_json, params);
                        }
                    }
                }
    }

    fn handle_target_message(&mut self, inner_json: OwnedValue, outer_params: &OwnedValue) {
        // A. If inner JSON has an ID, it's a response to a command sent TO the target
        if let Some(id) = inner_json.get_u64("id") {
            if let Some(sender) = self.pending_requests.remove(&id) {
                // Construct a fake TargetMessage structure compatible with upper layers
                // We wrap the inner_json as "params" to match expected structure if needed,
                // or we simplify. Here we stick to a structure similar to what `Tab` expects.
                // The `Tab` expects the result of `receivedMessageFromTarget`.
                let _ = sender.send(Ok(TransportResponse::Target(inner_json)));
            }
        }
        // B. If inner JSON has a method, it's an event FROM the target
        else if let Some(method) = inner_json.get_str("method")
            && let Some(session_id) = outer_params.get_str("sessionId") {
                let key = (session_id.to_string(), method.to_string());
                if let Some(senders) = self.event_listeners.remove(&key) {
                    for tx in senders {
                        let _ = tx.send(());
                    }
                }
            }
    }

    async fn handle_command(&mut self, msg: TransportMessage) -> bool {
        match msg {
            TransportMessage::Request(cmd, tx) => {
                if let Some(id) = cmd["id"].as_u64() {
                    // Use serde_json for serialization (outgoing is small)
                    if let Ok(text) = serde_json::to_string(&cmd) {
                        if self.ws_sink.send(Message::Text(text.into())).await.is_ok() {
                            self.pending_requests.insert(id, tx);
                        } else {
                            let _ = tx.send(Err(anyhow!("WebSocket send failed")));
                        }
                    }
                }
                true
            }
            TransportMessage::ListenTargetMessage(id, tx) => {
                self.pending_requests.insert(id, tx);
                true
            }
            TransportMessage::WaitForEvent(session_id, method, tx) => {
                self.event_listeners
                    .entry((session_id, method))
                    .or_default()
                    .push(tx);
                true
            }
            TransportMessage::Shutdown => {
                let _ = self
                    .ws_sink
                    .send(Message::Text(
                        serde_json::json!({
                            "id": next_id(),
                            "method": "Browser.close",
                            "params": {}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await;
                let _ = self.ws_sink.close().await;
                false
            }
        }
    }
}

#[derive(Debug)]
pub struct Transport {
    tx: mpsc::Sender<TransportMessage>,
}

impl Transport {
    pub async fn new(ws_url: &str) -> Result<Self> {
        let (ws_stream, _) = connect_async(ws_url).await?;
        let (ws_sink, ws_stream) = ws_stream.split();
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            let actor = TransportActor {
                pending_requests: HashMap::new(),
                event_listeners: HashMap::new(),
                ws_sink,
                command_rx: rx,
            };
            actor.run(ws_stream).await;
        });

        Ok(Self { tx })
    }

    pub async fn send(&self, command: JsonValue) -> Result<TransportResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TransportMessage::Request(command, tx))
            .await
            .map_err(|_| anyhow!("Transport actor dropped"))?;
        time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("Timeout waiting for response"))?
            .map_err(|_| anyhow!("Response channel closed"))?
    }

    pub async fn get_target_msg(&self, msg_id: usize) -> Result<TransportResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TransportMessage::ListenTargetMessage(msg_id as u64, tx))
            .await
            .map_err(|_| anyhow!("Transport actor dropped"))?;
        time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("Timeout waiting for target message"))?
            .map_err(|_| anyhow!("Response channel closed"))?
    }

    pub async fn wait_for_event(&self, session_id: &str, method: &str) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(TransportMessage::WaitForEvent(
                session_id.to_string(),
                method.to_string(),
                tx,
            ))
            .await
            .map_err(|_| anyhow!("Transport actor dropped"))?;

        time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("Timeout waiting for event {}", method))?
            .map_err(|_| anyhow!("Event channel closed"))?;
        Ok(())
    }

    pub async fn shutdown(&self) {
        let _ = self.tx.send(TransportMessage::Shutdown).await;
    }
}
