//! Verification criteria and results for tasks.
//!
//! Supports hybrid verification: programmatic checks with LLM fallback.
//!
//! # Design Principles
//! - Prefer programmatic verification when possible (deterministic, fast)
//! - Use LLM verification for subjective or complex assessments
//! - Hybrid mode tries programmatic first, falls back to LLM

use serde::{Deserialize, Serialize};

/// Programmatic checks that can be performed without LLM.
///
/// # Exhaustive Matching
/// All variants must be handled explicitly - no catch-all allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProgrammaticCheck {
    /// Check if a file exists at the given path
    FileExists { path: String },

    /// Check if a file contains specific content
    FileContains { path: String, content: String },

    /// Check if a command exits with code 0
    CommandSucceeds { command: String },

    /// Check if a command output matches expected pattern
    CommandOutputMatches { command: String, pattern: String },

    /// Check if a directory exists
    DirectoryExists { path: String },

    /// Check if a file matches a regex pattern
    FileMatchesRegex { path: String, pattern: String },

    /// Multiple checks that must all pass
    All(Vec<ProgrammaticCheck>),

    /// At least one check must pass
    Any(Vec<ProgrammaticCheck>),
}

impl ProgrammaticCheck {
    /// Create a file exists check.
    pub fn file_exists(path: impl Into<String>) -> Self {
        Self::FileExists { path: path.into() }
    }

    /// Create a command succeeds check.
    pub fn command_succeeds(command: impl Into<String>) -> Self {
        Self::CommandSucceeds {
            command: command.into(),
        }
    }

    /// Create an "all must pass" composite check.
    pub fn all(checks: Vec<ProgrammaticCheck>) -> Self {
        Self::All(checks)
    }

    /// Create an "any must pass" composite check.
    pub fn any(checks: Vec<ProgrammaticCheck>) -> Self {
        Self::Any(checks)
    }
}

/// How to verify a task was completed correctly.
///
/// # Variants
/// - `Programmatic`: Use only programmatic checks (fast, deterministic)
/// - `LlmBased`: Use LLM to verify (flexible, slower)
/// - `Hybrid`: Try programmatic first, fall back to LLM if inconclusive
/// - `None`: No verification required (use with caution)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationCriteria {
    /// Programmatic verification only
    Programmatic(ProgrammaticCheck),

    /// LLM-based verification with a prompt describing success criteria
    LlmBased {
        /// Prompt describing what "success" looks like
        success_criteria: String,
    },

    /// Try programmatic first, fall back to LLM
    Hybrid {
        /// Programmatic check to try first
        programmatic: ProgrammaticCheck,
        /// LLM prompt to use if programmatic is inconclusive
        llm_fallback: String,
    },

    /// No verification (task is considered complete when agent says so)
    None,
}

impl VerificationCriteria {
    /// Create a programmatic-only verification.
    pub fn programmatic(check: ProgrammaticCheck) -> Self {
        Self::Programmatic(check)
    }

    /// Create an LLM-based verification.
    pub fn llm_based(success_criteria: impl Into<String>) -> Self {
        Self::LlmBased {
            success_criteria: success_criteria.into(),
        }
    }

    /// Create a hybrid verification.
    pub fn hybrid(programmatic: ProgrammaticCheck, llm_fallback: impl Into<String>) -> Self {
        Self::Hybrid {
            programmatic,
            llm_fallback: llm_fallback.into(),
        }
    }

    /// Create a no-verification criteria.
    pub fn none() -> Self {
        Self::None
    }

    /// Check if this verification requires LLM access.
    ///
    /// # Returns
    /// `true` if LLM may be needed (LlmBased or Hybrid)
    pub fn may_require_llm(&self) -> bool {
        matches!(self, Self::LlmBased { .. } | Self::Hybrid { .. })
    }

    /// Check if this verification is purely programmatic.
    pub fn is_programmatic_only(&self) -> bool {
        matches!(self, Self::Programmatic(_))
    }
}

impl Default for VerificationCriteria {
    fn default() -> Self {
        Self::None
    }
}

/// Result of a verification attempt.
///
/// # Invariants
/// - If `passed == true`, the task is considered successfully completed
/// - `reasoning` should always explain why the verification passed or failed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the verification passed
    passed: bool,

    /// Explanation of why the verification passed or failed
    reasoning: String,

    /// Which method was used for verification
    method: VerificationMethod,

    /// Cost in cents if LLM was used
    cost_cents: u64,
}

impl VerificationResult {
    /// Create a passing result.
    ///
    /// # Postcondition
    /// `result.passed == true`
    pub fn pass(reasoning: impl Into<String>, method: VerificationMethod, cost_cents: u64) -> Self {
        Self {
            passed: true,
            reasoning: reasoning.into(),
            method,
            cost_cents,
        }
    }

    /// Create a failing result.
    ///
    /// # Postcondition
    /// `result.passed == false`
    pub fn fail(reasoning: impl Into<String>, method: VerificationMethod, cost_cents: u64) -> Self {
        Self {
            passed: false,
            reasoning: reasoning.into(),
            method,
            cost_cents,
        }
    }

    pub fn passed(&self) -> bool {
        self.passed
    }

    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    pub fn method(&self) -> &VerificationMethod {
        &self.method
    }

    pub fn cost_cents(&self) -> u64 {
        self.cost_cents
    }
}

/// Method used for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationMethod {
    /// Programmatic check was used
    Programmatic,
    /// LLM was used for verification
    Llm { model: String },
    /// No verification was performed
    None,
}
