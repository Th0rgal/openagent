//! Task module - defines tasks, subtasks, and verification criteria.
//!
//! This module is designed with formal verification in mind:
//! - All types use algebraic data types with exhaustive matching
//! - Invariants are documented and enforced in constructors
//! - Pure functions are separated from IO operations

pub mod task;
mod subtask;
mod verification;

pub use task::{Task, TaskId, TaskStatus, TaskError, TaskAnalysis, TokenUsageSummary};
pub use subtask::{Subtask, SubtaskPlan, SubtaskPlanError};
pub use verification::{VerificationCriteria, VerificationResult, VerificationMethod, ProgrammaticCheck};

