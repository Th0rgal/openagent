//! Code search tools: grep/regex search.
//!
//! ## Workspace-First Design
//!
//! Searches workspace by default:
//! - `grep_search("TODO")` → searches in `{workspace}/`
//! - `grep_search("error", "/var/log")` → searches system logs

use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::process::Command;

use super::{resolve_path, Tool};

/// Search file contents with regex/grep.
pub struct GrepSearch;

#[async_trait]
impl Tool for GrepSearch {
    fn name(&self) -> &str {
        "grep_search"
    }

    fn description(&self) -> &str {
        "Search for a pattern in file contents using regex. Searches workspace by default. Great for finding function definitions, usages, or patterns."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search. Defaults to workspace ('.'). Use relative paths for subdirectories or absolute for system search."
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Optional: only search files matching this glob (e.g., '*.rs', '*.py', '*.log')"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Whether search is case-sensitive (default: false)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' argument"))?;
        let path = args["path"].as_str().unwrap_or(".");
        let file_pattern = args["file_pattern"].as_str();
        let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);

        let resolution = resolve_path(path, working_dir);
        let search_path = resolution.resolved;

        // Try to use ripgrep (rg) if available, fall back to grep
        let mut cmd = if which_exists("rg") {
            let mut c = Command::new("rg");
            c.arg("--line-number");
            c.arg("--no-heading");
            c.arg("--color=never");

            if !case_sensitive {
                c.arg("-i");
            }

            if let Some(fp) = file_pattern {
                c.arg("-g").arg(fp);
            }

            c.arg("--").arg(pattern).arg(&search_path);
            c
        } else {
            let mut c = Command::new("grep");
            c.arg("-rn");

            if !case_sensitive {
                c.arg("-i");
            }

            if let Some(fp) = file_pattern {
                c.arg("--include").arg(fp);
            }

            c.arg(pattern).arg(&search_path);
            c
        };

        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to execute search: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // grep returns exit code 1 when no matches found
        if !output.status.success() && output.status.code() != Some(1) {
            if !stderr.is_empty() {
                return Err(anyhow::anyhow!("Search error: {}", stderr));
            }
        }

        if stdout.is_empty() {
            return Ok(format!("No matches found for pattern: {}", pattern));
        }

        // Show results with full paths for system-wide clarity
        let result: String = stdout
            .lines()
            .take(100) // Limit results
            .collect::<Vec<_>>()
            .join("\n");

        let line_count = result.lines().count();
        if line_count >= 100 {
            Ok(format!("{}\n\n... (showing first 100 matches)", result))
        } else {
            Ok(result)
        }
    }
}

/// Check if a command exists in PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
