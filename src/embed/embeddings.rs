//! Unified embedder enum — local (candle) or remote (OpenAI/Ollama API).

use crate::config::EmbeddingConfig;
use anyhow::Result;

pub enum Embedder {
    Local(std::sync::Mutex<super::BasedBertEmbedder>),
    Remote(super::remote::RemoteEmbedder),
}

impl Embedder {
    pub fn from_config(cfg: &EmbeddingConfig) -> Result<Self> {
        match cfg.backend {
            crate::config::EmbeddingBackend::None => {
                anyhow::bail!("Embedder::from_config called with backend=none")
            }
            crate::config::EmbeddingBackend::Local => {
                let model = super::CandleEmbedBuilder::new()
                    .with_device_cpu()
                    .build()?;
                Ok(Embedder::Local(std::sync::Mutex::new(model)))
            }
            crate::config::EmbeddingBackend::Remote => {
                let url = cfg
                    .remote_url
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("remote_url is required for remote embedding"))?;
                let model = cfg
                    .remote_model
                    .as_deref()
                    .unwrap_or("text-embedding-3-small");
                let remote = super::remote::RemoteEmbedder::new(
                    url,
                    cfg.remote_api_key.as_deref(),
                    model,
                )?;
                Ok(Embedder::Remote(remote))
            }
        }
    }

    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        match self {
            Embedder::Local(mutex) => {
                let guard = mutex.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
                guard.embed_one(text)
            }
            Embedder::Remote(remote) => remote.embed_one_async(text).await,
        }
    }
}
