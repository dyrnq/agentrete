//! Unified configuration: CLI args → env vars → config file → defaults.
//!
//! Priority (highest first): CLI args > env vars > config file > defaults.
//! Config file format auto-detected from extension: toml, yaml, yml, json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_PORT: u16 = 9092;

// ─── Embedding config ────────────────────────────────────────────────────────

/// Where to run embedding inference.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    /// No embedding — BM25 FTS only.
    None,
    /// Model2Vec static embeddings — distilled sentence-transformers, 10MB, ultra-fast CPU.
    #[default]
    Model2Vec,
    /// Remote API (URL auto-detects vendor: OpenAI/Anthropic/Ollama).
    Remote,
}

/// Remote embedding vendor (only relevant when backend = "remote").
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteVendor {
    /// OpenAI-compatible (OpenAI, vLLM, TEI, DeepSeek, etc.)
    #[serde(alias = "openai")]
    #[default]
    OpenAI,
    /// Anthropic embeddings endpoint.
    #[serde(alias = "anthropic")]
    Anthropic,
    /// Ollama local/remote embeddings.
    #[serde(alias = "ollama")]
    Ollama,
}

impl RemoteVendor {
    /// Try to auto-detect vendor from URL string.
    pub fn detect(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.contains(":11434") || lower.contains("ollama") {
            RemoteVendor::Ollama
        } else if lower.contains("anthropic") {
            RemoteVendor::Anthropic
        } else {
            RemoteVendor::OpenAI
        }
    }
}

/// Embedding model configuration.
/// Remote API sub-config (TOML: [embedding.remote]).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub vendor: Option<RemoteVendor>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dims: Option<u16>,
}

/// Local model sub-config (TOML: [embedding.local] or [embedding.model2vec]).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalConfig {
    #[serde(default = "default_model_id")]
    pub model: String,
    #[serde(default = "default_revision")]
    pub revision: String,
    #[serde(default = "default_dims")]
    pub dims: u16,
    /// Path to pre-distilled model2vec directory (for backend = "model2vec").
    /// If empty, uses model field to find source sentence-transformers model.
    #[serde(default)]
    pub model2vec_path: Option<String>,
    #[serde(default = "default_hf_endpoint")]
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Backend type: none, local, remote.
    #[serde(default)]
    pub backend: EmbeddingBackend,

    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub local: LocalConfig,
}

fn default_model_id() -> String {
    "BAAI/bge-small-zh-v1.5".to_string()
}
fn default_revision() -> String {
    "main".to_string()
}
fn default_dims() -> u16 {
    512
}
fn default_hf_endpoint() -> String {
    "https://hf-mirror.com".to_string()
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            backend: EmbeddingBackend::Model2Vec,
            local: LocalConfig {
                model: "BAAI/bge-small-zh-v1.5".to_string(),
                dims: 512,
                ..Default::default()
            },
            remote: RemoteConfig::default(),
        }
    }
}

// ─── Top-level config ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// HTTP server port.
    #[serde(default = "default_port_val")]
    pub port: u16,

    /// Embedding configuration.
    #[serde(default)]
    pub embedding: EmbeddingConfig,

    /// Custom database directory (default: $HOME/.agentrete).
    #[serde(default)]
    pub db_dir: Option<PathBuf>,

    /// Custom cache directory (default: $HOME/.cache).
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

fn default_port_val() -> u16 {
    DEFAULT_PORT
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            embedding: EmbeddingConfig::default(),
            db_dir: None,
            cache_dir: None,
        }
    }
}

impl Config {
    /// Load config from all sources: file → env vars → CLI args.
    pub fn load(cli_port: Option<u16>, cli_config: Option<&str>) -> Self {
        use ::config::{Config as ConfigBuilder, Environment, File};

        let mut builder = ConfigBuilder::builder();

        if let Some(path) = cli_config {
            builder = builder.add_source(File::with_name(path).required(true));
        } else {
            for loc in default_config_locations() {
                builder = builder.add_source(File::with_name(&loc).required(false));
            }
        }

        builder = builder.add_source(
            Environment::with_prefix("AGENTRETE")
                .separator("__")
                .try_parsing(true),
        );

        let mut cfg: Self = builder
            .build()
            .and_then(|c| c.try_deserialize())
            .unwrap_or_default();

        if let Some(port) = cli_port {
            cfg.port = port;
        }

        cfg
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

// ─── Default config file search paths ────────────────────────────────────────

fn default_config_locations() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let name = env!("CARGO_PKG_NAME");
    let exts = ["toml", "yaml", "yml", "json"];
    let mut paths = Vec::new();
    for ext in &exts {
        paths.push(format!("{}/.{}/config.{}", home, name, ext));
    }
    for ext in &exts {
        paths.push(format!("{}.{}", name, ext));
    }
    paths
}

#[allow(dead_code)]
pub fn generate_sample() -> String {
    toml::to_string_pretty(&Config::default()).unwrap_or_default()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 9092);
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::Model2Vec);
        assert_eq!(cfg.embedding.local.model, "BAAI/bge-small-zh-v1.5");
        assert_eq!(cfg.embedding.local.dims, 512);
    }

    #[test]
    fn test_parse_toml_embed_remote() {
        let toml_str = r#"
port = 9092

[embedding]
backend = "remote"
[embedding.remote]
url = "https://api.openai.com/v1"
api_key = "sk-xxx"
model = "qwen3-embedding:latest"
dims = 1536
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::Remote);
        assert_eq!(cfg.embedding.local.dims, 1536);
        assert_eq!(
            cfg.embedding.remote.url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(
            cfg.embedding
                .remote
                .vendor
                .unwrap_or_else(|| RemoteVendor::detect("")),
            RemoteVendor::OpenAI
        );
    }

    #[test]
    fn test_remote_vendor_explicit() {
        let toml_str = r#"
[embedding]
backend = "remote"
[embedding.remote]
url = "http://192.168.6.9:11434"
vendor = "ollama"
model = "granite-embedding:278m"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.embedding
                .remote
                .vendor
                .unwrap_or_else(|| RemoteVendor::detect("")),
            RemoteVendor::Ollama
        );
    }

    #[test]
    fn test_remote_vendor_auto_detect() {
        let toml_str = r#"
[embedding]
backend = "remote"
[embedding.remote]
url = "https://api.anthropic.com"
model = "voyage-3"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.embedding
                .remote
                .vendor
                .unwrap_or_else(|| RemoteVendor::detect("")),
            RemoteVendor::Anthropic
        );
    }

    #[test]
    fn test_parse_toml_embed_none() {
        let toml_str = r#"
[embedding]
backend = "none"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::None);
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::None);
    }

    #[test]
    fn test_parse_yaml_embed() {
        let yaml_str = r#"
embedding:
  backend: local
  model_id: BAAI/bge-small-zh-v1.5
  dims: 512
"#;
        let cfg: Config = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(cfg.embedding.local.model, "BAAI/bge-small-zh-v1.5");
        assert_eq!(cfg.embedding.local.dims, 512);
    }

    #[test]
    fn test_parse_json_embed() {
        let json_str = r#"{"embedding":{"backend":"none"}}"#;
        let cfg: Config = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::None);
    }

    #[test]
    fn test_env_override() {
        std::env::set_var("AGENTRETE__PORT", "1234");
        let cfg = Config::load(None, None);
        assert_eq!(cfg.port, 1234);
        std::env::remove_var("AGENTRETE__PORT");
    }
}
