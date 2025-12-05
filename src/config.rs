use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::fs;
use toml::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    // 全局指令前缀（支持多个，如 ["/", "#"]）
    #[serde(default = "default_prefix")]
    pub command_prefix: Vec<String>,

    // Bot 连接配置
    #[serde(default)]
    pub bots: Vec<BotConfig>,

    // 插件配置
    #[serde(flatten)]
    pub plugins: HashMap<String, Value>,
}

impl AppConfig {
    pub async fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let toml_string = toml::to_string_pretty(self)?;
        fs::write(path, toml_string).await?;
        Ok(())
    }
}

fn default_prefix() -> Vec<String> {
    vec!["/".to_string()]
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BotConfig {
    pub url: String,
    pub access_token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            command_prefix: vec!["/".to_string()],
            bots: vec![BotConfig {
                url: "ws://127.0.0.1:3001".to_string(),
                access_token: "".to_string(),
            }],
            plugins: HashMap::new(),
        }
    }
}

/// 辅助函数：构建默认配置 Value，并确保包含 enabled 字段
pub fn build_config<T: Serialize>(data: T) -> Value {
    let mut val = Value::try_from(data).unwrap_or(Value::Table(Default::default()));
    if let Value::Table(ref mut map) = val
        && !map.contains_key("enabled")
    {
        map.insert("enabled".to_string(), Value::Boolean(true));
    }
    val
}
