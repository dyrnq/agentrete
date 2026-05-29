//! Remote embedding backends: OpenAI-compatible and Ollama-compatible APIs.

use anyhow::Result;

#[derive(Debug, Clone)]
pub enum RemoteProvider {
    OpenAI,
    Ollama,
}

impl RemoteProvider {
    pub fn detect(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.contains("ollama") || lower.contains(":11434") {
            RemoteProvider::Ollama
        } else {
            RemoteProvider::OpenAI
        }
    }
}

pub struct RemoteEmbedder {
    url: String,
    api_key: Option<String>,
    model: String,
    provider: RemoteProvider,
    client: reqwest::Client,
}

impl RemoteEmbedder {
    pub fn new(url: &str, api_key: Option<&str>, model: &str) -> Result<Self> {
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
            model: model.to_string(),
            provider: RemoteProvider::detect(url),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.embed_one_async(text))
    }

    pub async fn embed_one_async(&self, text: &str) -> Result<Vec<f32>> {
        match self.provider {
            RemoteProvider::OpenAI => self.embed_openai(text).await,
            RemoteProvider::Ollama => self.embed_ollama(text).await,
        }
    }

    async fn embed_openai(&self, text: &str) -> Result<Vec<f32>> {
        let endpoint = format!("{}/embeddings", self.url);
        let body = serde_json::json!({ "model": self.model, "input": text });
        let mut req = self.client.post(&endpoint).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let resp: serde_json::Value = req.send().await?.json().await?;
        let arr = resp["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("OpenAI: missing data[0].embedding"))?;
        Ok(arr
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect())
    }

    async fn embed_ollama(&self, text: &str) -> Result<Vec<f32>> {
        let endpoint = format!("{}/api/embed", self.url);
        let body = serde_json::json!({ "model": self.model, "input": text });
        let resp: serde_json::Value = self
            .client
            .post(&endpoint)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        let arr = resp["embeddings"]
            .get(0)
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Ollama: missing embeddings[0] in response: {}",
                    serde_json::to_string(&resp).unwrap_or_default()
                )
            })?;
        Ok(arr
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect())
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

    #[test]
    fn test_ollama_embed_real() {
        // This test requires Ollama running on 192.168.6.9:11434
        // Skip if not available
        let client = reqwest::blocking::Client::new();
        if client
            .get("http://192.168.6.9:11434/api/tags")
            .send()
            .is_err()
        {
            eprintln!("Skipping: Ollama not reachable");
            return;
        }

        let emb = RemoteEmbedder::new("http://192.168.6.9:11434", None, "granite-embedding:278m")
            .unwrap();

        let vec = emb.embed_one("Hello world 你好").unwrap();
        assert_eq!(vec.len(), 768, "granite-embedding:278m should be 768d");
        assert!(vec.iter().any(|&v| v != 0.0), "should have non-zero values");
    }

    #[test]
    #[test]
    fn test_nomic_embed_real() {
        let client = reqwest::blocking::Client::new();
        if client
            .get("http://192.168.6.9:11434/api/tags")
            .send()
            .is_err()
        {
            return;
        }

        let emb = RemoteEmbedder::new("http://192.168.6.9:11434", None, "nomic-embed-text:latest")
            .unwrap();
        let vec = emb.embed_one("test").unwrap();
        assert_eq!(vec.len(), 768);
    }
}
