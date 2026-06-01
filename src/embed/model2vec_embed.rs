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
        let default_model_path = dirs::home_dir()
            .unwrap_or_else(|| "/tmp".into())
            .join(".cache/model2vec/bge-small-256d");
        let raw = cfg
            .model2vec
            .path
            .as_deref()
            .unwrap_or(default_model_path.to_str().unwrap_or("/tmp/bge-small-256d"));
        // Expand tilde (~) to home directory
        let path: String = if let Some(stripped) = raw.strip_prefix("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| "/tmp".into())
                .join(stripped)
                .to_string_lossy()
                .to_string()
        } else {
            raw.to_string()
        };

        // Guard: warn if model directory is unusually large (>2GB)
        let size_mb = if let Ok(meta) = std::fs::metadata(&path) {
            let size_bytes: u64 = if meta.is_dir() {
                walkdir::WalkDir::new(&path)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_file())
                    .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                    .sum()
            } else {
                meta.len()
            };
            let mb = size_bytes / (1024 * 1024);
            if mb > 2048 {
                log::warn!(
                    "model2vec: model at {} is {mb}MB — may cause OOM. Consider using a smaller model.", path
                );
            }
            mb
        } else {
            0
        };
        log::info!("model2vec: model size ~{size_mb}MB");

        let load_start = std::time::Instant::now();
        log::info!("model2vec: loading {} (~{size_mb}MB) ...", path);

        let model = StaticModel::from_pretrained(&path, None, None, None)
            .map_err(|e| anyhow::anyhow!("Failed to load model2vec model from {}: {e}", path))?;

        let elapsed = load_start.elapsed();
        log::info!(
            "Model2Vec loaded: {} ({}d) in {:.1}s",
            path,
            model.encode_single(".").len(),
            elapsed.as_secs_f64()
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
