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
    #[serde(default)]
    pub open_command: Option<String>,
    #[serde(default)]
    pub clipboard_command: Option<String>,
    #[serde(default = "default_image_preview_enabled")]
    pub image_preview_enabled: bool,
    #[serde(default = "default_image_protocol")]
    pub image_protocol: String,
    #[serde(default = "default_max_image_size_mb")]
    pub max_image_size_mb: u64,
    #[serde(default = "default_image_dpi_scale")]
    pub image_dpi_scale: f32,
}

fn default_icon_mode() -> String {
    "nerdfont".to_string()
}

fn default_image_preview_enabled() -> bool {
    true
}

fn default_image_protocol() -> String {
    "auto".to_string()
}

fn default_max_image_size_mb() -> u64 {
    20
}

fn default_image_dpi_scale() -> f32 {
    1.0
}
