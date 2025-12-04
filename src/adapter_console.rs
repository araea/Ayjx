use ayjx::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{RwLock, mpsc};

pub struct ConsoleAdapter {
    id: String,
    msg_seq: AtomicU64,
    event_tx: RwLock<Option<mpsc::Sender<Event>>>,
}

impl Default for ConsoleAdapter {
    fn default() -> Self {
        Self {
            id: "console-01".to_string(),
            msg_seq: AtomicU64::new(0),
            event_tx: RwLock::new(None),
        }
    }
}

#[async_trait]
impl Adapter for ConsoleAdapter {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Console Adapter"
    }
    fn platforms(&self) -> Vec<&str> {
        vec!["console"]
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&self, ctx: AdapterContext) -> AyjxResult<()> {
        {
            let mut tx = self.event_tx.write().await;
            *tx = Some(ctx.event_tx.clone());
        }

        let mock_user = User {
            id: "console_user".to_string(),
            name: Some("Developer".to_string()),
            nick: Some("Dev".to_string()),
            is_bot: Some(false),
            avatar: None,
        };

        let mock_channel = Channel {
            id: "main_terminal".to_string(),
            channel_type: ChannelType::Text,
            name: Some("Terminal".to_string()),
            parent_id: None,
        };

        let event_tx = ctx.event_tx.clone();
        let adapter_id = self.id.to_string();
        let mut sys_rx = ctx.system_rx.resubscribe();
        let _seq = 0;

        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let mut reader = BufReader::new(stdin).lines();
            let mut counter = 0u64;

            // --- 生命周期：创建登录信息 ---
            let mut login_info = Login::new("console", &adapter_id);
            login_info.user = Some(mock_user.clone());
            login_info.status = LoginStatus::Online;

            if let Err(e) = event_tx.send(Event::login_added(login_info.clone())).await {
                eprintln!("ConsoleAdapter 发送 login-added 失败: {}", e);
                return;
            }

            let _ = event_tx
                .send(Event::login_updated(login_info.clone()))
                .await;

            loop {
                tokio::select! {
                    Ok(SystemSignal::Shutdown) = sys_rx.recv() => {
                        break;
                    }
                    line_result = reader.next_line() => {
                        match line_result {
                            Ok(Some(text)) => {
                                let content = text.trim().to_string();
                                if content.is_empty() { continue; }

                                if content == "/exit" {
                                    break;
                                }

                                counter += 1;
                                let msg_id = format!("msg_{}", counter);
                                let mut msg = Message::new(msg_id, content);
                                msg.user = Some(mock_user.clone());
                                msg.channel = Some(mock_channel.clone());

                                let mut event = Event::message_created(msg);
                                event.login = Some(login_info.clone());

                                if let Err(e) = event_tx.send(event).await {
                                    eprintln!("发送事件失败: {}", e);
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                eprintln!("读取输入错误: {}", e);
                                break;
                            }
                        }
                    }
                }
            }

            // --- 生命周期：清理登录信息 ---
            login_info.status = LoginStatus::Offline;

            let _ = event_tx
                .send(Event::login_updated(login_info.clone()))
                .await;

            let _ = event_tx.send(Event::login_removed(login_info)).await;
        });

        Ok(())
    }

    async fn stop(&self) -> AyjxResult<()> {
        Ok(())
    }

    async fn get_login(&self) -> AyjxResult<Login> {
        Ok(Login::new("console", &self.id))
    }

    async fn send_message(&self, channel_id: &str, content: &str) -> AyjxResult<Vec<Message>> {
        let seq = self.msg_seq.fetch_add(1, Ordering::Relaxed);
        let msg_id = format!("reply_{}", seq);

        let bot_user = User {
            id: self.id.clone(),
            name: Some("Ayjx Bot".to_string()),
            nick: Some("Bot".to_string()),
            is_bot: Some(true),
            avatar: None,
        };

        let channel = Channel {
            id: channel_id.to_string(),
            channel_type: ChannelType::Text,
            name: Some("Terminal".to_string()),
            parent_id: None,
        };

        let mut message = Message::new(msg_id, content);
        message.user = Some(bot_user);
        message.channel = Some(channel);

        let mut event = Event::message_created(message.clone());
        event.login = Some(Login::new("console", &self.id));

        let tx_guard = self.event_tx.read().await;
        if let Some(tx) = &*tx_guard {
            if let Err(e) = tx.send(event).await {
                eprintln!("ConsoleAdapter 发送回复事件失败: {}", e);
            }
        } else {
            eprintln!("ConsoleAdapter 未初始化 Event Sender");
        }

        Ok(vec![message])
    }
}
