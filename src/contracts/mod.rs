pub mod bash;
pub mod handoff;
pub mod observe;
pub mod act;
pub mod verify;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Risk classification for any tool action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    /// Whether this risk level requires explicit user consent before execution.
    pub fn requires_consent(self) -> bool {
        matches!(self, Risk::High)
    }
}

/// Why an action was blocked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedBy {
    Privilege,
    Policy,
    Physical,
    Unsupported,
    Unknown,
}

/// Structured error in a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<BlockedBy>,
}

/// An artifact attached to a tool result (table, log, JSON blob, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub title: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Table,
    Log,
    Json,
    Text,
}

/// The unified result envelope returned by all 5 tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResult {
    /// Human-readable summary.
    pub output: String,
    /// Structured details (domain-specific normalized data).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    /// Rich attachments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    /// Risk classification of the action that produced this result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    /// Suggested follow-up tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_suggested_actions: Option<Vec<String>>,
    /// Structured error if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolError>,
}

impl UnifiedResult {
    /// Shortcut for a successful read-only result.
    pub fn ok(output: impl Into<String>, details: Value) -> Self {
        Self {
            output: output.into(),
            details: Some(details),
            artifacts: None,
            risk: Some(Risk::Low),
            next_suggested_actions: None,
            error: None,
        }
    }

    /// Shortcut for an error result.
    pub fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            output: String::new(),
            details: None,
            artifacts: None,
            risk: None,
            next_suggested_actions: None,
            error: Some(ToolError {
                code: code.into(),
                message: message.into(),
                retryable: None,
                blocked_by: None,
            }),
        }
    }

    /// Shortcut for unsupported-on-this-platform.
    pub fn unsupported(domain: &str) -> Self {
        Self {
            output: format!("{domain} is not supported on this platform."),
            details: None,
            artifacts: None,
            risk: None,
            next_suggested_actions: None,
            error: Some(ToolError {
                code: "unsupported".into(),
                message: format!("{domain} is not supported on this platform."),
                retryable: Some(false),
                blocked_by: Some(BlockedBy::Unsupported),
            }),
        }
    }

    /// Add suggested next actions.
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.next_suggested_actions = Some(suggestions);
        self
    }

    /// Set risk level.
    pub fn with_risk(mut self, risk: Risk) -> Self {
        self.risk = Some(risk);
        self
    }

    /// Add artifacts.
    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = Some(artifacts);
        self
    }
}
