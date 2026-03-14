use serde::{Deserialize, Serialize};

/// Arguments for the `bash` escape-hatch tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashArgs {
    /// The shell command to execute.
    pub command: String,
    /// Timeout in seconds (default 30).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<u32>,
    /// Plain-language reason for using bash instead of typed tools.
    pub reason: String,
}
