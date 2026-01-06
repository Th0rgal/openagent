//! AI Provider management API endpoints.
//!
//! Provides endpoints for managing inference providers:
//! - List providers
//! - Create provider
//! - Get provider details
//! - Update provider
//! - Delete provider
//! - Authenticate provider (OAuth flow)
//! - Set default provider

use std::path::PathBuf;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::ai_providers::{
    AIProvider, AuthMethod, OAuthCredentials, PendingOAuth, ProviderStatus, ProviderType,
};

/// Anthropic OAuth client ID (from opencode-anthropic-auth plugin)
const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Create AI provider routes.
pub fn routes() -> Router<Arc<super::routes::AppState>> {
    Router::new()
        .route("/", get(list_providers))
        .route("/", post(create_provider))
        .route("/types", get(list_provider_types))
        .route("/:id", get(get_provider))
        .route("/:id", put(update_provider))
        .route("/:id", delete(delete_provider))
        .route("/:id/auth", post(authenticate_provider))
        .route("/:id/auth/methods", get(get_auth_methods))
        .route("/:id/oauth/authorize", post(oauth_authorize))
        .route("/:id/oauth/callback", post(oauth_callback))
        .route("/:id/default", post(set_default))
}

// ─────────────────────────────────────────────────────────────────────────────
// Request/Response Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProviderTypeInfo {
    pub id: String,
    pub name: String,
    pub uses_oauth: bool,
    pub env_var: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub provider_type: ProviderType,
    pub name: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    pub name: Option<String>,
    pub api_key: Option<Option<String>>,
    pub base_url: Option<Option<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub id: Uuid,
    pub provider_type: ProviderType,
    pub provider_type_name: String,
    pub name: String,
    pub has_api_key: bool,
    pub has_oauth: bool,
    pub base_url: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub uses_oauth: bool,
    pub auth_methods: Vec<AuthMethod>,
    pub status: ProviderStatusResponse,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ProviderStatusResponse {
    Unknown,
    Connected,
    NeedsAuth { auth_url: Option<String> },
    Error { message: String },
}

impl From<ProviderStatus> for ProviderStatusResponse {
    fn from(status: ProviderStatus) -> Self {
        match status {
            ProviderStatus::Unknown => Self::Unknown,
            ProviderStatus::Connected => Self::Connected,
            ProviderStatus::NeedsAuth => Self::NeedsAuth { auth_url: None },
            ProviderStatus::Error(msg) => Self::Error { message: msg },
        }
    }
}

impl From<AIProvider> for ProviderResponse {
    fn from(p: AIProvider) -> Self {
        Self {
            id: p.id,
            provider_type: p.provider_type,
            provider_type_name: p.provider_type.display_name().to_string(),
            name: p.name.clone(),
            has_api_key: p.api_key.is_some(),
            has_oauth: p.oauth.is_some(),
            base_url: p.base_url,
            enabled: p.enabled,
            is_default: p.is_default,
            uses_oauth: p.provider_type.uses_oauth(),
            auth_methods: p.provider_type.auth_methods(),
            status: p.status.into(),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub success: bool,
    pub message: String,
    /// OAuth URL to redirect user to (if OAuth flow required)
    pub auth_url: Option<String>,
}

/// Request to initiate OAuth authorization.
#[derive(Debug, Deserialize)]
pub struct OAuthAuthorizeRequest {
    /// Index of the auth method to use (0-indexed)
    pub method_index: usize,
}

/// Response from OAuth authorization initiation.
#[derive(Debug, Serialize)]
pub struct OAuthAuthorizeResponse {
    /// URL to redirect user to for authorization
    pub url: String,
    /// Instructions to show the user
    pub instructions: String,
    /// Method for callback: "code" means user pastes code
    pub method: String,
}

/// Request to exchange OAuth code for credentials.
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackRequest {
    /// Index of the auth method used
    pub method_index: usize,
    /// Authorization code from the OAuth flow
    pub code: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// OpenCode Auth Sync
// ─────────────────────────────────────────────────────────────────────────────

/// Sync OAuth credentials to OpenCode's auth.json file.
///
/// OpenCode stores auth in `~/.local/share/opencode/auth.json` with format:
/// ```json
/// {
///   "anthropic": {
///     "type": "oauth",
///     "refresh": "sk-ant-ort01-...",
///     "access": "sk-ant-oat01-...",
///     "expires": 1767743285144
///   }
/// }
/// ```
fn sync_to_opencode_auth(
    provider_type: ProviderType,
    refresh_token: &str,
    access_token: &str,
    expires_at: i64,
) -> Result<(), String> {
    // Get OpenCode auth path
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let auth_path = PathBuf::from(&home)
        .join(".local/share/opencode/auth.json");

    // Ensure parent directory exists
    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create OpenCode auth directory: {}", e))?;
    }

    // Read existing auth or start fresh
    let mut auth: serde_json::Map<String, serde_json::Value> = if auth_path.exists() {
        let contents = std::fs::read_to_string(&auth_path)
            .map_err(|e| format!("Failed to read OpenCode auth: {}", e))?;
        serde_json::from_str(&contents).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    // Map our provider type to OpenCode's key
    let key = match provider_type {
        ProviderType::Anthropic => "anthropic",
        ProviderType::GithubCopilot => "github-copilot",
        _ => return Ok(()), // Skip providers that OpenCode doesn't use OAuth for
    };

    // Create the auth entry in OpenCode format
    let entry = serde_json::json!({
        "type": "oauth",
        "refresh": refresh_token,
        "access": access_token,
        "expires": expires_at
    });

    auth.insert(key.to_string(), entry);

    // Write back to file
    let contents = serde_json::to_string_pretty(&auth)
        .map_err(|e| format!("Failed to serialize OpenCode auth: {}", e))?;
    std::fs::write(&auth_path, contents)
        .map_err(|e| format!("Failed to write OpenCode auth: {}", e))?;

    tracing::info!(
        "Synced OAuth credentials to OpenCode auth.json for provider: {}",
        key
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /api/ai/providers/types - List available provider types.
async fn list_provider_types() -> Json<Vec<ProviderTypeInfo>> {
    let types = vec![
        ProviderTypeInfo {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            uses_oauth: true,
            env_var: Some("ANTHROPIC_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            uses_oauth: false,
            env_var: Some("OPENAI_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "google".to_string(),
            name: "Google AI".to_string(),
            uses_oauth: false,
            env_var: Some("GOOGLE_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "amazon-bedrock".to_string(),
            name: "Amazon Bedrock".to_string(),
            uses_oauth: false,
            env_var: None,
        },
        ProviderTypeInfo {
            id: "azure".to_string(),
            name: "Azure OpenAI".to_string(),
            uses_oauth: false,
            env_var: Some("AZURE_OPENAI_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "open-router".to_string(),
            name: "OpenRouter".to_string(),
            uses_oauth: false,
            env_var: Some("OPENROUTER_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "mistral".to_string(),
            name: "Mistral AI".to_string(),
            uses_oauth: false,
            env_var: Some("MISTRAL_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "groq".to_string(),
            name: "Groq".to_string(),
            uses_oauth: false,
            env_var: Some("GROQ_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "xai".to_string(),
            name: "xAI".to_string(),
            uses_oauth: false,
            env_var: Some("XAI_API_KEY".to_string()),
        },
        ProviderTypeInfo {
            id: "github-copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            uses_oauth: true,
            env_var: None,
        },
    ];
    Json(types)
}

/// GET /api/ai/providers - List all providers.
async fn list_providers(
    State(state): State<Arc<super::routes::AppState>>,
) -> Result<Json<Vec<ProviderResponse>>, (StatusCode, String)> {
    let providers = state.ai_providers.list().await;
    let responses: Vec<ProviderResponse> = providers.into_iter().map(Into::into).collect();
    Ok(Json(responses))
}

/// POST /api/ai/providers - Create a new provider.
async fn create_provider(
    State(state): State<Arc<super::routes::AppState>>,
    Json(req): Json<CreateProviderRequest>,
) -> Result<Json<ProviderResponse>, (StatusCode, String)> {
    if req.name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Name cannot be empty".to_string()));
    }

    // Validate base URL if provided
    if let Some(ref url) = req.base_url {
        if url::Url::parse(url).is_err() {
            return Err((StatusCode::BAD_REQUEST, "Invalid URL format".to_string()));
        }
    }

    let mut provider = AIProvider::new(req.provider_type, req.name);
    provider.api_key = req.api_key;
    provider.base_url = req.base_url;
    provider.enabled = req.enabled;

    // Set initial status
    if provider.provider_type.uses_oauth() && provider.api_key.is_none() {
        provider.status = ProviderStatus::NeedsAuth;
    } else if provider.api_key.is_some() {
        provider.status = ProviderStatus::Connected;
    }

    let id = state.ai_providers.add(provider.clone()).await;

    tracing::info!(
        "Created AI provider: {} ({}) [{}]",
        provider.name,
        provider.provider_type,
        id
    );

    // Refresh to get updated is_default flag
    let updated = state.ai_providers.get(id).await.unwrap_or(provider);

    Ok(Json(updated.into()))
}

/// GET /api/ai/providers/:id - Get provider details.
async fn get_provider(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<ProviderResponse>, (StatusCode, String)> {
    state
        .ai_providers
        .get(id)
        .await
        .map(|p| Json(p.into()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))
}

/// PUT /api/ai/providers/:id - Update a provider.
async fn update_provider(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, (StatusCode, String)> {
    let mut provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    if let Some(name) = req.name {
        if name.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "Name cannot be empty".to_string()));
        }
        provider.name = name;
    }

    if let Some(api_key) = req.api_key {
        provider.api_key = api_key;
        // Update status based on credentials
        if provider.api_key.is_some() {
            provider.status = ProviderStatus::Connected;
        } else if provider.provider_type.uses_oauth() {
            provider.status = ProviderStatus::NeedsAuth;
        }
    }

    if let Some(base_url) = req.base_url {
        if let Some(ref url) = base_url {
            if url::Url::parse(url).is_err() {
                return Err((StatusCode::BAD_REQUEST, "Invalid URL format".to_string()));
            }
        }
        provider.base_url = base_url;
    }

    if let Some(enabled) = req.enabled {
        provider.enabled = enabled;
    }

    let updated = state
        .ai_providers
        .update(id, provider)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    tracing::info!("Updated AI provider: {} ({})", updated.name, id);

    Ok(Json(updated.into()))
}

/// DELETE /api/ai/providers/:id - Delete a provider.
async fn delete_provider(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if state.ai_providers.delete(id).await {
        Ok((
            StatusCode::OK,
            format!("Provider {} deleted successfully", id),
        ))
    } else {
        Err((StatusCode::NOT_FOUND, format!("Provider {} not found", id)))
    }
}

/// POST /api/ai/providers/:id/auth - Initiate authentication for a provider.
async fn authenticate_provider(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<AuthResponse>, (StatusCode, String)> {
    let provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    // For OAuth providers, we need to return an auth URL
    if provider.provider_type.uses_oauth() {
        let auth_url = match provider.provider_type {
            ProviderType::Anthropic => {
                // For Anthropic/Claude, this would typically use Claude's OAuth flow
                // For now, we'll indicate that manual auth is needed
                Some("https://console.anthropic.com/settings/keys".to_string())
            }
            ProviderType::GithubCopilot => {
                // GitHub Copilot uses device code flow
                Some("https://github.com/login/device".to_string())
            }
            _ => None,
        };

        return Ok(Json(AuthResponse {
            success: false,
            message: format!(
                "Please authenticate with {} to connect this provider",
                provider.provider_type.display_name()
            ),
            auth_url,
        }));
    }

    // For API key providers, check if key is set
    if provider.api_key.is_some() {
        Ok(Json(AuthResponse {
            success: true,
            message: "Provider is authenticated".to_string(),
            auth_url: None,
        }))
    } else {
        Ok(Json(AuthResponse {
            success: false,
            message: "API key is required for this provider".to_string(),
            auth_url: None,
        }))
    }
}

/// POST /api/ai/providers/:id/default - Set as default provider.
async fn set_default(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<ProviderResponse>, (StatusCode, String)> {
    if !state.ai_providers.set_default(id).await {
        return Err((StatusCode::NOT_FOUND, format!("Provider {} not found", id)));
    }

    let provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    tracing::info!("Set default AI provider: {} ({})", provider.name, id);

    Ok(Json(provider.into()))
}

/// GET /api/ai/providers/:id/auth/methods - Get available auth methods for a provider.
async fn get_auth_methods(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<Vec<AuthMethod>>, (StatusCode, String)> {
    let provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    Ok(Json(provider.provider_type.auth_methods()))
}

/// Generate PKCE code verifier and challenge.
fn generate_pkce() -> (String, String) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let verifier: String = (0..43)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
            chars[idx] as char
        })
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

/// POST /api/ai/providers/:id/oauth/authorize - Initiate OAuth authorization.
async fn oauth_authorize(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
    Json(req): Json<OAuthAuthorizeRequest>,
) -> Result<Json<OAuthAuthorizeResponse>, (StatusCode, String)> {
    let provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    let auth_methods = provider.provider_type.auth_methods();
    let method = auth_methods
        .get(req.method_index)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Invalid method index".to_string()))?;

    match provider.provider_type {
        ProviderType::Anthropic => {
            // Generate PKCE
            let (verifier, challenge) = generate_pkce();

            // Determine mode based on method label
            let mode = if method.label.contains("Pro") || method.label.contains("Max") {
                "max"
            } else {
                "console"
            };

            // Build OAuth URL
            let base_url = if mode == "max" {
                "https://claude.ai/oauth/authorize"
            } else {
                "https://console.anthropic.com/oauth/authorize"
            };

            let mut url = url::Url::parse(base_url).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to parse URL: {}", e),
                )
            })?;

            url.query_pairs_mut()
                .append_pair("code", "true")
                .append_pair("client_id", ANTHROPIC_CLIENT_ID)
                .append_pair("response_type", "code")
                .append_pair(
                    "redirect_uri",
                    "https://console.anthropic.com/oauth/code/callback",
                )
                .append_pair("scope", "org:create_api_key user:profile user:inference")
                .append_pair("code_challenge", &challenge)
                .append_pair("code_challenge_method", "S256")
                .append_pair("state", &verifier);

            // Store pending OAuth
            state
                .ai_providers
                .set_pending_oauth(
                    id,
                    PendingOAuth {
                        verifier,
                        mode: mode.to_string(),
                        created_at: std::time::Instant::now(),
                    },
                )
                .await;

            Ok(Json(OAuthAuthorizeResponse {
                url: url.to_string(),
                instructions: "Visit the link above and paste the authorization code here"
                    .to_string(),
                method: "code".to_string(),
            }))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            "OAuth not supported for this provider".to_string(),
        )),
    }
}

/// POST /api/ai/providers/:id/oauth/callback - Exchange OAuth code for credentials.
async fn oauth_callback(
    State(state): State<Arc<super::routes::AppState>>,
    AxumPath(id): AxumPath<Uuid>,
    Json(req): Json<OAuthCallbackRequest>,
) -> Result<Json<ProviderResponse>, (StatusCode, String)> {
    let provider = state
        .ai_providers
        .get(id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Provider {} not found", id)))?;

    // Get pending OAuth state
    let pending = state
        .ai_providers
        .take_pending_oauth(id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "No pending OAuth authorization. Please start the OAuth flow again.".to_string(),
            )
        })?;

    // Check if OAuth hasn't expired (10 minutes)
    if pending.created_at.elapsed() > std::time::Duration::from_secs(600) {
        return Err((
            StatusCode::BAD_REQUEST,
            "OAuth authorization expired. Please start again.".to_string(),
        ));
    }

    match provider.provider_type {
        ProviderType::Anthropic => {
            // Exchange code for tokens
            let code = req.code.clone();
            let splits: Vec<&str> = code.split('#').collect();
            let code_part = splits.first().copied().unwrap_or(&code);
            let state_part = splits.get(1).copied();

            let client = reqwest::Client::new();
            let token_response = client
                .post("https://console.anthropic.com/v1/oauth/token")
                .json(&serde_json::json!({
                    "code": code_part,
                    "state": state_part,
                    "grant_type": "authorization_code",
                    "client_id": ANTHROPIC_CLIENT_ID,
                    "redirect_uri": "https://console.anthropic.com/oauth/code/callback",
                    "code_verifier": pending.verifier
                }))
                .send()
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        format!("Failed to exchange code: {}", e),
                    )
                })?;

            if !token_response.status().is_success() {
                let error_text = token_response.text().await.unwrap_or_default();
                return Err((
                    StatusCode::BAD_GATEWAY,
                    format!("OAuth token exchange failed: {}", error_text),
                ));
            }

            let token_data: serde_json::Value = token_response.json().await.map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to parse token response: {}", e),
                )
            })?;

            let auth_methods = provider.provider_type.auth_methods();
            let method = auth_methods.get(req.method_index);

            // Check if this is "Create an API Key" method
            let is_create_api_key = method
                .map(|m| m.label.contains("Create") && m.label.contains("API Key"))
                .unwrap_or(false);

            if is_create_api_key {
                // Create an API key using the access token
                let access_token = token_data["access_token"].as_str().ok_or_else(|| {
                    (
                        StatusCode::BAD_GATEWAY,
                        "No access token in response".to_string(),
                    )
                })?;

                let api_key_response = client
                    .post("https://api.anthropic.com/api/oauth/claude_cli/create_api_key")
                    .header("Authorization", format!("Bearer {}", access_token))
                    .header("Content-Type", "application/json")
                    .send()
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::BAD_GATEWAY,
                            format!("Failed to create API key: {}", e),
                        )
                    })?;

                if !api_key_response.status().is_success() {
                    let error_text = api_key_response.text().await.unwrap_or_default();
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        format!("API key creation failed: {}", error_text),
                    ));
                }

                let api_key_data: serde_json::Value =
                    api_key_response.json().await.map_err(|e| {
                        (
                            StatusCode::BAD_GATEWAY,
                            format!("Failed to parse API key response: {}", e),
                        )
                    })?;

                let api_key = api_key_data["raw_key"].as_str().ok_or_else(|| {
                    (
                        StatusCode::BAD_GATEWAY,
                        "No API key in response".to_string(),
                    )
                })?;

                // Store the API key
                let updated = state
                    .ai_providers
                    .set_api_key(id, api_key.to_string())
                    .await
                    .ok_or_else(|| (StatusCode::NOT_FOUND, "Provider not found".to_string()))?;

                tracing::info!(
                    "Created API key for provider: {} ({})",
                    updated.name,
                    id
                );

                Ok(Json(updated.into()))
            } else {
                // Store OAuth credentials (Claude Pro/Max mode)
                let refresh_token = token_data["refresh_token"].as_str().ok_or_else(|| {
                    (
                        StatusCode::BAD_GATEWAY,
                        "No refresh token in response".to_string(),
                    )
                })?;

                let access_token = token_data["access_token"].as_str().ok_or_else(|| {
                    (
                        StatusCode::BAD_GATEWAY,
                        "No access token in response".to_string(),
                    )
                })?;

                let expires_in = token_data["expires_in"].as_i64().unwrap_or(3600);
                let expires_at = chrono::Utc::now().timestamp_millis() + (expires_in * 1000);

                let credentials = OAuthCredentials {
                    refresh_token: refresh_token.to_string(),
                    access_token: access_token.to_string(),
                    expires_at,
                };

                let updated = state
                    .ai_providers
                    .set_oauth_credentials(id, credentials)
                    .await
                    .ok_or_else(|| (StatusCode::NOT_FOUND, "Provider not found".to_string()))?;

                tracing::info!(
                    "OAuth credentials saved for provider: {} ({})",
                    updated.name,
                    id
                );

                // Sync to OpenCode's auth.json so OpenCode can use these credentials
                if let Err(e) = sync_to_opencode_auth(
                    provider.provider_type,
                    refresh_token,
                    access_token,
                    expires_at,
                ) {
                    tracing::error!("Failed to sync credentials to OpenCode: {}", e);
                    // Don't fail the request, but log the error
                }

                Ok(Json(updated.into()))
            }
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            "OAuth not supported for this provider".to_string(),
        )),
    }
}
