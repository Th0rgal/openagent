//! Remote file explorer endpoints (list/upload/download) via SSH + SFTP (OpenSSH).
//!
//! Note: uploads/downloads use `sftp` for transfer performance; directory listing uses `ssh` to run a small
//! Python snippet that returns JSON (easier/safer than parsing `sftp ls` output).

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Multipart, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use super::routes::AppState;
use super::ssh_util::{materialize_private_key, sftp_batch, ssh_exec, ssh_exec_with_stdin};

/// Check if the SSH target is localhost (optimization to skip SFTP)
fn is_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct MkdirRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct RmRequest {
    pub path: String,
    pub recursive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub kind: String, // file/dir/link/other
    pub size: u64,
    pub mtime: i64,
}

const LIST_SCRIPT: &str = r#"
import os, sys, json, stat

path = sys.argv[1]
out = []
try:
  with os.scandir(path) as it:
    for e in it:
      try:
        st = e.stat(follow_symlinks=False)
        mode = st.st_mode
        if stat.S_ISDIR(mode):
          kind = "dir"
        elif stat.S_ISREG(mode):
          kind = "file"
        elif stat.S_ISLNK(mode):
          kind = "link"
        else:
          kind = "other"
        out.append({
          "name": e.name,
          "path": os.path.join(path, e.name),
          "kind": kind,
          "size": int(st.st_size),
          "mtime": int(st.st_mtime),
        })
      except Exception:
        continue
except FileNotFoundError:
  out = []

print(json.dumps(out))
"#;

async fn get_key_and_cfg(state: &Arc<AppState>) -> Result<(crate::config::ConsoleSshConfig, super::ssh_util::TempKeyFile), (StatusCode, String)> {
    let cfg = state.config.console_ssh.clone();
    let key = cfg
        .private_key
        .as_deref()
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "Console SSH not configured".to_string()))?;
    let key_file = materialize_private_key(key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((cfg, key_file))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Vec<FsEntry>>, (StatusCode, String)> {
    let (cfg, key_file) = get_key_and_cfg(&state).await?;

    // Optimization: if SSH target is localhost, read directory directly
    if is_localhost(&cfg.host) {
        let entries = list_directory_local(&q.path)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(entries));
    }

    // Remote listing via SSH + Python
    let out = ssh_exec_with_stdin(
        &cfg,
        key_file.path(),
        "python3",
        &vec!["-".into(), q.path.clone()],
        LIST_SCRIPT,
    )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let parsed = serde_json::from_str::<Vec<FsEntry>>(&out)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("parse error: {}", e)))?;
    Ok(Json(parsed))
}

/// List directory contents locally (for localhost optimization)
async fn list_directory_local(path: &str) -> anyhow::Result<Vec<FsEntry>> {
    use std::os::unix::fs::MetadataExt;
    
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(path).await?;
    
    while let Some(entry) = dir.next_entry().await? {
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        
        let kind = if metadata.is_dir() {
            "dir"
        } else if metadata.is_symlink() {
            "link"
        } else if metadata.is_file() {
            "file"
        } else {
            "other"
        };
        
        let mtime = metadata.mtime();
        
        entries.push(FsEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path().to_string_lossy().to_string(),
            kind: kind.to_string(),
            size: metadata.len(),
            mtime,
        });
    }
    
    Ok(entries)
}

pub async fn mkdir(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MkdirRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (cfg, key_file) = get_key_and_cfg(&state).await?;
    
    // Optimization: if SSH target is localhost, create directory directly
    if is_localhost(&cfg.host) {
        tokio::fs::create_dir_all(&req.path)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({ "ok": true })));
    }
    
    ssh_exec(&cfg, key_file.path(), "mkdir", &vec!["-p".into(), req.path])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RmRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (cfg, key_file) = get_key_and_cfg(&state).await?;
    let recursive = req.recursive.unwrap_or(false);
    
    // Optimization: if SSH target is localhost, delete directly
    if is_localhost(&cfg.host) {
        if recursive {
            tokio::fs::remove_dir_all(&req.path)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        } else {
            tokio::fs::remove_file(&req.path)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        return Ok(Json(serde_json::json!({ "ok": true })));
    }
    
    let mut args = vec![];
    if recursive {
        args.push("-rf".to_string());
    } else {
        args.push("-f".to_string());
    }
    args.push(req.path);
    ssh_exec(&cfg, key_file.path(), "rm", &args)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Response, (StatusCode, String)> {
    let (cfg, key_file) = get_key_and_cfg(&state).await?;

    let filename = q.path.split('/').last().unwrap_or("download");
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename)
            .parse()
            .unwrap(),
    );
    headers.insert(header::CONTENT_TYPE, "application/octet-stream".parse().unwrap());

    // Optimization: if SSH target is localhost, read file directly
    if is_localhost(&cfg.host) {
        let file = tokio::fs::File::open(&q.path)
            .await
            .map_err(|e| (StatusCode::NOT_FOUND, format!("File not found: {}", e)))?;
        let stream = ReaderStream::new(file);
        let body = Body::from_stream(stream);
        return Ok((headers, body).into_response());
    }

    // Remote download via SFTP
    let tmp = std::env::temp_dir().join(format!("open_agent_dl_{}", uuid::Uuid::new_v4()));
    let batch = format!("get -p \"{}\" \"{}\"\n", q.path, tmp.to_string_lossy());
    sftp_batch(&cfg, key_file.path(), &batch)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let file = tokio::fs::File::open(&tmp)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    // Best-effort cleanup (delete after a short delay).
    let tmp_cleanup = tmp.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let _ = tokio::fs::remove_file(tmp_cleanup).await;
    });

    Ok((headers, body).into_response())
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (cfg, key_file) = get_key_and_cfg(&state).await?;

    // Expect one file field.
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let file_name = field.file_name().map(|s| s.to_string()).unwrap_or_else(|| "upload.bin".to_string());
        // Stream to temp file first (avoid buffering large uploads in memory).
        let tmp = std::env::temp_dir().join(format!("open_agent_ul_{}", uuid::Uuid::new_v4()));
        let mut f = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let mut field = field;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
        {
            f.write_all(&chunk)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        f.flush()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let remote_path = if q.path.ends_with('/') {
            format!("{}{}", q.path, file_name)
        } else {
            format!("{}/{}", q.path, file_name)
        };

        // Ensure the target directory exists
        let target_dir = if q.path.ends_with('/') {
            q.path.trim_end_matches('/').to_string()
        } else {
            q.path.clone()
        };

        // Optimization: if SSH target is localhost, skip SFTP and use direct file operations
        if is_localhost(&cfg.host) {
            // Direct local file operations (much faster than SFTP to self)
            tokio::fs::create_dir_all(&target_dir)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create directory: {}", e)))?;
            
            // Try rename first (fast), fall back to copy+delete if across filesystems
            if tokio::fs::rename(&tmp, &remote_path).await.is_err() {
                tokio::fs::copy(&tmp, &remote_path)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to copy file: {}", e)))?;
                let _ = tokio::fs::remove_file(&tmp).await;
            }
        } else {
            // Remote upload via SFTP
            ssh_exec(&cfg, key_file.path(), "mkdir", &["-p".into(), target_dir])
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create directory: {}", e)))?;

            let batch = format!("put -p \"{}\" \"{}\"\n", tmp.to_string_lossy(), remote_path);
            sftp_batch(&cfg, key_file.path(), &batch)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let _ = tokio::fs::remove_file(tmp).await;
        }

        return Ok(Json(serde_json::json!({ "ok": true, "path": q.path, "name": file_name })));
    }

    Err((StatusCode::BAD_REQUEST, "missing file".to_string()))
}



