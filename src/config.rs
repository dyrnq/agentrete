//! Unified configuration: CLI args → env vars → config file → defaults.
//!
//! Priority (highest first): CLI args > env vars > config file > defaults.

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

fn default_port() -> u16 {
    DEFAULT_PORT
}
fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}
fn default_hf_endpoint() -> String {
    "https://hf-mirror.com".to_string()
}

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
            // Try default locations
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

    fn from_file(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    }

    fn from_env() -> Config {
        Config {
            port: std::env::var("AGENTRETE_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            model_id: std::env::var("AGENTRETE_MODEL").ok().unwrap_or_default(),
            hf_endpoint: std::env::var("HF_ENDPOINT")
                .ok()
                .unwrap_or_else(|| "https://hf-mirror.com".to_string()),
            no_embed: std::env::var("AGENTRETE_NO_EMBED")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
            db_dir: std::env::var("AGENTRETE_DB_DIR").ok().map(PathBuf::from),
            cache_dir: std::env::var("AGENTRETE_CACHE_DIR").ok().map(PathBuf::from),
        }
    }

    /// Merge non-zero/non-empty values from other into self.
    fn merge(&mut self, other: Config) {
        if other.port != 0 {
            self.port = other.port;
        }
        if !other.model_id.is_empty() {
            self.model_id = other.model_id;
        }
        if other.hf_endpoint != "https://hf-mirror.com" {
            self.hf_endpoint = other.hf_endpoint;
        }
        if other.no_embed {
            self.no_embed = true;
        }
        if other.db_dir.is_some() {
            self.db_dir = other.db_dir;
        }
        if other.cache_dir.is_some() {
            self.cache_dir = other.cache_dir;
        }
    }

    /// Resolved database directory path.
    pub fn db_dir(&self) -> PathBuf {
        self.db_dir.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".agentrete")
        })
    }

    /// Resolved HuggingFace cache directory.
    pub fn hf_cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".cache/huggingface/hub")
        })
    }
}

/// Default config file search locations.
fn default_config_locations() -> Vec<String> {
    vec![
        "agentrete.toml".to_string(),
        format!(
            "{}/.agentrete/config.toml",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
        ),
    ]
}

/// Generate a sample config file to stdout.
pub fn generate_sample() -> String {
    let cfg = Config::default();
    toml::to_string_pretty(&cfg).unwrap_or_default()
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
        let over = Config {
            port: 9999,
            model_id: String::new(), // empty → don't override
            no_embed: true,
            ..Config::default()
        };
        base.merge(over);
        assert_eq!(base.port, 9999);
        assert_eq!(base.model_id, "moka-ai/m3e-base"); // unchanged
        assert!(base.no_embed);
    }

    #[test]
    fn test_sample_config_can_parse() {
        let sample = generate_sample();
        let _: Config = toml::from_str(&sample).unwrap();
    }
}
