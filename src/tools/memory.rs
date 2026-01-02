//! Memory tools - allow the agent to search and store information in long-term memory.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::Tool;
use crate::memory::MemorySystem;

/// Shared memory system reference for tools.
pub type SharedMemory = Arc<RwLock<Option<MemorySystem>>>;

/// Tool for searching the agent's memory (past tasks, missions, learnings).
pub struct SearchMemory {
    memory: SharedMemory,
}

impl SearchMemory {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[derive(Debug, Deserialize)]
struct SearchMemoryArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    search_type: Option<String>, // "tasks", "missions", "facts", or "all" (default)
}

fn default_limit() -> usize {
    5
}

#[async_trait]
impl Tool for SearchMemory {
    fn name(&self) -> &str {
        "search_memory"
    }

    fn description(&self) -> &str {
        "Search past tasks, missions, and learnings from memory. Use when you need to recall how something was done before, find relevant past work, or check if you've solved a similar problem."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to search for in memory (e.g., 'authentication implementation', 'database migration')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5)",
                    "default": 5
                },
                "search_type": {
                    "type": "string",
                    "enum": ["tasks", "missions", "facts", "all"],
                    "description": "Type of memory to search: 'tasks' (past task outcomes), 'missions' (conversation history), 'facts' (stored user/project facts), 'all' (everything). Default: 'all'"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let args: SearchMemoryArgs = serde_json::from_value(args)?;

        let memory_guard = self.memory.read().await;
        let memory = memory_guard.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Memory system not available. No historical data to search.")
        })?;

        let search_type = args.search_type.as_deref().unwrap_or("all");
        let mut results = Vec::new();

        // Search chunks (general memory)
        if search_type == "all" || search_type == "tasks" {
            match memory
                .retriever
                .search(&args.query, Some(args.limit), None, None)
                .await
            {
                Ok(chunks) => {
                    for chunk in chunks {
                        results.push(format!(
                            "📋 **Past Task Context** (similarity: {:.0}%)\n{}",
                            chunk.similarity * 100.0,
                            truncate(&chunk.chunk_text, 500)
                        ));
                    }
                }
                Err(e) => {
                    tracing::warn!("Memory search error: {}", e);
                }
            }
        }

        // Search similar task outcomes
        if search_type == "all" || search_type == "tasks" {
            match memory
                .retriever
                .find_similar_tasks(&args.query, args.limit)
                .await
            {
                Ok(outcomes) => {
                    for outcome in outcomes {
                        let status = if outcome.success {
                            "✅ Success"
                        } else {
                            "❌ Failed"
                        };
                        let model = outcome.selected_model.as_deref().unwrap_or("unknown");
                        let iterations = outcome.iterations.unwrap_or(0);

                        results.push(format!(
                            "📊 **Similar Past Task**: {}\n   Status: {} | Model: {} | Iterations: {}",
                            truncate(&outcome.task_description, 200),
                            status, model, iterations
                        ));
                    }
                }
                Err(e) => {
                    tracing::warn!("Task outcome search error: {}", e);
                }
            }
        }

        // Search user facts
        if search_type == "all" || search_type == "facts" {
            match memory
                .supabase
                .search_user_facts(&args.query, args.limit)
                .await
            {
                Ok(facts) => {
                    for fact in facts {
                        let category = fact.category.as_deref().unwrap_or("general");
                        results.push(format!(
                            "💡 **Stored Fact** [{}]: {}",
                            category, fact.fact_text
                        ));
                    }
                }
                Err(e) => {
                    tracing::debug!("User facts search not available: {}", e);
                }
            }
        }

        // Search mission summaries
        if search_type == "all" || search_type == "missions" {
            match memory
                .supabase
                .search_mission_summaries(&args.query, args.limit)
                .await
            {
                Ok(summaries) => {
                    for summary in summaries {
                        let status = if summary.success { "✅" } else { "❌" };
                        results.push(format!(
                            "🎯 **Past Mission** {}: {}\n   Files: {}",
                            status,
                            summary.summary,
                            summary.key_files.join(", ")
                        ));
                    }
                }
                Err(e) => {
                    tracing::debug!("Mission summaries not available: {}", e);
                }
            }
        }

        if results.is_empty() {
            Ok("No relevant memories found for this query.".to_string())
        } else {
            Ok(format!(
                "## Memory Search Results for: \"{}\"\n\n{}",
                args.query,
                results.join("\n\n")
            ))
        }
    }
}

/// Tool for storing a fact about the user or project in long-term memory.
pub struct StoreFact {
    memory: SharedMemory,
}

impl StoreFact {
    pub fn new(memory: SharedMemory) -> Self {
        Self { memory }
    }
}

#[derive(Debug, Deserialize)]
struct StoreFactArgs {
    fact: String,
    category: Option<String>, // "preference", "project", "convention", "person"
}

#[async_trait]
impl Tool for StoreFact {
    fn name(&self) -> &str {
        "store_fact"
    }

    fn description(&self) -> &str {
        "Store a fact about the user or project in long-term memory. Use this to remember important preferences, conventions, or project details for future reference."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "fact": {
                    "type": "string",
                    "description": "The fact to store (e.g., 'User prefers Python over JavaScript', 'This project uses PostgreSQL')"
                },
                "category": {
                    "type": "string",
                    "enum": ["preference", "project", "convention", "person"],
                    "description": "Category of the fact: 'preference' (user likes/dislikes), 'project' (project-specific info), 'convention' (coding style), 'person' (about the user)"
                }
            },
            "required": ["fact"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let args: StoreFactArgs = serde_json::from_value(args)?;

        let memory_guard = self.memory.read().await;
        let memory = memory_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Memory system not available. Cannot store facts."))?;

        // Generate embedding for the fact
        let embedding = memory.embedder.embed(&args.fact).await.ok();

        // Store the fact
        memory
            .supabase
            .insert_user_fact(
                &args.fact,
                args.category.as_deref(),
                embedding.as_deref(),
                None, // mission_id - we don't track this currently
            )
            .await?;

        let category = args.category.as_deref().unwrap_or("general");
        Ok(format!("✅ Stored fact [{}]: {}", category, args.fact))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe_end = crate::memory::safe_truncate_index(s, max);
        format!("{}...", &s[..safe_end])
    }
}
