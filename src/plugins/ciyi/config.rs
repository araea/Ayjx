use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginConfig {
    #[serde(default)]
    pub at_user: bool,
    #[serde(default = "default_true")]
    pub quote_user: bool,
    #[serde(default)]
    pub direct_guess: bool,
    #[serde(default = "default_history_display")]
    pub history_display: usize,
    #[serde(default = "default_rank_display")]
    pub rank_display: usize,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            at_user: false,
            quote_user: true,
            direct_guess: false,
            history_display: 10,
            rank_display: 10,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_history_display() -> usize {
    10
}
fn default_rank_display() -> usize {
    10
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CiYiConfig {
    #[serde(default)]
    pub plugin: PluginConfig,
}
