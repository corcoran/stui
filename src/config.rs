use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    #[allow(dead_code)]
    pub path_map: HashMap<String, String>,
    #[serde(default)]
    pub vim_mode: bool,
    #[serde(default = "default_icon_mode")]
    pub icon_mode: String,
}

fn default_icon_mode() -> String {
    "nerdfont".to_string()
}
