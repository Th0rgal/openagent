//! OpenCode API client with SSE streaming support.
//!
//! Provides the OpenCode HTTP API client needed to run tasks via an external
//! OpenCode server, with real-time event streaming.

use anyhow::Context;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct OpenCodeClient {
    base_url: String,
    client: reqwest::Client,
    default_agent: Option<String>,
    permissive: bool,
}

impl OpenCodeClient {
    pub fn new(
        base_url: impl Into<String>,
        default_agent: Option<String>,
        permissive: bool,
    ) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            base_url,
            client: reqwest::Client::new(),
            default_agent,
            permissive,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn create_session(
        &self,
        directory: &str,
        title: Option<&str>,
    ) -> anyhow::Result<OpenCodeSession> {
        let mut url = format!("{}/session", self.base_url);
        if !directory.is_empty() {
            url.push_str("?directory=");
            url.push_str(&urlencoding::encode(directory));
        }

        let mut body = serde_json::Map::new();
        if let Some(t) = title {
            body.insert("title".to_string(), json!(t));
        }
        if self.permissive {
            body.insert(
                "permission".to_string(),
                json!([{
                    "permission": "*",
                    "pattern": "*",
                    "action": "allow"
                }]),
            );
        }

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to call OpenCode /session")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("OpenCode /session failed: {} - {}", status, text);
        }

        let session: OpenCodeSession = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse OpenCode session response: {}", text))?;
        Ok(session)
    }

    /// Send a message and stream events in real-time.
    /// Returns a channel receiver for events and a handle to await the final response.
    pub async fn send_message_streaming(
        &self,
        session_id: &str,
        directory: &str,
        content: &str,
        model: Option<&str>,
        agent: Option<&str>,
    ) -> anyhow::Result<(
        mpsc::Receiver<OpenCodeEvent>,
        tokio::task::JoinHandle<anyhow::Result<OpenCodeMessageResponse>>,
    )> {
        let session_id = session_id.to_string();
        let directory = directory.to_string();
        let content = content.to_string();
        let model = model.map(|s| s.to_string());
        let agent = agent.map(|s| s.to_string());
        let client = self.clone();

        let (event_tx, event_rx) = mpsc::channel::<OpenCodeEvent>(256);

        // Subscribe to SSE events
        let event_url = format!("{}/event", self.base_url);
        let sse_response = self
            .client
            .get(&event_url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .context("Failed to connect to OpenCode /event SSE")?;

        if !sse_response.status().is_success() {
            anyhow::bail!("OpenCode /event failed: {}", sse_response.status());
        }

        let session_id_clone = session_id.clone();
        let mut sse_state = SseState::default();

        // Spawn SSE event consumer task
        let sse_handle = tokio::spawn(async move {
            let mut stream = sse_response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            buffer.push_str(text);

                            // Process complete SSE events (ending with double newline)
                            while let Some(pos) = buffer.find("\n\n") {
                                let event_str = buffer[..pos].to_string();
                                buffer = buffer[pos + 2..].to_string();

                                if let Some(event) =
                                    parse_sse_event(&event_str, &session_id_clone, &mut sse_state)
                                {
                                    let is_complete =
                                        matches!(event, OpenCodeEvent::MessageComplete { .. });
                                    if event_tx.send(event).await.is_err() {
                                        return; // Receiver dropped
                                    }
                                    if is_complete {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("SSE stream error: {}", e);
                        break;
                    }
                }
            }
        });

        // Spawn message sending task
        let message_handle = tokio::spawn(async move {
            // Small delay to ensure SSE subscription is ready
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let result = client
                .send_message_internal(
                    &session_id,
                    &directory,
                    &content,
                    model.as_deref(),
                    agent.as_deref(),
                )
                .await;

            // Cancel SSE task after message completes
            sse_handle.abort();
            result
        });

        Ok((event_rx, message_handle))
    }

    /// Internal method to send message (blocking, waits for response).
    async fn send_message_internal(
        &self,
        session_id: &str,
        directory: &str,
        content: &str,
        model: Option<&str>,
        agent: Option<&str>,
    ) -> anyhow::Result<OpenCodeMessageResponse> {
        let mut url = format!("{}/session/{}/message", self.base_url, session_id);
        if !directory.is_empty() {
            url.push_str("?directory=");
            url.push_str(&urlencoding::encode(directory));
        }

        let mut body = serde_json::Map::new();
        body.insert(
            "parts".to_string(),
            json!([{
                "type": "text",
                "text": content
            }]),
        );

        let agent_value = agent
            .map(|s| s.to_string())
            .or_else(|| self.default_agent.clone());
        if let Some(agent_name) = agent_value {
            body.insert("agent".to_string(), json!(agent_name));
        }

        if let Some(model_str) = model {
            if let Some((provider_id, model_id)) = split_model(model_str) {
                body.insert(
                    "model".to_string(),
                    json!({
                        "providerID": provider_id,
                        "modelID": model_id
                    }),
                );
            }
        }

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to call OpenCode /session/{id}/message")?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("OpenCode message failed: {} - {}", status, text);
        }

        let message: OpenCodeMessageResponse = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse OpenCode message response: {}", text))?;
        Ok(message)
    }

    /// Legacy non-streaming send_message for backwards compatibility.
    pub async fn send_message(
        &self,
        session_id: &str,
        directory: &str,
        content: &str,
        model: Option<&str>,
        agent: Option<&str>,
    ) -> anyhow::Result<OpenCodeMessageResponse> {
        self.send_message_internal(session_id, directory, content, model, agent)
            .await
    }

    pub async fn abort_session(&self, session_id: &str, directory: &str) -> anyhow::Result<()> {
        let mut url = format!("{}/session/{}/abort", self.base_url, session_id);
        if !directory.is_empty() {
            url.push_str("?directory=");
            url.push_str(&urlencoding::encode(directory));
        }

        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .context("Failed to call OpenCode /session/{id}/abort")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenCode abort failed: {} - {}", status, text);
        }

        Ok(())
    }
}

/// Events emitted by OpenCode during execution.
#[derive(Debug, Clone)]
pub enum OpenCodeEvent {
    /// Agent is thinking/reasoning
    Thinking { content: String },
    /// Agent is calling a tool
    ToolCall {
        tool_call_id: String,
        name: String,
        args: serde_json::Value,
    },
    /// Tool execution completed
    ToolResult {
        tool_call_id: String,
        name: String,
        result: serde_json::Value,
    },
    /// Text content being streamed
    TextDelta { content: String },
    /// Message execution completed
    MessageComplete { session_id: String },
    /// Error occurred
    Error { message: String },
}

#[derive(Debug, Default)]
struct SseState {
    message_roles: HashMap<String, String>,
    part_buffers: HashMap<String, String>,
}

fn extract_str<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(v) = value.get(*key).and_then(|v| v.as_str()) {
            return Some(v);
        }
    }
    None
}

fn extract_part_text<'a>(part: &'a serde_json::Value, part_type: &str) -> Option<&'a str> {
    if part_type == "thinking" {
        part.get("thinking")
            .and_then(|v| v.as_str())
            .or_else(|| part.get("text").and_then(|v| v.as_str()))
    } else {
        part.get("text").and_then(|v| v.as_str())
    }
}

fn looks_like_user_prompt(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("Conversation so far:\n")
        || trimmed.starts_with("User:\n")
        || trimmed.contains("\nInstructions:\n")
}

fn handle_part_update(props: &serde_json::Value, state: &mut SseState) -> Option<OpenCodeEvent> {
    let part = props.get("part")?;
    let part_type = part.get("type").and_then(|v| v.as_str())?;
    if !matches!(part_type, "text" | "reasoning" | "thinking") {
        return None;
    }

    let part_id = extract_str(part, &["id", "partID", "partId"]);
    let message_id = extract_str(part, &["messageID", "messageId", "message_id"])
        .or_else(|| extract_str(props, &["messageID", "messageId", "message_id"]));
    let role = message_id
        .and_then(|id| state.message_roles.get(id))
        .map(|s| s.as_str());
    if matches!(role, Some(r) if r != "assistant") {
        return None;
    }

    let delta = props.get("delta").and_then(|v| v.as_str());
    let full_text = extract_part_text(part, part_type);
    let buffer_key = part_id.or(message_id).unwrap_or(part_type).to_string();
    let buffer = state.part_buffers.entry(buffer_key).or_default();

    let content = if let Some(delta) = delta {
        if !delta.is_empty() || full_text.is_none() {
            buffer.push_str(delta);
            buffer.clone()
        } else if let Some(full) = full_text {
            *buffer = full.to_string();
            buffer.clone()
        } else {
            return None;
        }
    } else if let Some(full) = full_text {
        *buffer = full.to_string();
        buffer.clone()
    } else {
        return None;
    };

    if role.is_none() && part_type == "text" && looks_like_user_prompt(&content) {
        return None;
    }

    if matches!(part_type, "reasoning" | "thinking") {
        Some(OpenCodeEvent::Thinking { content })
    } else {
        Some(OpenCodeEvent::TextDelta { content })
    }
}

/// Parse an SSE event line into an OpenCodeEvent.
fn parse_sse_event(
    event_str: &str,
    session_id: &str,
    state: &mut SseState,
) -> Option<OpenCodeEvent> {
    // SSE format: "data: {...json...}"
    let data_line = event_str.lines().find(|l| l.starts_with("data: "))?;
    let json_str = data_line.strip_prefix("data: ")?;

    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let event_type = json.get("type")?.as_str()?;
    let props = json.get("properties").cloned().unwrap_or(json!({}));

    // Filter by session ID if the event has one
    let event_session_id = props
        .get("sessionID")
        .or_else(|| props.get("info").and_then(|v| v.get("sessionID")))
        .or_else(|| props.get("part").and_then(|v| v.get("sessionID")))
        .and_then(|v| v.as_str());
    if let Some(event_session_id) = event_session_id {
        if event_session_id != session_id {
            return None;
        }
    }

    match event_type {
        // Message info updates
        "message.updated" => {
            if let Some(info) = props.get("info") {
                if let (Some(id), Some(role)) = (
                    info.get("id").and_then(|v| v.as_str()),
                    info.get("role").and_then(|v| v.as_str()),
                ) {
                    state.message_roles.insert(id.to_string(), role.to_string());
                }
            }
            if props.get("part").is_some() {
                handle_part_update(&props, state)
            } else {
                None
            }
        }

        // Message part streaming events
        "message.part.updated" => handle_part_update(&props, state),

        // Tool call events
        "tool.call" | "tool.calling" | "message.tool_call" => {
            let tool_call_id = props
                .get("id")
                .or(props.get("toolCallID"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let name = props
                .get("name")
                .or(props.get("tool"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = props
                .get("args")
                .or(props.get("input"))
                .cloned()
                .unwrap_or(json!({}));

            Some(OpenCodeEvent::ToolCall {
                tool_call_id,
                name,
                args,
            })
        }

        // Tool result events
        "tool.result" | "tool.completed" | "message.tool_result" => {
            let tool_call_id = props
                .get("id")
                .or(props.get("toolCallID"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let name = props
                .get("name")
                .or(props.get("tool"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let result = props
                .get("result")
                .or(props.get("output"))
                .cloned()
                .unwrap_or(json!({}));

            Some(OpenCodeEvent::ToolResult {
                tool_call_id,
                name,
                result,
            })
        }

        // Message completion
        "message.completed" | "assistant.message.completed" => {
            Some(OpenCodeEvent::MessageComplete {
                session_id: session_id.to_string(),
            })
        }

        // Error events
        "error" | "message.error" => {
            let message = props
                .get("message")
                .or(props.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            Some(OpenCodeEvent::Error { message })
        }

        _ => None,
    }
}

#[derive(Debug, Deserialize)]
pub struct OpenCodeSession {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenCodeMessageResponse {
    pub info: OpenCodeAssistantInfo,
    pub parts: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct OpenCodeAssistantInfo {
    #[serde(default)]
    #[serde(rename = "providerID")]
    pub provider_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "modelID")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

pub fn extract_text(parts: &[serde_json::Value]) -> String {
    let mut out = Vec::new();
    for part in parts {
        if part.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                out.push(text.to_string());
            }
        }
    }
    out.join("\n")
}

fn split_model(model: &str) -> Option<(String, String)> {
    let trimmed = model.trim();
    let mut parts = trimmed.splitn(2, '/');
    let provider = parts.next()?.trim();
    let model_id = parts.next()?.trim();
    if provider.is_empty() || model_id.is_empty() {
        None
    } else {
        Some((provider.to_string(), model_id.to_string()))
    }
}
