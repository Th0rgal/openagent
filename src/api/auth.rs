//! Minimal JWT auth for the dashboard (single-tenant).
//!
//! - Dashboard submits a password to `/api/auth/login`
//! - Server returns a JWT valid for ~30 days
//! - When `DEV_MODE=false`, all API endpoints require `Authorization: Bearer <jwt>`
//!
//! # Security notes
//! - This is intentionally minimal; it is NOT multi-tenant and does not implement RLS.
//! - Use a strong `JWT_SECRET` in production.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};

use super::routes::AppState;
use super::types::{LoginRequest, LoginResponse};
use crate::config::Config;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Claims {
    /// Subject (we only need a stable sentinel)
    sub: String,
    /// Issued-at unix seconds
    iat: i64,
    /// Expiration unix seconds
    exp: i64,
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a_bytes.len() {
        diff |= a_bytes[i] ^ b_bytes[i];
    }
    diff == 0
}

fn issue_jwt(secret: &str, ttl_days: i64) -> anyhow::Result<(String, i64)> {
    let now = Utc::now();
    let exp = now + Duration::days(ttl_days.max(1));
    let claims = Claims {
        sub: "open_agent_dashboard".to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok((token, claims.exp))
}

fn verify_jwt(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let validation = Validation::default();
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(token_data.claims)
}

/// Verify a JWT against the server config.
/// Returns true iff:
/// - auth is not required (dev mode), OR
/// - auth is required and the token is valid.
pub fn verify_token_for_config(token: &str, config: &Config) -> bool {
    if !config.auth.auth_required(config.dev_mode) {
        return true;
    }
    let secret = match config.auth.jwt_secret.as_deref() {
        Some(s) => s,
        None => return false,
    };
    verify_jwt(token, secret).is_ok()
}

pub async fn login(
    State(state): State<std::sync::Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, String)> {
    // If dev_mode is enabled, we still allow login, but it won't be required.
    let expected = state
        .config
        .auth
        .dashboard_password
        .as_deref()
        .unwrap_or("");

    if expected.is_empty() || !constant_time_eq(req.password.trim(), expected) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid password".to_string()));
    }

    let secret = state.config.auth.jwt_secret.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "JWT_SECRET not configured".to_string(),
        )
    })?;

    let (token, exp) = issue_jwt(secret, state.config.auth.jwt_ttl_days)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(LoginResponse { token, exp }))
}

pub async fn require_auth(
    State(state): State<std::sync::Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Dev mode => no auth checks.
    if state.config.dev_mode {
        return next.run(req).await;
    }

    // If auth isn't configured, fail closed in non-dev mode.
    let secret = match state.config.auth.jwt_secret.as_deref() {
        Some(s) => s,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "JWT_SECRET not configured",
            )
                .into_response();
        }
    };

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .unwrap_or("");

    if token.is_empty() {
        return (StatusCode::UNAUTHORIZED, "Missing Authorization header").into_response();
    }

    match verify_jwt(token, secret) {
        Ok(_claims) => next.run(req).await,
        Err(_) => (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response(),
    }
}
