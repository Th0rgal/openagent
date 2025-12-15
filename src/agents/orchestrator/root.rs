//! Root agent - top-level orchestrator of the agent tree.
//!
//! # Responsibilities
//! 1. Receive tasks from the API
//! 2. Estimate complexity
//! 3. Decide: execute directly or split into subtasks
//! 4. Delegate to appropriate children
//! 5. Aggregate results
//! 6. Handle failures with smart retry strategy

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::agents::tuning::TuningParams;
use crate::agents::{
    leaf::{ComplexityEstimator, ModelSelector, TaskExecutor, Verifier},
    Agent, AgentContext, AgentId, AgentRef, AgentResult, AgentType, Complexity, OrchestratorAgent,
};
use crate::budget::{Budget, RetryConfig, RetryRecommendation};
use crate::task::{Subtask, SubtaskPlan, Task, VerificationCriteria};

/// Root agent - the top of the agent tree.
///
/// # Task Processing Flow
/// ```text
/// 1. Estimate complexity (ComplexityEstimator)
/// 2. If simple: execute directly (TaskExecutor)
/// 3. If complex:
///    a. Split into subtasks (LLM-based)
///    b. Select model for each subtask (ModelSelector)
///    c. Execute subtasks (TaskExecutor)
///    d. Verify results (Verifier)
/// 4. Return aggregated result
/// ```
pub struct RootAgent {
    id: AgentId,

    // Child agents
    complexity_estimator: Arc<ComplexityEstimator>,
    model_selector: Arc<ModelSelector>,
    task_executor: Arc<TaskExecutor>,
    verifier: Arc<Verifier>,
}

impl RootAgent {
    /// Create a new root agent with default children.
    pub fn new() -> Self {
        Self::new_with_tuning(&TuningParams::default())
    }

    /// Create a new root agent using empirically tuned parameters.
    pub fn new_with_tuning(tuning: &TuningParams) -> Self {
        Self {
            id: AgentId::new(),
            complexity_estimator: Arc::new(ComplexityEstimator::with_params(
                tuning.complexity.prompt_variant,
                tuning.complexity.split_threshold,
                tuning.complexity.token_multiplier,
            )),
            model_selector: Arc::new(ModelSelector::with_params(
                tuning.model_selector.retry_multiplier,
                tuning.model_selector.inefficiency_scale,
                tuning.model_selector.max_failure_probability,
            )),
            task_executor: Arc::new(TaskExecutor::new()),
            verifier: Arc::new(Verifier::new()),
        }
    }

    /// Split a complex task into subtasks.
    ///
    /// Uses LLM to analyze the task and propose subtasks.
    ///
    /// # Returns
    /// A tuple of (SubtaskPlan, actual_cost_cents) on success.
    async fn split_task(
        &self,
        task: &Task,
        ctx: &AgentContext,
    ) -> Result<(SubtaskPlan, u64), AgentResult> {
        let model = "anthropic/claude-sonnet-4.5";
        let prompt = format!(
            r#"You are a task planner. Break down this task into smaller, manageable subtasks.

Task: {}

Respond with a JSON object:
{{
    "subtasks": [
        {{
            "description": "What to do",
            "verification": "How to verify it's done",
            "weight": 1.0
        }}
    ],
    "reasoning": "Why this breakdown makes sense"
}}

Guidelines:
- Each subtask should be independently executable
- Include verification for each subtask
- Weight indicates relative effort (higher = more work)
- Keep subtasks focused and specific

Respond ONLY with the JSON object."#,
            task.description()
        );

        let messages = vec![
            crate::llm::ChatMessage {
                role: crate::llm::Role::System,
                content: Some(
                    "You are a precise task planner. Respond only with JSON.".to_string(),
                ),
                tool_calls: None,
                tool_call_id: None,
            },
            crate::llm::ChatMessage {
                role: crate::llm::Role::User,
                content: Some(prompt),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let response = ctx
            .llm
            .chat_completion(model, &messages, None)
            .await
            .map_err(|e| AgentResult::failure(format!("LLM error: {}", e), 1))?;

        // Calculate actual cost from token usage
        let actual_cost_cents = if let Some(usage) = &response.usage {
            if let Some(pricing) = ctx.pricing.get_pricing(model).await {
                pricing.calculate_cost_cents(usage.prompt_tokens, usage.completion_tokens)
            } else {
                // Fallback: estimate based on typical rates if pricing unavailable
                let estimated = (usage.total_tokens as f64 * 0.00001 * 100.0).ceil() as u64;
                estimated.max(1)
            }
        } else {
            // No usage data available, use conservative estimate
            2
        };

        let content = response.content.unwrap_or_default();
        let plan = self.parse_subtask_plan(&content, task.id())?;
        Ok((plan, actual_cost_cents))
    }

    /// Parse LLM response into SubtaskPlan.
    fn parse_subtask_plan(
        &self,
        response: &str,
        parent_id: crate::task::TaskId,
    ) -> Result<SubtaskPlan, AgentResult> {
        let json: serde_json::Value = serde_json::from_str(response)
            .map_err(|e| AgentResult::failure(format!("Failed to parse subtasks: {}", e), 0))?;

        let reasoning = json["reasoning"]
            .as_str()
            .unwrap_or("No reasoning provided")
            .to_string();

        let subtasks: Vec<Subtask> = json["subtasks"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|s| {
                        let desc = s["description"].as_str().unwrap_or("").to_string();
                        let verification = s["verification"].as_str().unwrap_or("");
                        let weight = s["weight"].as_f64().unwrap_or(1.0);

                        Subtask::new(desc, VerificationCriteria::llm_based(verification), weight)
                    })
                    .collect()
            })
            .unwrap_or_default();

        if subtasks.is_empty() {
            return Err(AgentResult::failure("No subtasks generated", 1));
        }

        SubtaskPlan::new(parent_id, subtasks, reasoning)
            .map_err(|e| AgentResult::failure(format!("Invalid subtask plan: {}", e), 0))
    }

    /// Execute subtasks and aggregate results with smart retry on failure.
    async fn execute_subtasks(
        &self,
        subtask_plan: SubtaskPlan,
        parent_budget: &Budget,
        ctx: &AgentContext,
    ) -> AgentResult {
        // Convert plan to tasks
        let mut tasks = match subtask_plan.into_tasks(parent_budget) {
            Ok(t) => t,
            Err(e) => return AgentResult::failure(format!("Failed to create subtasks: {}", e), 0),
        };

        let mut results = Vec::new();
        let mut total_cost = 0u64;
        let retry_config = RetryConfig::default();

        // Execute each subtask with planning + verification + smart retry.
        for task in &mut tasks {
            if ctx.is_cancelled() {
                return AgentResult::failure("Cancelled", total_cost);
            }

            let subtask_result = self
                .execute_single_subtask_with_retry(task, ctx, &retry_config)
                .await;
            total_cost += subtask_result.cost_cents;
            results.push(subtask_result);
        }

        // Aggregate results
        let successes = results.iter().filter(|r| r.success).count();
        let total = results.len();

        if successes == total {
            AgentResult::success(
                format!("All {} subtasks completed successfully", total),
                total_cost,
            )
            .with_data(json!({
                "subtasks_total": total,
                "subtasks_succeeded": successes,
                "results": results.iter().map(|r| &r.output).collect::<Vec<_>>(),
            }))
        } else {
            AgentResult::failure(
                format!("{}/{} subtasks succeeded", successes, total),
                total_cost,
            )
            .with_data(json!({
                "subtasks_total": total,
                "subtasks_succeeded": successes,
                "results": results.iter().map(|r| json!({
                    "success": r.success,
                    "output": &r.output,
                })).collect::<Vec<_>>(),
            }))
        }
    }

    /// Execute a single subtask with smart retry on failure.
    ///
    /// Analyzes failure mode and retries with appropriate strategy:
    /// - If model lacks capability: upgrade to smarter model
    /// - If task needs more tokens: continue with same/cheaper model
    async fn execute_single_subtask_with_retry(
        &self,
        task: &mut Task,
        ctx: &AgentContext,
        retry_config: &RetryConfig,
    ) -> AgentResult {
        let mut total_cost = 0u64;
        let mut retry_count = 0u32;
        let mut _last_result: Option<AgentResult> = None;
        let mut retry_history = Vec::new();

        loop {
            if ctx.is_cancelled() {
                return AgentResult::failure("Cancelled", total_cost);
            }

            // 1) Estimate complexity for this subtask.
            let est = self.complexity_estimator.execute(task, ctx).await;
            total_cost += est.cost_cents;

            // 2) Select model based on complexity + subtask budget.
            let sel = self.model_selector.execute(task, ctx).await;
            total_cost += sel.cost_cents;

            // 3) Execute with signal tracking.
            let (exec, signals) = self.task_executor.execute_with_signals(task, ctx).await;
            total_cost += exec.cost_cents;

            // 4) Verify.
            let ver = self.verifier.execute(task, ctx).await;
            total_cost += ver.cost_cents;

            let success = exec.success && ver.success;

            let result = AgentResult {
                success,
                output: if ver.success {
                    exec.output.clone()
                } else {
                    format!("{}\n\nVerification failed: {}", exec.output, ver.output)
                },
                cost_cents: est.cost_cents + sel.cost_cents + exec.cost_cents + ver.cost_cents,
                model_used: exec.model_used.clone(),
                data: Some(json!({
                    "complexity_estimate": est.data,
                    "model_selection": sel.data,
                    "execution": exec.data,
                    "verification": ver.data,
                    "retry_count": retry_count,
                    "retry_history": retry_history.clone(),
                })),
            };

            // If successful, return immediately
            if success {
                return AgentResult {
                    cost_cents: total_cost,
                    ..result
                };
            }

            // Analyze failure and decide retry strategy
            let analysis = signals.analyze();

            tracing::info!(
                "Subtask failed - mode: {:?}, confidence: {:.2}, recommendation: {:?}",
                analysis.mode,
                analysis.confidence,
                analysis.recommendation
            );

            // Check if we should retry
            if retry_count >= retry_config.max_retries {
                tracing::warn!(
                    "Max retries ({}) reached for subtask",
                    retry_config.max_retries
                );
                return AgentResult {
                    cost_cents: total_cost,
                    data: Some(json!({
                        "original_result": result.data,
                        "failure_analysis": {
                            "mode": format!("{:?}", analysis.mode),
                            "confidence": analysis.confidence,
                            "evidence": analysis.evidence,
                        },
                        "retries_exhausted": true,
                    })),
                    ..result
                };
            }

            // Apply retry strategy based on analysis
            match &analysis.recommendation {
                RetryRecommendation::UpgradeModel {
                    suggested_model,
                    additional_budget_cents,
                    reason,
                } => {
                    if !retry_config.allow_model_upgrade {
                        tracing::info!("Model upgrade disabled, not retrying");
                        return AgentResult {
                            cost_cents: total_cost,
                            ..result
                        };
                    }

                    if let Some(new_model) = suggested_model {
                        tracing::info!(
                            "Upgrading model from {} to {} - {}",
                            signals.model_used,
                            new_model,
                            reason
                        );
                        task.analysis_mut().selected_model = Some(new_model.clone());

                        // Allocate additional budget if possible
                        let additional = (*additional_budget_cents).min(
                            (task.budget().total_cents() as f64
                                * retry_config.max_budget_multiplier)
                                as u64,
                        );
                        if additional > 0 {
                            // Note: In a real system, this would request budget from parent
                            tracing::debug!(
                                "Would request {} additional cents for retry",
                                additional
                            );
                        }

                        retry_history.push(json!({
                            "retry": retry_count,
                            "action": "upgrade_model",
                            "from": signals.model_used,
                            "to": new_model,
                            "reason": reason,
                        }));
                    } else {
                        // Already at top tier, can't upgrade
                        tracing::warn!("Cannot upgrade model further, already at top tier");
                        return AgentResult {
                            cost_cents: total_cost,
                            ..result
                        };
                    }
                }

                RetryRecommendation::TryCheaperModel {
                    suggested_model,
                    additional_budget_cents,
                    reason,
                } => {
                    if !retry_config.allow_model_downgrade {
                        tracing::info!("Model downgrade disabled, using same model");
                    } else if let Some(new_model) = suggested_model {
                        tracing::info!(
                            "Trying cheaper model: {} -> {} - {}",
                            signals.model_used,
                            new_model,
                            reason
                        );
                        task.analysis_mut().selected_model = Some(new_model.clone());

                        retry_history.push(json!({
                            "retry": retry_count,
                            "action": "downgrade_model",
                            "from": signals.model_used,
                            "to": new_model,
                            "reason": reason,
                            "additional_budget": additional_budget_cents,
                        }));
                    }
                }

                RetryRecommendation::ContinueSameModel {
                    additional_budget_cents,
                    reason,
                } => {
                    tracing::info!(
                        "Continuing with same model ({}) - {}",
                        signals.model_used,
                        reason
                    );

                    retry_history.push(json!({
                        "retry": retry_count,
                        "action": "continue_same",
                        "model": signals.model_used,
                        "reason": reason,
                        "additional_budget": additional_budget_cents,
                    }));
                }

                RetryRecommendation::RequestExtension {
                    estimated_additional_cents,
                    reason,
                } => {
                    tracing::warn!(
                        "Task requires budget extension: {} cents - {}",
                        estimated_additional_cents,
                        reason
                    );
                    // For now, we don't support budget extensions, so fail
                    return AgentResult {
                        cost_cents: total_cost,
                        data: Some(json!({
                            "original_result": result.data,
                            "failure_analysis": {
                                "mode": format!("{:?}", analysis.mode),
                                "recommendation": "request_extension",
                                "estimated_additional_cents": estimated_additional_cents,
                                "reason": reason,
                            },
                        })),
                        ..result
                    };
                }

                RetryRecommendation::DoNotRetry { reason } => {
                    tracing::info!("Not retrying: {}", reason);
                    return AgentResult {
                        cost_cents: total_cost,
                        ..result
                    };
                }
            }

            _last_result = Some(result);
            retry_count += 1;
        }
    }
}

impl Default for RootAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for RootAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn agent_type(&self) -> AgentType {
        AgentType::Root
    }

    fn description(&self) -> &str {
        "Root orchestrator: estimates complexity, splits tasks, delegates execution"
    }

    async fn execute(&self, task: &mut Task, ctx: &AgentContext) -> AgentResult {
        let mut total_cost = 0u64;

        if ctx.is_cancelled() {
            return AgentResult::failure("Cancelled", total_cost);
        }

        // Step 1: Estimate complexity (cost is tracked in the result)
        let complexity_result = self.complexity_estimator.execute(task, ctx).await;
        total_cost += complexity_result.cost_cents;

        let complexity = if let Some(data) = &complexity_result.data {
            let score = data["score"].as_f64().unwrap_or(0.5);
            let reasoning = data["reasoning"].as_str().unwrap_or("").to_string();
            let estimated_tokens = data["estimated_tokens"].as_u64().unwrap_or(2000);
            let should_split = data["should_split"].as_bool().unwrap_or(false);

            Complexity::new(score, reasoning, estimated_tokens).with_split(should_split)
        } else {
            Complexity::moderate("Could not estimate complexity")
        };

        tracing::info!(
            "Task complexity: {:.2} (should_split: {}, estimation_cost: {} cents)",
            complexity.score(),
            complexity.should_split(),
            complexity_result.cost_cents
        );

        // Step 2: Decide execution strategy
        if complexity.should_split() && ctx.can_split() {
            // Complex task: split and delegate
            match self.split_task(task, ctx).await {
                Ok((plan, split_cost)) => {
                    total_cost += split_cost;
                    tracing::debug!("Task split cost: {} cents", split_cost);

                    // Execute subtasks
                    let child_ctx = ctx.child_context();
                    let result = self.execute_subtasks(plan, task.budget(), &child_ctx).await;

                    return AgentResult {
                        success: result.success,
                        output: result.output,
                        cost_cents: total_cost + result.cost_cents,
                        model_used: result.model_used,
                        data: result.data,
                    };
                }
                Err(e) => {
                    // Couldn't split, fall back to direct execution
                    tracing::warn!("Couldn't split task, executing directly: {}", e.output);
                }
            }
        }

        // Simple task or failed to split: execute directly with smart retry
        let retry_config = RetryConfig::default();
        let exec_result = self
            .execute_direct_with_retry(task, ctx, &retry_config, &complexity)
            .await;

        AgentResult {
            cost_cents: total_cost + exec_result.cost_cents,
            ..exec_result
        }
    }
}

impl RootAgent {
    /// Execute a task directly (without subtask splitting) with smart retry on failure.
    async fn execute_direct_with_retry(
        &self,
        task: &mut Task,
        ctx: &AgentContext,
        retry_config: &RetryConfig,
        complexity: &Complexity,
    ) -> AgentResult {
        let mut total_cost = 0u64;
        let mut retry_count = 0u32;
        let mut retry_history = Vec::new();

        loop {
            if ctx.is_cancelled() {
                return AgentResult::failure("Cancelled", total_cost);
            }

            // Select model (U-curve) for execution
            let sel = self.model_selector.execute(task, ctx).await;
            total_cost += sel.cost_cents;

            // Execute with signal tracking
            let (exec, signals) = self.task_executor.execute_with_signals(task, ctx).await;
            total_cost += exec.cost_cents;

            // Verify
            let verification = self.verifier.execute(task, ctx).await;
            total_cost += verification.cost_cents;

            let success = exec.success && verification.success;

            let result = AgentResult {
                success,
                output: if verification.success {
                    exec.output.clone()
                } else {
                    format!(
                        "{}\n\nVerification failed: {}",
                        exec.output, verification.output
                    )
                },
                cost_cents: total_cost,
                model_used: exec.model_used.clone(),
                data: json!({
                    "complexity": complexity.score(),
                    "was_split": false,
                    "verification": verification.data,
                    "execution": exec.data,
                    "retry_count": retry_count,
                    "retry_history": retry_history.clone(),
                })
                .into(),
            };

            // If successful, return immediately
            if success {
                return result;
            }

            // Analyze failure and decide retry strategy
            let analysis = signals.analyze();

            tracing::info!(
                "Direct execution failed - mode: {:?}, confidence: {:.2}",
                analysis.mode,
                analysis.confidence
            );

            // Check if we should retry
            if retry_count >= retry_config.max_retries {
                tracing::warn!("Max retries ({}) reached", retry_config.max_retries);
                return AgentResult {
                    data: Some(json!({
                        "original_result": result.data,
                        "failure_analysis": {
                            "mode": format!("{:?}", analysis.mode),
                            "confidence": analysis.confidence,
                            "evidence": analysis.evidence,
                        },
                        "retries_exhausted": true,
                    })),
                    ..result
                };
            }

            // Apply retry strategy based on analysis
            match &analysis.recommendation {
                RetryRecommendation::UpgradeModel {
                    suggested_model,
                    reason,
                    ..
                } => {
                    if !retry_config.allow_model_upgrade {
                        tracing::info!("Model upgrade disabled, not retrying");
                        return result;
                    }

                    if let Some(new_model) = suggested_model {
                        tracing::info!(
                            "Upgrading model: {} -> {} - {}",
                            signals.model_used,
                            new_model,
                            reason
                        );
                        task.analysis_mut().selected_model = Some(new_model.clone());

                        retry_history.push(json!({
                            "retry": retry_count,
                            "action": "upgrade_model",
                            "from": signals.model_used,
                            "to": new_model,
                            "reason": reason,
                        }));
                    } else {
                        tracing::warn!("Cannot upgrade model further");
                        return result;
                    }
                }

                RetryRecommendation::TryCheaperModel {
                    suggested_model,
                    reason,
                    ..
                } => {
                    if !retry_config.allow_model_downgrade {
                        // Continue with same model
                        tracing::info!("Model downgrade disabled, continuing with same model");
                    } else if let Some(new_model) = suggested_model {
                        tracing::info!(
                            "Trying cheaper model: {} -> {} - {}",
                            signals.model_used,
                            new_model,
                            reason
                        );
                        task.analysis_mut().selected_model = Some(new_model.clone());

                        retry_history.push(json!({
                            "retry": retry_count,
                            "action": "downgrade_model",
                            "from": signals.model_used,
                            "to": new_model,
                            "reason": reason,
                        }));
                    }
                }

                RetryRecommendation::ContinueSameModel { reason, .. } => {
                    tracing::info!("Continuing with same model - {}", reason);

                    retry_history.push(json!({
                        "retry": retry_count,
                        "action": "continue_same",
                        "model": signals.model_used,
                        "reason": reason,
                    }));
                }

                RetryRecommendation::RequestExtension {
                    estimated_additional_cents,
                    reason,
                } => {
                    tracing::warn!(
                        "Budget extension needed: {} cents - {}",
                        estimated_additional_cents,
                        reason
                    );
                    return AgentResult {
                        data: Some(json!({
                            "original_result": result.data,
                            "failure_analysis": {
                                "mode": format!("{:?}", analysis.mode),
                                "recommendation": "request_extension",
                                "estimated_additional_cents": estimated_additional_cents,
                            },
                        })),
                        ..result
                    };
                }

                RetryRecommendation::DoNotRetry { reason } => {
                    tracing::info!("Not retrying: {}", reason);
                    return result;
                }
            }

            retry_count += 1;
        }
    }
}

#[async_trait]
impl OrchestratorAgent for RootAgent {
    fn children(&self) -> Vec<AgentRef> {
        vec![
            Arc::clone(&self.complexity_estimator) as AgentRef,
            Arc::clone(&self.model_selector) as AgentRef,
            Arc::clone(&self.task_executor) as AgentRef,
            Arc::clone(&self.verifier) as AgentRef,
        ]
    }

    fn find_child(&self, agent_type: AgentType) -> Option<AgentRef> {
        match agent_type {
            AgentType::ComplexityEstimator => {
                Some(Arc::clone(&self.complexity_estimator) as AgentRef)
            }
            AgentType::ModelSelector => Some(Arc::clone(&self.model_selector) as AgentRef),
            AgentType::TaskExecutor => Some(Arc::clone(&self.task_executor) as AgentRef),
            AgentType::Verifier => Some(Arc::clone(&self.verifier) as AgentRef),
            _ => None,
        }
    }

    async fn delegate(&self, task: &mut Task, child: AgentRef, ctx: &AgentContext) -> AgentResult {
        child.execute(task, ctx).await
    }

    async fn delegate_all(&self, tasks: &mut [Task], ctx: &AgentContext) -> Vec<AgentResult> {
        let mut results = Vec::with_capacity(tasks.len());

        for task in tasks {
            let result = self.task_executor.execute(task, ctx).await;
            results.push(result);
        }

        results
    }
}
