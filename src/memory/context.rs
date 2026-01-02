//! Context builder for structured prompt context injection.
//!
//! This module provides a unified way to build context for agent prompts,
//! including session metadata, memory retrieval, and conversation history.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let builder = ContextBuilder::new(&config.context, &working_dir);
//! let session = builder.build_session_metadata();
//! let memory = builder.build_memory_context(&memory_system, &task_description).await;
//! ```

use crate::config::ContextConfig;

/// Structured session metadata for context injection.
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// Current time (formatted)
    pub time: String,
    /// Working directory path
    pub working_dir: String,
    /// List of files in context directory
    pub context_files: Vec<String>,
    /// Mission title (if in a mission)
    pub mission_title: Option<String>,
}

impl SessionContext {
    /// Format as a string for prompt injection.
    pub fn format(&self) -> String {
        let files_str = if self.context_files.is_empty() {
            "  (empty - no files uploaded)".to_string()
        } else {
            self.context_files
                .iter()
                .map(|f| format!("  - {}", f))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let mut out = format!(
            r#"## Session Context
- **Time**: {}
- **Working Directory**: {}
- **Files in context/**:
{}
"#,
            self.time, self.working_dir, files_str
        );

        if let Some(title) = &self.mission_title {
            out.push_str(&format!("- **Mission**: {}\n", title));
        }

        // Add brief mission mode indicator
        if self.working_dir.contains("mission-") {
            out.push_str(
                "\n⚠️ **MISSION MODE**: All files must go in your working directory above.\n",
            );
        }

        out
    }
}

/// Memory context retrieved from the memory system.
#[derive(Debug, Clone, Default)]
pub struct MemoryContext {
    /// Relevant past task chunks
    pub past_experience: Vec<String>,
    /// User facts and preferences
    pub user_facts: Vec<(String, String)>, // (category, fact)
    /// Recent mission summaries
    pub mission_summaries: Vec<(bool, String)>, // (success, summary)
}

impl MemoryContext {
    /// Check if there's any memory context.
    pub fn is_empty(&self) -> bool {
        self.past_experience.is_empty()
            && self.user_facts.is_empty()
            && self.mission_summaries.is_empty()
    }

    /// Format as a string for prompt injection.
    pub fn format(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut sections = Vec::new();

        if !self.past_experience.is_empty() {
            let hints = self
                .past_experience
                .iter()
                .map(|h| format!("• {}", h))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Relevant Past Experience\n{}", hints));
        }

        if !self.user_facts.is_empty() {
            let facts = self
                .user_facts
                .iter()
                .map(|(cat, fact)| format!("• [{}] {}", cat, fact))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### User & Project Facts\n{}", facts));
        }

        if !self.mission_summaries.is_empty() {
            let summaries = self
                .mission_summaries
                .iter()
                .map(|(success, summary)| {
                    let icon = if *success { "✅" } else { "❌" };
                    format!("• {} {}", icon, summary)
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("### Recent Missions\n{}", summaries));
        }

        format!("## Memory Context\n\n{}\n", sections.join("\n\n"))
    }
}

/// Builder for constructing prompt context.
pub struct ContextBuilder<'a> {
    config: &'a ContextConfig,
    working_dir: String,
}

impl<'a> ContextBuilder<'a> {
    /// Create a new context builder.
    pub fn new(config: &'a ContextConfig, working_dir: &str) -> Self {
        Self {
            config,
            working_dir: working_dir.to_string(),
        }
    }

    /// Build session metadata (synchronous).
    pub fn build_session_metadata(&self, mission_title: Option<&str>) -> SessionContext {
        let now = chrono::Utc::now();
        let time = now.format("%A %B %d, %Y %H:%M UTC").to_string();

        let context_dir = self.config.context_dir(&self.working_dir);
        let context_files = self.list_context_files(&context_dir);

        SessionContext {
            time,
            working_dir: self.working_dir.clone(),
            context_files,
            mission_title: mission_title.map(|s| s.to_string()),
        }
    }

    /// Build memory context (async, requires memory system).
    ///
    /// This is the default mode that retrieves all available memory context.
    /// For mission-isolated context, use `build_memory_context_with_options` with a mission_id.
    pub async fn build_memory_context(
        &self,
        memory: &crate::memory::MemorySystem,
        task_description: &str,
    ) -> MemoryContext {
        self.build_memory_context_with_options(memory, task_description, None)
            .await
    }

    /// Build memory context with options for mission isolation.
    ///
    /// If `mission_id` is provided, memory is filtered to that mission only:
    /// - Past task chunks are skipped (no cross-mission contamination)
    /// - Mission summaries are filtered to the current mission only
    /// - User facts are still included (intentionally shared)
    pub async fn build_memory_context_with_options(
        &self,
        memory: &crate::memory::MemorySystem,
        task_description: &str,
        mission_id: Option<uuid::Uuid>,
    ) -> MemoryContext {
        let mut ctx = MemoryContext::default();
        let is_mission_mode = mission_id.is_some();

        // 1. Search for relevant past task chunks (skip in mission mode to prevent contamination)
        if !is_mission_mode {
            if let Ok(chunks) = memory
                .retriever
                .search(
                    task_description,
                    Some(self.config.memory_chunk_limit),
                    Some(self.config.memory_chunk_threshold),
                    None,
                )
                .await
            {
                for chunk in chunks {
                    let text = truncate(&chunk.chunk_text, 300);
                    let cleaned = text.replace('\n', " ");
                    ctx.past_experience.push(cleaned.trim().to_string());
                }
            }
        }

        // 2. Get user facts (always included - intentionally shared across missions)
        if let Ok(facts) = memory
            .supabase
            .get_all_user_facts(self.config.user_facts_limit)
            .await
        {
            for fact in facts {
                let category = fact.category.unwrap_or_else(|| "general".to_string());
                ctx.user_facts.push((category, fact.fact_text));
            }
        }

        // 3. Get mission summaries (filtered by mission in mission mode)
        let summaries = if let Some(mid) = mission_id {
            // In mission mode: only get summaries from THIS mission
            memory
                .supabase
                .get_mission_summaries_for_mission(mid, self.config.mission_summaries_limit)
                .await
                .unwrap_or_default()
        } else {
            // Regular mode: get recent summaries from all missions
            memory
                .supabase
                .get_recent_mission_summaries(self.config.mission_summaries_limit)
                .await
                .unwrap_or_default()
        };

        for summary in summaries {
            ctx.mission_summaries
                .push((summary.success, summary.summary));
        }

        ctx
    }

    /// Build conversation history context from history pairs.
    pub fn build_history_context(&self, history: &[(String, String)]) -> String {
        if history.is_empty() {
            return String::new();
        }

        let mut context = String::from("Conversation so far:\n");

        // Take only the most recent messages
        let start_idx = history
            .len()
            .saturating_sub(self.config.max_history_messages);
        let recent_history = &history[start_idx..];

        for (role, content) in recent_history {
            let truncated = truncate_message(content, self.config.max_message_chars);
            let entry = format!("{}: {}\n", role, truncated);

            // Check if adding this would exceed total limit
            if context.len() + entry.len() > self.config.max_history_total_chars {
                context.push_str("... [earlier messages omitted due to size limits]\n");
                break;
            }

            context.push_str(&entry);
        }

        context.push('\n');
        context
    }

    /// Truncate tool result content if too large, safe for UTF-8.
    pub fn truncate_tool_result(&self, content: &str) -> String {
        if content.len() <= self.config.max_tool_result_chars {
            content.to_string()
        } else {
            let safe_end = safe_truncate_index(content, self.config.max_tool_result_chars);
            format!(
                "{}... [truncated, {} chars total. For large data, consider writing to a file and reading specific sections]",
                &content[..safe_end],
                content.len()
            )
        }
    }

    /// Get the tools directory path.
    pub fn tools_dir(&self) -> String {
        self.config.tools_dir(&self.working_dir)
    }

    /// Get the context directory path.
    pub fn context_dir(&self) -> String {
        self.config.context_dir(&self.working_dir)
    }

    /// List files in the context directory.
    fn list_context_files(&self, context_dir: &str) -> Vec<String> {
        std::fs::read_dir(context_dir)
            .ok()
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .take(self.config.max_context_files)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Find a safe UTF-8 boundary for truncation.
/// Returns the byte index that is at or before `max_bytes` and on a char boundary.
pub fn safe_truncate_index(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }
    // Find the last valid char boundary at or before max_bytes
    s.char_indices()
        .take_while(|(i, _)| *i < max_bytes)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0)
}

/// Truncate a string with ellipsis, safe for UTF-8.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe_end = safe_truncate_index(s, max);
        format!("{}...", &s[..safe_end])
    }
}

/// Truncate a message with size info, safe for UTF-8.
fn truncate_message(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        content.to_string()
    } else {
        let safe_end = safe_truncate_index(content, max_chars);
        format!(
            "{}... [truncated, {} chars total]",
            &content[..safe_end],
            content.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_context_format() {
        let ctx = SessionContext {
            time: "Monday January 01, 2024 12:00 UTC".to_string(),
            working_dir: "/root".to_string(),
            context_files: vec!["file1.txt".to_string(), "file2.py".to_string()],
            mission_title: Some("Test Mission".to_string()),
        };

        let formatted = ctx.format();
        assert!(formatted.contains("Monday January 01"));
        assert!(formatted.contains("/root"));
        assert!(formatted.contains("file1.txt"));
        assert!(formatted.contains("Test Mission"));
    }

    #[test]
    fn test_memory_context_empty() {
        let ctx = MemoryContext::default();
        assert!(ctx.is_empty());
        assert_eq!(ctx.format(), "");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    #[test]
    fn test_history_context() {
        let config = ContextConfig::default();
        let builder = ContextBuilder::new(&config, "/root");

        let history = vec![
            ("user".to_string(), "Hello".to_string()),
            ("assistant".to_string(), "Hi there!".to_string()),
        ];

        let ctx = builder.build_history_context(&history);
        assert!(ctx.contains("user: Hello"));
        assert!(ctx.contains("assistant: Hi there!"));
    }
}
