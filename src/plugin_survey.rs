use ayjx::prelude::*;

pub struct InteractivePlugin;

#[async_trait]
impl Plugin for InteractivePlugin {
    fn id(&self) -> &str {
        "interactive"
    }
    fn name(&self) -> &str {
        "Survey Demo"
    }
    fn description(&self) -> &str {
        "Demonstrates multi-turn conversation"
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        if event.event_type != event_types::MESSAGE_CREATED {
            return Ok(EventResult::Continue);
        }

        let content = event.content().unwrap_or("");
        let sender_id = event.sender_id().unwrap_or("unknown");
        let channel_id = event.channel_id();

        if content.trim() == "/survey" {
            ctx.reply(event, "你好！欢迎参加 Ayjx 体验调查。请问你的名字是？")
                .await?;

            match ctx.prompt(sender_id, channel_id).await {
                Ok(name) => {
                    ctx.reply(
                        event,
                        &format!("好的 {}，请问你给 Ayjx 框架打几分 (1-10)？", name),
                    )
                    .await?;

                    match ctx.prompt(sender_id, channel_id).await {
                        Ok(score) => {
                            let reply = MessageBuilder::new()
                                .text("调查完成！感谢反馈。")
                                .br()
                                .text(format!("姓名: {}", name))
                                .br()
                                .text(format!("评分: {}", score))
                                .build();
                            ctx.reply(event, &reply).await?;
                        }
                        Err(_) => {
                            ctx.reply(event, "等待评分超时，会话结束。").await?;
                        }
                    }
                }
                Err(_) => {
                    ctx.reply(event, "等待名字输入超时，会话结束。").await?;
                }
            }

            return Ok(EventResult::Stop);
        }

        Ok(EventResult::Continue)
    }
}
