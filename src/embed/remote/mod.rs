//! Remote embedding providers (OpenAI, Anthropic, Ollama, etc.)
//! Each vendor lives in its own submodule. The `RemoteEmbedder` enum dispatches
//! to the correct implementation at call time.

pub mod anthropic;
pub mod ollama;
pub mod openai;

use anyhow::Result;
use reqwest::Client;

use anthropic::AnthropicEmbedder;
use ollama::OllamaEmbedder;
use openai::OpenAIEmbedder;

/// Auto-detect provider from URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProvider {
    OpenAI,
    Anthropic,
    Ollama,
}

impl RemoteProvider {
    pub fn detect(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.contains(":11434") || lower.contains("ollama") {
            RemoteProvider::Ollama
        } else if lower.contains("anthropic") {
            RemoteProvider::Anthropic
        } else {
            RemoteProvider::OpenAI
        }
    }
}

pub enum RemoteEmbedder {
    OpenAI(OpenAIEmbedder),
    Anthropic(AnthropicEmbedder),
    Ollama(OllamaEmbedder),
}

impl RemoteEmbedder {
    pub fn new(url: &str, api_key: Option<&str>, model: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(match RemoteProvider::detect(url) {
            RemoteProvider::OpenAI => {
                RemoteEmbedder::OpenAI(OpenAIEmbedder::new(url, api_key, model, client))
            }
            RemoteProvider::Anthropic => {
                let key =
                    api_key.ok_or_else(|| anyhow::anyhow!("Anthropic requires an API key"))?;
                RemoteEmbedder::Anthropic(AnthropicEmbedder::new(url, key, model, client))
            }
            RemoteProvider::Ollama => {
                RemoteEmbedder::Ollama(OllamaEmbedder::new(url, model, client))
            }
        })
    }

    pub async fn embed_one_async(&self, text: &str) -> Result<Vec<f32>> {
        match self {
            RemoteEmbedder::OpenAI(e) => e.embed(text).await,
            RemoteEmbedder::Anthropic(e) => e.embed(text).await,
            RemoteEmbedder::Ollama(e) => e.embed(text).await,
        }
    }

    pub async fn embed_batch_async(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        match self {
            RemoteEmbedder::OpenAI(e) => e.embed_batch(texts).await,
            RemoteEmbedder::Anthropic(e) => e.embed_batch(texts).await,
            RemoteEmbedder::Ollama(e) => e.embed_batch(texts).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_openai() {
        assert!(matches!(
            RemoteProvider::detect("https://api.openai.com/v1"),
            RemoteProvider::OpenAI
        ));
    }

    #[test]
    fn test_detect_anthropic() {
        assert!(matches!(
            RemoteProvider::detect("https://api.anthropic.com"),
            RemoteProvider::Anthropic
        ));
    }

    #[test]
    fn test_detect_ollama_port() {
        assert!(matches!(
            RemoteProvider::detect("http://localhost:11434"),
            RemoteProvider::Ollama
        ));
    }

    #[test]
    fn test_detect_ollama_name() {
        assert!(matches!(
            RemoteProvider::detect("https://ollama.example.com"),
            RemoteProvider::Ollama
        ));
    }
}
