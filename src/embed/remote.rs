//! Remote embedding backends: OpenAI-compatible and Ollama-compatible APIs.

use anyhow::Result;

/// Supported remote embedding API providers.
#[derive(Debug, Clone)]
pub enum RemoteProvider {
    /// OpenAI / OpenAI-compatible (e.g., deepseek, zhipu, xiaomimimo).
    OpenAI,
    /// Ollama (local or remote).
    Ollama,
}

impl RemoteProvider {
    /// Detect provider from the remote_url.
    pub fn detect(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.contains("ollama") || lower.contains(":11434") {
            RemoteProvider::Ollama
        } else {
            // Default: OpenAI-compatible format
            RemoteProvider::OpenAI
        }
    }
}

/// Remote embedding client.
pub struct RemoteEmbedder {
    url: String,
    api_key: Option<String>,
    model: String,
    provider: RemoteProvider,
    client: reqwest::blocking::Client,
}

impl RemoteEmbedder {
    pub fn new(url: &str, api_key: Option<&str>, model: &str) -> Result<Self> {
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
            model: model.to_string(),
            provider: RemoteProvider::detect(url),
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// Embed a single text, returning a float vector.
    /// Automatically selects the correct API format based on provider.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        match self.provider {
            RemoteProvider::OpenAI => self.embed_openai(text),
            RemoteProvider::Ollama => self.embed_ollama(text),
        }
    }

    // ─── OpenAI format ───────────────────────────────────────────────────────

    fn embed_openai(&self, text: &str) -> Result<Vec<f32>> {
        let endpoint = format!("{}/embeddings", self.url);

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let mut req = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json");

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp: serde_json::Value = req.send()?.json()?;

        // OpenAI format: { "data": [{ "embedding": [...] }] }
        let embedding = resp["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("OpenAI: missing data[0].embedding in response"))?;

        let vec: Vec<f32> = embedding
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        if vec.is_empty() {
            anyhow::bail!("OpenAI: empty embedding returned");
        }

        Ok(vec)
    }

    // ─── Ollama format ───────────────────────────────────────────────────────

    fn embed_ollama(&self, text: &str) -> Result<Vec<f32>> {
        let endpoint = format!("{}/api/embed", self.url);

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let resp: serde_json::Value = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .send()?
            .json()?;

        // Ollama format: { "embeddings": [[...]] }
        let embedding = resp["embeddings"][0]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Ollama: missing embeddings[0] in response"))?;

        let vec: Vec<f32> = embedding
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        if vec.is_empty() {
            anyhow::bail!("Ollama: empty embedding returned");
        }

        Ok(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_openai() {
        let p = RemoteProvider::detect("https://api.openai.com/v1");
        assert!(matches!(p, RemoteProvider::OpenAI));
    }

    #[test]
    fn test_detect_ollama_local() {
        let p = RemoteProvider::detect("http://localhost:11434");
        assert!(matches!(p, RemoteProvider::Ollama));
    }

    #[test]
    fn test_detect_ollama_remote() {
        let p = RemoteProvider::detect("https://ollama.example.com");
        assert!(matches!(p, RemoteProvider::Ollama));
    }
}
