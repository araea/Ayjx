// plugin_repeater.rs

use ayjx::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// 复读插件配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeaterConfig {
    /// 最少重复次数触发复读
    pub min_times: usize,
    /// 复读发生概率 (0.0 - 1.0)
    pub probability: f64,
}

impl Default for RepeaterConfig {
    fn default() -> Self {
        Self {
            min_times: 2,
            probability: 1.0,
        }
    }
}

/// 频道复读状态
#[derive(Debug, Default, Clone)]
struct ChannelState {
    /// 当前记录的消息内容
    content: String,
    /// 是否已经复读过
    repeated: bool,
    /// 连续出现的次数
    times: usize,
    /// 最后一条消息的发送者ID
    last_user_id: String,
}

/// 复读插件主结构
pub struct RepeaterPlugin {
    /// 状态存储: ChannelID -> State
    states: Arc<Mutex<HashMap<String, ChannelState>>>,
}

impl RepeaterPlugin {
    pub fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 简单的概率检查
    fn check_probability(&self, probability: f64) -> bool {
        if probability >= 1.0 {
            return true;
        }
        if probability <= 0.0 {
            return false;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let random_val = (nanos % 100) as f64 / 100.0;
        random_val < probability
    }
}

impl Default for RepeaterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RepeaterPlugin {
    fn id(&self) -> &str {
        "repeater"
    }

    fn name(&self) -> &str {
        "Repeater"
    }

    fn description(&self) -> &str {
        "复读机插件：当检测到连续相同的消息时进行复读"
    }

    fn version(&self) -> &str {
        "0.1.4"
    }

    fn default_config(&self) -> Option<toml::Value> {
        let config = RepeaterConfig::default();
        toml::Value::try_from(config).ok()
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        let channel_id = match event.channel_id() {
            Some(id) => id.to_string(),
            None => return Ok(EventResult::Continue),
        };

        let content = match event.content() {
            Some(c) => c.to_string(),
            None => return Ok(EventResult::Continue),
        };

        // 2. 根据事件类型分发逻辑
        match event.event_type.as_str() {
            // === 场景 A: 接收到新消息 (Message Created) ===
            event_types::MESSAGE_CREATED => {
                // 忽略机器人的消息
                if let Some(user) = &event.user
                    && user.is_bot.unwrap_or(false)
                {
                    return Ok(EventResult::Continue);
                }

                // 获取当前消息发送者的 ID
                let current_user_id = event
                    .user
                    .as_ref()
                    .map(|u| u.id.clone())
                    .unwrap_or_default();

                let config: RepeaterConfig = ctx.plugin_config().await.unwrap_or_default();

                let should_repeat = {
                    let mut states = self.states.lock().unwrap();
                    let state = states.entry(channel_id.clone()).or_default();

                    if state.content == content {
                        if state.last_user_id != current_user_id {
                            state.times += 1;
                            state.last_user_id = current_user_id;

                            if state.times >= config.min_times
                                && !state.repeated
                                && self.check_probability(config.probability)
                            {
                                state.repeated = true;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        state.content = content.clone();
                        state.times = 1;
                        state.repeated = false;
                        state.last_user_id = current_user_id;
                        false
                    }
                };

                if should_repeat {
                    ctx.reply(event, &content).await?;
                }
            }

            // === 场景 B: 消息发送前 (Before Send) ===
            event_types::BEFORE_SEND => {
                let mut states = self.states.lock().unwrap();
                let state = states.entry(channel_id).or_default();

                state.repeated = true;
                let bot_marker = "<BOT_SELF>".to_string();

                if state.content == content {
                    if state.last_user_id != bot_marker {
                        state.times += 1;
                        state.last_user_id = bot_marker;
                    }
                } else {
                    state.content = content;
                    state.times = 1;
                    state.last_user_id = bot_marker;
                }
            }

            _ => {}
        }

        Ok(EventResult::Continue)
    }
}
