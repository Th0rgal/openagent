//! Embedding client using OpenRouter API.

use reqwest::Client;
use serde::{Deserialize, Serialize};

const OPENROUTER_EMBEDDINGS_URL: &str = "https://openrouter.ai/api/v1/embeddings";

/// Client for generating embeddings via OpenRouter.
pub struct EmbeddingClient {
    client: Client,
    api_key: String,
    model: String,
    dimension: usize,
}

impl EmbeddingClient {
    /// Create a new embedding client.
    pub fn new(api_key: String, model: String, dimension: usize) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            dimension,
        }
    }
    
    /// Get the configured embedding dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    
    /// Get the configured model.
    pub fn model(&self) -> &str {
        &self.model
    }
    
    /// Generate embedding for a single text.
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()]).await?;
        embeddings.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }
    
    /// Generate embeddings for multiple texts.
    pub async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        
        let request = EmbeddingRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };
        
        let resp = self.client
            .post(OPENROUTER_EMBEDDINGS_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/open-agent")
            .header("X-Title", "Open Agent Memory")
            .json(&request)
            .send()
            .await?;
        
        let status = resp.status();
        let text = resp.text().await?;
        
        if !status.is_success() {
            tracing::error!("Embedding API error: {} - {}", status, text);
            anyhow::bail!("Embedding API error: {} - {}", status, text);
        }
        
        let response: EmbeddingResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Failed to parse embedding response: {} - {}", e, text))?;
        
        // Sort by index and extract embeddings
        let mut data = response.data;
        data.sort_by_key(|d| d.index);
        
        let embeddings: Vec<Vec<f32>> = data.into_iter()
            .map(|d| d.embedding)
            .collect();
        
        // Verify dimensions
        for (i, emb) in embeddings.iter().enumerate() {
            if emb.len() != self.dimension {
                tracing::warn!(
                    "Embedding {} has dimension {} but expected {}",
                    i, emb.len(), self.dimension
                );
            }
        }
        
        Ok(embeddings)
    }
    
    /// Estimate tokens for a text (rough: 4 chars per token).
    pub fn estimate_tokens(text: &str) -> usize {
        (text.len() + 3) / 4
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
    #[serde(default)]
    usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingUsage {
    prompt_tokens: u32,
    total_tokens: u32,
}


