//! Unified configuration: CLI args → env vars → config file → defaults.
//!
//! Priority (highest first): CLI args > env vars > config file > defaults.
//! Config file format auto-detected from extension: toml, yaml, yml, json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_PORT: u16 = 9092;

// ─── Embedding config ────────────────────────────────────────────────────────

/// Where to run embedding inference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    /// No embedding — BM25 FTS only.
    None,
    /// Local on-device model (candle + HuggingFace model).
    Local,
    /// Remote API (URL auto-detects vendor: OpenAI/Anthropic/Ollama).
    Remote,
}

impl Default for EmbeddingBackend {
    fn default() -> Self {
        EmbeddingBackend::Local
    }
}

/// Remote embedding vendor (only relevant when backend = "remote").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteVendor {
    /// OpenAI-compatible (OpenAI, vLLM, TEI, DeepSeek, etc.)
    #[serde(alias = "openai")]
    OpenAI,
    /// Anthropic embeddings endpoint.
    #[serde(alias = "anthropic")]
    Anthropic,
    /// Ollama local/remote embeddings.
    #[serde(alias = "ollama")]
    Ollama,
}

impl Default for RemoteVendor {
    fn default() -> Self {
        RemoteVendor::OpenAI
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Backend type: none, local, remote.
    #[serde(default)]
    pub backend: EmbeddingBackend,

    /// HuggingFace model ID (only for `local` backend).
    #[serde(default = "default_model_id")]
    pub model_id: String,

    /// HuggingFace model revision.
    #[serde(default = "default_revision")]
    pub revision: String,

    /// Embedding vector dimensions (model-specific).
    /// 768 for m3e-base, 1024 for bge-m3, 4096 for qwen3-embedding.
    #[serde(default = "default_dims")]
    pub dims: u16,

    /// HuggingFace endpoint mirror.
    #[serde(default = "default_hf_endpoint")]
    pub hf_endpoint: String,

    // ─── Remote backend fields ───────────────────────────────────────────────
    /// Remote embeddings API base URL (e.g., https://api.openai.com/v1).
    #[serde(default)]
    pub remote_url: Option<String>,

    /// Remote embeddings API key.
    #[serde(default)]
    pub remote_api_key: Option<String>,

    /// Remote embeddings vendor (auto-detected from URL if unset).
    #[serde(default)]
    pub remote_vendor: Option<RemoteVendor>,

    /// Remote embeddings model name (e.g., text-embedding-3-small).
    #[serde(default)]
    pub remote_model: Option<String>,
}

fn default_model_id() -> String {
    "moka-ai/m3e-base".to_string()
}
fn default_revision() -> String {
    "main".to_string()
}
fn default_dims() -> u16 {
    768
}
fn default_hf_endpoint() -> String {
    "https://hf-mirror.com".to_string()
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            backend: EmbeddingBackend::Local,
            model_id: default_model_id(),
            revision: default_revision(),
            dims: default_dims(),
            hf_endpoint: default_hf_endpoint(),
            remote_url: None,
            remote_api_key: None,
            remote_vendor: None,
            remote_model: None,
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
        let mut cfg = Config::default();

        // 1. Config file
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

        // 3. CLI args
        if let Some(port) = cli_port {
            cfg.port = port;
        }

        cfg
    }

    fn from_file(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let cfg = if path.ends_with(".yaml") || path.ends_with(".yml") {
            serde_yaml::from_str(&content)?
        } else if path.ends_with(".json") {
            serde_json::from_str(&content)?
        } else {
            toml::from_str(&content)?
        };
        Ok(cfg)
    }

    fn from_env() -> Config {
        let backend = match std::env::var("AGENTRETE_EMBED_BACKEND").ok().as_deref() {
            Some("none") | Some("false") | Some("0") => EmbeddingBackend::None,
            Some("remote") => EmbeddingBackend::Remote,
            _ => EmbeddingBackend::Local,
        };

        Config {
            port: std::env::var("AGENTRETE_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            embedding: EmbeddingConfig {
                backend,
                model_id: std::env::var("AGENTRETE_MODEL_ID").ok().unwrap_or_default(),
                revision: std::env::var("AGENTRETE_MODEL_REVISION")
                    .ok()
                    .unwrap_or_default(),
                dims: std::env::var("AGENTRETE_EMBED_DIMS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                hf_endpoint: std::env::var("HF_ENDPOINT").ok().unwrap_or_default(),
                remote_url: std::env::var("AGENTRETE_REMOTE_URL").ok(),
                remote_api_key: std::env::var("AGENTRETE_REMOTE_API_KEY").ok(),
                remote_model: std::env::var("AGENTRETE_REMOTE_MODEL").ok(),
                remote_vendor: std::env::var("AGENTRETE_REMOTE_VENDOR").ok().map(|v| {
                    match v.to_lowercase().as_str() {
                        "ollama" => RemoteVendor::Ollama,
                        "anthropic" => RemoteVendor::Anthropic,
                        _ => RemoteVendor::OpenAI,
                    }
                }),
            },
            db_dir: std::env::var("AGENTRETE_DB_DIR").ok().map(PathBuf::from),
            cache_dir: std::env::var("AGENTRETE_CACHE_DIR").ok().map(PathBuf::from),
        }
    }

    fn merge(&mut self, other: Config) {
        if other.port != 0 {
            self.port = other.port;
        }

        // Embedding config: merge non-zero/non-empty fields
        if other.embedding.backend != EmbeddingBackend::Local {
            self.embedding.backend = other.embedding.backend;
        }
        if !other.embedding.model_id.is_empty() {
            self.embedding.model_id = other.embedding.model_id;
        }
        if !other.embedding.revision.is_empty() && other.embedding.revision != "main" {
            self.embedding.revision = other.embedding.revision;
        }
        if other.embedding.dims != 0 {
            self.embedding.dims = other.embedding.dims;
        }
        if !other.embedding.hf_endpoint.is_empty()
            && other.embedding.hf_endpoint != default_hf_endpoint()
        {
            self.embedding.hf_endpoint = other.embedding.hf_endpoint;
        }
        if other.embedding.remote_url.is_some() {
            self.embedding.remote_url = other.embedding.remote_url;
        }
        if other.embedding.remote_api_key.is_some() {
            self.embedding.remote_api_key = other.embedding.remote_api_key;
        }
        if other.embedding.remote_vendor.is_some() {
            self.embedding.remote_vendor = other.embedding.remote_vendor;
        }
        if other.embedding.remote_model.is_some() {
            self.embedding.remote_model = other.embedding.remote_model;
        }

        if other.db_dir.is_some() {
            self.db_dir = other.db_dir;
        }
        if other.cache_dir.is_some() {
            self.cache_dir = other.cache_dir;
        }
    }

    // ─── Convenience accessors ───────────────────────────────────────────────

    /// Whether embedding is disabled (backend = none).
    pub fn embed_enabled(&self) -> bool {
        self.embedding.backend != EmbeddingBackend::None
    }

    /// Whether using remote embeddings API.
    pub fn embed_is_remote(&self) -> bool {
        self.embedding.backend == EmbeddingBackend::Remote
    }

    /// Resolve remote vendor: explicit config first, auto-detect from URL as fallback.
    pub fn remote_vendor(&self) -> RemoteVendor {
        self.embedding.remote_vendor.unwrap_or_else(|| {
            self.embedding
                .remote_url
                .as_deref()
                .map(RemoteVendor::detect)
                .unwrap_or_default()
        })
    }

    /// The effective embedding model ID (remote_model for remote, model_id for local).
    pub fn effective_model_id(&self) -> String {
        if self.embed_is_remote() {
            self.embedding
                .remote_model
                .clone()
                .unwrap_or_else(|| self.embedding.model_id.clone())
        } else {
            self.embedding.model_id.clone()
        }
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
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::Local);
        assert_eq!(cfg.embedding.model_id, "moka-ai/m3e-base");
        assert_eq!(cfg.embedding.dims, 768);
    }

    #[test]
    fn test_parse_toml_embed_remote() {
        let toml_str = r#"
port = 9092

[embedding]
backend = "remote"
remote_url = "https://api.openai.com/v1"
remote_api_key = "sk-xxx"
remote_model = "qwen3-embedding:latest"
dims = 1536
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::Remote);
        assert_eq!(cfg.embedding.dims, 1536);
        assert_eq!(
            cfg.embedding.remote_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(cfg.remote_vendor(), RemoteVendor::OpenAI);
    }

    #[test]
    fn test_remote_vendor_explicit() {
        let toml_str = r#"
[embedding]
backend = "remote"
remote_url = "http://192.168.6.9:11434"
remote_vendor = "ollama"
remote_model = "granite-embedding:278m"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.remote_vendor(), RemoteVendor::Ollama);
    }

    #[test]
    fn test_remote_vendor_auto_detect() {
        let toml_str = r#"
[embedding]
backend = "remote"
remote_url = "https://api.anthropic.com"
remote_model = "voyage-3"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.remote_vendor(), RemoteVendor::Anthropic);
    }

    #[test]
    fn test_parse_toml_embed_none() {
        let toml_str = r#"
[embedding]
backend = "none"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.embedding.backend, EmbeddingBackend::None);
        assert!(!cfg.embed_enabled());
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
        assert_eq!(cfg.embedding.model_id, "BAAI/bge-small-zh-v1.5");
        assert_eq!(cfg.embedding.dims, 512);
    }

    #[test]
    fn test_parse_json_embed() {
        let json_str = r#"{"embedding":{"backend":"none"}}"#;
        let cfg: Config = serde_json::from_str(json_str).unwrap();
        assert!(!cfg.embed_enabled());
    }

    #[test]
    fn test_env_override() {
        std::env::set_var("AGENTRETE_EMBED_BACKEND", "none");
        std::env::set_var("AGENTRETE_EMBED_DIMS", "1024");
        let env = Config::from_env();
        assert_eq!(env.embedding.backend, EmbeddingBackend::None);
        assert_eq!(env.embedding.dims, 1024);
        std::env::remove_var("AGENTRETE_EMBED_BACKEND");
        std::env::remove_var("AGENTRETE_EMBED_DIMS");
    }
}
