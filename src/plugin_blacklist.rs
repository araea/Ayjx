// plugin_blacklist.rs

use ayjx::prelude::*;

pub struct BlacklistPlugin;

#[async_trait]
impl Plugin for BlacklistPlugin {
    fn id(&self) -> &str {
        "ayjx-blacklist"
    }

    fn name(&self) -> &str {
        "Blacklist System"
    }

    fn description(&self) -> &str {
        "拦截黑名单用户和群组的消息事件"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn priority(&self) -> i32 {
        -100
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        let cfg = ctx.config().await;

        // 1. 检查发送者是否在黑名单中
        if let Some(user_id) = event.sender_id()
            && cfg.is_user_blacklisted(user_id)
        {
            println!(
                "[Blacklist] 拦截了黑名单用户 {} 的事件 (type: {})",
                user_id, event.event_type
            );
            return Ok(EventResult::Stop);
        }

        // 2. 检查群组是否在黑名单中
        if let Some(guild_id) = event.guild_id()
            && cfg.is_guild_blacklisted(guild_id)
        {
            println!(
                "[Blacklist] 拦截了黑名单群组 {} 的事件 (type: {})",
                guild_id, event.event_type
            );
            return Ok(EventResult::Stop);
        }

        // 未命中黑名单，继续传递事件
        Ok(EventResult::Continue)
    }
}
