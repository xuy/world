//! Structured event logging for tool calls.
//! In-memory for now; can persist to SQLite later.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub tool: String,
    pub domain: Option<String>,
    pub action: Option<String>,
    pub target: Option<String>,
    /// Observation schema paths this action mutates.
    pub mutates: Option<Vec<String>>,
    pub duration_ms: u64,
    pub success: bool,
    pub error_code: Option<String>,
    pub blocked_by: Option<String>,
    /// Links this event to a verification event.
    pub verification_of: Option<String>,
}

impl ToolCallEvent {
    pub fn new(tool: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            tool: tool.to_string(),
            domain: None,
            action: None,
            target: None,
            mutates: None,
            duration_ms: 0,
            success: false,
            error_code: None,
            blocked_by: None,
            verification_of: None,
        }
    }
}

/// Simple in-memory telemetry store.
pub struct TelemetryLog {
    events: Mutex<Vec<ToolCallEvent>>,
}

impl TelemetryLog {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, event: ToolCallEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    pub fn events(&self) -> Vec<ToolCallEvent> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn last_remediation_id(&self) -> Option<String> {
        self.events
            .lock()
            .ok()
            .and_then(|events| {
                events
                    .iter()
                    .rev()
                    .find(|e| e.tool == "act")
                    .map(|e| e.id.clone())
            })
    }
}

impl Default for TelemetryLog {
    fn default() -> Self {
        Self::new()
    }
}
