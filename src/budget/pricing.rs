//! OpenRouter pricing information and caching.
//!
//! Fetches real-time pricing from OpenRouter API to enable
//! cost-aware model selection.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Pricing information for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingInfo {
    /// Model identifier (e.g., "openai/gpt-4.1-mini")
    pub model_id: String,
    
    /// Cost per 1M input tokens in dollars
    pub prompt_cost_per_million: f64,
    
    /// Cost per 1M output tokens in dollars
    pub completion_cost_per_million: f64,
    
    /// Context window size in tokens
    pub context_length: u64,
    
    /// Maximum output tokens
    pub max_output_tokens: Option<u64>,
    
    /// Whether this model supports tool/function calling
    pub supports_tools: bool,
}

impl PricingInfo {
    /// Calculate cost in cents for given token counts.
    /// 
    /// # Formula
    /// `cost = (input_tokens * prompt_rate + output_tokens * completion_rate) / 1_000_000 * 100`
    /// 
    /// # Postcondition
    /// `result >= 0`
    pub fn calculate_cost_cents(&self, input_tokens: u64, output_tokens: u64) -> u64 {
        let input_cost = (input_tokens as f64) * self.prompt_cost_per_million / 1_000_000.0;
        let output_cost = (output_tokens as f64) * self.completion_cost_per_million / 1_000_000.0;
        let total_dollars = input_cost + output_cost;
        (total_dollars * 100.0).ceil() as u64
    }

    /// Estimate cost for a task given estimated token counts.
    /// 
    /// Adds a safety margin of 20% for estimation errors.
    pub fn estimate_cost_cents(&self, estimated_input: u64, estimated_output: u64) -> u64 {
        let base_cost = self.calculate_cost_cents(estimated_input, estimated_output);
        // Add 20% safety margin
        (base_cost as f64 * 1.2).ceil() as u64
    }

    /// Get the average cost per token (for rough comparisons).
    pub fn average_cost_per_token(&self) -> f64 {
        (self.prompt_cost_per_million + self.completion_cost_per_million) / 2.0 / 1_000_000.0
    }
}

/// Model pricing cache and fetcher.
pub struct ModelPricing {
    /// Cached pricing data
    cache: Arc<RwLock<HashMap<String, PricingInfo>>>,
    
    /// HTTP client for fetching pricing
    client: reqwest::Client,
}

impl ModelPricing {
    /// Create a new pricing manager.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            client: reqwest::Client::new(),
        }
    }

    /// Create with pre-populated pricing data (for testing or offline use).
    pub fn with_pricing(pricing: HashMap<String, PricingInfo>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(pricing)),
            client: reqwest::Client::new(),
        }
    }

    /// Get pricing for a specific model.
    /// 
    /// Returns cached data if available, otherwise fetches from API.
    pub async fn get_pricing(&self, model_id: &str) -> Option<PricingInfo> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(info) = cache.get(model_id) {
                return Some(info.clone());
            }
        }

        // If not in cache, try to fetch all models
        if let Ok(()) = self.refresh_pricing().await {
            let cache = self.cache.read().await;
            return cache.get(model_id).cloned();
        }

        // Fall back to hardcoded defaults for common models
        self.default_pricing(model_id)
    }

    /// Get all cached pricing info.
    pub async fn all_pricing(&self) -> HashMap<String, PricingInfo> {
        self.cache.read().await.clone()
    }

    /// Refresh pricing from OpenRouter API.
    pub async fn refresh_pricing(&self) -> Result<(), PricingError> {
        let response = self
            .client
            .get("https://openrouter.ai/api/v1/models")
            .send()
            .await
            .map_err(|e| PricingError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PricingError::ApiError(format!(
                "Status: {}",
                response.status()
            )));
        }

        let data: OpenRouterModelsResponse = response
            .json()
            .await
            .map_err(|e| PricingError::ParseError(e.to_string()))?;

        let mut cache = self.cache.write().await;
        
        for model in data.data {
            // Check if model supports tools/function calling
            let supports_tools = model.supported_parameters
                .as_ref()
                .map(|params| params.iter().any(|p| p == "tools"))
                .unwrap_or(false);
            
            let info = PricingInfo {
                model_id: model.id.clone(),
                prompt_cost_per_million: parse_price(&model.pricing.prompt),
                completion_cost_per_million: parse_price(&model.pricing.completion),
                context_length: model.context_length.unwrap_or(4096),
                max_output_tokens: model.top_provider.as_ref()
                    .and_then(|p| p.max_completion_tokens),
                supports_tools,
            };
            cache.insert(model.id, info);
        }

        Ok(())
    }

    /// Get default pricing for common models (fallback).
    fn default_pricing(&self, model_id: &str) -> Option<PricingInfo> {
        // Hardcoded defaults for when API is unavailable
        // All these models support tool calling
        // Pricing in $ per 1M tokens
        let defaults = [
            // Claude 4.x family (newest, recommended)
            ("anthropic/claude-sonnet-4.5", 3.00, 15.00, 1_000_000),
            ("anthropic/claude-sonnet-4", 3.00, 15.00, 1_000_000),
            ("anthropic/claude-haiku-4.5", 0.80, 4.00, 200_000),
            // Claude 3.x family
            ("anthropic/claude-3.7-sonnet", 3.00, 15.00, 200_000),
            ("anthropic/claude-3.5-sonnet", 6.00, 30.00, 200_000),
            ("anthropic/claude-3.5-haiku", 0.80, 4.00, 200_000),
            ("anthropic/claude-3-haiku", 0.25, 1.25, 200_000),
            // OpenAI
            ("openai/gpt-4o", 2.50, 10.00, 128_000),
            ("openai/gpt-4o-mini", 0.15, 0.60, 128_000),
            // Google
            ("google/gemini-2.0-flash-001", 0.10, 0.40, 1_000_000),
        ];

        for (id, prompt, completion, context) in defaults {
            if model_id == id || model_id.contains(id.split('/').last().unwrap_or("")) {
                return Some(PricingInfo {
                    model_id: model_id.to_string(),
                    prompt_cost_per_million: prompt,
                    completion_cost_per_million: completion,
                    context_length: context,
                    max_output_tokens: None,
                    supports_tools: true, // All defaults support tools
                });
            }
        }

        None
    }

    /// Get models sorted by cost (cheapest first).
    pub async fn models_by_cost(&self) -> Vec<PricingInfo> {
        self.models_by_cost_filtered(false).await
    }
    
    /// Get models sorted by cost, optionally filtering to only tool-supporting models.
    /// 
    /// Automatically excludes:
    /// - Free models (ending in :free) as they often don't work reliably
    /// - Models with $0 pricing
    /// - "Lite" or small model variants
    /// - Models not in the explicit allowlist
    /// 
    /// # Model Allowlist Maintenance
    /// This list should be kept in sync with the model families defined in
    /// `models_with_benchmarks.json` (generated by `scripts/merge_benchmarks.py`).
    /// The ModelResolver auto-upgrades outdated model names to latest versions.
    pub async fn models_by_cost_filtered(&self, require_tools: bool) -> Vec<PricingInfo> {
        // Explicitly allowed model patterns (exact match or prefix with version suffix like -001)
        // These are the ONLY models that will be considered for task execution.
        // 
        // IMPORTANT: Keep in sync with MODEL_FAMILY_PATTERNS in scripts/merge_benchmarks.py
        // When new model versions are released, add them here and run the merge script.
        const CAPABLE_MODEL_BASES: &[&str] = &[
            // === Anthropic Claude ===
            // Flagship tier
            "anthropic/claude-opus-4.5",
            "anthropic/claude-opus-4",
            // Mid tier (balanced cost/performance)
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-sonnet-4",
            "anthropic/claude-3.7-sonnet",
            "anthropic/claude-3.5-sonnet",
            // Fast tier (cheap/fast)
            "anthropic/claude-haiku-4.5",
            "anthropic/claude-3.5-haiku",
            "anthropic/claude-3-haiku",
            
            // === OpenAI ===
            // Flagship tier
            "openai/o1",
            "openai/o1-preview",
            // Mid tier
            "openai/gpt-4.1",
            "openai/gpt-4o",
            "openai/gpt-4-turbo",
            "openai/o1-mini",
            "openai/o3-mini",
            // Fast tier
            "openai/gpt-4.1-mini",
            "openai/gpt-4o-mini",
            
            // === Google Gemini ===
            // Mid tier (large models ONLY - no lite/flash-lite)
            "google/gemini-2.5-pro",
            "google/gemini-1.5-pro",
            "google/gemini-pro",
            // Fast tier
            "google/gemini-2.0-flash",
            "google/gemini-1.5-flash",
            
            // === Mistral ===
            "mistralai/mistral-large",
            "mistralai/mistral-medium",
            "mistralai/mistral-small",
            
            // === DeepSeek ===
            "deepseek/deepseek-r1",
            "deepseek/deepseek-chat",
            "deepseek/deepseek-coder",
            
            // === Meta Llama ===
            "meta-llama/llama-3.3-70b",
            "meta-llama/llama-3.2-90b",
            "meta-llama/llama-3.1-405b",
            
            // === Qwen ===
            "qwen/qwen-2.5-72b",
            "qwen/qwq-32b",
            "qwen/qwen3-vl-235b-a22b-thinking",
            
            // === Test Models for Benchmarking ===
            // MoonshotAI
            "moonshotai/kimi-k2-thinking",
            "moonshotai/kimi-k2",
            // xAI Grok
            "x-ai/grok-4.1-fast",
            "x-ai/grok-4-fast",
            "x-ai/grok-4",
            "x-ai/grok-3",
            // Google Gemini 3
            "google/gemini-3-flash-preview",
            "google/gemini-3-pro-preview",
            // DeepSeek v3.2
            "deepseek/deepseek-v3.2-speciale",
            "deepseek/deepseek-v3.2",
            "deepseek/deepseek-v3.1-terminus",
            // Amazon Nova
            "amazon/nova-pro-v1",
            // Zhipu GLM
            "z-ai/glm-4.6v",
            "z-ai/glm-4.6",
            "z-ai/glm-4.5v",
            "z-ai/glm-4.5",
        ];
        
        // Patterns to exclude even if they match an allowed base
        const EXCLUDED_PATTERNS: &[&str] = &[
            "-lite",
            "-mini-", // but not ending in -mini
            "-nano",
            "-tiny",
            "-small",
            "-3b",
            "-7b",
            "-8b",
        ];
        
        let cache = self.cache.read().await;
        let mut models: Vec<_> = cache.values()
            .filter(|m| {
                let model_id_lower = m.model_id.to_lowercase();
                
                // Must support tools if required
                if require_tools && !m.supports_tools {
                    return false;
                }
                
                // Exclude free models - they're unreliable
                if m.model_id.ends_with(":free") {
                    return false;
                }
                
                // Exclude zero-cost models
                if m.prompt_cost_per_million <= 0.0 && m.completion_cost_per_million <= 0.0 {
                    return false;
                }
                
                // Exclude "lite" and other small variants
                if EXCLUDED_PATTERNS.iter().any(|p| model_id_lower.contains(p)) {
                    return false;
                }
                
                // Must match an allowed model base (exact match or with version suffix)
                CAPABLE_MODEL_BASES.iter().any(|base| {
                    // Exact match
                    if m.model_id == *base {
                        return true;
                    }
                    // Match with version suffix (e.g., base-001, base:version)
                    if m.model_id.starts_with(base) {
                        let suffix = &m.model_id[base.len()..];
                        // Allow version suffixes like -001, :latest, etc.
                        return suffix.starts_with('-') || suffix.starts_with(':');
                    }
                    false
                })
            })
            .cloned()
            .collect();
        models.sort_by(|a, b| {
            a.average_cost_per_token()
                .partial_cmp(&b.average_cost_per_token())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        models
    }
}

impl Default for ModelPricing {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse price string from OpenRouter API.
fn parse_price(price: &str) -> f64 {
    price.parse().unwrap_or(0.0)
}

/// OpenRouter API response structures.
#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    pricing: OpenRouterPricing,
    context_length: Option<u64>,
    top_provider: Option<OpenRouterProvider>,
    #[serde(default)]
    supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPricing {
    prompt: String,
    completion: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterProvider {
    max_completion_tokens: Option<u64>,
}

/// Pricing-related errors.
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("API error: {0}")]
    ApiError(String),
    
    #[error("Parse error: {0}")]
    ParseError(String),
}

