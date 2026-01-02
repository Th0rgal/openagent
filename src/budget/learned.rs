//! Learned model selection and budget estimation.
//!
//! This module provides model selection and budget estimation based on
//! historical task outcomes, replacing static benchmark-based selection.
//!
//! # How It Works
//!
//! 1. Task outcomes are recorded with predicted vs actual costs, model used, success, etc.
//! 2. Aggregated stats are computed per model per task type (success_rate, avg_cost)
//! 3. Model selection uses these learned stats when sufficient data exists
//! 4. Falls back to static benchmarks for cold start scenarios

use serde::{Deserialize, Serialize};

/// Learned model performance statistics.
///
/// Aggregated from `task_outcomes` table via `model_performance` view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedModelStats {
    pub selected_model: String,
    pub task_type: Option<String>,
    pub total_tasks: i64,
    pub success_rate: f64,
    pub avg_cost_cents: Option<f64>,
    pub avg_iterations: Option<f64>,
    pub cost_p90: Option<f64>,
    pub last_used: Option<String>,
}

/// Learned budget estimate for a task type and complexity level.
///
/// Aggregated from `task_outcomes` table via `budget_estimates` view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedBudgetEstimate {
    pub task_type: Option<String>,
    pub complexity_bucket: f64,
    pub sample_count: i64,
    pub avg_cost: Option<f64>,
    pub cost_p80: Option<f64>,
    pub avg_iterations: Option<f64>,
}

/// Configuration for learned model selection.
#[derive(Debug, Clone)]
pub struct LearnedSelectionConfig {
    /// Minimum tasks before using learned data for a model
    pub min_samples: i64,
    /// Minimum success rate to consider a model
    pub success_threshold: f64,
    /// Buffer multiplier on P80 cost estimate
    pub budget_buffer: f64,
    /// Rolling window in days for stats
    pub window_days: i64,
}

impl Default for LearnedSelectionConfig {
    fn default() -> Self {
        Self {
            min_samples: std::env::var("MODEL_SELECTION_MIN_SAMPLES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            success_threshold: std::env::var("MODEL_SELECTION_SUCCESS_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.7),
            budget_buffer: std::env::var("BUDGET_ESTIMATE_BUFFER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.2),
            window_days: std::env::var("LEARNED_STATS_WINDOW_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        }
    }
}

/// Select the best model based on learned performance data.
///
/// Algorithm:
/// 1. Filter to models with sufficient samples and success rate
/// 2. Score by success_rate / ln(avg_cost + 1) - higher success, lower cost = better
/// 3. Return the best scoring model, or fallback if no learned data
pub fn select_model_from_learned(
    task_type: &str,
    learned_stats: &[LearnedModelStats],
    config: &LearnedSelectionConfig,
    fallback: &str,
) -> String {
    // Filter to models that meet criteria for this task type
    let candidates: Vec<_> = learned_stats
        .iter()
        .filter(|s| {
            s.task_type.as_deref() == Some(task_type)
                && s.total_tasks >= config.min_samples
                && s.success_rate >= config.success_threshold
        })
        .collect();

    if candidates.is_empty() {
        tracing::debug!(
            task_type = task_type,
            "No learned data for task type, using fallback: {}",
            fallback
        );
        return fallback.to_string();
    }

    // Score by success_rate * cost_efficiency
    // Using ln(cost + 1) to dampen cost differences while still preferring cheaper
    let best = candidates
        .into_iter()
        .max_by(|a, b| {
            let cost_a = a.avg_cost_cents.unwrap_or(100.0);
            let cost_b = b.avg_cost_cents.unwrap_or(100.0);

            let score_a = a.success_rate / (cost_a + 1.0).ln();
            let score_b = b.success_rate / (cost_b + 1.0).ln();

            score_a
                .partial_cmp(&score_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|s| s.selected_model.clone());

    match best {
        Some(model) => {
            tracing::info!(
                task_type = task_type,
                model = %model,
                "Selected model from learned data"
            );
            model
        }
        None => fallback.to_string(),
    }
}

/// Estimate budget based on learned historical data.
///
/// Uses P80 cost from similar tasks with a safety buffer.
pub fn estimate_budget_from_learned(
    task_type: &str,
    complexity: f64,
    learned_estimates: &[LearnedBudgetEstimate],
    config: &LearnedSelectionConfig,
    fallback_cents: u64,
) -> u64 {
    let bucket = (complexity * 10.0).floor() / 10.0;

    // Find matching estimate (exact bucket or closest)
    let estimate = learned_estimates
        .iter()
        .filter(|e| e.task_type.as_deref() == Some(task_type))
        .min_by(|a, b| {
            let diff_a = (a.complexity_bucket - bucket).abs();
            let diff_b = (b.complexity_bucket - bucket).abs();
            diff_a
                .partial_cmp(&diff_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(est) = estimate {
        // Only use if bucket is close enough (within 0.2)
        if (est.complexity_bucket - bucket).abs() <= 0.2 {
            if let Some(p80) = est.cost_p80 {
                let budget = (p80 * config.budget_buffer).ceil() as u64;
                tracing::debug!(
                    task_type = task_type,
                    complexity = complexity,
                    bucket = est.complexity_bucket,
                    p80 = p80,
                    budget = budget,
                    "Budget estimated from learned data"
                );
                return budget.max(10); // Minimum 10 cents
            }
        }
    }

    tracing::debug!(
        task_type = task_type,
        complexity = complexity,
        fallback = fallback_cents,
        "No learned budget data, using fallback"
    );
    fallback_cents
}

/// Get the best model for each task type from learned data.
///
/// Returns a map of task_type -> best_model for quick lookup.
pub fn get_best_models_by_task_type(
    learned_stats: &[LearnedModelStats],
    config: &LearnedSelectionConfig,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;

    let mut best_by_type: HashMap<String, (String, f64)> = HashMap::new();

    for stat in learned_stats {
        if stat.total_tasks < config.min_samples || stat.success_rate < config.success_threshold {
            continue;
        }

        let task_type = match &stat.task_type {
            Some(t) => t.clone(),
            None => continue,
        };

        let cost = stat.avg_cost_cents.unwrap_or(100.0);
        let score = stat.success_rate / (cost + 1.0).ln();

        best_by_type
            .entry(task_type)
            .and_modify(|(current_model, current_score)| {
                if score > *current_score {
                    *current_model = stat.selected_model.clone();
                    *current_score = score;
                }
            })
            .or_insert((stat.selected_model.clone(), score));
    }

    best_by_type.into_iter().map(|(k, (v, _))| (k, v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stats(
        model: &str,
        task_type: &str,
        tasks: i64,
        success: f64,
        cost: f64,
    ) -> LearnedModelStats {
        LearnedModelStats {
            selected_model: model.to_string(),
            task_type: Some(task_type.to_string()),
            total_tasks: tasks,
            success_rate: success,
            avg_cost_cents: Some(cost),
            avg_iterations: Some(5.0),
            cost_p90: Some(cost * 1.5),
            last_used: None,
        }
    }

    #[test]
    fn test_select_model_prefers_high_success_low_cost() {
        let config = LearnedSelectionConfig::default();
        // Scoring formula: success_rate / ln(cost + 1)
        // This strongly rewards lower cost while accounting for success rate
        let stats = vec![
            make_stats("model-a", "code", 10, 0.95, 30.0), // Highest success, low-moderate cost -> score = 0.95/ln(31) = 0.277
            make_stats("model-b", "code", 10, 0.8, 20.0), // Lower success, low cost -> score = 0.8/ln(21) = 0.263
            make_stats("model-c", "code", 10, 0.95, 200.0), // Highest success, high cost -> score = 0.95/ln(201) = 0.179
        ];

        let selected = select_model_from_learned("code", &stats, &config, "fallback");

        // model-a should win: best balance of high success and reasonable cost
        assert_eq!(selected, "model-a");
    }

    #[test]
    fn test_select_model_filters_low_samples() {
        let config = LearnedSelectionConfig {
            min_samples: 10,
            ..Default::default()
        };
        let stats = vec![
            make_stats("model-a", "code", 5, 0.9, 50.0), // Not enough samples
            make_stats("model-b", "code", 10, 0.8, 50.0), // Enough samples
        ];

        let selected = select_model_from_learned("code", &stats, &config, "fallback");
        assert_eq!(selected, "model-b");
    }

    #[test]
    fn test_select_model_fallback_when_no_data() {
        let config = LearnedSelectionConfig::default();
        let stats: Vec<LearnedModelStats> = vec![];

        let selected = select_model_from_learned("code", &stats, &config, "fallback-model");
        assert_eq!(selected, "fallback-model");
    }

    #[test]
    fn test_budget_estimation() {
        let config = LearnedSelectionConfig {
            budget_buffer: 1.2,
            ..Default::default()
        };
        let estimates = vec![LearnedBudgetEstimate {
            task_type: Some("code".to_string()),
            complexity_bucket: 0.5,
            sample_count: 10,
            avg_cost: Some(40.0),
            cost_p80: Some(60.0),
            avg_iterations: Some(8.0),
        }];

        let budget = estimate_budget_from_learned("code", 0.5, &estimates, &config, 100);

        // Should be P80 * 1.2 = 60 * 1.2 = 72
        assert_eq!(budget, 72);
    }
}
