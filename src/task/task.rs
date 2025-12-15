//! Core Task type with budget and verification criteria.
//!
//! # Invariants
//! - `budget.allocated_cents <= budget.total_cents`
//! - `id` is unique within an agent tree execution

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::verification::VerificationCriteria;
use crate::budget::Budget;

/// Analysis and telemetry for a task.
///
/// This is mutable, but only via explicit `analysis_mut()` accessor on `Task`.
///
/// # Design Notes (Provability)
/// - This is intended as *auxiliary metadata*; it must not affect the logical
///   correctness of task execution.
/// - In future proofs, we can treat this as observational data (logs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskAnalysis {
    /// Estimated complexity score in [0.0, 1.0]
    pub complexity_score: Option<f64>,
    /// Reasoning for complexity estimate
    pub complexity_reasoning: Option<String>,
    /// Whether the task should be split (per estimator)
    pub should_split: Option<bool>,
    /// Estimated total tokens for completing the task (input + output)
    pub estimated_total_tokens: Option<u64>,

    /// Model chosen for execution (if selected)
    pub selected_model: Option<String>,
    /// Estimated cost in cents (if computed)
    pub estimated_cost_cents: Option<u64>,

    /// Actual usage aggregated over all LLM calls during execution
    pub actual_usage: Option<TokenUsageSummary>,
}

/// Aggregate token usage (LLM telemetry).
///
/// # Invariants
/// - `total_tokens == prompt_tokens + completion_tokens` (enforced in constructor)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsageSummary {
    /// Create a new token usage summary.
    ///
    /// # Postcondition
    /// `total_tokens == prompt_tokens + completion_tokens`
    pub fn new(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
        }
    }

    /// Add another usage summary.
    ///
    /// # Postcondition
    /// Totals are component-wise sums.
    pub fn add(&self, other: &TokenUsageSummary) -> TokenUsageSummary {
        TokenUsageSummary::new(
            self.prompt_tokens.saturating_add(other.prompt_tokens),
            self.completion_tokens
                .saturating_add(other.completion_tokens),
        )
    }
}

/// Unique identifier for a task.
/// 
/// # Properties
/// - Globally unique within an execution context
/// - Immutable once created
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(Uuid);

impl TaskId {
    /// Create a new unique task ID.
    /// 
    /// # Postcondition
    /// Returns a fresh ID that has never been used before in this process.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a task in its lifecycle.
/// 
/// # State Machine
/// ```text
/// Pending -> Running -> Completed
///                   \-> Failed
///        \-> Cancelled
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is waiting to be executed
    Pending,
    /// Task is currently being executed
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed { reason: String },
    /// Task was cancelled before completion
    Cancelled,
}

impl TaskStatus {
    /// Check if the task is in a terminal state.
    /// 
    /// # Returns
    /// `true` if the task is Completed, Failed, or Cancelled.
    /// 
    /// # Property
    /// `is_terminal() => !can_transition()`
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Failed { .. } | TaskStatus::Cancelled)
    }

    /// Check if the task is still active (can make progress).
    /// 
    /// # Returns
    /// `true` if the task is Pending or Running.
    pub fn is_active(&self) -> bool {
        matches!(self, TaskStatus::Pending | TaskStatus::Running)
    }
}

/// A task to be executed by an agent.
/// 
/// # Invariants
/// - `budget.allocated_cents <= budget.total_cents`
/// - If `parent_id.is_some()`, this is a subtask
/// 
/// # Design for Provability
/// - All fields are immutable after construction (except status via explicit transitions)
/// - Budget constraints are checked at construction time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for this task
    id: TaskId,
    
    /// Human-readable description of what to accomplish
    description: String,
    
    /// How to verify the task was completed correctly
    verification: VerificationCriteria,
    
    /// Budget constraints for this task
    budget: Budget,
    
    /// Analysis and telemetry (optional)
    analysis: TaskAnalysis,

    /// Parent task ID if this is a subtask
    parent_id: Option<TaskId>,
    
    /// Current status
    status: TaskStatus,
}

impl Task {
    /// Create a new task with the given parameters.
    /// 
    /// # Preconditions
    /// - `budget.allocated_cents <= budget.total_cents`
    /// - `description` is non-empty
    /// 
    /// # Postconditions
    /// - Returns a task with `status == Pending`
    /// - `task.id` is a fresh unique identifier
    /// 
    /// # Errors
    /// Returns `Err` if preconditions are violated.
    pub fn new(
        description: String,
        verification: VerificationCriteria,
        budget: Budget,
    ) -> Result<Self, TaskError> {
        if description.is_empty() {
            return Err(TaskError::EmptyDescription);
        }
        
        // Budget invariant is enforced by Budget::new()
        
        Ok(Self {
            id: TaskId::new(),
            description,
            verification,
            budget,
            analysis: TaskAnalysis::default(),
            parent_id: None,
            status: TaskStatus::Pending,
        })
    }

    /// Create a subtask with a parent reference.
    /// 
    /// # Preconditions
    /// - Same as `new()`
    /// - `parent_id` refers to an existing task
    pub fn new_subtask(
        description: String,
        verification: VerificationCriteria,
        budget: Budget,
        parent_id: TaskId,
    ) -> Result<Self, TaskError> {
        let mut task = Self::new(description, verification, budget)?;
        task.parent_id = Some(parent_id);
        Ok(task)
    }

    // Getters - all return references to preserve immutability semantics

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn verification(&self) -> &VerificationCriteria {
        &self.verification
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    pub fn budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }

    pub fn analysis(&self) -> &TaskAnalysis {
        &self.analysis
    }

    pub fn analysis_mut(&mut self) -> &mut TaskAnalysis {
        &mut self.analysis
    }

    pub fn parent_id(&self) -> Option<TaskId> {
        self.parent_id
    }

    pub fn status(&self) -> &TaskStatus {
        &self.status
    }

    /// Check if this task is a subtask (has a parent).
    pub fn is_subtask(&self) -> bool {
        self.parent_id.is_some()
    }

    // State transitions - explicit and validated

    /// Transition the task to Running state.
    /// 
    /// # Precondition
    /// `self.status == Pending`
    /// 
    /// # Errors
    /// Returns `Err` if the task is not in Pending state.
    pub fn start(&mut self) -> Result<(), TaskError> {
        match &self.status {
            TaskStatus::Pending => {
                self.status = TaskStatus::Running;
                Ok(())
            }
            other => Err(TaskError::InvalidTransition {
                from: format!("{:?}", other),
                to: "Running".to_string(),
            }),
        }
    }

    /// Transition the task to Completed state.
    /// 
    /// # Precondition
    /// `self.status == Running`
    pub fn complete(&mut self) -> Result<(), TaskError> {
        match &self.status {
            TaskStatus::Running => {
                self.status = TaskStatus::Completed;
                Ok(())
            }
            other => Err(TaskError::InvalidTransition {
                from: format!("{:?}", other),
                to: "Completed".to_string(),
            }),
        }
    }

    /// Transition the task to Failed state.
    /// 
    /// # Precondition
    /// `self.status == Running`
    pub fn fail(&mut self, reason: String) -> Result<(), TaskError> {
        match &self.status {
            TaskStatus::Running => {
                self.status = TaskStatus::Failed { reason };
                Ok(())
            }
            other => Err(TaskError::InvalidTransition {
                from: format!("{:?}", other),
                to: "Failed".to_string(),
            }),
        }
    }

    /// Transition the task to Cancelled state.
    /// 
    /// # Precondition
    /// `self.status.is_active()`
    pub fn cancel(&mut self) -> Result<(), TaskError> {
        if self.status.is_active() {
            self.status = TaskStatus::Cancelled;
            Ok(())
        } else {
            Err(TaskError::InvalidTransition {
                from: format!("{:?}", self.status),
                to: "Cancelled".to_string(),
            })
        }
    }
}

/// Errors that can occur during task operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TaskError {
    #[error("Task description cannot be empty")]
    EmptyDescription,
    
    #[error("Invalid state transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
    
    #[error("Budget error: {0}")]
    BudgetError(String),
}

