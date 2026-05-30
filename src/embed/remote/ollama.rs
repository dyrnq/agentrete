//! Ollama embeddings endpoint.

use anyhow::Result;
use reqwest::Client;

pub struct OllamaEmbedder {
    url: String,
    model: String,
    client: Client,
}

impl OllamaEmbedder {
    pub fn new(url: &str, model: &str, client: Client) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            client,
        }
    }

    async fn do_embed(&self, input: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
        let endpoint = format!("{}/api/embed", self.url);
        let body = serde_json::json!({ "model": self.model, "input": input });
        let resp: serde_json::Value = self
            .client
            .post(&endpoint)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        let arr = resp["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Ollama: missing embeddings array"))?;
        arr.iter()
            .map(|v| {
                v.as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("Ollama: invalid embedding format"))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.do_embed(&serde_json::json!(text)).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Ollama: empty response"))
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.do_embed(&serde_json::json!(texts)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_embed_real() {
        let client = reqwest::blocking::Client::new();
        if client
            .get("http://localhost:11434/api/tags")
            .send()
            .is_err()
        {
            eprintln!("Skipping: Ollama not reachable");
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let emb = OllamaEmbedder::new(
            "http://localhost:11434",
            "granite-embedding:278m",
            reqwest::Client::new(),
        );

        let vec = rt.block_on(emb.embed("Hello world 你好")).unwrap();
        assert_eq!(vec.len(), 768, "granite-embedding:278m should be 768d");
        assert!(vec.iter().any(|&v| v != 0.0), "should have non-zero values");
    }

    #[test]
    fn test_nomic_embed_real() {
        let client = reqwest::blocking::Client::new();
        if client
            .get("http://localhost:11434/api/tags")
            .send()
            .is_err()
        {
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let emb = OllamaEmbedder::new(
            "http://localhost:11434",
            "nomic-embed-text:latest",
            reqwest::Client::new(),
        );
        let vec = rt.block_on(emb.embed("test")).unwrap();
        assert_eq!(vec.len(), 768);
    }
}
