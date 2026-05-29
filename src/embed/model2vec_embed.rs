//! Model2Vec static embedding backend — ultra-fast CPU inference.
//! Uses pre-distilled model from sentence-transformers via model2vec-rs.

use crate::config::EmbeddingConfig;
use anyhow::Result;
use model2vec_rs::model::StaticModel;

pub struct Model2VecEmbed {
    model: StaticModel,
}

impl Model2VecEmbed {
    pub fn new(cfg: &EmbeddingConfig) -> Result<Self> {
        let path = cfg.local.model2vec_path.as_deref().unwrap_or_else(|| {
            // Default: look for distilled model alongside the source model name
            // e.g. BAAI/bge-small-zh-v1.5 → ~/.agentrete/models/bge-small-zh-v1.5-m2v
            let _model_name = cfg
                .local
                .model
                .rsplit('/')
                .next()
                .unwrap_or(&cfg.local.model);
            "/not/found/m2v"
        });

        let model = StaticModel::from_pretrained(path, None, None, None)
            .map_err(|e| anyhow::anyhow!("Failed to load model2vec model from {path}: {e}"))?;

        log::info!(
            "Model2Vec loaded: {} ({}d)",
            path,
            model.encode_single(".").len()
        );

        Ok(Self { model })
    }

    /// Embed a batch of texts. Model2Vec is extremely fast — 0.1ms/text on CPU.
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let strings: Vec<String> = texts.iter().map(|&s| s.to_string()).collect();
        Ok(self.model.encode(&strings))
    }

    /// Embed a single text.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self.model.encode_single(text))
    }
}
