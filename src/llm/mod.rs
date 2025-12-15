//! LLM client module for interacting with language models.
//!
//! This module provides a trait-based abstraction over LLM providers,
//! with OpenRouter as the primary implementation.

mod error;
mod openrouter;

pub use error::{LlmError, LlmErrorKind, RetryConfig, classify_http_status};
pub use openrouter::OpenRouterClient;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Role in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Function call details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition with schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Response from a chat completion.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    pub model: Option<String>,
}

/// Token usage information (if provided by the upstream provider).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    /// Create a usage object ensuring `total_tokens` is consistent.
    pub fn new(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
        }
    }
}

/// Optional parameters for chat completions.
///
/// These are intentionally conservative; the goal is reproducibility.
#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    /// Sampling temperature (0 = deterministic).
    pub temperature: Option<f64>,
    /// Top-p nucleus sampling.
    pub top_p: Option<f64>,
    /// Maximum output tokens to generate.
    pub max_tokens: Option<u64>,
}

/// Trait for LLM clients.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat completion request.
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> anyhow::Result<ChatResponse>;

    /// Send a chat completion request with optional parameters.
    ///
    /// Default implementation ignores options and calls `chat_completion`.
    async fn chat_completion_with_options(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        _options: ChatOptions,
    ) -> anyhow::Result<ChatResponse> {
        self.chat_completion(model, messages, tools).await
    }
}

