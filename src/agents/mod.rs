//! Agents module - task execution via OpenCode.
//!
//! # Agent Types
//! - **OpenCodeAgent**: Delegates task execution to an OpenCode server (Claude Max)
//!
//! # Design Principles
//! - OpenCode handles all task execution
//! - Real-time event streaming (thinking, tool calls, results)
//! - Integration with Claude Max subscriptions

mod context;
mod opencode;
mod types;

use std::sync::Arc;

pub use opencode::OpenCodeAgent;

pub use context::AgentContext;
pub use types::{AgentError, AgentId, AgentResult, AgentType, Complexity, TerminalReason};

use crate::task::Task;
use async_trait::async_trait;

/// Reference to an agent (thread-safe shared pointer).
pub type AgentRef = Arc<dyn Agent>;

/// Base trait for all agents.
///
/// # Invariants
/// - `execute()` returns `Ok` only if the task was actually completed or delegated
/// - `execute()` never panics; all errors are returned as `Err`
#[async_trait]
pub trait Agent: Send + Sync {
    /// Get the unique identifier for this agent.
    fn id(&self) -> &AgentId;

    /// Get the type/role of this agent.
    fn agent_type(&self) -> AgentType;

    /// Execute a task.
    ///
    /// # Preconditions
    /// - `task.budget().remaining_cents() > 0` (has budget)
    /// - `task.status() == Pending || task.status() == Running`
    ///
    /// # Postconditions
    /// - On success: task is completed or delegated appropriately
    /// - `result.cost_cents <= task.budget().total_cents()`
    async fn execute(&self, task: &mut Task, ctx: &AgentContext) -> AgentResult;

    /// Get a human-readable description of this agent.
    fn description(&self) -> &str {
        "Generic agent"
    }
}

/// Capabilities of leaf agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeafCapability {
    /// Can estimate task complexity
    ComplexityEstimation,

    /// Can select optimal model for a task
    ModelSelection,

    /// Can execute tasks using tools
    TaskExecution,

    /// Can verify task completion
    Verification,
}

impl LeafCapability {
    /// Get the agent type for this capability.
    pub fn agent_type(&self) -> AgentType {
        match self {
            Self::ComplexityEstimation => AgentType::ComplexityEstimator,
            Self::ModelSelection => AgentType::ModelSelector,
            Self::TaskExecution => AgentType::TaskExecutor,
            Self::Verification => AgentType::Verifier,
        }
    }
}
