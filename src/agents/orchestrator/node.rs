//! Node agent - intermediate orchestrator in the agent tree.
//!
//! Node agents are like mini-root agents that can:
//! - Receive delegated tasks from parent
//! - Estimate complexity and split complex subtasks further (recursive)
//! - Delegate to their own children
//! - Aggregate results for parent

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::agents::{
    Agent, AgentContext, AgentId, AgentRef, AgentResult, AgentType, Complexity, OrchestratorAgent,
    leaf::{ComplexityEstimator, ModelSelector, TaskExecutor, Verifier},
};
use crate::budget::Budget;
use crate::llm::{ChatMessage, Role};
use crate::task::{Task, Subtask, SubtaskPlan, VerificationCriteria};

/// Node agent - intermediate orchestrator.
/// 
/// # Purpose
/// Handles subtasks that may still be complex enough
/// to warrant further splitting. Now with full recursive
/// splitting capabilities like RootAgent.
/// 
/// # Recursive Splitting
/// NodeAgent can estimate complexity of its subtasks and
/// recursively split them if they're still too complex,
/// respecting the `max_split_depth` in context.
pub struct NodeAgent {
    id: AgentId,
    
    /// Name for identification in logs
    name: String,
    
    // Child agents - full pipeline for recursive splitting
    complexity_estimator: Arc<ComplexityEstimator>,
    model_selector: Arc<ModelSelector>,
    task_executor: Arc<TaskExecutor>,
    verifier: Arc<Verifier>,
    
    // Child node agents (for further splitting)
    child_nodes: Vec<Arc<NodeAgent>>,
}

impl NodeAgent {
    /// Create a new node agent with full recursive capabilities.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: AgentId::new(),
            name: name.into(),
            complexity_estimator: Arc::new(ComplexityEstimator::new()),
            model_selector: Arc::new(ModelSelector::new()),
            task_executor: Arc::new(TaskExecutor::new()),
            verifier: Arc::new(Verifier::new()),
            child_nodes: Vec::new(),
        }
    }

    /// Create a node with custom executor.
    pub fn with_executor(mut self, executor: Arc<TaskExecutor>) -> Self {
        self.task_executor = executor;
        self
    }

    /// Add a child node for hierarchical delegation.
    pub fn add_child_node(&mut self, child: Arc<NodeAgent>) {
        self.child_nodes.push(child);
    }

    /// Get the node's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Estimate complexity of a task.
    async fn estimate_complexity(&self, task: &mut Task, ctx: &AgentContext) -> Complexity {
        let result = self.complexity_estimator.execute(task, ctx).await;
        
        if let Some(data) = result.data {
            let score = data["score"].as_f64().unwrap_or(0.5);
            let reasoning = data["reasoning"].as_str().unwrap_or("").to_string();
            let estimated_tokens = data["estimated_tokens"].as_u64().unwrap_or(2000);
            let should_split = data["should_split"].as_bool().unwrap_or(false);
            
            Complexity::new(score, reasoning, estimated_tokens)
                .with_split(should_split)
        } else {
            Complexity::moderate("Could not estimate complexity")
        }
    }

    /// Split a complex task into subtasks.
    async fn split_task(&self, task: &Task, ctx: &AgentContext) -> Result<SubtaskPlan, AgentResult> {
        let prompt = format!(
            r#"You are a task planner. Break down this task into smaller, manageable subtasks.

Task: {}

Respond with a JSON object:
{{
    "subtasks": [
        {{
            "description": "What to do",
            "verification": "How to verify it's done",
            "weight": 1.0,
            "dependencies": []
        }}
    ],
    "reasoning": "Why this breakdown makes sense"
}}

Guidelines:
- Each subtask should be independently executable once its dependencies are complete
- The "dependencies" array contains indices (0-based) of subtasks that MUST complete before this one can start
- For example, if subtask 2 needs subtask 0's output, set "dependencies": [0]
- Include verification for each subtask
- Weight indicates relative effort (higher = more work)
- Keep subtasks focused and specific
- Aim for 2-4 subtasks typically
- IMPORTANT: If subtasks have a logical order (e.g., download before analyze), specify dependencies!

PREFER COMMAND-LINE APPROACHES:
- For downloading files: use curl/wget, NOT browser automation
- For Chrome extensions: download CRX directly via URL pattern, then unzip
- For file analysis: use grep/find/ripgrep, NOT GUI tools
- For web APIs: use curl/fetch_url, NOT browser clicks
- Desktop automation is a LAST RESORT only when no CLI option exists

Respond ONLY with the JSON object."#,
            task.description()
        );

        let messages = vec![
            ChatMessage::new(Role::System, "You are a precise task planner. Respond only with JSON."),
            ChatMessage::new(Role::User, prompt),
        ];

        let response = ctx.llm
            .chat_completion("openai/gpt-4.1-mini", &messages, None)
            .await
            .map_err(|e| AgentResult::failure(format!("LLM error: {}", e), 1))?;

        let content = response.content.unwrap_or_default();
        self.parse_subtask_plan(&content, task.id())
    }

    /// Extract JSON from LLM response (handles markdown code blocks).
    fn extract_json(response: &str) -> String {
        let trimmed = response.trim();
        
        // Check for markdown code block
        if trimmed.starts_with("```") {
            // Find the end of the opening fence
            if let Some(start_idx) = trimmed.find('\n') {
                let after_fence = &trimmed[start_idx + 1..];
                // Find the closing fence
                if let Some(end_idx) = after_fence.rfind("```") {
                    return after_fence[..end_idx].trim().to_string();
                }
            }
        }
        
        // Try to find JSON object in the response
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if end > start {
                    return trimmed[start..=end].to_string();
                }
            }
        }
        
        // Return as-is if no extraction needed
        trimmed.to_string()
    }

    /// Parse LLM response into SubtaskPlan.
    fn parse_subtask_plan(
        &self,
        response: &str,
        parent_id: crate::task::TaskId,
    ) -> Result<SubtaskPlan, AgentResult> {
        let extracted = Self::extract_json(response);
        let json: serde_json::Value = serde_json::from_str(&extracted)
            .map_err(|e| AgentResult::failure(format!("Failed to parse subtasks: {} (raw: {}...)", e, response.chars().take(100).collect::<String>()), 0))?;

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
                        
                        // Parse dependencies array
                        let dependencies: Vec<usize> = s["dependencies"]
                            .as_array()
                            .map(|deps| {
                                deps.iter()
                                    .filter_map(|d| d.as_u64().map(|n| n as usize))
                                    .collect()
                            })
                            .unwrap_or_default();
                        
                        Subtask::new(
                            desc,
                            VerificationCriteria::llm_based(verification),
                            weight,
                        ).with_dependencies(dependencies)
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

    /// Execute subtasks recursively, potentially splitting further.
    async fn execute_subtasks(
        &self,
        subtask_plan: SubtaskPlan,
        parent_budget: &Budget,
        ctx: &AgentContext,
        requested_model: Option<&str>,
    ) -> AgentResult {
        // Convert plan to tasks
        let mut tasks = match subtask_plan.into_tasks(parent_budget) {
            Ok(t) => t,
            Err(e) => return AgentResult::failure(format!("Failed to create subtasks: {}", e), 0),
        };

        // Propagate requested_model to all subtasks
        if let Some(model) = requested_model {
            for task in &mut tasks {
                task.analysis_mut().requested_model = Some(model.to_string());
            }
        }

        let mut results = Vec::new();
        let mut total_cost = 0u64;

        // Create a child context with reduced split depth
        let child_ctx = ctx.child_context();

        // Execute each subtask recursively
        for task in &mut tasks {
            tracing::info!(
                "NodeAgent '{}' processing subtask: {}",
                self.name,
                task.description().chars().take(80).collect::<String>()
            );

            // Create a child NodeAgent for this subtask (recursive)
            let child_node = NodeAgent::new(format!("{}-sub", self.name));
            
            // Execute through the child node (which may split further)
            let result = child_node.execute(task, &child_ctx).await;
            total_cost += result.cost_cents;

            results.push(result);
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

    /// Execute with tree updates for visualization.
    /// This method updates the parent's tree structure as this node executes.
    pub async fn execute_with_tree(
        &self,
        task: &mut Task,
        ctx: &AgentContext,
        node_id: &str,
        root_tree: &mut crate::api::control::AgentTreeNode,
        emit_ctx: &AgentContext,
    ) -> AgentResult {
        use crate::api::control::AgentTreeNode;
        
        let mut total_cost = 0u64;

        tracing::info!(
            "NodeAgent '{}' executing task (depth remaining: {}): {}",
            self.name,
            ctx.max_split_depth,
            task.description().chars().take(80).collect::<String>()
        );

        // Step 1: Estimate complexity
        ctx.emit_phase("estimating_complexity", Some("Analyzing subtask..."), Some(&self.name));
        let complexity = self.estimate_complexity(task, ctx).await;
        total_cost += 1;

        // Update node with complexity
        if let Some(node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
            node.complexity = Some(complexity.score());
        }
        emit_ctx.emit_tree(root_tree.clone());

        tracing::info!(
            "NodeAgent '{}' complexity: {:.2} (should_split: {}, can_split: {})",
            self.name,
            complexity.score(),
            complexity.should_split(),
            ctx.can_split()
        );

        // Step 2: Decide execution strategy
        if complexity.should_split() && ctx.can_split() {
            ctx.emit_phase("splitting_task", Some("Decomposing subtask..."), Some(&self.name));
            tracing::info!("NodeAgent '{}' splitting task into sub-subtasks", self.name);

            match self.split_task(task, ctx).await {
                Ok(plan) => {
                    total_cost += 2;
                    
                    // Add child nodes to this node in the tree
                    if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
                        for (i, subtask) in plan.subtasks().iter().enumerate() {
                            let child_node = AgentTreeNode::new(
                                &format!("{}-sub-{}", node_id, i + 1),
                                "Node",
                                &format!("Sub-subtask {}", i + 1),
                                &subtask.description.chars().take(40).collect::<String>(),
                            )
                            .with_status("pending");
                            parent_node.children.push(child_node);
                        }
                    }
                    emit_ctx.emit_tree(root_tree.clone());
                    
                    let subtask_count = plan.subtasks().len();
                    tracing::info!(
                        "NodeAgent '{}' created {} sub-subtasks",
                        self.name,
                        subtask_count
                    );

                    // Execute subtasks recursively with tree updates
                    let child_ctx = ctx.child_context();
                    let requested_model = task.analysis().requested_model.as_deref();
                    let result = self.execute_subtasks_with_tree(plan, task.budget(), &child_ctx, node_id, root_tree, emit_ctx, requested_model).await;
                    
                    return AgentResult {
                        success: result.success,
                        output: result.output,
                        cost_cents: total_cost + result.cost_cents,
                        model_used: result.model_used,
                        data: result.data,
                    };
                }
                Err(e) => {
                    tracing::warn!(
                        "NodeAgent '{}' couldn't split, executing directly: {}",
                        self.name,
                        e.output
                    );
                }
            }
        }

        // Simple task: add child nodes for executor and verifier
        if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
            parent_node.children.push(
                AgentTreeNode::new(
                    &format!("{}-executor", node_id),
                    "TaskExecutor",
                    "Task Executor",
                    "Execute subtask",
                )
                .with_status("running")
            );
            parent_node.children.push(
                AgentTreeNode::new(
                    &format!("{}-verifier", node_id),
                    "Verifier",
                    "Verifier",
                    "Verify result",
                )
                .with_status("pending")
            );
        }
        emit_ctx.emit_tree(root_tree.clone());

        // Select model
        ctx.emit_phase("selecting_model", Some("Choosing model..."), Some(&self.name));
        let sel_result = self.model_selector.execute(task, ctx).await;
        total_cost += sel_result.cost_cents;

        // Execute
        ctx.emit_phase("executing", Some("Running subtask..."), Some(&self.name));
        let result = self.task_executor.execute(task, ctx).await;
        total_cost += result.cost_cents;

        // Update executor status
        if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
            if let Some(exec_node) = parent_node.children.iter_mut().find(|n| n.id == format!("{}-executor", node_id)) {
                exec_node.status = if result.success { "completed".to_string() } else { "failed".to_string() };
                exec_node.budget_spent = result.cost_cents;
            }
        }
        emit_ctx.emit_tree(root_tree.clone());

        // Store the executor output for verification
        task.set_last_output(result.output.clone());

        if !result.success {
            return AgentResult::failure(result.output, total_cost)
                .with_data(json!({
                    "node_name": self.name,
                    "complexity": complexity.score(),
                    "was_split": false,
                    "execution": result.data,
                }));
        }

        // Verify
        if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
            if let Some(ver_node) = parent_node.children.iter_mut().find(|n| n.id == format!("{}-verifier", node_id)) {
                ver_node.status = "running".to_string();
            }
        }
        emit_ctx.emit_tree(root_tree.clone());

        ctx.emit_phase("verifying", Some("Checking results..."), Some(&self.name));
        let verification = self.verifier.execute(task, ctx).await;
        total_cost += verification.cost_cents;

        // Update verifier status
        if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == node_id) {
            if let Some(ver_node) = parent_node.children.iter_mut().find(|n| n.id == format!("{}-verifier", node_id)) {
                ver_node.status = if verification.success { "completed".to_string() } else { "failed".to_string() };
                ver_node.budget_spent = verification.cost_cents;
            }
        }
        emit_ctx.emit_tree(root_tree.clone());

        if verification.success {
            AgentResult::success(result.output, total_cost)
                .with_model(result.model_used.unwrap_or_default())
                .with_data(json!({
                    "node_name": self.name,
                    "complexity": complexity.score(),
                    "was_split": false,
                    "execution": result.data,
                    "verification": verification.data,
                }))
        } else {
            AgentResult::failure(
                format!(
                    "Task completed but verification failed: {}",
                    verification.output
                ),
                total_cost,
            )
            .with_data(json!({
                "node_name": self.name,
                "complexity": complexity.score(),
                "was_split": false,
                "execution": result.data,
                "verification": verification.data,
            }))
        }
    }

    /// Execute subtasks with tree updates for visualization.
    async fn execute_subtasks_with_tree(
        &self,
        subtask_plan: SubtaskPlan,
        parent_budget: &Budget,
        ctx: &AgentContext,
        parent_node_id: &str,
        root_tree: &mut crate::api::control::AgentTreeNode,
        emit_ctx: &AgentContext,
        requested_model: Option<&str>,
    ) -> AgentResult {
        let mut tasks = match subtask_plan.into_tasks(parent_budget) {
            Ok(t) => t,
            Err(e) => return AgentResult::failure(format!("Failed to create subtasks: {}", e), 0),
        };

        // Propagate requested_model to all subtasks
        if let Some(model) = requested_model {
            for task in &mut tasks {
                task.analysis_mut().requested_model = Some(model.to_string());
            }
        }

        let mut results = Vec::new();
        let mut total_cost = 0u64;
        let child_ctx = ctx.child_context();

        for (i, task) in tasks.iter_mut().enumerate() {
            let subtask_id = format!("{}-sub-{}", parent_node_id, i + 1);
            
            // Update subtask status to running
            if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == parent_node_id) {
                if let Some(child_node) = parent_node.children.iter_mut().find(|n| n.id == subtask_id) {
                    child_node.status = "running".to_string();
                }
            }
            emit_ctx.emit_tree(root_tree.clone());

            tracing::info!(
                "NodeAgent '{}' processing sub-subtask: {}",
                self.name,
                task.description().chars().take(80).collect::<String>()
            );

            // Create and execute a child NodeAgent
            let child_node_agent = NodeAgent::new(subtask_id.clone());
            let result = child_node_agent.execute(task, &child_ctx).await;
            total_cost += result.cost_cents;

            // Update subtask status
            if let Some(parent_node) = root_tree.children.iter_mut().find(|n| n.id == parent_node_id) {
                if let Some(child_node) = parent_node.children.iter_mut().find(|n| n.id == subtask_id) {
                    child_node.status = if result.success { "completed".to_string() } else { "failed".to_string() };
                    child_node.budget_spent = result.cost_cents;
                }
            }
            emit_ctx.emit_tree(root_tree.clone());

            results.push(result);
        }

        let successes = results.iter().filter(|r| r.success).count();
        let total = results.len();

        if successes == total {
            AgentResult::success(
                format!("All {} sub-subtasks completed successfully", total),
                total_cost,
            )
            .with_data(json!({
                "subtasks_total": total,
                "subtasks_succeeded": successes,
            }))
        } else {
            AgentResult::failure(
                format!("{}/{} sub-subtasks succeeded", successes, total),
                total_cost,
            )
            .with_data(json!({
                "subtasks_total": total,
                "subtasks_succeeded": successes,
            }))
        }
    }
}

impl Default for NodeAgent {
    fn default() -> Self {
        Self::new("node")
    }
}

#[async_trait]
impl Agent for NodeAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn agent_type(&self) -> AgentType {
        AgentType::Node
    }

    fn description(&self) -> &str {
        "Intermediate orchestrator with recursive splitting capabilities"
    }

    async fn execute(&self, task: &mut Task, ctx: &AgentContext) -> AgentResult {
        let mut total_cost = 0u64;

        tracing::info!(
            "NodeAgent '{}' executing task (depth remaining: {}): {}",
            self.name,
            ctx.max_split_depth,
            task.description().chars().take(80).collect::<String>()
        );

        // Step 1: Estimate complexity
        ctx.emit_phase("estimating_complexity", Some("Analyzing subtask..."), Some(&self.name));
        let complexity = self.estimate_complexity(task, ctx).await;
        total_cost += 1;

        tracing::info!(
            "NodeAgent '{}' complexity: {:.2} (should_split: {}, can_split: {})",
            self.name,
            complexity.score(),
            complexity.should_split(),
            ctx.can_split()
        );

        // Step 2: Decide execution strategy
        if complexity.should_split() && ctx.can_split() {
            // Complex subtask: split further recursively
            ctx.emit_phase("splitting_task", Some("Decomposing subtask..."), Some(&self.name));
            tracing::info!("NodeAgent '{}' splitting task into sub-subtasks", self.name);

            match self.split_task(task, ctx).await {
                Ok(plan) => {
                    total_cost += 2; // Splitting cost
                    
                    let subtask_count = plan.subtasks().len();
                    tracing::info!(
                        "NodeAgent '{}' created {} sub-subtasks",
                        self.name,
                        subtask_count
                    );

                    // Execute subtasks recursively
                    let requested_model = task.analysis().requested_model.as_deref();
                    let result = self.execute_subtasks(plan, task.budget(), ctx, requested_model).await;
                    
                    return AgentResult {
                        success: result.success,
                        output: result.output,
                        cost_cents: total_cost + result.cost_cents,
                        model_used: result.model_used,
                        data: result.data,
                    };
                }
                Err(e) => {
                    tracing::warn!(
                        "NodeAgent '{}' couldn't split, executing directly: {}",
                        self.name,
                        e.output
                    );
                }
            }
        }

        // Simple task or failed to split: execute directly
        // Select model
        ctx.emit_phase("selecting_model", Some("Choosing model..."), Some(&self.name));
        let sel_result = self.model_selector.execute(task, ctx).await;
        total_cost += sel_result.cost_cents;

        // Execute
        ctx.emit_phase("executing", Some("Running subtask..."), Some(&self.name));
        let result = self.task_executor.execute(task, ctx).await;
        total_cost += result.cost_cents;

        // Store the executor output for verification
        task.set_last_output(result.output.clone());

        if !result.success {
            return AgentResult::failure(result.output, total_cost)
                .with_data(json!({
                    "node_name": self.name,
                    "complexity": complexity.score(),
                    "was_split": false,
                    "execution": result.data,
                }));
        }

        // Verify
        ctx.emit_phase("verifying", Some("Checking results..."), Some(&self.name));
        let verification = self.verifier.execute(task, ctx).await;
        total_cost += verification.cost_cents;

        if verification.success {
            AgentResult::success(result.output, total_cost)
                .with_model(result.model_used.unwrap_or_default())
                .with_data(json!({
                    "node_name": self.name,
                    "complexity": complexity.score(),
                    "was_split": false,
                    "execution": result.data,
                    "verification": verification.data,
                }))
        } else {
            AgentResult::failure(
                format!(
                    "Task completed but verification failed: {}",
                    verification.output
                ),
                total_cost,
            )
            .with_data(json!({
                "node_name": self.name,
                "complexity": complexity.score(),
                "was_split": false,
                "execution": result.data,
                "verification": verification.data,
            }))
        }
    }
}

#[async_trait]
impl OrchestratorAgent for NodeAgent {
    fn children(&self) -> Vec<AgentRef> {
        let mut children: Vec<AgentRef> = vec![
            Arc::clone(&self.complexity_estimator) as AgentRef,
            Arc::clone(&self.model_selector) as AgentRef,
            Arc::clone(&self.task_executor) as AgentRef,
            Arc::clone(&self.verifier) as AgentRef,
        ];

        for node in &self.child_nodes {
            children.push(Arc::clone(node) as AgentRef);
        }

        children
    }

    fn find_child(&self, agent_type: AgentType) -> Option<AgentRef> {
        match agent_type {
            AgentType::ComplexityEstimator => Some(Arc::clone(&self.complexity_estimator) as AgentRef),
            AgentType::ModelSelector => Some(Arc::clone(&self.model_selector) as AgentRef),
            AgentType::TaskExecutor => Some(Arc::clone(&self.task_executor) as AgentRef),
            AgentType::Verifier => Some(Arc::clone(&self.verifier) as AgentRef),
            AgentType::Node => self.child_nodes.first().map(|n| Arc::clone(n) as AgentRef),
            _ => None,
        }
    }

    async fn delegate(&self, task: &mut Task, child: AgentRef, ctx: &AgentContext) -> AgentResult {
        child.execute(task, ctx).await
    }

    async fn delegate_all(&self, tasks: &mut [Task], ctx: &AgentContext) -> Vec<AgentResult> {
        let mut results = Vec::with_capacity(tasks.len());

        for task in tasks {
            // Use recursive execution for each task
            let result = self.execute(task, ctx).await;
            results.push(result);
        }

        results
    }
}

