//! OpenRouter API client implementation with automatic retry for transient errors.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use super::error::{classify_http_status, LlmError, LlmErrorKind, RetryConfig};
use super::{ChatMessage, ChatOptions, ChatResponse, LlmClient, TokenUsage, ToolCall, ToolDefinition};

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// OpenRouter API client with automatic retry for transient errors.
pub struct OpenRouterClient {
    client: Client,
    api_key: String,
    retry_config: RetryConfig,
}

impl OpenRouterClient {
    /// Create a new OpenRouter client with default retry configuration.
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            retry_config: RetryConfig::default(),
        }
    }

    /// Create a new OpenRouter client with custom retry configuration.
    pub fn with_retry_config(api_key: String, retry_config: RetryConfig) -> Self {
        Self {
            client: Client::new(),
            api_key,
            retry_config,
        }
    }

    /// Parse Retry-After header if present.
    fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                // Try parsing as seconds first
                s.parse::<u64>().ok().map(Duration::from_secs)
            })
    }

    /// Create an LlmError from HTTP response status and body.
    fn create_error(
        status: reqwest::StatusCode,
        body: &str,
        retry_after: Option<Duration>,
    ) -> LlmError {
        let status_code = status.as_u16();
        let kind = classify_http_status(status_code);

        match kind {
            LlmErrorKind::RateLimited => LlmError::rate_limited(body.to_string(), retry_after),
            LlmErrorKind::ServerError => LlmError::server_error(status_code, body.to_string()),
            LlmErrorKind::ClientError => LlmError::client_error(status_code, body.to_string()),
            _ => LlmError::server_error(status_code, body.to_string()),
        }
    }

    /// Execute a single request without retry.
    async fn execute_request(
        &self,
        request: &OpenRouterRequest,
    ) -> Result<ChatResponse, LlmError> {
        let response = match self
            .client
            .post(OPENROUTER_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/open-agent")
            .header("X-Title", "Open Agent")
            .json(request)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // Network or connection error
                if e.is_timeout() {
                    return Err(LlmError::network_error(format!("Request timeout: {}", e)));
                } else if e.is_connect() {
                    return Err(LlmError::network_error(format!("Connection failed: {}", e)));
                } else {
                    return Err(LlmError::network_error(format!("Request failed: {}", e)));
                }
            }
        };

        let status = response.status();
        let retry_after = Self::parse_retry_after(response.headers());
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(Self::create_error(status, &body, retry_after));
        }

        let parsed: OpenRouterResponse = serde_json::from_str(&body).map_err(|e| {
            LlmError::parse_error(format!("Failed to parse response: {}, body: {}", e, body))
        })?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::parse_error("No choices in response".to_string()))?;

        Ok(ChatResponse {
            content: choice.message.content,
            tool_calls: choice.message.tool_calls,
            finish_reason: choice.finish_reason,
            usage: parsed
                .usage
                .map(|u| TokenUsage::new(u.prompt_tokens, u.completion_tokens)),
            model: parsed.model.or_else(|| Some(request.model.clone())),
        })
    }

    /// Execute a request with automatic retry for transient errors.
    async fn execute_with_retry(
        &self,
        request: &OpenRouterRequest,
    ) -> anyhow::Result<ChatResponse> {
        let start = Instant::now();
        let mut attempt = 0;
        let mut last_error: Option<LlmError> = None;

        loop {
            // Check if we've exceeded max retry duration
            if start.elapsed() > self.retry_config.max_retry_duration {
                let err = last_error.unwrap_or_else(|| {
                    LlmError::network_error("Max retry duration exceeded".to_string())
                });
                return Err(anyhow::anyhow!("{}", err));
            }

            match self.execute_request(request).await {
                Ok(response) => {
                    if attempt > 0 {
                        tracing::info!(
                            "Request succeeded after {} retries (total time: {:?})",
                            attempt,
                            start.elapsed()
                        );
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let should_retry =
                        self.retry_config.should_retry(&error) && attempt < self.retry_config.max_retries;

                    if should_retry {
                        let delay = error.suggested_delay(attempt);

                        // Make sure we won't exceed max retry duration
                        let remaining = self
                            .retry_config
                            .max_retry_duration
                            .saturating_sub(start.elapsed());
                        let actual_delay = delay.min(remaining);

                        if actual_delay.is_zero() {
                            tracing::warn!(
                                "Retry attempt {} failed, no time remaining: {}",
                                attempt + 1,
                                error
                            );
                            return Err(anyhow::anyhow!("{}", error));
                        }

                        tracing::warn!(
                            "Retry attempt {} failed with {}, retrying in {:?}: {}",
                            attempt + 1,
                            error.kind,
                            actual_delay,
                            error.message
                        );

                        tokio::time::sleep(actual_delay).await;
                        attempt += 1;
                        last_error = Some(error);
                    } else {
                        // Non-retryable error or max retries exceeded
                        if attempt > 0 {
                            tracing::error!(
                                "Request failed after {} retries (total time: {:?}): {}",
                                attempt,
                                start.elapsed(),
                                error
                            );
                        } else {
                            tracing::error!("Request failed (non-retryable): {}", error);
                        }
                        return Err(anyhow::anyhow!("{}", error));
                    }
                }
            }
        }
    }
}

#[async_trait]
impl LlmClient for OpenRouterClient {
    async fn chat_completion(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
    ) -> anyhow::Result<ChatResponse> {
        self.chat_completion_with_options(model, messages, tools, ChatOptions::default())
            .await
    }

    async fn chat_completion_with_options(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        options: ChatOptions,
    ) -> anyhow::Result<ChatResponse> {
        let request = OpenRouterRequest {
            model: model.to_string(),
            messages: messages.to_vec(),
            tools: tools.map(|t| t.to_vec()),
            tool_choice: tools.map(|_| "auto".to_string()),
            temperature: options.temperature,
            top_p: options.top_p,
            max_tokens: options.max_tokens,
        };

        tracing::debug!("Sending request to OpenRouter: model={}", model);

        self.execute_with_retry(&request).await
    }
}

/// OpenRouter API request format.
#[derive(Debug, Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

/// OpenRouter API response format.
#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
    #[serde(default)]
    usage: Option<OpenRouterUsage>,
    #[serde(default)]
    model: Option<String>,
}

/// A choice in the OpenRouter response.
#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
    finish_reason: Option<String>,
}

/// Message in OpenRouter response.
#[derive(Debug, Deserialize)]
struct OpenRouterMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

/// Usage data (OpenAI-compatible).
#[derive(Debug, Deserialize)]
struct OpenRouterUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(rename = "total_tokens")]
    _total_tokens: u64,
}

