-- Migration: Add model performance views for learned selection
-- Run this in Supabase SQL Editor

-- View: Aggregated model performance per task type
-- Used for learned model selection based on actual outcomes
CREATE OR REPLACE VIEW model_performance AS
SELECT 
    selected_model,
    task_type,
    COUNT(*) as total_tasks,
    SUM(CASE WHEN success THEN 1 ELSE 0 END)::float / NULLIF(COUNT(*), 0) as success_rate,
    AVG(actual_cost_cents) as avg_cost_cents,
    AVG(iterations) as avg_iterations,
    STDDEV(actual_cost_cents) as cost_stddev,
    PERCENTILE_CONT(0.9) WITHIN GROUP (ORDER BY actual_cost_cents) as cost_p90,
    MAX(created_at) as last_used
FROM task_outcomes
WHERE selected_model IS NOT NULL
  AND created_at > NOW() - INTERVAL '30 days'
GROUP BY selected_model, task_type
HAVING COUNT(*) >= 3;

-- View: Budget estimates per task type and complexity bucket
-- Used for learned budget estimation
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
  AND predicted_complexity IS NOT NULL
  AND created_at > NOW() - INTERVAL '30 days'
GROUP BY task_type, FLOOR(predicted_complexity * 10) / 10
HAVING COUNT(*) >= 5;

-- Index for faster queries on task_outcomes
CREATE INDEX IF NOT EXISTS idx_task_outcomes_model_type 
ON task_outcomes(selected_model, task_type, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_outcomes_complexity 
ON task_outcomes(task_type, predicted_complexity, created_at DESC);

-- Grant access to the views
GRANT SELECT ON model_performance TO authenticated;
GRANT SELECT ON model_performance TO anon;
GRANT SELECT ON budget_estimates TO authenticated;
GRANT SELECT ON budget_estimates TO anon;
