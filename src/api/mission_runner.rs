//! Mission Runner - Isolated execution context for a single mission.
//!
//! This module provides a clean abstraction for running missions in parallel.
//! Each MissionRunner manages its own:
//! - Conversation history
//! - Message queue  
//! - Execution state
//! - Cancellation token
//! - Deliverable tracking
//! - Health monitoring
//! - Working directory (isolated per mission)

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agents::{AgentRef, AgentResult, TerminalReason};
use crate::backend::claudecode::client::{ClaudeEvent, ContentBlock, StreamEvent};
use crate::config::Config;
use crate::mcp::McpRegistry;
use crate::secrets::SecretsStore;
use crate::task::{extract_deliverables, DeliverableSet};
use crate::workspace::{self, Workspace, WorkspaceType};
use crate::workspace_exec::WorkspaceExec;

use super::control::{
    AgentEvent, AgentTreeNode, ControlStatus, ExecutionProgress, FrontendToolHub,
};
use super::library::SharedLibrary;

/// State of a running mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionRunState {
    /// Waiting in queue
    Queued,
    /// Currently executing
    Running,
    /// Waiting for frontend tool input
    WaitingForTool,
    /// Finished (check result)
    Finished,
}

/// Health status of a mission.
#[derive(Debug, Clone, serde::Serialize)]
pub enum MissionHealth {
    /// Mission is progressing normally
    Healthy,
    /// Mission may be stalled
    Stalled {
        seconds_since_activity: u64,
        last_state: String,
    },
    /// Mission completed without deliverables
    MissingDeliverables { missing: Vec<String> },
    /// Mission ended unexpectedly
    UnexpectedEnd { reason: String },
}

/// A message queued for this mission.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub content: String,
    /// Optional agent override for this specific message (e.g., from @agent mention)
    pub agent: Option<String>,
}

/// Isolated runner for a single mission.
pub struct MissionRunner {
    /// Mission ID
    pub mission_id: Uuid,

    /// Workspace ID where this mission should run
    pub workspace_id: Uuid,

    /// Backend ID used for this mission
    pub backend_id: String,

    /// Current state
    pub state: MissionRunState,

    /// Agent override for this mission
    pub agent_override: Option<String>,

    /// Message queue for this mission
    pub queue: VecDeque<QueuedMessage>,

    /// Conversation history: (role, content)
    pub history: Vec<(String, String)>,

    /// Cancellation token for the current execution
    pub cancel_token: Option<CancellationToken>,

    /// Running task handle
    running_handle: Option<tokio::task::JoinHandle<(Uuid, String, AgentResult)>>,

    /// Tree snapshot for this mission
    pub tree_snapshot: Arc<RwLock<Option<AgentTreeNode>>>,

    /// Progress snapshot for this mission
    pub progress_snapshot: Arc<RwLock<ExecutionProgress>>,

    /// Expected deliverables extracted from the initial message
    pub deliverables: DeliverableSet,

    /// Last activity timestamp for health monitoring
    pub last_activity: Instant,

    /// Whether complete_mission was explicitly called
    pub explicitly_completed: bool,
}

impl MissionRunner {
    /// Create a new mission runner.
    pub fn new(
        mission_id: Uuid,
        workspace_id: Uuid,
        agent_override: Option<String>,
        backend_id: Option<String>,
    ) -> Self {
        Self {
            mission_id,
            workspace_id,
            backend_id: backend_id.unwrap_or_else(|| "opencode".to_string()),
            state: MissionRunState::Queued,
            agent_override,
            queue: VecDeque::new(),
            history: Vec::new(),
            cancel_token: None,
            running_handle: None,
            tree_snapshot: Arc::new(RwLock::new(None)),
            progress_snapshot: Arc::new(RwLock::new(ExecutionProgress::default())),
            deliverables: DeliverableSet::default(),
            last_activity: Instant::now(),
            explicitly_completed: false,
        }
    }

    /// Check if this runner is currently executing.
    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            MissionRunState::Running | MissionRunState::WaitingForTool
        )
    }

    /// Check if this runner has finished.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, MissionRunState::Finished)
    }

    /// Update the last activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check the health of this mission.
    pub async fn check_health(&self) -> MissionHealth {
        let seconds_since = self.last_activity.elapsed().as_secs();

        // If running and no activity for 60+ seconds, consider stalled
        if self.is_running() && seconds_since > 60 {
            return MissionHealth::Stalled {
                seconds_since_activity: seconds_since,
                last_state: format!("{:?}", self.state),
            };
        }

        // If finished without explicit completion and has deliverables, check them
        if !self.is_running()
            && !self.explicitly_completed
            && !self.deliverables.deliverables.is_empty()
        {
            let missing = self.deliverables.missing_paths().await;
            if !missing.is_empty() {
                return MissionHealth::MissingDeliverables { missing };
            }
        }

        MissionHealth::Healthy
    }

    /// Extract deliverables from initial mission message.
    pub fn set_initial_message(&mut self, message: &str) {
        self.deliverables = extract_deliverables(message);
        if !self.deliverables.deliverables.is_empty() {
            tracing::info!(
                "Mission {} has {} expected deliverables: {:?}",
                self.mission_id,
                self.deliverables.deliverables.len(),
                self.deliverables
                    .deliverables
                    .iter()
                    .filter_map(|d| d.path())
                    .collect::<Vec<_>>()
            );
        }
    }

    /// Queue a message for this mission.
    pub fn queue_message(&mut self, id: Uuid, content: String, agent: Option<String>) {
        self.queue.push_back(QueuedMessage { id, content, agent });
    }

    /// Cancel the current execution.
    pub fn cancel(&mut self) {
        if let Some(token) = &self.cancel_token {
            token.cancel();
        }
    }

    /// Start executing the next queued message (if any and not already running).
    /// Returns true if execution was started.
    pub fn start_next(
        &mut self,
        config: Config,
        root_agent: AgentRef,
        mcp: Arc<McpRegistry>,
        workspaces: workspace::SharedWorkspaceStore,
        library: SharedLibrary,
        events_tx: broadcast::Sender<AgentEvent>,
        tool_hub: Arc<FrontendToolHub>,
        status: Arc<RwLock<ControlStatus>>,
        mission_cmd_tx: mpsc::Sender<crate::tools::mission::MissionControlCommand>,
        current_mission: Arc<RwLock<Option<Uuid>>>,
        secrets: Option<Arc<SecretsStore>>,
    ) -> bool {
        // Don't start if already running
        if self.is_running() {
            return false;
        }

        // Get next message from queue
        let msg = match self.queue.pop_front() {
            Some(m) => m,
            None => return false,
        };

        self.state = MissionRunState::Running;

        let cancel = CancellationToken::new();
        self.cancel_token = Some(cancel.clone());

        let hist_snapshot = self.history.clone();
        let tree_ref = Arc::clone(&self.tree_snapshot);
        let progress_ref = Arc::clone(&self.progress_snapshot);
        let mission_id = self.mission_id;
        let workspace_id = self.workspace_id;
        let agent_override = self.agent_override.clone();
        let backend_id = self.backend_id.clone();
        let user_message = msg.content.clone();
        let msg_id = msg.id;
        tracing::info!(
            mission_id = %mission_id,
            workspace_id = %workspace_id,
            agent_override = ?agent_override,
            message_id = %msg_id,
            message_len = user_message.len(),
            "Mission runner starting"
        );

        // Create mission control for complete_mission tool
        let mission_ctrl = crate::tools::mission::MissionControl {
            current_mission_id: current_mission,
            cmd_tx: mission_cmd_tx,
        };

        // Emit user message event with mission context
        let _ = events_tx.send(AgentEvent::UserMessage {
            id: msg_id,
            content: user_message.clone(),
            queued: false,
            mission_id: Some(mission_id),
        });

        let handle = tokio::spawn(async move {
            let result = run_mission_turn(
                config,
                root_agent,
                mcp,
                workspaces,
                library,
                events_tx,
                tool_hub,
                status,
                cancel,
                hist_snapshot,
                user_message.clone(),
                Some(mission_ctrl),
                tree_ref,
                progress_ref,
                mission_id,
                Some(workspace_id),
                backend_id,
                agent_override,
                secrets,
            )
            .await;
            (msg_id, user_message, result)
        });

        self.running_handle = Some(handle);
        true
    }

    /// Poll for completion. Returns Some(result) if finished.
    pub async fn poll_completion(&mut self) -> Option<(Uuid, String, AgentResult)> {
        let handle = self.running_handle.take()?;

        // Check if handle is finished
        if handle.is_finished() {
            match handle.await {
                Ok(result) => {
                    self.touch(); // Update last activity
                    self.state = MissionRunState::Queued; // Ready for next message

                    // Check if complete_mission was called
                    if result.2.output.contains("Mission marked as")
                        || result.2.output.contains("complete_mission")
                    {
                        self.explicitly_completed = true;
                    }

                    // Add to history
                    self.history.push(("user".to_string(), result.1.clone()));
                    self.history
                        .push(("assistant".to_string(), result.2.output.clone()));

                    // Log warning if deliverables are missing and task ended
                    if !self.explicitly_completed && !self.deliverables.deliverables.is_empty() {
                        let missing = self.deliverables.missing_paths().await;
                        if !missing.is_empty() {
                            tracing::warn!(
                                "Mission {} ended but deliverables are missing: {:?}",
                                self.mission_id,
                                missing
                            );
                        }
                    }

                    Some(result)
                }
                Err(e) => {
                    tracing::error!("Mission runner task failed: {}", e);
                    self.state = MissionRunState::Finished;
                    None
                }
            }
        } else {
            // Not finished, put handle back
            self.running_handle = Some(handle);
            None
        }
    }

    /// Check if the running task is finished (non-blocking).
    pub fn check_finished(&self) -> bool {
        self.running_handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(true)
    }
}

/// Build a history context string from conversation history.
fn build_history_context(history: &[(String, String)], max_chars: usize) -> String {
    let mut result = String::new();
    let mut total_chars = 0;
    for (role, content) in history.iter().rev() {
        let entry = format!("{}: {}\n\n", role.to_uppercase(), content);
        if total_chars + entry.len() > max_chars && !result.is_empty() {
            break;
        }
        result = format!("{}{}", entry, result);
        total_chars += entry.len();
    }
    result
}

/// Execute a single turn for a mission.
async fn run_mission_turn(
    config: Config,
    _root_agent: AgentRef,
    mcp: Arc<McpRegistry>,
    workspaces: workspace::SharedWorkspaceStore,
    library: SharedLibrary,
    events_tx: broadcast::Sender<AgentEvent>,
    _tool_hub: Arc<FrontendToolHub>,
    _status: Arc<RwLock<ControlStatus>>,
    cancel: CancellationToken,
    history: Vec<(String, String)>,
    user_message: String,
    _mission_control: Option<crate::tools::mission::MissionControl>,
    _tree_snapshot: Arc<RwLock<Option<AgentTreeNode>>>,
    _progress_snapshot: Arc<RwLock<ExecutionProgress>>,
    mission_id: Uuid,
    workspace_id: Option<Uuid>,
    backend_id: String,
    agent_override: Option<String>,
    secrets: Option<Arc<SecretsStore>>,
) -> AgentResult {
    let mut config = config;
    let effective_agent = agent_override.clone();
    if let Some(ref agent) = effective_agent {
        config.opencode_agent = Some(agent.clone());
    }
    tracing::info!(
        mission_id = %mission_id,
        workspace_id = ?workspace_id,
        opencode_agent = ?config.opencode_agent,
        history_len = history.len(),
        user_message_len = user_message.len(),
        "Mission turn started"
    );

    // Build context with history
    let max_history_chars = config.context.max_history_total_chars;
    let history_context = build_history_context(&history, max_history_chars);

    // Extract deliverables to include in instructions
    let deliverable_set = extract_deliverables(&user_message);
    let deliverable_reminder = if !deliverable_set.deliverables.is_empty() {
        let paths: Vec<String> = deliverable_set
            .deliverables
            .iter()
            .filter_map(|d| d.path())
            .map(|p| p.display().to_string())
            .collect();
        format!(
            "\n\n**REQUIRED DELIVERABLES** (do not stop until these exist):\n{}\n",
            paths
                .iter()
                .map(|p| format!("- {}", p))
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        String::new()
    };

    let is_multi_step = deliverable_set.is_research_task
        || deliverable_set.requires_report
        || user_message.contains("1.")
        || user_message.contains("- ")
        || user_message.to_lowercase().contains("then");

    let multi_step_instructions = if is_multi_step {
        r#"

**MULTI-STEP TASK RULES:**
- This task has multiple steps. Complete ALL steps before stopping.
- After each tool call, ask yourself: "Have I completed the FULL goal?"
- DO NOT stop after just one step - keep working until ALL deliverables exist.
- If you made progress but aren't done, continue in the same turn.
- Only call complete_mission when ALL requested outputs have been created."#
    } else {
        ""
    };

    let mut convo = String::new();
    convo.push_str(&history_context);
    convo.push_str("User:\n");
    convo.push_str(&user_message);
    convo.push_str(&deliverable_reminder);
    convo.push_str("\n\nInstructions:\n- Continue the conversation helpfully.\n- Use available tools to gather information or make changes.\n- For large data processing tasks (>10KB), prefer executing scripts rather than inline processing.\n- USE information already provided in the message - do not ask for URLs, paths, or details that were already given.\n- When you have fully completed the user's goal or determined it cannot be completed, state that clearly in your final response.");
    convo.push_str(multi_step_instructions);
    convo.push_str("\n");

    // Ensure mission workspace exists and is configured for OpenCode.
    let workspace = workspace::resolve_workspace(&workspaces, &config, workspace_id).await;
    let workspace_root = workspace.path.clone();
    let mission_work_dir = match {
        let lib_guard = library.read().await;
        let lib_ref = lib_guard.as_ref().map(|l| l.as_ref());
        workspace::prepare_mission_workspace_with_skills_backend(
            &workspace,
            &mcp,
            lib_ref,
            mission_id,
            &backend_id,
        )
        .await
    } {
        Ok(dir) => {
            tracing::info!(
                "Mission {} workspace directory: {}",
                mission_id,
                dir.display()
            );
            dir
        }
        Err(e) => {
            tracing::warn!("Failed to prepare mission workspace, using default: {}", e);
            workspace_root
        }
    };

    // Execute based on backend
    let result = match backend_id.as_str() {
        "claudecode" => {
            run_claudecode_turn(
                &workspace,
                &mission_work_dir,
                &user_message,
                config.default_model.as_deref(),
                effective_agent.as_deref(),
                mission_id,
                events_tx.clone(),
                cancel,
                secrets,
                &config.working_dir,
            )
            .await
        }
        "opencode" => {
            // Use per-workspace CLI execution for all workspace types to ensure
            // native bash + correct filesystem scope.
            run_opencode_turn(
                &workspace,
                &mission_work_dir,
                &convo,
                config.default_model.as_deref(),
                effective_agent.as_deref(),
                mission_id,
                events_tx.clone(),
                cancel,
                &config.working_dir,
            )
            .await
        }
        _ => {
            // Don't send Error event - the failure will be emitted as an AssistantMessage
            // with success=false by the caller (control.rs), avoiding duplicate messages.
            AgentResult::failure(format!("Unsupported backend: {}", backend_id), 0)
                .with_terminal_reason(TerminalReason::LlmError)
        }
    };

    tracing::info!(
        mission_id = %mission_id,
        success = result.success,
        cost_cents = result.cost_cents,
        model = ?result.model_used,
        terminal_reason = ?result.terminal_reason,
        "Mission turn finished"
    );
    result
}

fn read_backend_configs() -> Option<Vec<serde_json::Value>> {
    let home = std::env::var("HOME").ok()?;
    let candidates = [
        std::path::PathBuf::from(&home)
            .join(".openagent")
            .join("backend_config.json"),
        std::path::PathBuf::from(&home)
            .join(".openagent")
            .join("data")
            .join("backend_configs.json"),
    ];

    for path in candidates {
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(_) => continue,
        };
        if let Ok(configs) = serde_json::from_str::<Vec<serde_json::Value>>(&contents) {
            return Some(configs);
        }
    }
    None
}

/// Read CLI path from backend config file if available.
fn get_claudecode_cli_path_from_config(_app_working_dir: &std::path::Path) -> Option<String> {
    let configs = read_backend_configs()?;

    for config in configs {
        if config.get("id")?.as_str()? == "claudecode" {
            if let Some(settings) = config.get("settings") {
                if let Some(cli_path) = settings.get("cli_path").and_then(|v| v.as_str()) {
                    if !cli_path.is_empty() {
                        tracing::info!(
                            "Using Claude Code CLI path from backend config: {}",
                            cli_path
                        );
                        return Some(cli_path.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Execute a turn using Claude Code CLI backend.
///
/// For Host workspaces: spawns the CLI directly on the host.
/// For Chroot workspaces: spawns the CLI inside the container using systemd-nspawn.
pub async fn run_claudecode_turn(
    workspace: &Workspace,
    work_dir: &std::path::Path,
    message: &str,
    model: Option<&str>,
    agent: Option<&str>,
    mission_id: Uuid,
    events_tx: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    secrets: Option<Arc<SecretsStore>>,
    app_working_dir: &std::path::Path,
) -> AgentResult {
    use super::ai_providers::{
        get_anthropic_api_key_for_claudecode,
    };
    use std::collections::HashMap;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};    // Try to get API key from Anthropic provider configured for Claude Code backend
    let api_key = if let Some(key) = get_anthropic_api_key_for_claudecode(app_working_dir) {
        tracing::info!("Using Anthropic API key from provider for Claude Code");
        Some(key)
    } else {
        // Fall back to secrets vault (legacy support)
        if let Some(ref store) = secrets {
            match store.get_secret("claudecode", "api_key").await {
                Ok(key) => {
                    tracing::info!("Using Claude Code API key from secrets vault (legacy)");
                    Some(key)
                }
                Err(e) => {
                    tracing::warn!("Failed to get Claude API key from secrets: {}", e);
                    // Fall back to environment variable
                    std::env::var("ANTHROPIC_API_KEY").ok()
                }
            }
        } else {
            std::env::var("ANTHROPIC_API_KEY").ok()
        }
    };

    // Determine CLI path: prefer backend config, then env var, then default
    let cli_path = get_claudecode_cli_path_from_config(app_working_dir)
        .or_else(|| std::env::var("CLAUDE_CLI_PATH").ok())
        .unwrap_or_else(|| "claude".to_string());

    let session_id = Uuid::new_v4().to_string();

    let workspace_exec = WorkspaceExec::new(workspace.clone());
    if let Err(err_msg) =
        ensure_claudecode_cli_available(&workspace_exec, work_dir, &cli_path).await
    {
        tracing::error!("{}", err_msg);
        return AgentResult::failure(err_msg, 0).with_terminal_reason(TerminalReason::LlmError);
    }

    tracing::info!(
        mission_id = %mission_id,
        session_id = %session_id,
        work_dir = %work_dir.display(),
        workspace_type = ?workspace.workspace_type,
        model = ?model,
        agent = ?agent,
        "Starting Claude Code execution via WorkspaceExec"
    );

    // Build CLI arguments
    let mut args = vec![
        "--print".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
    ];

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }

    args.push("--session-id".to_string());
    args.push(session_id.clone());

    if let Some(a) = agent {
        args.push("--agent".to_string());
        args.push(a.to_string());
    }

    // Build environment variables
    let mut env: HashMap<String, String> = HashMap::new();
    if let Some(ref key) = api_key {
        if key.starts_with("sk-ant-oat") {
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), key.clone());
            tracing::debug!("Using OAuth token for Claude CLI authentication");
        } else {
            env.insert("ANTHROPIC_API_KEY".to_string(), key.clone());
            tracing::debug!("Using API key for Claude CLI authentication");
        }
    }

    // Use WorkspaceExec to spawn the CLI in the correct workspace context
    let mut child = match workspace_exec
        .spawn_streaming(work_dir, &cli_path, &args, env)
        .await
    {
        Ok(child) => child,
        Err(e) => {
            let err_msg = format!("Failed to start Claude CLI: {}", e);
            tracing::error!("{}", err_msg);
            return AgentResult::failure(err_msg, 0).with_terminal_reason(TerminalReason::LlmError);
        }
    };

    // Write message to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let msg = message.to_string();
        tokio::spawn(async move {
            if let Err(e) = stdin.write_all(msg.as_bytes()).await {
                tracing::error!("Failed to write to Claude stdin: {}", e);
            }
            // Close stdin to signal end of input
            drop(stdin);
        });
    }

    // Get stdout for reading events
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let err_msg = "Failed to capture Claude stdout";
            tracing::error!("{}", err_msg);
            return AgentResult::failure(err_msg.to_string(), 0)
                .with_terminal_reason(TerminalReason::LlmError);
        }
    };

    // Track tool calls for result mapping
    let mut pending_tools: HashMap<String, String> = HashMap::new();
    let mut total_cost_usd = 0.0f64;
    let mut final_result = String::new();
    let mut had_error = false;

    // Track content block types and accumulated content for Claude Code streaming
    // This is needed because Claude sends incremental deltas that need to be accumulated
    let mut block_types: HashMap<u32, String> = HashMap::new();
    let mut thinking_buffer: HashMap<u32, String> = HashMap::new();
    let mut text_buffer: HashMap<u32, String> = HashMap::new();
    let mut last_thinking_len: usize = 0; // Track last emitted length to avoid re-sending same content

    // Create a buffered reader for stdout
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Process events until completion or cancellation
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(mission_id = %mission_id, "Claude Code execution cancelled, killing process");
                // Kill the process to stop consuming API resources
                let _ = child.kill().await;
                return AgentResult::failure("Cancelled".to_string(), 0)
                    .with_terminal_reason(TerminalReason::Cancelled);
            }
            line_result = lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if line.is_empty() {
                            continue;
                        }

                        let claude_event: ClaudeEvent = match serde_json::from_str(&line) {
                            Ok(event) => event,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to parse Claude event: {} - line: {}",
                                    e,
                                    if line.len() > 200 {
                                        format!("{}...", &line[..200])
                                    } else {
                                        line.clone()
                                    }
                                );
                                continue;
                            }
                        };

                        match claude_event {
                            ClaudeEvent::System(sys) => {
                                tracing::debug!(
                                    "Claude session init: session_id={}, model={:?}",
                                    sys.session_id, sys.model
                                );
                            }
                            ClaudeEvent::StreamEvent(wrapper) => {
                                match wrapper.event {
                                    StreamEvent::ContentBlockDelta { index, delta } => {
                                        // Only process deltas that have text content
                                        if let Some(text) = delta.text {
                                            if text.is_empty() {
                                                continue;
                                            }

                                            // Check the delta type to determine where to route content
                                            // "thinking_delta" -> thinking panel
                                            // "text_delta" -> text output (not thinking)
                                            if delta.delta_type == "thinking_delta" {
                                                // Accumulate thinking content
                                                let buffer = thinking_buffer.entry(index).or_default();
                                                buffer.push_str(&text);

                                                // Send accumulated thinking content (cumulative, like OpenCode)
                                                // Only send if we have new content since last emit
                                                let total_len = thinking_buffer.values().map(|s| s.len()).sum::<usize>();
                                                if total_len > last_thinking_len {
                                                    // Combine all thinking buffers for the cumulative content
                                                    let accumulated: String = thinking_buffer.values().cloned().collect::<Vec<_>>().join("");
                                                    last_thinking_len = total_len;

                                                    let _ = events_tx.send(AgentEvent::Thinking {
                                                        content: accumulated,
                                                        done: false,
                                                        mission_id: Some(mission_id),
                                                    });
                                                }
                                            } else if delta.delta_type == "text_delta" {
                                                // Accumulate text content (will be used for final response)
                                                let buffer = text_buffer.entry(index).or_default();
                                                buffer.push_str(&text);
                                                // Don't send text deltas as thinking events
                                            }
                                            // Ignore other delta types (e.g., input_json_delta for tool use)
                                        }
                                    }
                                    StreamEvent::ContentBlockStart { index, content_block } => {
                                        // Track the block type so we know how to handle deltas
                                        block_types.insert(index, content_block.block_type.clone());

                                        if content_block.block_type == "tool_use" {
                                            if let (Some(id), Some(name)) = (content_block.id, content_block.name) {
                                                pending_tools.insert(id, name);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            ClaudeEvent::Assistant(evt) => {
                                for block in evt.message.content {
                                    match block {
                                        ContentBlock::Text { text } => {
                                            // Text content is the final assistant response
                                            // Don't send as Thinking - it will be in the final AssistantMessage
                                            if !text.is_empty() {
                                                final_result = text;
                                            }
                                        }
                                        ContentBlock::ToolUse { id, name, input } => {
                                            pending_tools.insert(id.clone(), name.clone());
                                            let _ = events_tx.send(AgentEvent::ToolCall {
                                                tool_call_id: id.clone(),
                                                name: name.clone(),
                                                args: input,
                                                mission_id: Some(mission_id),
                                            });
                                        }
                                        ContentBlock::Thinking { thinking } => {
                                            // Only send if this is new content not already streamed
                                            // The streaming deltas already accumulated this, so this is
                                            // typically the final complete thinking block
                                            if !thinking.is_empty() {
                                                let _ = events_tx.send(AgentEvent::Thinking {
                                                    content: thinking,
                                                    done: true, // Mark as done since this is the final block
                                                    mission_id: Some(mission_id),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            ClaudeEvent::User(evt) => {
                                for block in evt.message.content {
                                    if let ContentBlock::ToolResult { tool_use_id, content, is_error } = block {
                                        let name = pending_tools
                                            .get(&tool_use_id)
                                            .cloned()
                                            .unwrap_or_else(|| "unknown".to_string());

                                        let result_value = if let Some(ref extra) = evt.tool_use_result {
                                            serde_json::json!({
                                                "content": content,
                                                "stdout": extra.stdout,
                                                "stderr": extra.stderr,
                                                "is_error": is_error,
                                            })
                                        } else {
                                            serde_json::Value::String(content)
                                        };

                                        let _ = events_tx.send(AgentEvent::ToolResult {
                                            tool_call_id: tool_use_id,
                                            name,
                                            result: result_value,
                                            mission_id: Some(mission_id),
                                        });
                                    }
                                }
                            }
                            ClaudeEvent::Result(res) => {
                                if let Some(cost) = res.total_cost_usd {
                                    total_cost_usd = cost;
                                }
                                if res.is_error || res.subtype == "error" {
                                    had_error = true;
                                    let err_msg = res.result.unwrap_or_else(|| "Unknown error".to_string());
                                    // Don't send an Error event here - let the failure propagate
                                    // through the AgentResult. control.rs will emit an AssistantMessage
                                    // with success=false which the UI displays as a failure message.
                                    // Sending Error here would cause duplicate messages.
                                    final_result = err_msg;
                                } else if let Some(result) = res.result {
                                    final_result = result;
                                }
                                tracing::info!(
                                    mission_id = %mission_id,
                                    cost_usd = total_cost_usd,
                                    "Claude Code execution completed"
                                );
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        // EOF - process finished
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Error reading from Claude CLI: {}", e);
                        break;
                    }
                }
            }
        }
    }

    // Wait for child process to finish and clean up
    let _ = child.wait().await;

    // Convert cost from USD to cents
    let cost_cents = (total_cost_usd * 100.0) as u64;

    if final_result.trim().is_empty() && !had_error {
        had_error = true;
        final_result =
            "Claude Code produced no output. Check CLI installation or authentication.".to_string();
    }

    if had_error {
        AgentResult::failure(final_result, cost_cents)
            .with_terminal_reason(TerminalReason::LlmError)
    } else {
        AgentResult::success(final_result, cost_cents)
    }
}

/// Read CLI path for opencode from backend config file if available.
fn get_opencode_cli_path_from_config(_app_working_dir: &std::path::Path) -> Option<String> {
    let configs = read_backend_configs()?;

    for config in configs {
        if config.get("id")?.as_str()? == "opencode" {
            if let Some(settings) = config.get("settings") {
                if let Some(cli_path) = settings.get("cli_path").and_then(|v| v.as_str()) {
                    if !cli_path.is_empty() {
                        tracing::info!("Using OpenCode CLI path from backend config: {}", cli_path);
                        return Some(cli_path.to_string());
                    }
                }
            }
        }
    }
    None
}

fn get_opencode_permissive_from_config(_app_working_dir: &std::path::Path) -> Option<bool> {
    let configs = read_backend_configs()?;

    for config in configs {
        if config.get("id")?.as_str()? == "opencode" {
            if let Some(settings) = config.get("settings") {
                if let Some(permissive) = settings.get("permissive").and_then(|v| v.as_bool()) {
                    tracing::info!(
                        "Using OpenCode permissive setting from backend config: {}",
                        permissive
                    );
                    return Some(permissive);
                }
            }
        }
    }
    None
}

fn workspace_path_for_env(
    workspace: &Workspace,
    host_path: &std::path::Path,
) -> std::path::PathBuf {
    if workspace.workspace_type == workspace::WorkspaceType::Chroot {
        if let Ok(rel) = host_path.strip_prefix(&workspace.path) {
            return std::path::PathBuf::from("/").join(rel);
        }
    }
    host_path.to_path_buf()
}

fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ANSI escape sequences like "\x1b[31m"
            if let Some('[') = chars.peek() {
                let _ = chars.next();
                while let Some(c) = chars.next() {
                    if c == 'm' {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn parse_opencode_session_token(value: &str) -> Option<String> {
    let mut token = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            token.push(ch);
        } else {
            break;
        }
    }
    if token.starts_with("ses_") {
        return Some(token);
    }
    if token.len() < 8 {
        None
    } else {
        Some(token)
    }
}

fn extract_opencode_session_id(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        for key in ["session id:", "session:", "session_id:", "session="] {
            if let Some(idx) = lower.find(key) {
                let rest = trimmed[idx + key.len()..].trim();
                if let Some(token) = parse_opencode_session_token(rest) {
                    return Some(token);
                }
            }
        }
    }
    None
}

fn opencode_output_needs_fallback(output: &str) -> bool {
    let cleaned = strip_ansi_codes(output);
    let mut lines: Vec<String> = cleaned
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if lines.is_empty() {
        return true;
    }

    for line in lines.drain(..) {
        let lower = line.to_lowercase();
        let is_banner = lower.contains("starting opencode server")
            || lower.contains("opencode server started")
            || lower.contains("sending prompt")
            || lower.contains("waiting for completion")
            || lower.contains("all tasks completed")
            || lower.contains("completed")
            || lower.contains("session id:")
            || lower.contains("session:");
        if !is_banner {
            return false;
        }
    }

    true
}

fn resolve_opencode_storage_root(workspace: &Workspace) -> std::path::PathBuf {
    match workspace.workspace_type {
        WorkspaceType::Chroot => workspace
            .path
            .join("root")
            .join(".local")
            .join("share")
            .join("opencode")
            .join("storage"),
        _ => {
            let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                format!("{}/.local/share", home)
            });
            std::path::PathBuf::from(data_home)
                .join("opencode")
                .join("storage")
        }
    }
}

fn load_latest_opencode_assistant_text(
    workspace: &Workspace,
    session_id: &str,
) -> Option<String> {
    let storage_root = resolve_opencode_storage_root(workspace);
    let message_dir = storage_root.join("message").join(session_id);
    if !message_dir.exists() {
        return None;
    }

    let mut latest_time = 0i64;
    let mut latest_message_id: Option<String> = None;

    let entries = std::fs::read_dir(&message_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&content).ok()?;
        let role = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let created = value
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if created >= latest_time {
            latest_time = created;
            latest_message_id = value
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }

    let message_id = latest_message_id?;
    let parts_dir = storage_root.join("part").join(&message_id);
    if !parts_dir.exists() {
        return None;
    }

    let mut parts: Vec<(i64, String, String)> = Vec::new();
    let part_entries = std::fs::read_dir(&parts_dir).ok()?;
    for entry in part_entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&content).ok()?;
        let part_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if part_type != "text" {
            continue;
        }
        let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let start = value
            .get("time")
            .and_then(|t| t.get("start"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        parts.push((start, filename, text.to_string()));
    }

    if parts.is_empty() {
        return None;
    }

    parts.sort_by(|a, b| {
        let time_cmp = a.0.cmp(&b.0);
        if time_cmp == std::cmp::Ordering::Equal {
            a.1.cmp(&b.1)
        } else {
            time_cmp
        }
    });

    let mut combined = String::new();
    for (_, _, text) in parts {
        combined.push_str(&text);
    }
    if combined.trim().is_empty() {
        None
    } else {
        Some(combined)
    }
}

fn env_var_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        Err(_) => default,
    }
}

async fn command_available(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
    program: &str,
) -> bool {
    if workspace_exec.workspace.workspace_type == WorkspaceType::Host {
        if program.contains('/') {
            return std::path::Path::new(program).is_file();
        }
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(':') {
                if dir.is_empty() {
                    continue;
                }
                let candidate = std::path::Path::new(dir).join(program);
                if candidate.is_file() {
                    return true;
                }
            }
        }
        return false;
    }

    async fn check_dir(
        workspace_exec: &WorkspaceExec,
        cwd: &std::path::Path,
        program: &str,
    ) -> Option<bool> {
        let mut args = Vec::new();
        args.push("-lc".to_string());
        if program.contains('/') {
            args.push(format!("test -x {}", program));
        } else {
            args.push(format!("command -v {} 2>/dev/null", program));
        }
        let output = workspace_exec
            .output(cwd, "/bin/sh", &args, HashMap::new())
            .await
            .ok()?;
        if !output.status.success() {
            return Some(false);
        }
        if program.contains('/') {
            return Some(true);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(!stdout.trim().is_empty())
    }

    if let Some(found) = check_dir(workspace_exec, cwd, program).await {
        if found {
            return true;
        }
    }

    let fallback_dir = &workspace_exec.workspace.path;
    if cwd != fallback_dir {
        if let Some(found) = check_dir(workspace_exec, fallback_dir, program).await {
            return found;
        }
    }

    false
}

async fn ensure_claudecode_cli_available(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
    cli_path: &str,
) -> Result<(), String> {
    if command_available(workspace_exec, cwd, cli_path).await {
        return Ok(());
    }

    let auto_install = env_var_bool("OPEN_AGENT_AUTO_INSTALL_CLAUDECODE", true);
    if !auto_install {
        return Err(format!(
            "Claude Code CLI '{}' not found in workspace. Install it or set CLAUDE_CLI_PATH.",
            cli_path
        ));
    }

    if !command_available(workspace_exec, cwd, "npm").await {
        return Err(format!(
            "Claude Code CLI '{}' not found and npm is missing in the workspace. Install Node.js/npm in the workspace template or set CLAUDE_CLI_PATH.",
            cli_path
        ));
    }

    let mut args = Vec::new();
    args.push("-lc".to_string());
    args.push("npm install -g @anthropic-ai/claude-code@latest".to_string());
    let output = workspace_exec
        .output(cwd, "/bin/sh", &args, HashMap::new())
        .await
        .map_err(|e| format!("Failed to run npm install for Claude Code: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut message = String::new();
        if !stderr.trim().is_empty() {
            message.push_str(stderr.trim());
        }
        if !stdout.trim().is_empty() {
            if !message.is_empty() {
                message.push_str(" | ");
            }
            message.push_str(stdout.trim());
        }
        if message.is_empty() {
            message = "npm install for Claude Code failed with no output".to_string();
        }
        return Err(format!("Claude Code install failed: {}", message));
    }

    if !command_available(workspace_exec, cwd, cli_path).await {
        return Err(format!(
            "Claude Code install completed but '{}' is still not available in workspace PATH.",
            cli_path
        ));
    }

    Ok(())
}

fn runner_is_oh_my_opencode(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "oh-my-opencode")
        .unwrap_or(false)
}

async fn resolve_opencode_installer_fetcher(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
) -> Option<String> {
    let curl_candidates = ["curl", "/usr/bin/curl", "/bin/curl"];
    for candidate in curl_candidates {
        if command_available(workspace_exec, cwd, candidate).await {
            return Some(format!(
                "{} -fsSL https://opencode.ai/install",
                candidate
            ));
        }
    }

    let wget_candidates = ["wget", "/usr/bin/wget", "/bin/wget"];
    for candidate in wget_candidates {
        if command_available(workspace_exec, cwd, candidate).await {
            return Some(format!(
                "{} -qO- https://opencode.ai/install",
                candidate
            ));
        }
    }

    None
}

async fn opencode_binary_available(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
) -> bool {
    if command_available(workspace_exec, cwd, "opencode").await {
        return true;
    }
    if command_available(workspace_exec, cwd, "/usr/local/bin/opencode").await {
        return true;
    }
    if command_available(workspace_exec, cwd, "$HOME/.opencode/bin/opencode").await {
        return true;
    }
    false
}

async fn cleanup_opencode_listeners(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
) {
    let mut args = Vec::new();
    args.push("-lc".to_string());
    args.push(
        "if command -v lsof >/dev/null 2>&1; then \
           pids=$(lsof -t -iTCP:4096 -sTCP:LISTEN 2>/dev/null || true); \
           if [ -n \"$pids\" ]; then kill -9 $pids || true; fi; \
         fi"
        .to_string(),
    );
    let _ = workspace_exec.output(cwd, "/bin/sh", &args, HashMap::new()).await;
}

async fn ensure_opencode_cli_available(
    workspace_exec: &WorkspaceExec,
    cwd: &std::path::Path,
) -> Result<(), String> {
    if opencode_binary_available(workspace_exec, cwd).await {
        return Ok(());
    }

    let auto_install = env_var_bool("OPEN_AGENT_AUTO_INSTALL_OPENCODE", true);
    if !auto_install {
        return Err(
            "OpenCode CLI 'opencode' not found in workspace. Install it or disable OpenCode."
                .to_string(),
        );
    }

    let fetcher = resolve_opencode_installer_fetcher(workspace_exec, cwd).await.ok_or_else(|| {
        "OpenCode CLI 'opencode' not found and neither curl nor wget is available in the workspace. Install curl/wget in the workspace template or disable OpenCode."
            .to_string()
    })?;

    let mut args = Vec::new();
    args.push("-lc".to_string());
    args.push(
        format!(
            "{} | bash -s -- --no-modify-path \
        && if [ -x \"$HOME/.opencode/bin/opencode\" ]; then install -m 0755 \"$HOME/.opencode/bin/opencode\" /usr/local/bin/opencode; fi"
            , fetcher
        ),
    );
    let output = workspace_exec
        .output(cwd, "/bin/sh", &args, HashMap::new())
        .await
        .map_err(|e| format!("Failed to run OpenCode installer: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut message = String::new();
        if !stderr.trim().is_empty() {
            message.push_str(stderr.trim());
        }
        if !stdout.trim().is_empty() {
            if !message.is_empty() {
                message.push_str(" | ");
            }
            message.push_str(stdout.trim());
        }
        if message.is_empty() {
            message = "OpenCode install failed with no output".to_string();
        }
        return Err(format!("OpenCode install failed: {}", message));
    }

    if !opencode_binary_available(workspace_exec, cwd).await {
        return Err("OpenCode install completed but 'opencode' is still not available in workspace PATH.".to_string());
    }

    Ok(())
}

/// Execute a turn using OpenCode CLI backend.
///
/// For Host workspaces: spawns the CLI directly on the host.
/// For Chroot workspaces: spawns the CLI inside the container using systemd-nspawn.
///
/// This uses the `oh-my-opencode run` CLI which creates an embedded OpenCode server,
/// enabling per-workspace isolation without network issues.
pub async fn run_opencode_turn(
    workspace: &Workspace,
    work_dir: &std::path::Path,
    message: &str,
    model: Option<&str>,
    agent: Option<&str>,
    mission_id: Uuid,
    events_tx: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    _app_working_dir: &std::path::Path,
) -> AgentResult {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    // Determine CLI runner: prefer backend config, then env var, then try bunx/npx
    // We use 'bunx oh-my-opencode run' or 'npx oh-my-opencode run' for per-workspace execution.
    let workspace_exec = WorkspaceExec::new(workspace.clone());
    if let Err(err) = ensure_opencode_cli_available(&workspace_exec, work_dir).await {
        tracing::error!("{}", err);
        return AgentResult::failure(err, 0).with_terminal_reason(TerminalReason::LlmError);
    }

    let configured_runner = get_opencode_cli_path_from_config(_app_working_dir)
        .or_else(|| std::env::var("OPENCODE_CLI_PATH").ok());

    let mut runner_is_direct = false;
    let cli_runner = if let Some(path) = configured_runner {
        if command_available(&workspace_exec, work_dir, &path).await {
            runner_is_direct = runner_is_oh_my_opencode(&path);
            path
        } else {
            let err_msg = format!(
                "OpenCode CLI runner '{}' not found in workspace. Install it or update OPENCODE_CLI_PATH.",
                path
            );
            tracing::error!("{}", err_msg);
            return AgentResult::failure(err_msg, 0).with_terminal_reason(TerminalReason::LlmError);
        }
    } else {
        if command_available(&workspace_exec, work_dir, "oh-my-opencode").await {
            runner_is_direct = true;
            "oh-my-opencode".to_string()
        } else {
            let auto_install = env_var_bool("OPEN_AGENT_AUTO_INSTALL_OPENCODE", true);
            if auto_install && command_available(&workspace_exec, work_dir, "npm").await {
                let mut install_args = Vec::new();
                install_args.push("-lc".to_string());
                install_args.push("npm install -g oh-my-opencode@latest".to_string());
                if let Err(e) = workspace_exec
                    .output(work_dir, "/bin/sh", &install_args, HashMap::new())
                    .await
                {
                    tracing::warn!("Failed to auto-install oh-my-opencode: {}", e);
                }
            }

            if command_available(&workspace_exec, work_dir, "oh-my-opencode").await {
                runner_is_direct = true;
                "oh-my-opencode".to_string()
            } else if command_available(&workspace_exec, work_dir, "bunx").await {
                "bunx".to_string()
            } else if command_available(&workspace_exec, work_dir, "npx").await {
                "npx".to_string()
            } else {
                let err_msg =
                    "No OpenCode CLI runner found in workspace (expected oh-my-opencode, bunx, or npx)."
                        .to_string();
                tracing::error!("{}", err_msg);
                return AgentResult::failure(err_msg, 0)
                    .with_terminal_reason(TerminalReason::LlmError);
            }
        }
    };

    tracing::info!(
        mission_id = %mission_id,
        work_dir = %work_dir.display(),
        workspace_type = ?workspace.workspace_type,
        model = ?model,
        agent = ?agent,
        cli_runner = %cli_runner,
        "Starting OpenCode execution via WorkspaceExec (per-workspace CLI mode)"
    );

    let work_dir_env = workspace_path_for_env(workspace, work_dir);
    let work_dir_arg = work_dir_env.to_string_lossy().to_string();

    // Build CLI arguments for oh-my-opencode run
    // The 'run' command takes a prompt and executes it with completion detection
    // Arguments: bunx oh-my-opencode run [--agent <agent>] [--directory <path>] [--timeout <ms>] <message>
    let mut args = if runner_is_direct {
        vec!["run".to_string()]
    } else {
        vec!["oh-my-opencode".to_string(), "run".to_string()]
    };

    if let Some(a) = agent {
        args.push("--agent".to_string());
        args.push(a.to_string());
    }

    args.push("--directory".to_string());
    args.push(work_dir_arg.clone());

    // Add timeout (0 = no timeout, let the agent complete)
    args.push("--timeout".to_string());
    args.push("0".to_string());

    // The message is passed as the final argument
    args.push(message.to_string());

    tracing::debug!(
        mission_id = %mission_id,
        runner_is_direct = runner_is_direct,
        cli_args = ?args,
        "OpenCode CLI args prepared"
    );

    // Build environment variables
    let mut env: HashMap<String, String> = HashMap::new();

    // Pass the model if specified
    if let Some(m) = model {
        // Parse provider/model format
        if let Some((provider, model_id)) = m.split_once('/') {
            env.insert("OPENCODE_PROVIDER".to_string(), provider.to_string());
            env.insert("OPENCODE_MODEL".to_string(), model_id.to_string());
        } else {
            env.insert("OPENCODE_MODEL".to_string(), m.to_string());
        }
    }

    // Ensure OpenCode uses workspace-local config
    let opencode_config_dir = workspace_path_for_env(workspace, &work_dir.join(".opencode"));
    let opencode_config_path = workspace_path_for_env(workspace, &work_dir.join("opencode.json"));
    env.insert(
        "OPENCODE_CONFIG_DIR".to_string(),
        opencode_config_dir.to_string_lossy().to_string(),
    );
    env.insert(
        "OPENCODE_CONFIG".to_string(),
        opencode_config_path.to_string_lossy().to_string(),
    );

    if let Some(permissive) = get_opencode_permissive_from_config(_app_working_dir) {
        env.insert("OPENCODE_PERMISSIVE".to_string(), permissive.to_string());
    } else if let Ok(value) = std::env::var("OPENCODE_PERMISSIVE") {
        if !value.trim().is_empty() {
            env.insert("OPENCODE_PERMISSIVE".to_string(), value);
        }
    }

    // Disable ANSI color codes for easier parsing
    env.insert("NO_COLOR".to_string(), "1".to_string());
    env.insert("FORCE_COLOR".to_string(), "0".to_string());

    // Set non-interactive mode
    env.insert("OPENCODE_NON_INTERACTIVE".to_string(), "true".to_string());
    env.insert("OPENCODE_RUN".to_string(), "true".to_string());
    env.entry("OPEN_AGENT_WORKSPACE_TYPE".to_string())
        .or_insert_with(|| workspace.workspace_type.as_str().to_string());

    cleanup_opencode_listeners(&workspace_exec, work_dir).await;

    // Use WorkspaceExec to spawn the CLI in the correct workspace context
    let mut child = match workspace_exec
        .spawn_streaming(work_dir, &cli_runner, &args, env)
        .await
    {
        Ok(child) => child,
        Err(e) => {
            let err_msg = format!("Failed to start OpenCode CLI: {}", e);
            tracing::error!("{}", err_msg);
            return AgentResult::failure(err_msg, 0).with_terminal_reason(TerminalReason::LlmError);
        }
    };

    // Get stdout and stderr for reading output
    // oh-my-opencode run writes:
    // - stdout: assistant text output (the actual response)
    // - stderr: event logs (tool calls, results, session status)
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let err_msg = "Failed to capture OpenCode stdout";
            tracing::error!("{}", err_msg);
            return AgentResult::failure(err_msg.to_string(), 0)
                .with_terminal_reason(TerminalReason::LlmError);
        }
    };

    let stderr = child.stderr.take();

    let mut final_result = String::new();
    let mut had_error = false;
    let session_id_capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut stdout_reader = stdout;

    // Spawn a task to read stderr events if available
    let events_tx_clone = events_tx.clone();
    let mission_id_clone = mission_id;
    let session_id_clone = session_id_capture.clone();
    let stderr_handle = if let Some(stderr) = stderr {
        Some(tokio::spawn(async move {
            let stderr_reader = BufReader::new(stderr);
            let mut stderr_lines = stderr_reader.lines();
            let mut last_tool_id: Option<String> = None;
            let mut last_tool_name: Option<String> = None;

            while let Ok(Some(line)) = stderr_lines.next_line().await {
                let clean = strip_ansi_codes(&line);
                let clean = clean.trim().to_string();
                if clean.is_empty() {
                    continue;
                }

                tracing::debug!(mission_id = %mission_id_clone, line = %clean, "OpenCode CLI stderr");

                if let Some(session) = extract_opencode_session_id(&clean) {
                    let mut guard = session_id_clone.lock().unwrap();
                    if guard.is_none() {
                        *guard = Some(session);
                    }
                }

                // Parse stderr for tool execution events
                // Format: "[MAIN] ⚡ TOOL.EXECUTE: <tool>" or "✓ TOOL.RESULT: \"...\""
                if clean.contains("TOOL.EXECUTE:") {
                    // Extract tool name from the line
                    if let Some(name_start) = clean.find("TOOL.EXECUTE:") {
                        let name_part = &clean[name_start + 14..];
                        let tool_name = name_part.trim().trim_matches('"');
                        let tool_id = format!("opencode-{}", uuid::Uuid::new_v4());
                        last_tool_id = Some(tool_id.clone());
                        last_tool_name = Some(tool_name.to_string());
                        let _ = events_tx_clone.send(AgentEvent::ToolCall {
                            tool_call_id: tool_id,
                            name: tool_name.to_string(),
                            args: serde_json::json!({}),
                            mission_id: Some(mission_id_clone),
                        });
                    }
                } else if clean.contains("TOOL.RESULT:") {
                    // Emit tool result using the most recent tool call if available
                    let tool_id = last_tool_id
                        .clone()
                        .unwrap_or_else(|| format!("opencode-{}", uuid::Uuid::new_v4()));
                    let tool_name = last_tool_name
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let _ = events_tx_clone.send(AgentEvent::ToolResult {
                        tool_call_id: tool_id,
                        name: tool_name,
                        result: serde_json::json!({ "output": clean }),
                        mission_id: Some(mission_id_clone),
                    });
                } else if clean.contains("SESSION.ERROR:")
                    || clean.contains("Error:")
                    || clean.contains("error:")
                {
                    // Emit error event
                    let _ = events_tx_clone.send(AgentEvent::Error {
                        message: clean.clone(),
                        mission_id: Some(mission_id_clone),
                        resumable: true,
                    });
                }

                // Also forward as thinking for UI visibility
                let _ = events_tx_clone.send(AgentEvent::Thinking {
                    content: clean,
                    done: false,
                    mission_id: Some(mission_id_clone),
                });
            }
        }))
    } else {
        None
    };

    // Process stdout until completion or cancellation
    // stdout contains the actual assistant response text
    let mut buffer = [0u8; 4096];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(mission_id = %mission_id, "OpenCode execution cancelled, killing process");
                let _ = child.kill().await;
                if let Some(handle) = stderr_handle {
                    handle.abort();
                }
                return AgentResult::failure("Cancelled".to_string(), 0)
                    .with_terminal_reason(TerminalReason::Cancelled);
            }
            read_result = stdout_reader.read(&mut buffer) => {
                match read_result {
                    Ok(0) => {
                        // EOF - process finished
                        break;
                    }
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buffer[..n]);
                        if !chunk.is_empty() {
                            tracing::debug!(mission_id = %mission_id, chunk = %chunk, "OpenCode CLI stdout");
                            final_result.push_str(&chunk);

                            if chunk.contains("Error:") || chunk.contains("error:") {
                                had_error = true;
                            }

                            let _ = events_tx.send(AgentEvent::Thinking {
                                content: chunk.to_string(),
                                done: false,
                                mission_id: Some(mission_id),
                            });
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading from OpenCode CLI stdout: {}", e);
                        break;
                    }
                }
            }
        }
    }

    // Wait for stderr task to complete
    if let Some(handle) = stderr_handle {
        let _ = handle.await;
    }

    // Wait for child process to finish and clean up
    let exit_status = child.wait().await;

    // Check exit status
    if let Ok(status) = exit_status {
        if !status.success() {
            had_error = true;
            if final_result.is_empty() {
                final_result = format!("OpenCode CLI exited with status: {}", status);
            }
        }
    }

    // Emit final thinking done marker
    let _ = events_tx.send(AgentEvent::Thinking {
        content: String::new(),
        done: true,
        mission_id: Some(mission_id),
    });

    if opencode_output_needs_fallback(&final_result) {
        let session_id = session_id_capture.lock().unwrap().clone();
        let session_id = session_id.or_else(|| extract_opencode_session_id(&final_result));
        if let Some(session_id) = session_id {
            if let Some(text) = load_latest_opencode_assistant_text(workspace, &session_id) {
                tracing::info!(
                    mission_id = %mission_id,
                    session_id = %session_id,
                    text_len = text.len(),
                    "Recovered OpenCode assistant output from storage"
                );
                final_result = text;
            } else {
                tracing::warn!(
                    mission_id = %mission_id,
                    session_id = %session_id,
                    "OpenCode assistant output not found in storage"
                );
            }
        } else {
            tracing::warn!(
                mission_id = %mission_id,
                "OpenCode output was empty/banner-only and no session id was detected"
            );
        }
    }

    tracing::info!(
        mission_id = %mission_id,
        had_error = had_error,
        result_len = final_result.len(),
        "OpenCode CLI execution completed"
    );

    if had_error {
        AgentResult::failure(final_result, 0).with_terminal_reason(TerminalReason::LlmError)
    } else {
        AgentResult::success(final_result, 0)
    }
}

/// Compact info about a running mission (for API responses).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunningMissionInfo {
    pub mission_id: Uuid,
    pub state: String,
    pub queue_len: usize,
    pub history_len: usize,
    pub seconds_since_activity: u64,
    pub expected_deliverables: usize,
}

impl From<&MissionRunner> for RunningMissionInfo {
    fn from(runner: &MissionRunner) -> Self {
        Self {
            mission_id: runner.mission_id,
            state: match runner.state {
                MissionRunState::Queued => "queued".to_string(),
                MissionRunState::Running => "running".to_string(),
                MissionRunState::WaitingForTool => "waiting_for_tool".to_string(),
                MissionRunState::Finished => "finished".to_string(),
            },
            queue_len: runner.queue.len(),
            history_len: runner.history.len(),
            seconds_since_activity: runner.last_activity.elapsed().as_secs(),
            expected_deliverables: runner.deliverables.deliverables.len(),
        }
    }
}
