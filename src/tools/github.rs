//! GitHub tools using `gh` CLI and git.
//!
//! These tools provide reliable GitHub operations without relying on
//! fragile web scraping or manual API calls.
//!
//! ## Authentication
//!
//! - **Public repos**: Work without authentication
//! - **Private repos**: Set `GH_TOKEN` env var with a GitHub Personal Access Token
//! - **Git operations**: Use SSH keys (configure in ~/.ssh)

use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;

use super::Tool;

/// Clone a GitHub repository or all repositories from an organization.
pub struct GitHubClone;

#[async_trait]
impl Tool for GitHubClone {
    fn name(&self) -> &str {
        "github_clone"
    }

    fn description(&self) -> &str {
        "Clone a GitHub repository or all repositories from an organization.

Examples:
- Clone single repo: {\"owner\": \"IncrementFi\", \"repo\": \"Swap\"}
- Clone all org repos: {\"owner\": \"IncrementFi\"}
- Clone to specific dir: {\"owner\": \"IncrementFi\", \"repo\": \"Swap\", \"target_dir\": \"swap-audit\"}

Uses SSH for git operations (configure SSH key in ~/.ssh).
For private repos, ensure your SSH key has access or set GH_TOKEN env var."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "GitHub username or organization name"
                },
                "repo": {
                    "type": "string",
                    "description": "Repository name. If omitted, clones ALL repos from the owner/org."
                },
                "target_dir": {
                    "type": "string",
                    "description": "Target directory name. Defaults to repo name."
                }
            },
            "required": ["owner"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let owner = args["owner"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'owner' argument"))?;
        let repo = args["repo"].as_str();
        let target_dir = args["target_dir"].as_str();

        if let Some(repo_name) = repo {
            // Clone single repo
            clone_single_repo(owner, repo_name, target_dir, working_dir).await
        } else {
            // Clone all repos from org/user
            clone_all_repos(owner, working_dir).await
        }
    }
}

/// Clone a single repository using git SSH.
async fn clone_single_repo(
    owner: &str,
    repo: &str,
    target_dir: Option<&str>,
    working_dir: &Path,
) -> anyhow::Result<String> {
    let ssh_url = format!("git@github.com:{}/{}.git", owner, repo);
    let target = target_dir.unwrap_or(repo);
    let target_path = working_dir.join(target);

    // Check if already exists
    if target_path.exists() {
        return Ok(format!(
            "Directory '{}' already exists. Use a different target_dir or delete it first.",
            target
        ));
    }

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &ssh_url, target])
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        // Try HTTPS as fallback (for public repos without SSH)
        let https_url = format!("https://github.com/{}/{}.git", owner, repo);
        let fallback = Command::new("git")
            .args(["clone", "--depth", "1", &https_url, target])
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !fallback.status.success() {
            let fallback_err = String::from_utf8_lossy(&fallback.stderr);
            return Err(anyhow::anyhow!(
                "Failed to clone {}/{}:\nSSH: {}\nHTTPS: {}",
                owner,
                repo,
                stderr.trim(),
                fallback_err.trim()
            ));
        }
    }

    // Get some info about what was cloned
    let info = get_repo_info(&target_path).await;

    Ok(format!(
        "✓ Cloned {}/{} to '{}'\n{}",
        owner, repo, target, info
    ))
}

/// Clone all repositories from an organization/user.
async fn clone_all_repos(owner: &str, working_dir: &Path) -> anyhow::Result<String> {
    // First, list all repos
    let repos = list_repos_internal(owner, None, 100).await?;

    if repos.is_empty() {
        return Ok(format!("No repositories found for '{}'", owner));
    }

    let mut results = Vec::new();
    let mut success_count = 0;
    let mut skip_count = 0;
    let mut fail_count = 0;

    for repo in &repos {
        let target_path = working_dir.join(&repo.name);

        if target_path.exists() {
            results.push(format!("⊘ {} (already exists)", repo.name));
            skip_count += 1;
            continue;
        }

        // Try SSH first, then HTTPS
        let ssh_url = format!("git@github.com:{}/{}.git", owner, repo.name);
        let clone_result = Command::new("git")
            .args(["clone", "--depth", "1", &ssh_url, &repo.name])
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        let success = match clone_result {
            Ok(output) if output.status.success() => true,
            _ => {
                // Try HTTPS fallback
                let https_url = format!("https://github.com/{}/{}.git", owner, repo.name);
                Command::new("git")
                    .args(["clone", "--depth", "1", &https_url, &repo.name])
                    .current_dir(working_dir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        };

        if success {
            let lang = repo.language.as_deref().unwrap_or("unknown");
            results.push(format!("✓ {} ({})", repo.name, lang));
            success_count += 1;
        } else {
            results.push(format!("✗ {} (failed)", repo.name));
            fail_count += 1;
        }
    }

    Ok(format!(
        "Cloned {}/{} repositories from '{}' ({} skipped, {} failed):\n\n{}",
        success_count,
        repos.len(),
        owner,
        skip_count,
        fail_count,
        results.join("\n")
    ))
}

/// Get basic info about a cloned repo.
async fn get_repo_info(repo_path: &Path) -> String {
    // Count files by extension
    let mut info = String::new();

    if let Ok(output) = Command::new("find")
        .args([
            ".", "-type", "f", "-name", "*.cdc", "-o", "-name", "*.sol", "-o", "-name", "*.rs",
            "-o", "-name", "*.ts", "-o", "-name", "*.go",
        ])
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        let files = String::from_utf8_lossy(&output.stdout);
        let file_list: Vec<&str> = files.lines().collect();

        if !file_list.is_empty() {
            // Count by extension
            let mut cdc = 0;
            let mut sol = 0;
            let mut rs = 0;
            let mut ts = 0;
            let mut go = 0;

            for f in &file_list {
                if f.ends_with(".cdc") {
                    cdc += 1;
                } else if f.ends_with(".sol") {
                    sol += 1;
                } else if f.ends_with(".rs") {
                    rs += 1;
                } else if f.ends_with(".ts") {
                    ts += 1;
                } else if f.ends_with(".go") {
                    go += 1;
                }
            }

            let mut counts = Vec::new();
            if cdc > 0 {
                counts.push(format!("{} Cadence (.cdc)", cdc));
            }
            if sol > 0 {
                counts.push(format!("{} Solidity (.sol)", sol));
            }
            if rs > 0 {
                counts.push(format!("{} Rust (.rs)", rs));
            }
            if ts > 0 {
                counts.push(format!("{} TypeScript (.ts)", ts));
            }
            if go > 0 {
                counts.push(format!("{} Go (.go)", go));
            }

            if !counts.is_empty() {
                info = format!("Files: {}", counts.join(", "));
            }
        }
    }

    info
}

/// List repositories for a GitHub user or organization.
pub struct GitHubListRepos;

#[derive(Debug, Deserialize)]
struct RepoInfo {
    name: String,
    description: Option<String>,
    language: Option<String>,
    #[serde(rename = "stargazerCount")]
    stars: Option<u32>,
    url: Option<String>,
    #[serde(rename = "isArchived")]
    is_archived: Option<bool>,
}

#[async_trait]
impl Tool for GitHubListRepos {
    fn name(&self) -> &str {
        "github_list_repos"
    }

    fn description(&self) -> &str {
        "List repositories for a GitHub user or organization with metadata.

Returns: name, description, language, stars, and URL for each repo.

For private repos, set GH_TOKEN env var with a Personal Access Token."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "GitHub username or organization name"
                },
                "language": {
                    "type": "string",
                    "description": "Filter by programming language (e.g., 'Cadence', 'Solidity', 'Rust')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of repos to return (default: 30, max: 100)"
                }
            },
            "required": ["owner"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let owner = args["owner"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'owner' argument"))?;
        let language = args["language"].as_str();
        let limit = args["limit"].as_u64().unwrap_or(30).min(100) as u32;

        let repos = list_repos_internal(owner, language, limit).await?;

        if repos.is_empty() {
            return Ok(format!("No repositories found for '{}'", owner));
        }

        // Format output
        let mut output = format!("## Repositories for {}\n\n", owner);

        for repo in &repos {
            let stars = repo.stars.unwrap_or(0);
            let lang = repo.language.as_deref().unwrap_or("—");
            let desc = repo
                .description
                .as_deref()
                .unwrap_or("No description")
                .chars()
                .take(60)
                .collect::<String>();
            let archived = if repo.is_archived.unwrap_or(false) {
                " [archived]"
            } else {
                ""
            };

            output.push_str(&format!(
                "- **{}**{} ({}, ⭐{})\n  {}\n",
                repo.name, archived, lang, stars, desc
            ));
        }

        output.push_str(&format!("\nTotal: {} repositories", repos.len()));

        Ok(output)
    }
}

/// Internal function to list repos using gh CLI.
async fn list_repos_internal(
    owner: &str,
    language: Option<&str>,
    limit: u32,
) -> anyhow::Result<Vec<RepoInfo>> {
    let output = Command::new("gh")
        .args([
            "repo",
            "list",
            owner,
            "--json",
            "name,description,language,stargazerCount,url,isArchived",
            "--limit",
            &limit.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("gh repo list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut repos: Vec<RepoInfo> = serde_json::from_str(&stdout)?;

    // Filter by language if specified
    if let Some(lang) = language {
        let lang_lower = lang.to_lowercase();
        repos.retain(|r| {
            r.language
                .as_ref()
                .map(|l| l.to_lowercase().contains(&lang_lower))
                .unwrap_or(false)
        });
    }

    Ok(repos)
}

/// Get a file from a GitHub repository without cloning.
pub struct GitHubGetFile;

#[async_trait]
impl Tool for GitHubGetFile {
    fn name(&self) -> &str {
        "github_get_file"
    }

    fn description(&self) -> &str {
        "Get a file from a GitHub repository without cloning the entire repo.

Useful for quickly inspecting specific files like README, configs, or contracts.

For private repos, set GH_TOKEN env var."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "GitHub username or organization name"
                },
                "repo": {
                    "type": "string",
                    "description": "Repository name"
                },
                "path": {
                    "type": "string",
                    "description": "Path to the file (e.g., 'contracts/Token.cdc' or 'README.md')"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch, tag, or commit SHA (default: default branch)"
                }
            },
            "required": ["owner", "repo", "path"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let owner = args["owner"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'owner' argument"))?;
        let repo = args["repo"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'repo' argument"))?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
        let git_ref = args["ref"].as_str();

        // Build API endpoint
        let endpoint = if let Some(r) = git_ref {
            format!("repos/{}/{}/contents/{}?ref={}", owner, repo, path, r)
        } else {
            format!("repos/{}/{}/contents/{}", owner, repo, path)
        };

        let output = Command::new("gh")
            .args(["api", &endpoint, "--jq", ".content"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to get {}/{}/{}: {}",
                owner,
                repo,
                path,
                stderr.trim()
            ));
        }

        // Content is base64 encoded
        let base64_content = String::from_utf8_lossy(&output.stdout);
        let base64_clean = base64_content.trim().replace('\n', "");

        use base64::{engine::general_purpose::STANDARD, Engine};
        let decoded = STANDARD
            .decode(&base64_clean)
            .map_err(|e| anyhow::anyhow!("Failed to decode base64: {}", e))?;

        let content = String::from_utf8_lossy(&decoded);

        // Truncate if too long
        const MAX_LENGTH: usize = 15000;
        if content.len() > MAX_LENGTH {
            let truncated = &content[..MAX_LENGTH];
            Ok(format!(
                "## {}/{}/{}\n\n```\n{}\n```\n\n... [truncated, showing first {} chars of {}]",
                owner,
                repo,
                path,
                truncated,
                MAX_LENGTH,
                content.len()
            ))
        } else {
            Ok(format!(
                "## {}/{}/{}\n\n```\n{}\n```",
                owner, repo, path, content
            ))
        }
    }
}

/// Search code on GitHub.
pub struct GitHubSearchCode;

#[async_trait]
impl Tool for GitHubSearchCode {
    fn name(&self) -> &str {
        "github_search_code"
    }

    fn description(&self) -> &str {
        "Search for code across GitHub repositories.

Examples:
- Search in org: {\"query\": \"reentrancy\", \"owner\": \"IncrementFi\"}
- Search in repo: {\"query\": \"transfer\", \"owner\": \"IncrementFi\", \"repo\": \"Swap\"}
- Search by language: {\"query\": \"oracle price\", \"language\": \"Cadence\"}

Note: GitHub code search requires authentication. Set GH_TOKEN env var."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "owner": {
                    "type": "string",
                    "description": "Filter to repos owned by this user/org"
                },
                "repo": {
                    "type": "string",
                    "description": "Filter to a specific repository (requires owner)"
                },
                "language": {
                    "type": "string",
                    "description": "Filter by programming language"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default: 20, max: 50)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
        let owner = args["owner"].as_str();
        let repo = args["repo"].as_str();
        let language = args["language"].as_str();
        let limit = args["limit"].as_u64().unwrap_or(20).min(50);

        // Build search query - pre-allocate strings to avoid lifetime issues
        let limit_str = limit.to_string();
        let mut search_args: Vec<&str> = vec!["search", "code", query, "--limit", &limit_str];

        let owner_arg;
        if let Some(o) = owner {
            owner_arg = format!("--owner={}", o);
            search_args.push(&owner_arg);
        }

        let repo_arg;
        if let (Some(o), Some(r)) = (owner, repo) {
            repo_arg = format!("--repo={}/{}", o, r);
            search_args.push(&repo_arg);
        }

        let lang_arg;
        if let Some(l) = language {
            lang_arg = format!("--language={}", l);
            search_args.push(&lang_arg);
        }

        let output = Command::new("gh")
            .args(&search_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("authentication") || stderr.contains("401") {
                return Err(anyhow::anyhow!(
                    "GitHub code search requires authentication. Set GH_TOKEN env var with a Personal Access Token."
                ));
            }
            return Err(anyhow::anyhow!("Search failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.trim().is_empty() {
            return Ok(format!("No results found for: {}", query));
        }

        Ok(format!("## Search results for '{}'\n\n{}", query, stdout))
    }
}
