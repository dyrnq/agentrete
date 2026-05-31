//! OpenAI-compatible embeddings endpoint (OpenAI, vLLM, text-embeddings-inference, etc.)

use anyhow::Result;
use reqwest::Client;

pub struct OpenAIEmbedder {
    url: String,
    api_key: Option<String>,
    model: String,
    client: Client,
}

impl OpenAIEmbedder {
    pub fn new(url: &str, api_key: Option<&str>, model: &str, client: Client) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.map(|s| s.to_string()),
            model: model.to_string(),
            client,
        }
    }

    async fn do_embed(&self, input: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
        let endpoint = format!("{}/embeddings", self.url);
        let body = serde_json::json!({ "model": self.model, "input": input });
        let mut req = self.client.post(&endpoint).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let resp: serde_json::Value = req.send().await?.json().await?;
        let data = resp["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("OpenAI: missing data array"))?;
        data.iter()
            .map(|d| {
                d["embedding"]
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("OpenAI: invalid embedding format"))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.do_embed(&serde_json::json!(text)).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("OpenAI: empty response"))
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.do_embed(&serde_json::json!(texts)).await
    }
}
