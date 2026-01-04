//! Tuning parameters (legacy).
//!
//! This module is kept for backwards compatibility but is largely unused
//! since SimpleAgent doesn't require tuning.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level tuning parameters (legacy).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuningParams {
    // Empty - SimpleAgent doesn't use tuning
}

impl TuningParams {
    /// Load tuning parameters from the working directory, if present.
    pub async fn load_from_working_dir(_working_dir: &Path) -> Self {
        Self::default()
    }

    /// Save tuning parameters to the working directory.
    pub async fn save_to_working_dir(&self, working_dir: &Path) -> anyhow::Result<PathBuf> {
        let dir = working_dir.join(".openagent");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("tuning.json");
        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, content).await?;
        Ok(path)
    }
}
