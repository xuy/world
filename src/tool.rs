//! Tool trait and types — the interface that host applications use.
//!
//! This was originally `noah-tools` but is now inlined so that
//! world is a standalone crate with no external workspace deps.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Safety ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SafetyTier {
    ReadOnly,
    SafeAction,
    NeedsApproval,
}

// ─── Results ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub description: String,
    pub undo_tool: String,
    pub undo_input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub data: Value,
    pub changes: Vec<ChangeRecord>,
}

impl ToolResult {
    pub fn read_only(output: String, data: Value) -> Self {
        Self {
            output,
            data,
            changes: vec![],
        }
    }

    pub fn with_changes(output: String, data: Value, changes: Vec<ChangeRecord>) -> Self {
        Self {
            output,
            data,
            changes,
        }
    }
}

// ─── Trait ──────────────────────────────────────────────────────────────────

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    fn safety_tier(&self) -> SafetyTier;

    /// Determine the safety tier based on the specific input.
    /// Override this to implement input-dependent safety checks (e.g.,
    /// auto-approve safe shell commands but block dangerous ones).
    fn safety_tier_for_input(&self, _input: &Value) -> SafetyTier {
        self.safety_tier()
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult>;
}
