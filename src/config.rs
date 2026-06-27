use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    pub admin: i64,
    pub bot_token: String,
    #[serde(default)]
    pub member: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenListConfig {
    pub openlist_host: String,
    pub openlist_token: String,
    #[serde(default = "default_slash")]
    pub download_path: String,
    #[serde(default = "default_download_tool")]
    pub download_tool: String,
}

fn default_slash() -> String {
    "/".to_string()
}

fn default_download_tool() -> String {
    "qbittorrent".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PanSouConfig {
    pub pansou_host: String,
    pub pansou_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enable: bool,
    pub hostname: String,
    pub port: u16,
    #[serde(default = "default_scheme")]
    pub scheme: String,
}

fn default_scheme() -> String {
    "http".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchConfig {
    #[serde(default = "default_sources")]
    pub allowed_sources: Vec<String>,
}

fn default_sources() -> Vec<String> {
    vec![
        "baidu".to_string(),
        "aliyun".to_string(),
        "quark".to_string(),
        "tianyi".to_string(),
        "115".to_string(),
        "pikpak".to_string(),
        "xunlei".to_string(),
        "123".to_string(),
        "magnet".to_string(),
        "ed2k".to_string(),
        "uc".to_string(),
        "sukebei".to_string(),
    ]
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            allowed_sources: default_sources(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub user: UserConfig,
    pub openlist: OpenListConfig,
    pub pansou: Option<PanSouConfig>,
    pub proxy: Option<ProxyConfig>,
    #[serde(default)]
    pub search: SearchConfig,
}

fn default_log_level() -> String {
    "INFO".to_string()
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path = Path::new("config.yaml");
        let content = if path.exists() {
            info!("Loading config from config.yaml");
            fs::read_to_string(path)?
        } else {
            let example_path = Path::new("config.example.yaml");
            if example_path.exists() {
                info!("config.yaml not found, loading from config.example.yaml");
                fs::read_to_string(example_path)?
            } else {
                return Err("Neither config.yaml nor config.example.yaml exists".into());
            }
        };
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let content = serde_yaml::to_string(self)?;
        fs::write("config.yaml", content)?;
        info!("Saved config to config.yaml");
        Ok(())
    }
}
