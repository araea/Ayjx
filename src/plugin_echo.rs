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
        ayjx_debug!("EventType: {} | {:?}", event.event_type, event);
        if let Some(platform_data) = &event.platform_data {
            if let Some(json_value) = platform_data.downcast_ref::<serde_json::Value>() {
                println!("event.platform_data as JSON: {}", json_value);
            }
        } else {
            println!("event.platform_data is None");
        }

        if event.event_type != event_types::MESSAGE_CREATED {
            return Ok(EventResult::Continue);
        }

        let content = event.content().unwrap_or("");

        if let Some(args) = command::strip_prefix(content, "/echo") {
            // 修改说明：
            // 使用 .raw(args) 替代 .text(args)。
            // event.content() 返回的是协议层的 XML 字符串。
            // .text() 会转义 XML 标签（如 <img ...> 变成 &lt;img ...&gt;），导致无法显示图片。
            // .raw() 会按原样输出内容，保留富文本结构。
            let response = MessageBuilder::new().text("收到: ").raw(args).build();

            ctx.reply(event, &response).await?;
            return Ok(EventResult::Stop);
        }

        Ok(EventResult::Continue)
    }
}
