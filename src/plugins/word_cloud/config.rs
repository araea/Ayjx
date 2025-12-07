use crate::config::build_config;
use serde::{Deserialize, Serialize};
use toml::Value;

#[derive(Serialize, Deserialize, Clone)]
pub struct WordCloudConfig {
    pub enabled: bool,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default = "default_max_msg")]
    pub max_msg: usize,

    // === 每日推送 ===
    #[serde(default)]
    pub daily_push_enabled: bool,
    #[serde(default = "default_daily_push_time")]
    pub daily_push_time: String, // 格式 "HH:MM:SS"
    #[serde(default)]
    pub debug_push_interval: u64, // 调试用：如果不为0，则按此秒数间隔推送
}

fn default_limit() -> usize {
    50
}

fn default_width() -> u32 {
    800
}

fn default_height() -> u32 {
    600
}

fn default_max_msg() -> usize {
    50000
}

fn default_daily_push_time() -> String {
    "23:30:00".to_string()
}

pub fn default_config() -> Value {
    build_config(WordCloudConfig {
        enabled: true,
        limit: 50,
        width: 800,
        height: 600,
        font_path: None,
        max_msg: 50000,
        daily_push_enabled: false,
        daily_push_time: "23:30:00".to_string(),
        debug_push_interval: 0,
    })
}
