//! Smart retry strategy for budget overflow.
//!
//! Analyzes failure mode and recommends appropriate retry action:
//! - If model lacks capability: upgrade to smarter model
//! - If task just needs more tokens: continue with same/cheaper model
//! - If completely stuck: request budget extension

use serde::{Deserialize, Serialize};

/// Result of analyzing why a task failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysis {
    /// The primary failure mode detected
    pub mode: FailureMode,
    
    /// Confidence in this analysis (0.0 - 1.0)
    pub confidence: f64,
    
    /// Evidence supporting this analysis
    pub evidence: Vec<String>,
    
    /// Recommended retry action
    pub recommendation: RetryRecommendation,
}

/// Why the task failed to complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureMode {
    /// Model is capable but ran out of budget/iterations while making progress
    BudgetExhaustedWithProgress,
    
    /// Model is stuck, repeating actions, or making errors
    ModelCapabilityInsufficient,
    
    /// External errors (API failures, tool errors)
    ExternalError,
    
    /// Task is fundamentally impossible or ill-defined
    TaskInfeasible,
    
    /// Unknown failure mode
    Unknown,
}

/// What action to take on retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetryRecommendation {
    /// Continue with the same model, just needs more budget
    ContinueSameModel {
        additional_budget_cents: u64,
        reason: String,
    },
    
    /// Try a cheaper model (task is simple, just needs more tokens)
    TryCheaperModel {
        suggested_model: Option<String>,
        additional_budget_cents: u64,
        reason: String,
    },
    
    /// Upgrade to a smarter model (current model lacks capability)
    UpgradeModel {
        suggested_model: Option<String>,
        additional_budget_cents: u64,
        reason: String,
    },
    
    /// Request human intervention or budget approval
    RequestExtension {
        estimated_additional_cents: u64,
        reason: String,
    },
    
    /// Don't retry - task is infeasible
    DoNotRetry {
        reason: String,
    },
}

/// Signals from task execution used for failure analysis.
#[derive(Debug, Clone, Default)]
pub struct ExecutionSignals {
    /// Number of iterations completed
    pub iterations: u32,
    
    /// Maximum iterations allowed
    pub max_iterations: u32,
    
    /// Number of successful tool calls
    pub successful_tool_calls: u32,
    
    /// Number of failed tool calls
    pub failed_tool_calls: u32,
    
    /// Whether any files were created/modified
    pub files_modified: bool,
    
    /// Whether the same tool was called repeatedly with same args
    pub repetitive_actions: bool,
    
    /// Whether there were explicit error messages in output
    pub has_error_messages: bool,
    
    /// Whether progress was being made (partial results visible)
    pub partial_progress: bool,
    
    /// Cost spent so far
    pub cost_spent_cents: u64,
    
    /// Original budget
    pub budget_total_cents: u64,
    
    /// The final output/error message
    pub final_output: String,
    
    /// The model that was used
    pub model_used: String,
}

impl ExecutionSignals {
    /// Analyze the signals to determine failure mode.
    pub fn analyze(&self) -> FailureAnalysis {
        let mut evidence = Vec::new();
        
        // Calculate progress indicators
        let iteration_ratio = if self.max_iterations > 0 {
            self.iterations as f64 / self.max_iterations as f64
        } else {
            0.0
        };
        
        let _tool_success_rate = if self.successful_tool_calls + self.failed_tool_calls > 0 {
            self.successful_tool_calls as f64 / (self.successful_tool_calls + self.failed_tool_calls) as f64
        } else {
            0.0
        };
        
        let budget_used_ratio = if self.budget_total_cents > 0 {
            self.cost_spent_cents as f64 / self.budget_total_cents as f64
        } else {
            0.0
        };
        
        // Detect capability issues
        let capability_score = self.calculate_capability_score(&mut evidence);
        
        // Detect progress indicators
        let progress_score = self.calculate_progress_score(&mut evidence);
        
        // Determine failure mode
        let (mode, confidence) = if self.has_external_error() {
            evidence.push("External error detected in output".to_string());
            (FailureMode::ExternalError, 0.9)
        } else if capability_score < 0.3 && !self.partial_progress {
            (FailureMode::ModelCapabilityInsufficient, capability_score.abs())
        } else if progress_score > 0.6 && budget_used_ratio > 0.8 {
            evidence.push(format!("High progress score ({:.2}) with budget mostly used ({:.0}%)", 
                progress_score, budget_used_ratio * 100.0));
            (FailureMode::BudgetExhaustedWithProgress, progress_score)
        } else if iteration_ratio > 0.9 && progress_score > 0.4 {
            evidence.push("Max iterations reached while making progress".to_string());
            (FailureMode::BudgetExhaustedWithProgress, 0.7)
        } else if capability_score < 0.5 {
            (FailureMode::ModelCapabilityInsufficient, 0.6)
        } else {
            (FailureMode::Unknown, 0.4)
        };
        
        // Generate recommendation based on mode
        let recommendation = self.recommend_action(mode, progress_score, capability_score);
        
        FailureAnalysis {
            mode,
            confidence,
            evidence,
            recommendation,
        }
    }
    
    /// Calculate a score indicating model capability (higher = more capable).
    fn calculate_capability_score(&self, evidence: &mut Vec<String>) -> f64 {
        let mut score: f64 = 0.5; // Start neutral
        
        // Repetitive actions suggest the model is stuck
        if self.repetitive_actions {
            score -= 0.3;
            evidence.push("Model repeating same actions (stuck)".to_string());
        }
        
        // High tool failure rate suggests capability issues
        let tool_success_rate: f64 = if self.successful_tool_calls + self.failed_tool_calls > 0 {
            self.successful_tool_calls as f64 / (self.successful_tool_calls + self.failed_tool_calls) as f64
        } else {
            0.5
        };
        
        if tool_success_rate < 0.5 {
            score -= 0.2;
            evidence.push(format!("Low tool success rate: {:.0}%", tool_success_rate * 100.0));
        } else if tool_success_rate > 0.8 {
            score += 0.2;
            evidence.push(format!("High tool success rate: {:.0}%", tool_success_rate * 100.0));
        }
        
        // Error messages in output suggest problems
        if self.has_error_messages {
            score -= 0.15;
            evidence.push("Error messages present in output".to_string());
        }
        
        // Files modified suggests productive work
        if self.files_modified {
            score += 0.15;
            evidence.push("Files were created/modified (productive work)".to_string());
        }
        
        score.clamp(0.0, 1.0)
    }
    
    /// Calculate a score indicating progress (higher = more progress).
    fn calculate_progress_score(&self, evidence: &mut Vec<String>) -> f64 {
        let mut score: f64 = 0.0;
        
        // Files modified is strong progress signal
        if self.files_modified {
            score += 0.3;
        }
        
        // Successful tool calls indicate progress
        if self.successful_tool_calls > 0 {
            let tool_factor: f64 = (self.successful_tool_calls as f64 / 10.0).min(0.3);
            score += tool_factor;
            evidence.push(format!("{} successful tool calls", self.successful_tool_calls));
        }
        
        // Partial progress flag
        if self.partial_progress {
            score += 0.2;
            evidence.push("Partial progress detected".to_string());
        }
        
        // Not stuck in a loop
        if !self.repetitive_actions {
            score += 0.1;
        }
        
        score.clamp(0.0, 1.0)
    }
    
    /// Check if there's an external/API error.
    fn has_external_error(&self) -> bool {
        let output_lower = self.final_output.to_lowercase();
        output_lower.contains("api error")
            || output_lower.contains("network error")
            || output_lower.contains("connection refused")
            || output_lower.contains("timeout")
            || output_lower.contains("rate limit")
            || output_lower.contains("429")
            || output_lower.contains("too many requests")
            || output_lower.contains("rate limited")
            || output_lower.contains("server error")
            || output_lower.contains("502")
            || output_lower.contains("503")
            || output_lower.contains("504")
    }

    /// Check if this is specifically a rate limit error.
    pub fn is_rate_limit_error(&self) -> bool {
        let output_lower = self.final_output.to_lowercase();
        output_lower.contains("rate limit")
            || output_lower.contains("429")
            || output_lower.contains("too many requests")
            || output_lower.contains("rate limited")
    }
    
    /// Generate a retry recommendation based on analysis.
    fn recommend_action(
        &self,
        mode: FailureMode,
        progress_score: f64,
        capability_score: f64,
    ) -> RetryRecommendation {
        match mode {
            FailureMode::BudgetExhaustedWithProgress => {
                // Model is capable, just needs more resources
                let additional = self.estimate_additional_budget(progress_score);
                
                if capability_score > 0.7 {
                    // Very capable, might even use cheaper model
                    RetryRecommendation::TryCheaperModel {
                        suggested_model: self.suggest_cheaper_model(),
                        additional_budget_cents: additional,
                        reason: format!(
                            "Task was progressing well (score: {:.2}). \
                             A cheaper model may complete it with more budget.",
                            progress_score
                        ),
                    }
                } else {
                    RetryRecommendation::ContinueSameModel {
                        additional_budget_cents: additional,
                        reason: format!(
                            "Task was making progress (score: {:.2}). \
                             Continue with same model and additional budget.",
                            progress_score
                        ),
                    }
                }
            }
            
            FailureMode::ModelCapabilityInsufficient => {
                // Model is struggling, need smarter model
                let additional = self.cost_spent_cents; // Budget similar to what was spent
                
                RetryRecommendation::UpgradeModel {
                    suggested_model: self.suggest_smarter_model(),
                    additional_budget_cents: additional.max(50), // At least 50 cents
                    reason: format!(
                        "Model appears to lack capability for this task \
                         (capability score: {:.2}). Upgrading to smarter model.",
                        capability_score
                    ),
                }
            }
            
            FailureMode::ExternalError => {
                // Check if it's a rate limit error
                if self.is_rate_limit_error() {
                    // Rate limit - try a different model to avoid hitting the same limit
                    RetryRecommendation::TryCheaperModel {
                        suggested_model: self.suggest_alternative_model(),
                        additional_budget_cents: self.cost_spent_cents,
                        reason: "Rate limited by provider. Trying alternative model to avoid same limit.".to_string(),
                    }
                } else {
                    // Other external issue, retry with same setup
                    RetryRecommendation::ContinueSameModel {
                        additional_budget_cents: self.cost_spent_cents / 2, // Less budget needed for retry
                        reason: "External error occurred. Retry with same configuration.".to_string(),
                    }
                }
            }
            
            FailureMode::TaskInfeasible => {
                RetryRecommendation::DoNotRetry {
                    reason: "Task appears to be infeasible or ill-defined.".to_string(),
                }
            }
            
            FailureMode::Unknown => {
                // Uncertain - request human decision
                RetryRecommendation::RequestExtension {
                    estimated_additional_cents: self.cost_spent_cents,
                    reason: format!(
                        "Uncertain failure mode. Progress: {:.2}, Capability: {:.2}. \
                         Manual review recommended.",
                        progress_score, capability_score
                    ),
                }
            }
        }
    }
    
    /// Estimate additional budget needed to complete the task.
    fn estimate_additional_budget(&self, progress_score: f64) -> u64 {
        if progress_score > 0.8 {
            // Almost done, need just a bit more
            (self.cost_spent_cents as f64 * 0.3).ceil() as u64
        } else if progress_score > 0.5 {
            // Halfway there
            (self.cost_spent_cents as f64 * 0.6).ceil() as u64
        } else {
            // Early stages, might need similar amount
            self.cost_spent_cents
        }
    }
    
    /// Suggest a cheaper model based on current model.
    fn suggest_cheaper_model(&self) -> Option<String> {
        // Model upgrade/downgrade ladder
        match self.model_used.as_str() {
            "anthropic/claude-sonnet-4.5" | "anthropic/claude-sonnet-4" => {
                Some("anthropic/claude-haiku-4.5".to_string())
            }
            "anthropic/claude-3.5-sonnet" | "anthropic/claude-3.7-sonnet" => {
                Some("anthropic/claude-3.5-haiku".to_string())
            }
            "openai/gpt-4o" => {
                Some("openai/gpt-4o-mini".to_string())
            }
            _ => None,
        }
    }
    
    /// Suggest a smarter model based on current model.
    fn suggest_smarter_model(&self) -> Option<String> {
        match self.model_used.as_str() {
            "anthropic/claude-haiku-4.5" | "anthropic/claude-3.5-haiku" | "anthropic/claude-3-haiku" => {
                Some("anthropic/claude-sonnet-4.5".to_string())
            }
            "openai/gpt-4o-mini" => {
                Some("openai/gpt-4o".to_string())
            }
            "google/gemini-2.0-flash-001" => {
                Some("anthropic/claude-sonnet-4.5".to_string())
            }
            // Already using top-tier model
            "anthropic/claude-sonnet-4.5" | "anthropic/claude-sonnet-4" | "openai/gpt-4o" => {
                None // Already at top tier
            }
            _ => {
                // Default upgrade path
                Some("anthropic/claude-sonnet-4.5".to_string())
            }
        }
    }

    /// Suggest an alternative model from a different provider (useful for rate limits).
    fn suggest_alternative_model(&self) -> Option<String> {
        // When rate limited, try a model from a different provider
        match self.model_used.as_str() {
            // Anthropic -> OpenAI
            m if m.starts_with("anthropic/") => {
                Some("openai/gpt-4o-mini".to_string())
            }
            // OpenAI -> Google
            m if m.starts_with("openai/") => {
                Some("google/gemini-2.0-flash-001".to_string())
            }
            // Google -> Anthropic
            m if m.starts_with("google/") => {
                Some("anthropic/claude-haiku-4.5".to_string())
            }
            // Mistral -> Anthropic (particularly for free tier rate limits)
            m if m.starts_with("mistralai/") || m.contains("mistral") => {
                Some("anthropic/claude-haiku-4.5".to_string())
            }
            // Default: try Anthropic
            _ => {
                Some("anthropic/claude-haiku-4.5".to_string())
            }
        }
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,
    
    /// Maximum additional budget to allocate (as multiplier of original)
    pub max_budget_multiplier: f64,
    
    /// Whether to allow automatic model upgrades
    pub allow_model_upgrade: bool,
    
    /// Whether to allow automatic model downgrades
    pub allow_model_downgrade: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            max_budget_multiplier: 3.0,
            allow_model_upgrade: true,
            allow_model_downgrade: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_exhausted_with_progress() {
        let signals = ExecutionSignals {
            iterations: 45,
            max_iterations: 50,
            successful_tool_calls: 12,
            failed_tool_calls: 1,
            files_modified: true,
            repetitive_actions: false,
            has_error_messages: false,
            partial_progress: true,
            cost_spent_cents: 80,
            budget_total_cents: 100,
            final_output: "Budget exhausted before completion".to_string(),
            model_used: "anthropic/claude-sonnet-4.5".to_string(),
        };
        
        let analysis = signals.analyze();
        assert_eq!(analysis.mode, FailureMode::BudgetExhaustedWithProgress);
        
        // Should recommend cheaper model or continue
        match analysis.recommendation {
            RetryRecommendation::TryCheaperModel { .. } |
            RetryRecommendation::ContinueSameModel { .. } => {}
            _ => panic!("Expected cheaper model or continue recommendation"),
        }
    }

    #[test]
    fn test_capability_insufficient() {
        let signals = ExecutionSignals {
            iterations: 30,
            max_iterations: 50,
            successful_tool_calls: 2,
            failed_tool_calls: 8,
            files_modified: false,
            repetitive_actions: true,
            has_error_messages: true,
            partial_progress: false,
            cost_spent_cents: 50,
            budget_total_cents: 100,
            final_output: "Unable to complete task".to_string(),
            model_used: "anthropic/claude-haiku-4.5".to_string(),
        };
        
        let analysis = signals.analyze();
        assert_eq!(analysis.mode, FailureMode::ModelCapabilityInsufficient);
        
        // Should recommend model upgrade
        match analysis.recommendation {
            RetryRecommendation::UpgradeModel { suggested_model, .. } => {
                assert!(suggested_model.is_some());
            }
            _ => panic!("Expected upgrade model recommendation"),
        }
    }
}

