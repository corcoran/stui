use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    #[allow(dead_code)]
    #[serde(default)]
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
    /// Show the current folder, path or previewed file in the terminal title
    #[serde(default = "default_terminal_title")]
    pub terminal_title: bool,
    /// Editor `o` runs inside stui's window; None falls back to $VISUAL, then $EDITOR
    #[serde(default)]
    pub editor: Option<String>,
    /// Text files open in the editor on the desktop too, not only over SSH
    #[serde(default)]
    pub open_text_in_editor: bool,
}

fn default_icon_mode() -> String {
    "nerdfont".to_string()
}

fn default_image_preview_enabled() -> bool {
    true
}

fn default_terminal_title() -> bool {
    true
}

fn default_image_protocol() -> String {
    "auto".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "api_key: k\nbase_url: http://127.0.0.1:8384\npath_map: {}\n";

    #[test]
    fn test_terminal_title_defaults_on() {
        let config: Config = serde_yaml::from_str(MINIMAL).unwrap();
        assert!(config.terminal_title);
    }

    #[test]
    fn test_terminal_title_can_be_turned_off() {
        let yaml = format!("{}terminal_title: false\n", MINIMAL);
        let config: Config = serde_yaml::from_str(&yaml).unwrap();
        assert!(!config.terminal_title);
    }

    #[test]
    fn test_path_map_is_optional() {
        let config: Config =
            serde_yaml::from_str("api_key: k\nbase_url: http://127.0.0.1:8384\n").unwrap();
        assert!(config.path_map.is_empty());
    }

    #[test]
    fn test_shipped_example_config_parses() {
        let config: Config = serde_yaml::from_str(include_str!("../config.yaml.example")).unwrap();
        assert!(config.terminal_title);
    }

    #[test]
    fn test_readme_config_matches_shipped_example() {
        let example = include_str!("../config.yaml.example").trim();
        assert!(
            include_str!("../README.md").contains(example),
            "README's config block differs from config.yaml.example"
        );
    }

    #[test]
    fn test_editor_settings_default_off() {
        let config: Config = serde_yaml::from_str(MINIMAL).unwrap();
        assert_eq!(config.editor, None);
        assert!(!config.open_text_in_editor);
    }

    #[test]
    fn test_editor_settings_parse() {
        let yaml = format!(
            "{}editor: \"nvim -R\"\nopen_text_in_editor: true\n",
            MINIMAL
        );
        let config: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.editor.as_deref(), Some("nvim -R"));
        assert!(config.open_text_in_editor);
    }
}
