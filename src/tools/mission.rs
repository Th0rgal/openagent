//! Mission control tool - allows the agent to complete or fail the current mission.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::Tool;

/// Command sent by the mission tool to the control session.
#[derive(Debug, Clone)]
pub enum MissionControlCommand {
    SetStatus {
        status: MissionStatusValue,
        summary: Option<String>,
    },
}

/// Mission status values (mirrors api::control::MissionStatus but simplified for tool use).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionStatusValue {
    Completed,
    Failed,
}

impl std::fmt::Display for MissionStatusValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Shared state for mission control, passed to the tool.
#[derive(Clone)]
pub struct MissionControl {
    pub current_mission_id: Arc<RwLock<Option<Uuid>>>,
    pub cmd_tx: mpsc::Sender<MissionControlCommand>,
}

/// Tool that allows the agent to mark the current mission as completed or failed.
pub struct CompleteMission {
    pub control: Option<MissionControl>,
}

impl CompleteMission {
    pub fn new() -> Self {
        Self { control: None }
    }

    pub fn with_control(control: MissionControl) -> Self {
        Self {
            control: Some(control),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CompleteMissionArgs {
    /// Status: "completed" or "failed"
    status: String,
    /// Optional summary explaining the outcome
    summary: Option<String>,
}

#[async_trait]
impl Tool for CompleteMission {
    fn name(&self) -> &str {
        "complete_mission"
    }

    fn description(&self) -> &str {
        "Mark the current mission as completed or failed. Use this when you have finished the user's goal or when you cannot complete it. The user can still reopen or change the mission status later."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "The final status of the mission. Use 'completed' when the goal has been achieved, 'failed' when it cannot be completed."
                },
                "summary": {
                    "type": "string",
                    "description": "Optional summary explaining the outcome (e.g., what was accomplished or why it failed)."
                }
            },
            "required": ["status"]
        })
    }

    async fn execute(&self, args: Value, _working_dir: &Path) -> anyhow::Result<String> {
        let args: CompleteMissionArgs = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("Invalid arguments: {}", e))?;

        let status = match args.status.to_lowercase().as_str() {
            "completed" => MissionStatusValue::Completed,
            "failed" => MissionStatusValue::Failed,
            other => {
                return Err(anyhow::anyhow!(
                    "Invalid status '{}'. Must be 'completed' or 'failed'.",
                    other
                ))
            }
        };

        let Some(control) = &self.control else {
            return Ok("Mission control not available in this context. The mission status was not changed.".to_string());
        };

        // Check if there's a current mission
        let mission_id = control.current_mission_id.read().await.clone();
        if mission_id.is_none() {
            return Ok("No active mission to complete. Start a mission first.".to_string());
        }

        // Send the command
        control
            .cmd_tx
            .send(MissionControlCommand::SetStatus {
                status,
                summary: args.summary.clone(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send mission control command"))?;

        let summary_msg = args
            .summary
            .map(|s| format!(" Summary: {}", s))
            .unwrap_or_default();

        Ok(format!(
            "Mission marked as {}.{}",
            status, summary_msg
        ))
    }
}
