//! Unified configuration: CLI args → env vars → config file → defaults.
//!
//! Priority (highest first): CLI args > env vars > config file > defaults.
//! Config file format auto-detected from extension: toml, yaml, yml, json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_PORT: u16 = 9092;
const DEFAULT_MODEL: &str = "moka-ai/m3e-base";

/// Full configuration for agentrete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// HTTP server port
    #[serde(default = "default_port")]
    pub port: u16,
    /// Embedding model ID on HuggingFace
    #[serde(default = "default_model")]
    pub model_id: String,
    /// HuggingFace endpoint (mirror)
    #[serde(default = "default_hf_endpoint")]
    pub hf_endpoint: String,
    /// Disable embedding (BM25 only)
    #[serde(default)]
    pub no_embed: bool,
    /// Custom database directory (default: $HOME/.agentrete)
    #[serde(default)]
    pub db_dir: Option<PathBuf>,
    /// Custom cache directory (default: $HOME/.cache)
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

fn default_port() -> u16 { DEFAULT_PORT }
fn default_model() -> String { DEFAULT_MODEL.to_string() }
fn default_hf_endpoint() -> String { "https://hf-mirror.com".to_string() }

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            model_id: DEFAULT_MODEL.to_string(),
            hf_endpoint: "https://hf-mirror.com".to_string(),
            no_embed: false,
            db_dir: None,
            cache_dir: None,
        }
    }
}

impl Config {
    /// Load config from all sources: env vars → config file → merge with defaults.
    pub fn load(cli_port: Option<u16>, cli_config: Option<&str>) -> Self {
        let mut cfg = Config::default();

        // 1. Config file (lowest priority among overrides)
        if let Some(path) = cli_config {
            if let Ok(c) = Self::from_file(path) {
                cfg.merge(c);
            }
        } else {
            for loc in default_config_locations() {
                if let Ok(c) = Self::from_file(&loc) {
                    cfg.merge(c);
                    break;
                }
            }
        }

        // 2. Environment variables
        cfg.merge(Self::from_env());

        // 3. CLI args (highest priority)
        if let Some(port) = cli_port {
            cfg.port = port;
        }

        cfg
    }

    /// Parse config file, auto-detecting format from extension.
    fn from_file(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let cfg = if path.ends_with(".yaml") || path.ends_with(".yml") {
            serde_yaml::from_str(&content)?
        } else if path.ends_with(".json") {
            serde_json::from_str(&content)?
        } else {
            // Default: TOML (also covers .toml extension)
            toml::from_str(&content)?
        };
        Ok(cfg)
    }

    fn from_env() -> Config {
        Config {
            port: std::env::var("AGENTRETE_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
            model_id: std::env::var("AGENTRETE_MODEL").ok().unwrap_or_default(),
            hf_endpoint: std::env::var("HF_ENDPOINT").ok().unwrap_or_else(|| "https://hf-mirror.com".to_string()),
            no_embed: std::env::var("AGENTRETE_NO_EMBED").map(|v| v == "1" || v == "true").unwrap_or(false),
            db_dir: std::env::var("AGENTRETE_DB_DIR").ok().map(PathBuf::from),
            cache_dir: std::env::var("AGENTRETE_CACHE_DIR").ok().map(PathBuf::from),
        }
    }

    fn merge(&mut self, other: Config) {
        if other.port != 0 { self.port = other.port; }
        if !other.model_id.is_empty() { self.model_id = other.model_id; }
        if other.hf_endpoint != "https://hf-mirror.com" { self.hf_endpoint = other.hf_endpoint; }
        if other.no_embed { self.no_embed = true; }
        if other.db_dir.is_some() { self.db_dir = other.db_dir; }
        if other.cache_dir.is_some() { self.cache_dir = other.cache_dir; }
    }

    pub fn db_dir(&self) -> PathBuf {
        self.db_dir.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".agentrete")
        })
    }

    #[allow(dead_code)]
    pub fn hf_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".cache/huggingface/hub")
        })
    }
}

fn default_config_locations() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    vec![
        format!("{}/.agentrete/config.toml", home),
        format!("{}/.agentrete/config.yaml", home),
        format!("{}/.agentrete/config.yml", home),
        format!("{}/.agentrete/config.json", home),
        "agentrete.toml".to_string(),
        "agentrete.yaml".to_string(),
        "agentrete.yml".to_string(),
        "agentrete.json".to_string(),
    ]
}

#[allow(dead_code)]
pub fn generate_sample() -> String {
    toml::to_string_pretty(&Config::default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 9092);
        assert_eq!(cfg.model_id, "moka-ai/m3e-base");
        assert!(!cfg.no_embed);
    }

    #[test]
    fn test_env_override() {
        std::env::set_var("AGENTRETE_PORT", "9999");
        std::env::set_var("AGENTRETE_NO_EMBED", "1");
        let env = Config::from_env();
        assert_eq!(env.port, 9999);
        assert!(env.no_embed);
        std::env::remove_var("AGENTRETE_PORT");
        std::env::remove_var("AGENTRETE_NO_EMBED");
    }

    #[test]
    fn test_merge() {
        let mut base = Config::default();
        let over = Config { port: 9999, model_id: String::new(), no_embed: true, ..Config::default() };
        base.merge(over);
        assert_eq!(base.port, 9999);
        assert_eq!(base.model_id, "moka-ai/m3e-base");
        assert!(base.no_embed);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"port = 8888
model_id = "x/y"
no_embed = true
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.port, 8888);
        assert!(cfg.no_embed);
    }

    #[test]
    fn test_parse_yaml() {
        let yaml_str = "port: 7777\nmodel_id: a/b\nno_embed: true\n";
        let cfg: Config = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(cfg.port, 7777);
    }

    #[test]
    fn test_parse_json() {
        let json_str = r#"{"port": 6666, "model_id": "c/d", "no_embed": true}"#;
        let cfg: Config = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.port, 6666);
    }
}
