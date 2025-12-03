// ================================================================================
// Ayjx Plugin: Console Logger
// 描述：标准的控制台日志输出插件，支持调试信息打印。
// ================================================================================

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
    #[serde(default)]
    pub show_internal: bool,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            time_format: "%H:%M:%S".to_string(),
            debug: false,
            show_internal: false,
        }
    }
}

fn default_time_format() -> String {
    "%H:%M:%S".to_string()
}

// ============================================================================
// 2. 插件实现
// ============================================================================

// 重命名结构体以符合功能语义
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
    // ID 改为 ayjx_console_logger，更具辨识度
    fn id(&self) -> &str {
        "ayjx_console_logger"
    }

    // 显示名称改为 Console Logger，一目了然
    fn name(&self) -> &str {
        "Console Logger"
    }

    fn description(&self) -> &str {
        "标准控制台日志输出插件，支持多平台消息展示与调试"
    }

    fn version(&self) -> &str {
        "1.1.0"
    }

    fn priority(&self) -> i32 {
        0 // 最高优先级
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

    async fn on_load(&self, ctx: &PluginContext) -> AyjxResult<()> {
        let config: LoggerConfig = ctx.plugin_config().await.unwrap_or_default();

        println!(
            "[Ayjx] 控制台日志就绪 (Time: '{}', Debug: {})",
            config.time_format,
            if config.debug { "ON" } else { "OFF" }
        );
        Ok(())
    }

    async fn on_event(&self, ctx: &PluginContext, event: &Event) -> AyjxResult<EventResult> {
        let config: LoggerConfig = ctx.plugin_config().await.unwrap_or_default();

        // 过滤内部事件
        if event.event_type == event_types::INTERNAL && !config.show_internal {
            return Ok(EventResult::Continue);
        }

        let time_str = self.gray(&Local::now().format(&config.time_format).to_string());

        // ----------------------------------------------------------------
        // 消息事件处理
        // ----------------------------------------------------------------
        if event.is_message_event() {
            let platform = event.platform().unwrap_or("sys");
            let adapter = event.adapter().unwrap_or("core");
            let platform_tag = self.magenta(&format!("[{}:{}]", platform, adapter));

            let context_tag = if let Some(guild) = &event.guild {
                let g_name = guild.name.as_deref().unwrap_or(guild.id.as_str());
                self.blue(&format!("[{}]", g_name))
            } else {
                self.green("[私聊]")
            };

            let sender_name = if let Some(member) = &event.member {
                member.display_name().unwrap_or("Unknown").to_string()
            } else if let Some(user) = &event.user {
                user.display_name().to_string()
            } else {
                "System".to_string()
            };
            let sender_tag = self.cyan(&sender_name);

            let action = match event.event_type.as_str() {
                event_types::MESSAGE_UPDATED => self.yellow(" [编辑]"),
                event_types::MESSAGE_DELETED => self.red(" [撤回]"),
                _ => String::new(),
            };

            let raw_content = event.content().unwrap_or("");
            let elements = message_elements::parse(raw_content);
            let plain_text = message_elements::to_plain_text(&elements);
            let clean_text = plain_text.trim();

            println!(
                "{} {} {} {}{}: {}",
                time_str, platform_tag, context_tag, sender_tag, action, clean_text
            );
        }
        // ----------------------------------------------------------------
        // 群组事件处理
        // ----------------------------------------------------------------
        else if event.is_guild_event() {
            let platform_tag = self.magenta(&format!(
                "[{}:{}]",
                event.platform().unwrap_or("?"),
                event.adapter().unwrap_or("?")
            ));

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
        // 登录/连接状态
        // ----------------------------------------------------------------
        else if event.event_type == event_types::LOGIN_UPDATED
            && let Some(login) = &event.login
        {
            let status_str = match login.status {
                LoginStatus::Online => self.green("ONLINE"),
                LoginStatus::Offline => self.red("OFFLINE"),
                LoginStatus::Connect => self.yellow("CONNECTING"),
                _ => self.yellow("STATUS"),
            };
            let platform_tag = self.magenta(&format!(
                "[{}:{}]",
                login.platform.as_deref().unwrap_or("?"),
                login.adapter.as_deref().unwrap_or("?")
            ));
            println!("{} {} Bot状态变更: {}", time_str, platform_tag, status_str);
        }

        // ----------------------------------------------------------------
        // 调试模式输出 (Updated)
        // ----------------------------------------------------------------
        if config.debug {
            let debug_prefix = self.gray("DEBUG");

            // 1. 打印原始 XML 内容
            if let Some(content) = event.content() {
                println!("{} [XML] {}", debug_prefix, content);
            }

            // 2. 打印 Platform Data (JSON)
            // Ayjx 理念：提供底层数据的透明度，辅助开发者排查适配器问题
            if let Some(platform_data) = &event.platform_data {
                // 尝试转换为 JSON Value 打印
                if let Some(json_value) = platform_data.downcast_ref::<serde_json::Value>() {
                    // 使用 {} 打印紧凑 JSON，或用 {:#} 打印格式化 JSON。
                    // 考虑到日志行数，这里暂时用紧凑格式，防止刷屏。
                    println!("{} [Data] {}", debug_prefix, json_value);
                } else {
                    println!(
                        "{} [Data] (Present, but not serde_json::Value)",
                        debug_prefix
                    );
                }
            } else {
                // 如果需要确认 data 是否丢失，可以取消注释下面这行：
                // println!("{} [Data] None", debug_prefix);
            }
        }

        Ok(EventResult::Continue)
    }
}
