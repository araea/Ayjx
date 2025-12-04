// plugin_recall.rs

use ayjx::prelude::*;

/// 当用户引用一条消息并发送“撤回”指令时：
/// 1. 撤回被引用的那条消息
/// 2. 撤回用户发送的这条指令消息
pub struct RecallPlugin;

#[async_trait]
impl Plugin for RecallPlugin {
    fn id(&self) -> &str {
        "recall"
    }
    fn name(&self) -> &str {
        "Message Recall"
    }
    fn description(&self) -> &str {
        "引用消息并发送“撤回”指令，自动撤回原消息与指令消息"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        if let Some(cmd) = ctx.parse_command(event, "撤回").await {
            if let Some(target_msg_id) = cmd.quote_id {
                if let (Some(adapter_id), Some(channel_id), Some(command_msg_id)) =
                    (event.adapter(), event.channel_id(), event.message_id())
                {
                    if let Some(adapter) = ctx.get_adapter(adapter_id).await {
                        // A. 撤回被引用的目标消息
                        let _ = adapter.delete_message(channel_id, &target_msg_id).await;

                        // B. 撤回用户发送的这条指令消息
                        let _ = adapter.delete_message(channel_id, command_msg_id).await;

                        return Ok(EventResult::Stop);
                    }
                }
            }
        }

        Ok(EventResult::Continue)
    }
}
