use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::fs;
use toml::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    // 全局指令前缀
    #[serde(default = "default_prefix")]
    pub command_prefix: String,

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

fn default_prefix() -> String {
    "/".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BotConfig {
    pub url: String,
    pub access_token: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            command_prefix: "/".to_string(),
            bots: vec![BotConfig {
                url: "ws://127.0.0.1:3001".to_string(),
                access_token: "".to_string(),
            }],
            plugins: HashMap::new(),
        }
    }
}
