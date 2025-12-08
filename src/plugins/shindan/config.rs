use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginConfig {
    pub enabled: bool,
    #[serde(default)]
    pub domain: String, // "Jp", "Cn" 等
    #[serde(default = "default_true")]
    pub random_return_command: bool,
    #[serde(default = "default_rank_max")]
    pub rank_max: u32,
}

fn default_true() -> bool {
    true
}
fn default_rank_max() -> u32 {
    20
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            domain: "Jp".to_string(),
            random_return_command: true,
            rank_max: 20,
        }
    }
}

/// 对应 res/shindans.toml 中的神断定义
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShindanDefinition {
    pub id: String,
    pub title: String,
    pub description: String,
    pub command: String,
    pub mode: String, // "image" | "text"
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ShindanList {
    pub shindan: Vec<ShindanDefinition>,
}
