# Learned Model Selection & Budget Estimation

## Problem Statement

Current issues:
1. **Static benchmarks are stale** - `models_with_benchmarks.json` requires manual updates and doesn't reflect real-world performance
2. **Budget estimation is naive** - Always returns ~$10 regardless of actual task complexity
3. **No learning from experience** - Agent doesn't improve model selection based on actual outcomes
4. **Model capabilities vary by use case** - Gemini excels at code audits, Qwen at reasoning, etc.

## Proposed Solution: Learning from Task Outcomes

### Core Idea

Replace static benchmarks with **learned performance metrics** derived from `task_outcomes` table. The system already records:
- `predicted_complexity` vs actual iterations
- `predicted_cost_cents` vs `actual_cost_cents`
- `selected_model` and `success` rate
- `task_type` (code, reasoning, tool_calling, etc.)

**Use this data to:**
1. Build per-model, per-task-type success rates
2. Learn actual cost distributions per task type
3. Auto-discover model capabilities from real usage

### Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Model Selection Flow                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Task → TaskType Inference → Query Learned Stats → Select Model     │
│                                    │                                 │
│                         ┌──────────┴──────────┐                     │
│                         │  model_performance  │                     │
│                         │  (Supabase view)    │                     │
│                         └──────────┬──────────┘                     │
│                                    │                                 │
│              Aggregates from task_outcomes:                         │
│              - success_rate per model per task_type                 │
│              - avg_cost_cents per task_type                         │
│              - avg_iterations per complexity bucket                 │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Database Changes

#### New Supabase View: `model_performance`

```sql
CREATE OR REPLACE VIEW model_performance AS
SELECT 
    selected_model,
    task_type,
    COUNT(*) as total_tasks,
    SUM(CASE WHEN success THEN 1 ELSE 0 END)::float / COUNT(*) as success_rate,
    AVG(actual_cost_cents) as avg_cost_cents,
    AVG(iterations) as avg_iterations,
    STDDEV(actual_cost_cents) as cost_stddev,
    PERCENTILE_CONT(0.9) WITHIN GROUP (ORDER BY actual_cost_cents) as cost_p90,
    MAX(created_at) as last_used
FROM task_outcomes
WHERE selected_model IS NOT NULL
  AND created_at > NOW() - INTERVAL '30 days'  -- Rolling window
GROUP BY selected_model, task_type
HAVING COUNT(*) >= 3;  -- Minimum sample size
```

#### New Supabase View: `budget_estimates`

```sql
CREATE OR REPLACE VIEW budget_estimates AS
SELECT 
    task_type,
    FLOOR(predicted_complexity * 10) / 10 as complexity_bucket,
    COUNT(*) as sample_count,
    AVG(actual_cost_cents) as avg_cost,
    PERCENTILE_CONT(0.8) WITHIN GROUP (ORDER BY actual_cost_cents) as cost_p80,
    AVG(iterations) as avg_iterations
FROM task_outcomes
WHERE actual_cost_cents IS NOT NULL
  AND created_at > NOW() - INTERVAL '30 days'
GROUP BY task_type, FLOOR(predicted_complexity * 10) / 10
HAVING COUNT(*) >= 5;
```

### Rust Changes

#### 1. New `LearnedModelStats` struct

```rust
// src/budget/learned.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedModelStats {
    pub model_id: String,
    pub task_type: String,
    pub total_tasks: i64,
    pub success_rate: f64,
    pub avg_cost_cents: f64,
    pub cost_p90: f64,
    pub last_used: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]  
pub struct LearnedBudgetEstimate {
    pub task_type: String,
    pub complexity_bucket: f64,
    pub avg_cost: f64,
    pub cost_p80: f64,
    pub sample_count: i64,
}
```

#### 2. New Selection Algorithm

```rust
// Pseudocode for improved model selection

pub async fn select_model(
    task: &str,
    complexity: f64,
    learned_stats: &[LearnedModelStats],
    fallback_benchmarks: &BenchmarkRegistry,  // Keep as fallback
) -> String {
    let task_type = TaskType::infer_from_description(task);
    
    // 1. Filter to models with good success rate for this task type
    let candidates: Vec<_> = learned_stats
        .iter()
        .filter(|s| s.task_type == task_type.category_key())
        .filter(|s| s.success_rate >= 0.7)  // Minimum 70% success
        .filter(|s| s.total_tasks >= 5)      // Minimum sample size
        .collect();
    
    if candidates.is_empty() {
        // Fall back to static benchmarks if no learned data
        return fallback_benchmarks.top_models(task_type, 1)
            .first()
            .map(|(id, _)| id.to_string())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    }
    
    // 2. Score by success_rate * cost_efficiency
    // Higher success + lower cost = better
    candidates
        .into_iter()
        .max_by(|a, b| {
            let score_a = a.success_rate / (a.avg_cost_cents + 1.0).ln();
            let score_b = b.success_rate / (b.avg_cost_cents + 1.0).ln();
            score_a.partial_cmp(&score_b).unwrap()
        })
        .map(|s| s.model_id.clone())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}
```

#### 3. Improved Budget Estimation

```rust
pub async fn estimate_budget(
    task: &str,
    complexity: f64,
    learned_estimates: &[LearnedBudgetEstimate],
) -> u64 {
    let task_type = TaskType::infer_from_description(task);
    let bucket = (complexity * 10.0).floor() / 10.0;
    
    // Find matching learned estimate
    if let Some(estimate) = learned_estimates
        .iter()
        .find(|e| e.task_type == task_type.category_key() 
                && (e.complexity_bucket - bucket).abs() < 0.1)
    {
        // Use P80 with 20% buffer for safety
        return (estimate.cost_p80 * 1.2).ceil() as u64;
    }
    
    // Fall back to formula-based estimate
    estimate_budget_for_complexity(complexity)
}
```

### Migration Path

1. **Phase 1: Collect Data (Passive)**
   - Current `task_outcomes` recording continues
   - No changes to selection algorithm yet
   - Build up historical data

2. **Phase 2: Add Views (No Code Change)**
   - Create `model_performance` and `budget_estimates` views
   - Verify data quality

3. **Phase 3: Hybrid Selection**
   - Query learned stats at runtime
   - Use learned data when available (>5 samples)
   - Fall back to static benchmarks otherwise

4. **Phase 4: Deprecate Static Benchmarks**
   - After 30+ days of data
   - Remove `models_with_benchmarks.json` dependency
   - Keep as emergency fallback only

### Benefits

1. **Self-Improving** - Gets better with every task
2. **No Maintenance** - No need to manually update benchmark files
3. **Real-World Performance** - Reflects actual usage, not synthetic benchmarks
4. **Task-Specific** - Learns that Gemini is great for audits, Qwen for reasoning
5. **Cost-Aware** - Budget estimates based on actual historical costs

### Cold Start Strategy

For new deployments with no historical data:

1. Use current static benchmarks as initial baseline
2. Seed with known good models per task type:
   - `code`: `google/gemini-3-flash-preview`
   - `reasoning`: `qwen/qwen3-235b-a22b-instruct`
   - `tool_calling`: `google/gemini-3-flash-preview`
   - `general`: `google/gemini-3-flash-preview`

3. After 10 tasks, start blending learned data
4. After 50 tasks, primarily use learned data

### Observability

Add API endpoint to expose learned stats:

```
GET /api/models/performance
```

Returns:
```json
{
  "learned_stats": [
    {
      "model_id": "google/gemini-3-flash-preview",
      "task_type": "code",
      "success_rate": 0.92,
      "avg_cost_cents": 15.3,
      "total_tasks": 47
    }
  ],
  "budget_estimates": [
    {
      "task_type": "code", 
      "complexity_bucket": 0.5,
      "avg_cost": 25.0,
      "cost_p80": 42.0
    }
  ]
}
```

### Configuration

New environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `MODEL_SELECTION_MIN_SAMPLES` | 5 | Min tasks before using learned data |
| `MODEL_SELECTION_SUCCESS_THRESHOLD` | 0.7 | Min success rate to consider model |
| `BUDGET_ESTIMATE_BUFFER` | 1.2 | Multiplier on P80 estimate |
| `LEARNED_STATS_WINDOW_DAYS` | 30 | Rolling window for stats |

---

## Immediate Changes (This PR)

1. Change default model from `qwen/qwen3-next-80b-a3b-thinking` to `google/gemini-3-flash-preview`
2. Update frontend to suggest `gemini-3-flash` as default
3. Document the learning system design for future implementation
