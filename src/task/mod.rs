//! Task module - defines tasks, subtasks, and verification criteria.
//!
//! This module is designed with formal verification in mind:
//! - All types use algebraic data types with exhaustive matching
//! - Invariants are documented and enforced in constructors
//! - Pure functions are separated from IO operations

pub mod deliverables;
mod subtask;
pub mod task;
mod verification;

pub use deliverables::{extract_deliverables, Deliverable, DeliverableSet};
pub use subtask::{Subtask, SubtaskPlan, SubtaskPlanError};
pub use task::{Task, TaskAnalysis, TaskError, TaskId, TaskStatus, TokenUsageSummary};
pub use verification::{
    ProgrammaticCheck, VerificationCriteria, VerificationMethod, VerificationResult,
};
