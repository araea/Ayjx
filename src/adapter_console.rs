use ayjx::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, BufReader};

/// 控制台适配器
pub struct ConsoleAdapter {
    id: String,
    // 用于生成简单的消息 ID
    msg_seq: AtomicU64,
}

impl Default for ConsoleAdapter {
    fn default() -> Self {
        Self {
            id: "console-01".to_string(),
            msg_seq: AtomicU64::new(0),
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

    async fn start(&self, ctx: AdapterContext) -> AyjxResult<()> {
        println!("--------------------------------------------------");
        println!("[Console] 控制台适配器已启动。");
        println!("[Console] 输入 /exit 退出程序");
        println!("[Console] 尝试输入 /echo hello 或 /survey 开始体验");
        println!("--------------------------------------------------");

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
        // 简单的序列号生成
        let _seq = 0;

        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let mut reader = BufReader::new(stdin).lines();
            let mut counter = 0u64;

            loop {
                tokio::select! {
                    Ok(SystemSignal::Shutdown) = sys_rx.recv() => {
                        println!("ConsoleAdapter 停止输入监听");
                        break;
                    }
                    line_result = reader.next_line() => {
                        match line_result {
                            Ok(Some(text)) => {
                                let content = text.trim().to_string();
                                if content.is_empty() { continue; }

                                if content == "/exit" {
                                    println!("正在退出...");
                                    std::process::exit(0);
                                }

                                counter += 1;
                                let msg_id = format!("msg_{}", counter);
                                let mut msg = Message::new(msg_id, content);
                                msg.user = Some(mock_user.clone());
                                msg.channel = Some(mock_channel.clone());

                                let mut event = Event::message_created(msg);
                                let mut login = Login::new("console", &adapter_id);
                                login.status = LoginStatus::Online;
                                event.login = Some(login);

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
        });

        Ok(())
    }

    async fn stop(&self) -> AyjxResult<()> {
        println!("控制台适配器已关闭");
        Ok(())
    }

    async fn get_login(&self) -> AyjxResult<Login> {
        Ok(Login::new("console", &self.id))
    }

    async fn send_message(&self, _channel_id: &str, content: &str) -> AyjxResult<Vec<Message>> {
        let elements = message_elements::parse(content);
        let plain_text = message_elements::to_plain_text(&elements);

        // 使用终端颜色代码美化输出
        println!("\x1b[36m[Ayjx Bot] > {}\x1b[0m", plain_text);

        let seq = self.msg_seq.fetch_add(1, Ordering::Relaxed);
        Ok(vec![Message::new(format!("reply_{}", seq), content)])
    }
}
