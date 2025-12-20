//! File operation tools: read, write, delete files.
//!
//! These tools have full system access - they can read/write any file on the machine.
//! Paths can be absolute (e.g., `/etc/hosts`) or relative to the working directory.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::Tool;

/// Resolve a path - if absolute, use as-is; if relative, join with working_dir.
fn resolve_path(path_str: &str, working_dir: &Path) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}

/// Read the contents of a file.
pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from anywhere on the system. Returns the file content as text. Use this to inspect files before editing them."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Can be absolute (e.g., /etc/hosts) or relative to working directory."
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional: start reading from this line number (1-indexed)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional: stop reading at this line number (inclusive)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let full_path = resolve_path(path, working_dir);

        if !full_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", path));
        }

        let content = tokio::fs::read_to_string(&full_path).await?;

        // Handle optional line range
        let start_line = args["start_line"].as_u64().map(|n| n as usize);
        let end_line = args["end_line"].as_u64().map(|n| n as usize);

        if start_line.is_some() || end_line.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let start = start_line.unwrap_or(1).saturating_sub(1).min(total_lines);
            let end = end_line.unwrap_or(total_lines).min(total_lines);

            // Ensure start <= end
            let (start, end) = if start > end {
                (end, start)
            } else {
                (start, end)
            };

            if start >= total_lines {
                return Ok(format!(
                    "File has {} lines, requested start line {} is beyond end of file",
                    total_lines,
                    start + 1
                ));
            }

            let selected: Vec<String> = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{:4}| {}", start + i + 1, line))
                .collect();

            return Ok(selected.join("\n"));
        }

        // Return with line numbers for context
        let numbered: Vec<String> = content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:4}| {}", i + 1, line))
            .collect();

        Ok(numbered.join("\n"))
    }
}

/// Write content to a file (create or overwrite).
pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file anywhere on the system. Creates the file if it doesn't exist, or overwrites if it does. Creates parent directories as needed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Can be absolute (e.g., /root/tools/script.py) or relative to working directory."
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        let full_path = resolve_path(path, working_dir);

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&full_path, content).await?;

        // Verify write by reading back
        let written = tokio::fs::read_to_string(&full_path).await?;
        if written.len() != content.len() {
            return Err(anyhow::anyhow!(
                "Write verification failed: expected {} bytes, got {}",
                content.len(),
                written.len()
            ));
        }

        // Check for potential truncation indicators
        let mut warnings = Vec::new();

        // Check if content appears truncated (common patterns)
        let content_trimmed = content.trim();

        // Markdown with unclosed code blocks
        let code_block_count = content.matches("```").count();
        if code_block_count % 2 != 0 {
            warnings.push("Content has unclosed code block (odd number of ```)");
        }

        // Sentence cut off mid-word (ends with letter, no punctuation)
        if !content_trimmed.is_empty() {
            let last_char = content_trimmed.chars().last().unwrap();
            if last_char.is_alphabetic() && !content_trimmed.ends_with("etc") {
                let last_line = content_trimmed.lines().last().unwrap_or("");
                // Check if last line looks incomplete (short and no punctuation)
                if last_line.len() < 80 && !last_line.ends_with(|c: char| c.is_ascii_punctuation())
                {
                    warnings.push("Content may be truncated (ends mid-sentence)");
                }
            }
        }

        // Markdown headings without content after them
        if content_trimmed.ends_with('#')
            || content_trimmed.ends_with("##")
            || content_trimmed.ends_with("###")
        {
            warnings.push("Content ends with empty heading");
        }

        let mut result = format!("Successfully wrote {} bytes to {}", content.len(), path);

        if !warnings.is_empty() {
            result.push_str("\n\n⚠️ **POTENTIAL TRUNCATION WARNINGS:**\n");
            for warning in &warnings {
                result.push_str(&format!("- {}\n", warning));
            }
            result.push_str("\nIf the content appears incomplete, consider:\n");
            result.push_str("1. Re-generating the content in smaller sections\n");
            result.push_str("2. Using append_file to add remaining content\n");
            result.push_str("3. Verifying with read_file that the output is complete");
        }

        Ok(result)
    }
}

/// Delete a file.
pub struct DeleteFile;

#[async_trait]
impl Tool for DeleteFile {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn description(&self) -> &str {
        "Delete a file anywhere on the system. Use with caution."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to delete. Can be absolute or relative to working directory."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let full_path = resolve_path(path, working_dir);

        if !full_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", path));
        }

        tokio::fs::remove_file(&full_path).await?;

        Ok(format!("Successfully deleted {}", path))
    }
}
