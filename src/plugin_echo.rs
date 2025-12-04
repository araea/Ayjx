// plugin_echo.rs

use ayjx::prelude::*;

pub struct EchoPlugin;

#[async_trait]
impl Plugin for EchoPlugin {
    fn id(&self) -> &str {
        "echo"
    }
    fn name(&self) -> &str {
        "Echo"
    }
    fn description(&self) -> &str {
        "重复你说的话"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        if let Some(cmd) = ctx.parse_command(event, "echo").await {
            if cmd.args_elements.is_empty() {
                return Ok(EventResult::Continue);
            } else {
                let mut builder = MessageBuilder::new();
                for elem in cmd.args_elements {
                    builder = builder.raw(elem.to_string());
                }
                ctx.reply(event, &builder.build()).await?;
            }
            return Ok(EventResult::Stop);
        }

        Ok(EventResult::Continue)
    }
}
