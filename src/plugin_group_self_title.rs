// plugin_group_self_title.rs

use crate::adapter_napcat::{NapCatAdapter, NapCatApi};
use ayjx::prelude::*;

pub struct SelfTitlePlugin;

#[async_trait]
impl Plugin for SelfTitlePlugin {
    fn id(&self) -> &str {
        "group_self_title"
    }
    fn name(&self) -> &str {
        "Group Self Title"
    }
    fn description(&self) -> &str {
        "自助头衔插件"
    }
    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        if let Some(cmd) = ctx.parse_command(event, "我要头衔").await {
            let channel_id = match event.channel_id() {
                Some(id) => id,
                None => return Ok(EventResult::Continue),
            };

            let adapter_id = event.adapter().unwrap_or("napcat");
            if let Some(adapter) = ctx.get_adapter(adapter_id).await
                && let Some(napcat) = adapter.as_any().downcast_ref::<NapCatAdapter>()
            {
                let self_id = event.bot_id().unwrap_or_default();

                let bot_info = napcat
                    .get_group_member_info(channel_id, self_id, Some(false), Some(self_id))
                    .await?;

                let role = bot_info
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("member");

                if role != "owner" {
                    return Ok(EventResult::Continue);
                }

                let mut title = String::new();
                for elem in cmd.args_elements {
                    title.push_str(&elem.to_string());
                }
                let title = title.trim();

                let user_id = event.user_id().unwrap_or_default();

                let _ = napcat
                    .set_group_special_title(channel_id, user_id, title, Some(self_id))
                    .await;

                return Ok(EventResult::Stop);
            }
        }

        Ok(EventResult::Continue)
    }
}
