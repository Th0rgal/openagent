//! WebSocket-backed SSH console (PTY) for the dashboard.

use std::sync::Arc;

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;

use super::auth;
use super::routes::AppState;
use super::ssh_util::materialize_private_key;

#[derive(Debug, Deserialize)]
#[serde(tag = "t")]
enum ClientMsg {
    #[serde(rename = "i")]
    Input { d: String },
    #[serde(rename = "r")]
    Resize { c: u16, r: u16 },
}

fn extract_jwt_from_protocols(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())?;
    // Client sends: ["openagent", "jwt.<token>"]
    for part in raw.split(',').map(|s| s.trim()) {
        if let Some(rest) = part.strip_prefix("jwt.") {
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

pub async fn console_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Enforce auth in non-dev mode by taking JWT from Sec-WebSocket-Protocol.
    if state.config.auth.auth_required(state.config.dev_mode) {
        let token = match extract_jwt_from_protocols(&headers) {
            Some(t) => t,
            None => return (StatusCode::UNAUTHORIZED, "Missing websocket JWT").into_response(),
        };
        if !auth::verify_token_for_config(&token, &state.config) {
            return (StatusCode::UNAUTHORIZED, "Invalid or expired token").into_response();
        }
    }

    // Select a stable subprotocol if client offered it.
    ws.protocols(["openagent"]).on_upgrade(move |socket| handle_console(socket, state))
}

async fn handle_console(mut socket: WebSocket, state: Arc<AppState>) {
    let cfg = state.config.console_ssh.clone();
    let key = match cfg.private_key.as_deref() {
        Some(k) if !k.trim().is_empty() => k,
        _ => {
            let _ = socket
                .send(Message::Text("Console SSH is not configured on the server.".into()))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let key_file = match materialize_private_key(key).await {
        Ok(k) => k,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to load SSH key: {}", e)))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to open PTY: {}", e)))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let mut cmd = CommandBuilder::new("ssh");
    cmd.arg("-i");
    cmd.arg(key_file.path());
    cmd.arg("-p");
    cmd.arg(cfg.port.to_string());
    cmd.arg("-o");
    cmd.arg("BatchMode=yes");
    cmd.arg("-o");
    cmd.arg("StrictHostKeyChecking=accept-new");
    cmd.arg("-o");
    cmd.arg(format!(
        "UserKnownHostsFile={}",
        std::env::temp_dir()
            .join("open_agent_known_hosts")
            .to_string_lossy()
    ));
    // Allocate PTY on the remote side too.
    cmd.arg("-tt");
    cmd.arg(format!("{}@{}", cfg.user, cfg.host));
    cmd.env("TERM", "xterm-256color");

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to spawn ssh: {}", e)))
                .await;
            let _ = socket.close().await;
            return;
        }
    };
    drop(pair.slave);

    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(_) => {
            let _ = socket.close().await;
            let _ = child.kill();
            return;
        }
    };

    let (to_pty_tx, mut to_pty_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (from_pty_tx, mut from_pty_rx) = mpsc::unbounded_channel::<String>();

    // Writer/resizer thread.
    let master_for_writer = pair.master;
    let mut writer = match master_for_writer.take_writer() {
        Ok(w) => w,
        Err(_) => {
            let _ = socket.close().await;
            let _ = child.kill();
            return;
        }
    };

    let writer_task = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        while let Some(msg) = to_pty_rx.blocking_recv() {
            match msg {
                ClientMsg::Input { d } => {
                    let _ = writer.write_all(d.as_bytes());
                    let _ = writer.flush();
                }
                ClientMsg::Resize { c, r } => {
                    let _ = master_for_writer.resize(PtySize {
                        rows: r,
                        cols: c,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
            }
        }
    });

    // Reader thread.
    let reader_task = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = from_pty_tx.send(s);
                }
                Err(_) => break,
            }
        }
    });

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Pump PTY output to WS.
    let send_task = tokio::spawn(async move {
        while let Some(chunk) = from_pty_rx.recv().await {
            if ws_sender.send(Message::Text(chunk)).await.is_err() {
                break;
            }
        }
    });

    // WS -> PTY
    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(t) => {
                if let Ok(parsed) = serde_json::from_str::<ClientMsg>(&t) {
                    let _ = to_pty_tx.send(parsed);
                }
            }
            Message::Binary(_) => {}
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup
    let _ = child.kill();
    drop(to_pty_tx);
    let _ = writer_task.await;
    let _ = reader_task.await;
    let _ = send_task.await;
}



