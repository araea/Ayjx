use ayjx::prelude::*;

pub struct EchoPlugin;

#[async_trait]
impl Plugin for EchoPlugin {
    fn id(&self) -> &str {
        "echo"
    }
    fn name(&self) -> &str {
        "Echo Plugin"
    }
    fn description(&self) -> &str {
        "Repeats what you say"
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        if event.event_type != event_types::MESSAGE_CREATED {
            return Ok(EventResult::Continue);
        }

        let content = event.content().unwrap_or("");

        if let Some(args) = content.strip_prefix("/echo") {
            let response = MessageBuilder::new().raw(args).build();

            ctx.reply(event, &response).await?;
            return Ok(EventResult::Stop);
        }

        Ok(EventResult::Continue)
    }
}
