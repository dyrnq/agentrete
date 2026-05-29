//! Unified embedder enum — local (candle) or remote (OpenAI/Ollama/Anthropic API).

use crate::config::{EmbeddingConfig, RemoteVendor};
use anyhow::Result;

use super::model2vec_embed::Model2VecEmbed;
use super::remote::anthropic::AnthropicEmbedder;
use super::remote::ollama::OllamaEmbedder;
use super::remote::openai::OpenAIEmbedder;

#[allow(clippy::large_enum_variant)]
pub enum Embedder {
    Local(Box<std::sync::Mutex<super::BasedBertEmbedder>>),
    Model2Vec(Model2VecEmbed),
    OpenAI(OpenAIEmbedder),
    Anthropic(AnthropicEmbedder),
    Ollama(OllamaEmbedder),
}

impl Embedder {
    pub fn from_config(cfg: &EmbeddingConfig) -> Result<Self> {
        match cfg.backend {
            crate::config::EmbeddingBackend::None => {
                anyhow::bail!("Embedder::from_config called with backend=none")
            }
            crate::config::EmbeddingBackend::Local => {
                let model = super::CandleEmbedBuilder::new()
                    .custom_embedding_model(&cfg.local.model)
                    .custom_model_revision(&cfg.local.revision)
                    .with_device_cpu()
                    .build()?;
                Ok(Embedder::Local(Box::new(std::sync::Mutex::new(model))))
            }
            crate::config::EmbeddingBackend::Model2Vec => {
                Model2VecEmbed::new(cfg).map(Embedder::Model2Vec)
            }
            crate::config::EmbeddingBackend::Remote => {
                let url = &cfg.remote.url.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("remote_url is required for remote embedding")
                })?;
                let model = &cfg
                    .remote
                    .model
                    .as_deref()
                    .unwrap_or("qwen3-embedding:latest");
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .build()?;
                let vendor = &cfg
                    .remote
                    .vendor
                    .unwrap_or_else(|| RemoteVendor::detect(url));
                Ok(match vendor {
                    RemoteVendor::OpenAI => Embedder::OpenAI(OpenAIEmbedder::new(
                        url,
                        cfg.remote.api_key.as_deref(),
                        model,
                        client,
                    )),
                    RemoteVendor::Anthropic => {
                        let key = &cfg
                            .remote
                            .api_key
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("Anthropic requires an API key"))?;
                        Embedder::Anthropic(AnthropicEmbedder::new(url, key, model, client))
                    }
                    RemoteVendor::Ollama => {
                        Embedder::Ollama(OllamaEmbedder::new(url, model, client))
                    }
                })
            }
        }
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        match self {
            Embedder::Local(mutex) => {
                let guard = mutex.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                guard.embed_batch(texts)
            }
            Embedder::OpenAI(e) => e.embed_batch(texts).await,
            Embedder::Anthropic(e) => e.embed_batch(texts).await,
            Embedder::Ollama(e) => e.embed_batch(texts).await,
            Embedder::Model2Vec(ref m) => m.embed_batch(texts),
        }
    }

    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        match self {
            Embedder::Local(mutex) => {
                let guard = mutex.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                guard.embed_one(text)
            }
            Embedder::OpenAI(e) => e.embed(text).await,
            Embedder::Anthropic(e) => e.embed(text).await,
            Embedder::Ollama(e) => e.embed(text).await,
            Embedder::Model2Vec(ref m) => m.embed_one(text),
        }
    }
}
