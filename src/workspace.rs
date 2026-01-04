//! Workspace management for OpenCode sessions.
//!
//! Open Agent acts as a workspace host for OpenCode. This module creates
//! per-task/mission workspace directories and writes `opencode.json`
//! with the currently configured MCP servers.

use std::path::{Path, PathBuf};

use serde_json::json;
use uuid::Uuid;

use crate::config::Config;
use crate::mcp::{McpRegistry, McpServerConfig, McpTransport};

fn sanitize_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect::<String>()
        .to_lowercase()
        .replace('-', "_")
}

fn unique_key(base: &str, used: &mut std::collections::HashSet<String>) -> String {
    if !used.contains(base) {
        used.insert(base.to_string());
        return base.to_string();
    }
    let mut i = 2;
    loop {
        let candidate = format!("{}_{}", base, i);
        if !used.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
        i += 1;
    }
}

/// Root directory for Open Agent config data (versioned with repo).
pub fn config_root(working_dir: &Path) -> PathBuf {
    working_dir.join(".openagent")
}

/// Root directory for workspace folders.
pub fn workspaces_root(working_dir: &Path) -> PathBuf {
    working_dir.join("workspaces")
}

/// Workspace directory for a mission.
pub fn mission_workspace_dir(working_dir: &Path, mission_id: Uuid) -> PathBuf {
    let short_id = &mission_id.to_string()[..8];
    workspaces_root(working_dir).join(format!("mission-{}", short_id))
}

/// Workspace directory for a task.
pub fn task_workspace_dir(working_dir: &Path, task_id: Uuid) -> PathBuf {
    let short_id = &task_id.to_string()[..8];
    workspaces_root(working_dir).join(format!("task-{}", short_id))
}

fn opencode_entry_from_mcp(config: &McpServerConfig, workspace_dir: &Path) -> serde_json::Value {
    match &config.transport {
        McpTransport::Http { endpoint } => json!({
            "type": "http",
            "endpoint": endpoint,
            "enabled": config.enabled,
        }),
        McpTransport::Stdio { command, args, env } => {
            let mut entry = serde_json::Map::new();
            entry.insert("type".to_string(), json!("local"));
            let mut cmd = vec![command.clone()];
            cmd.extend(args.clone());
            entry.insert("command".to_string(), json!(cmd));
            entry.insert("enabled".to_string(), json!(config.enabled));
            let mut merged_env = env.clone();
            merged_env
                .entry("OPEN_AGENT_WORKSPACE".to_string())
                .or_insert_with(|| workspace_dir.to_string_lossy().to_string());
            if !merged_env.is_empty() {
                entry.insert("environment".to_string(), json!(merged_env));
            }
            serde_json::Value::Object(entry)
        }
    }
}

async fn write_opencode_config(
    workspace_dir: &Path,
    mcp_configs: Vec<McpServerConfig>,
) -> anyhow::Result<()> {
    let mut mcp_map = serde_json::Map::new();
    let mut used = std::collections::HashSet::new();

    for config in mcp_configs.into_iter().filter(|c| c.enabled) {
        let base = sanitize_key(&config.name);
        let key = unique_key(&base, &mut used);
        mcp_map.insert(key, opencode_entry_from_mcp(&config, workspace_dir));
    }

    let config_json = json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": mcp_map,
    });

    let config_path = workspace_dir.join("opencode.json");
    tokio::fs::write(config_path, serde_json::to_string_pretty(&config_json)?).await?;
    Ok(())
}

async fn prepare_workspace_dir(path: &Path) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(path.join("output")).await?;
    tokio::fs::create_dir_all(path.join("temp")).await?;
    Ok(path.to_path_buf())
}

/// Prepare a custom workspace directory and write `opencode.json`.
pub async fn prepare_custom_workspace(
    _config: &Config,
    mcp: &McpRegistry,
    workspace_dir: PathBuf,
) -> anyhow::Result<PathBuf> {
    prepare_workspace_dir(&workspace_dir).await?;
    let mcp_configs = mcp.list_configs().await;
    write_opencode_config(&workspace_dir, mcp_configs).await?;
    Ok(workspace_dir)
}

/// Prepare a workspace directory for a mission and write `opencode.json`.
pub async fn prepare_mission_workspace(
    config: &Config,
    mcp: &McpRegistry,
    mission_id: Uuid,
) -> anyhow::Result<PathBuf> {
    let dir = mission_workspace_dir(&config.working_dir, mission_id);
    prepare_workspace_dir(&dir).await?;
    let mcp_configs = mcp.list_configs().await;
    write_opencode_config(&dir, mcp_configs).await?;
    Ok(dir)
}

/// Prepare a workspace directory for a task and write `opencode.json`.
pub async fn prepare_task_workspace(
    config: &Config,
    mcp: &McpRegistry,
    task_id: Uuid,
) -> anyhow::Result<PathBuf> {
    let dir = task_workspace_dir(&config.working_dir, task_id);
    prepare_workspace_dir(&dir).await?;
    let mcp_configs = mcp.list_configs().await;
    write_opencode_config(&dir, mcp_configs).await?;
    Ok(dir)
}

/// Regenerate `opencode.json` for all workspace directories.
pub async fn sync_all_workspaces(config: &Config, mcp: &McpRegistry) -> anyhow::Result<usize> {
    let root = workspaces_root(&config.working_dir);
    if !root.exists() {
        return Ok(0);
    }

    let mut count = 0;
    let mcp_configs = mcp.list_configs().await;

    let mut entries = tokio::fs::read_dir(&root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if write_opencode_config(&path, mcp_configs.clone())
            .await
            .is_ok()
        {
            count += 1;
        }
    }

    Ok(count)
}
