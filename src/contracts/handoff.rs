use serde::{Deserialize, Serialize};

/// Severity of the issue being handed off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Urgent,
}

/// Recommended human owner for the handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffOwner {
    User,
    ItAdmin,
    Vendor,
    Isp,
}

/// Arguments for the `handoff` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffArgs {
    /// Summary of the issue and what was tried.
    pub summary: String,
    /// References to evidence (observation IDs, artifact titles, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_refs: Option<Vec<String>>,
    /// Severity classification.
    pub severity: Severity,
    /// Recommended owner for the handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_human_owner: Option<HandoffOwner>,
}
