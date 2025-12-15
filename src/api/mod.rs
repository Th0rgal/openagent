//! HTTP API for the Open Agent.
//!
//! ## Endpoints
//!
//! - `POST /api/task` - Submit a new task
//! - `GET /api/task/{id}` - Get task status and result
//! - `GET /api/task/{id}/stream` - Stream task progress via SSE
//! - `GET /api/health` - Health check

mod routes;
mod auth;
mod console;
pub mod control;
mod fs;
mod ssh_util;
pub mod types;

pub use routes::serve;
pub use types::*;

