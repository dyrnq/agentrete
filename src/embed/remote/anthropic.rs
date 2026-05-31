//! Anthropic embeddings endpoint.

use anyhow::Result;
use reqwest::Client;

pub struct AnthropicEmbedder {
    url: String,
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicEmbedder {
    pub fn new(url: &str, api_key: &str, model: &str, client: Client) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            client,
        }
    }

    async fn do_embed(&self, input: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
        let endpoint = format!("{}/embeddings", self.url);
        let body = serde_json::json!({
            "model": self.model,
            "input": input,
        });
        let resp: serde_json::Value = self
            .client
            .post(&endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        let data = resp["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Anthropic: missing embeddings array"))?;
        data.iter()
            .map(|d| {
                d.as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("Anthropic: invalid embedding format"))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.do_embed(&serde_json::json!(text)).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Anthropic: empty response"))
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.do_embed(&serde_json::json!(texts)).await
    }
}
