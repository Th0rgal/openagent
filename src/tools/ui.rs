//! Frontend (Tool UI) tools.
//!
//! These tools are intended to be rendered in the dashboard UI rather than executed
//! as real side-effecting operations. They exist mainly to provide tool schemas to
//! the LLM so it can request structured UI renderings.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

use super::Tool;

/// Ask the user to pick from a list of options (interactive).
pub struct UiOptionList;

#[async_trait]
impl Tool for UiOptionList {
    fn name(&self) -> &str {
        "ui_optionList"
    }

    fn description(&self) -> &str {
        "Render an interactive option list for the user to choose from (frontend Tool UI)."
    }

    fn parameters_schema(&self) -> Value {
        // Intentionally permissive: we validate on the frontend before rendering.
        json!({
            "type": "object",
            "required": ["id", "title", "options"],
            "properties": {
                "id": { "type": "string", "description": "Stable identifier for this UI element." },
                "title": { "type": "string" },
                "description": { "type": "string" },
                "multiple": { "type": "boolean", "default": false },
                "confirmLabel": { "type": "string" },
                "options": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "required": ["id", "label"],
                        "properties": {
                            "id": { "type": "string" },
                            "label": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "additionalProperties": true
                    }
                }
            },
            "additionalProperties": true
        })
    }

    async fn execute(&self, args: Value, _workspace: &Path) -> anyhow::Result<String> {
        // The agent runtime intercepts ui_* tools and routes them to the frontend.
        // This is a safe fallback for non-control-session executions.
        Ok(serde_json::to_string(&args).unwrap_or_else(|_| args.to_string()))
    }
}

/// Render a read-only data table (non-interactive).
pub struct UiDataTable;

#[async_trait]
impl Tool for UiDataTable {
    fn name(&self) -> &str {
        "ui_dataTable"
    }

    fn description(&self) -> &str {
        "Render a data table with rows/columns (frontend Tool UI)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["id", "columns", "rows"],
            "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "columns": { "type": "array", "items": { "type": "object" }, "minItems": 1 },
                "rows": { "type": "array", "items": { "type": "object" } }
            },
            "additionalProperties": true
        })
    }

    async fn execute(&self, args: Value, _workspace: &Path) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&args).unwrap_or_else(|_| args.to_string()))
    }
}

