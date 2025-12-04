// plugin_logger.rs

use ayjx::prelude::*;
use chrono::Local;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. 配置定义
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default)]
    pub debug: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            time_format: "%H:%M:%S".to_string(),
            debug: false,
        }
    }
}

fn default_time_format() -> String {
    "%H:%M:%S".to_string()
}

// ============================================================================
// 2. 插件实现
// ============================================================================

pub struct ConsoleLoggerPlugin;

impl ConsoleLoggerPlugin {
    pub fn new() -> Self {
        Self
    }

    // 简单的颜色工具
    fn style(&self, text: &str, code: &str) -> String {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    }

    // 常用颜色快捷方式
    fn gray(&self, t: &str) -> String {
        self.style(t, "90")
    }
    fn red(&self, t: &str) -> String {
        self.style(t, "31")
    }
    fn green(&self, t: &str) -> String {
        self.style(t, "32")
    }
    fn yellow(&self, t: &str) -> String {
        self.style(t, "33")
    }
    fn blue(&self, t: &str) -> String {
        self.style(t, "34")
    }
    fn magenta(&self, t: &str) -> String {
        self.style(t, "35")
    }
    fn cyan(&self, t: &str) -> String {
        self.style(t, "36")
    }
}

#[async_trait]
impl Plugin for ConsoleLoggerPlugin {
    fn id(&self) -> &str {
        "console_logger"
    }

    fn name(&self) -> &str {
        "Console Logger"
    }

    fn description(&self) -> &str {
        "标准控制台日志输出插件，支持多平台消息展示与调试"
    }

    fn version(&self) -> &str {
        "1.2.1"
    }

    fn priority(&self) -> i32 {
        0
    }

    fn default_config(&self) -> Option<toml::Value> {
        let config = LoggerConfig::default();
        match toml::Value::try_from(config) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[Console Logger] 默认配置生成失败: {}", e);
                None
            }
        }
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        let config: LoggerConfig = ctx.plugin_config().await.unwrap_or_default();

        if event.event_type == event_types::INTERNAL && !config.debug {
            return Ok(EventResult::Continue);
        }

        let time_str = self.gray(&Local::now().format(&config.time_format).to_string());

        // 平台标签
        let platform = event.platform().unwrap_or("sys");
        let adapter = event.adapter().unwrap_or("core");
        let platform_tag = self.magenta(&format!("[{}:{}]", platform, adapter));

        // ----------------------------------------------------------------
        // 1. 消息事件处理 (包含发送前预处理)
        // ----------------------------------------------------------------
        if event.is_message_event() {
            let guild_name = event
                .guild
                .as_ref()
                .or(event.message.as_ref().and_then(|m| m.guild.as_ref()))
                .and_then(|g| g.name.as_deref().or(Some(g.id.as_str())));

            let context_tag = if let Some(name) = guild_name {
                self.blue(&format!("[{}]", name))
            } else if event.message.as_ref().is_some_and(|m| m.is_direct()) {
                self.green("[私聊]")
            } else {
                let target = event
                    .channel_id()
                    .or(event.message.as_ref().and_then(|m| m.channel_id()))
                    .unwrap_or("?");
                self.blue(&format!("[To:{}]", target))
            };

            let member_ref = event
                .member
                .as_ref()
                .or(event.message.as_ref().and_then(|m| m.member.as_ref()));
            let user_ref = event
                .user
                .as_ref()
                .or(event.message.as_ref().and_then(|m| m.user.as_ref()));

            let sender_name = if let Some(member) = member_ref {
                member
                    .display_name()
                    .map(|s| s.to_string())
                    .or_else(|| user_ref.map(|u| u.display_name().to_string()))
                    .unwrap_or_else(|| "Unknown".to_string())
            } else if let Some(user) = user_ref {
                user.display_name().to_string()
            } else {
                "System".to_string()
            };

            let (direction_tag, sender_tag) = if event.event_type == event_types::BEFORE_SEND {
                (self.green("<< SEND"), self.cyan("Bot"))
            } else {
                (String::new(), self.cyan(&sender_name))
            };

            let action = match event.event_type.as_str() {
                event_types::MESSAGE_UPDATED => self.yellow(" [编辑]"),
                event_types::MESSAGE_DELETED => self.red(" [撤回]"),
                event_types::BEFORE_SEND => String::new(),
                _ => String::new(),
            };

            let raw_content = event.content().unwrap_or("");
            let elements = message_elements::parse(raw_content);
            let plain_text = message_elements::to_plain_text(&elements);
            let clean_text = plain_text.trim();

            println!(
                "{} {} {} {}{}{}: {}",
                time_str, platform_tag, context_tag, direction_tag, sender_tag, action, clean_text
            );
        }
        // ----------------------------------------------------------------
        // 2. 交互事件 (指令、按钮、戳一戳)
        // ----------------------------------------------------------------
        else if event.event_type == event_types::INTERACTION_COMMAND {
            let user_name = event.user.as_ref().map(|u| u.display_name()).unwrap_or("?");
            if let Some(argv) = &event.argv {
                let args_str = argv
                    .arguments
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                println!(
                    "{} {} {} 用户 {} 触发指令: {}{}",
                    time_str,
                    platform_tag,
                    self.yellow("[Command]"),
                    self.cyan(user_name),
                    self.green(&argv.name),
                    if args_str.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", args_str)
                    }
                );
            }
        } else if event.event_type == event_types::INTERACTION_BUTTON {
            let user_name = event.user.as_ref().map(|u| u.display_name()).unwrap_or("?");
            let btn_id = event.button.as_ref().map(|b| b.id.as_str()).unwrap_or("?");
            println!(
                "{} {} {} 用户 {} 点击按钮: {}",
                time_str,
                platform_tag,
                self.yellow("[Button]"),
                self.cyan(user_name),
                self.green(btn_id)
            );
        } else if event.event_type == "interaction/poke" {
            let operator_name = event
                .operator
                .as_ref()
                .map(|u| u.display_name())
                .unwrap_or("?");
            let target_name = event.user.as_ref().map(|u| u.display_name()).unwrap_or("?");

            let context_tag = if let Some(g) = &event.guild {
                let name = g.name.as_deref().unwrap_or(&g.id);
                self.blue(&format!("[{}]", name))
            } else {
                self.green("[私聊]")
            };

            println!(
                "{} {} {} {} 用户 {} 戳了戳 {}",
                time_str,
                platform_tag,
                context_tag,
                self.yellow("[Poke]"),
                self.cyan(operator_name),
                self.cyan(target_name)
            );
        }
        // ----------------------------------------------------------------
        // 3. 表态事件 (Reactions)
        // ----------------------------------------------------------------
        else if event.event_type == event_types::REACTION_ADDED
            || event.event_type == event_types::REACTION_REMOVED
        {
            let operator_id = event.operator_id().unwrap_or("?");
            let msg_id = event.message_id().unwrap_or("?");
            let action = if event.event_type == event_types::REACTION_ADDED {
                self.green("添加表态")
            } else {
                self.red("移除表态")
            };
            let emoji = event.content().unwrap_or("?");

            println!(
                "{} {} {} 用户 {} 对消息 {} {}: {}",
                time_str,
                platform_tag,
                self.yellow("[Reaction]"),
                self.cyan(operator_id),
                msg_id,
                action,
                emoji
            );
        }
        // ----------------------------------------------------------------
        // 4. 群组/角色事件处理
        // ----------------------------------------------------------------
        else if event.is_guild_event() {
            let guild_name = event
                .guild
                .as_ref()
                .and_then(|g| g.name.as_deref())
                .or(event.guild_id())
                .unwrap_or("?");

            let info = match event.event_type.as_str() {
                event_types::GUILD_MEMBER_ADDED => {
                    format!(
                        "成员加入: {}",
                        self.cyan(event.user.as_ref().map(|u| u.display_name()).unwrap_or("?"))
                    )
                }
                event_types::GUILD_MEMBER_REMOVED => {
                    format!(
                        "成员离开: {}",
                        self.cyan(event.user.as_ref().map(|u| u.display_name()).unwrap_or("?"))
                    )
                }
                event_types::GUILD_ROLE_CREATED
                | event_types::GUILD_ROLE_UPDATED
                | event_types::GUILD_ROLE_DELETED => {
                    let role_name = event
                        .role
                        .as_ref()
                        .and_then(|r| r.name.as_deref())
                        .unwrap_or("?");
                    let role_id = event.role.as_ref().map(|r| r.id.as_str()).unwrap_or("?");
                    match event.event_type.as_str() {
                        event_types::GUILD_ROLE_CREATED => {
                            format!("角色创建: {} ({})", role_name, role_id)
                        }
                        event_types::GUILD_ROLE_UPDATED => {
                            format!("角色更新: {} ({})", role_name, role_id)
                        }
                        event_types::GUILD_ROLE_DELETED => {
                            format!("角色删除: {} ({})", role_name, role_id)
                        }
                        _ => unreachable!(),
                    }
                }
                _ => event.event_type.clone(),
            };

            println!(
                "{} {} {} [Guild:{}] {}",
                time_str,
                platform_tag,
                self.yellow("NOTICE"),
                guild_name,
                info
            );
        }
        // ----------------------------------------------------------------
        // 5. 好友请求
        // ----------------------------------------------------------------
        else if event.event_type == event_types::FRIEND_REQUEST {
            let user_name = event.user.as_ref().map(|u| u.display_name()).unwrap_or("?");
            let user_id = event.user.as_ref().map(|u| u.id.as_str()).unwrap_or("?");
            println!(
                "{} {} {} 收到来自 {} ({}) 的好友请求",
                time_str,
                platform_tag,
                self.red("REQUEST"),
                self.cyan(user_name),
                user_id
            );
        }
        // ----------------------------------------------------------------
        // 6. 登录状态更新 (仅 Status Updated)
        // ----------------------------------------------------------------
        else if event.event_type == event_types::LOGIN_UPDATED
            && let Some(login) = &event.login
        {
            let status_str = match login.status {
                LoginStatus::Online => self.green("ONLINE"),
                LoginStatus::Offline => self.red("OFFLINE"),
                LoginStatus::Connect => self.yellow("CONNECTING"),
                LoginStatus::Disconnect => self.red("DISCONNECTING"),
                LoginStatus::Reconnect => self.yellow("RECONNECTING"),
            };

            let bot_id = login.user.as_ref().map(|u| u.id.as_str()).unwrap_or("?");

            println!(
                "{} {} Bot [{}] 状态变更: {}",
                time_str, platform_tag, bot_id, status_str
            );
        }
        // ----------------------------------------------------------------
        // 7. 其他未知事件
        // ----------------------------------------------------------------
        else {
            if event.event_type != event_types::LOGIN_ADDED
                && event.event_type != event_types::LOGIN_REMOVED
            {
                println!(
                    "{} {} Unhandled Event: {}",
                    time_str, platform_tag, event.event_type
                );
            }
        }

        // ----------------------------------------------------------------
        // 8. 调试模式输出
        // ----------------------------------------------------------------
        if config.debug {
            let debug_prefix = self.gray("DEBUG");
            println!("{} [Full Event] {:?}", debug_prefix, event);

            if let Some(content) = event.content() {
                println!("{} [XML] {}", debug_prefix, content);
            }

            if let Some(platform_data) = &event.platform_data {
                if let Some(json_value) = platform_data.downcast_ref::<serde_json::Value>() {
                    println!("{} [Data] {}", debug_prefix, json_value);
                } else {
                    println!(
                        "{} [Data] (Present, but not serde_json::Value)",
                        debug_prefix
                    );
                }
            } else {
                println!("{} [Data] None", debug_prefix);
            }
        }

        Ok(EventResult::Continue)
    }
}
