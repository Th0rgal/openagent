//! Budget module - cost tracking and model pricing.
//!
//! # Key Concepts
//! - Budget: tracks total and allocated costs for a task
//! - Pricing: fetches and caches OpenRouter model pricing
//! - Allocation: algorithms for distributing budget across subtasks
//! - Retry: smart retry strategies for budget overflow
//! - Benchmarks: model capability scores for task-aware selection
//! - Resolver: auto-upgrade outdated model names to latest equivalents
//! - Compatibility: track which models support proper function calling
//! - Learned: model selection and budget estimation from historical outcomes

mod allocation;
pub mod benchmarks;
mod budget;
pub mod compatibility;
pub mod learned;
mod pricing;
pub mod resolver;
mod retry;

pub use allocation::{allocate_budget, AllocationStrategy};
pub use benchmarks::{load_benchmarks, BenchmarkRegistry, SharedBenchmarkRegistry, TaskType};
pub use budget::{Budget, BudgetError};
pub use compatibility::{
    create_shared_registry, CompatibilityRegistry, ModelCompatibility, SharedCompatibilityRegistry,
    ToolCallFormat,
};
pub use learned::{
    estimate_budget_from_learned, select_model_from_learned, LearnedBudgetEstimate,
    LearnedModelStats, LearnedSelectionConfig,
};
pub use pricing::{ModelPricing, PricingInfo};
pub use resolver::{load_resolver, ModelFamily, ModelResolver, ResolvedModel, SharedModelResolver};
pub use retry::{ExecutionSignals, FailureAnalysis, FailureMode, RetryConfig, RetryRecommendation};
