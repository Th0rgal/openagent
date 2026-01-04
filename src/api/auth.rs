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
use crate::config::{AuthMode, Config, UserAccount};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Claims {
    /// Subject (we only need a stable sentinel)
    sub: String,
    /// Username (for display/auditing)
    usr: String,
    /// Issued-at unix seconds
    iat: i64,
    /// Expiration unix seconds
    exp: i64,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
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

fn issue_jwt(secret: &str, ttl_days: i64, user: &AuthUser) -> anyhow::Result<(String, i64)> {
    let now = Utc::now();
    let exp = now + Duration::days(ttl_days.max(1));
    let claims = Claims {
        sub: user.id.clone(),
        usr: user.username.clone(),
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
    let Ok(claims) = verify_jwt(token, secret) else {
        return false;
    };
    match config.auth.auth_mode(config.dev_mode) {
        AuthMode::MultiUser => user_for_claims(&claims, &config.auth.users).is_some(),
        AuthMode::SingleTenant => true,
        AuthMode::Disabled => true,
    }
}

pub async fn login(
    State(state): State<std::sync::Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, String)> {
    let auth_mode = state.config.auth.auth_mode(state.config.dev_mode);
    let user = match auth_mode {
        AuthMode::MultiUser => {
            let username = req.username.as_deref().unwrap_or("").trim();
            if username.is_empty() {
                return Err((StatusCode::UNAUTHORIZED, "Username required".to_string()));
            }
            // Find user and verify password. Use a single generic error message
            // for both invalid username and invalid password to prevent username enumeration.
            let account = state
                .config
                .auth
                .users
                .iter()
                .find(|u| u.username == username);

            let valid = match account {
                Some(acc) => {
                    !acc.password.trim().is_empty()
                        && constant_time_eq(req.password.trim(), acc.password.trim())
                }
                None => {
                    // Perform a dummy comparison to prevent timing attacks
                    let _ = constant_time_eq(req.password.trim(), "dummy_password_for_timing");
                    false
                }
            };

            if !valid {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    "Invalid username or password".to_string(),
                ));
            }

            let account = account.unwrap();

            AuthUser {
                id: account.id.clone(),
                username: account.username.clone(),
            }
        }
        AuthMode::SingleTenant | AuthMode::Disabled => {
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

            AuthUser {
                id: "default".to_string(),
                username: "default".to_string(),
            }
        }
    };

    let secret = state.config.auth.jwt_secret.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "JWT_SECRET not configured".to_string(),
        )
    })?;

    let (token, exp) = issue_jwt(secret, state.config.auth.jwt_ttl_days, &user)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(LoginResponse { token, exp }))
}

pub async fn require_auth(
    State(state): State<std::sync::Arc<AppState>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Dev mode => no auth checks.
    if state.config.dev_mode {
        req.extensions_mut().insert(AuthUser {
            id: "dev".to_string(),
            username: "dev".to_string(),
        });
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
        Ok(claims) => {
            let user = match state.config.auth.auth_mode(state.config.dev_mode) {
                AuthMode::MultiUser => match user_for_claims(&claims, &state.config.auth.users) {
                    Some(u) => u,
                    None => {
                        return (StatusCode::UNAUTHORIZED, "Invalid user").into_response();
                    }
                },
                AuthMode::SingleTenant => AuthUser {
                    id: claims.sub,
                    username: claims.usr,
                },
                AuthMode::Disabled => AuthUser {
                    id: "default".to_string(),
                    username: "default".to_string(),
                },
            };
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        Err(_) => (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response(),
    }
}

fn user_for_claims(claims: &Claims, users: &[UserAccount]) -> Option<AuthUser> {
    users.iter().find(|u| u.id == claims.sub).map(|u| AuthUser {
        id: u.id.clone(),
        username: u.username.clone(),
    })
}
