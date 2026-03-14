use anyhow::Result;
use async_trait::async_trait;
use crate::tool::{SafetyTier, Tool, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::contracts::handoff::HandoffArgs;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

pub struct HandoffTool {
    telemetry: Arc<TelemetryLog>,
}

impl HandoffTool {
    pub fn new(telemetry: Arc<TelemetryLog>) -> Self {
        Self { telemetry }
    }
}

#[async_trait]
impl Tool for HandoffTool {
    fn name(&self) -> &str {
        "handoff"
    }

    fn description(&self) -> &str {
        "Structured escalation when the agent cannot complete a task. Creates a handoff summary with evidence, severity, and recommended human owner. Use when blocked by privilege, policy, physical access, or when action attempts have been exhausted."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Summary of the issue and what was tried."
                },
                "evidence_refs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "References to observations or artifacts that support this handoff."
                },
                "severity": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "urgent"],
                    "description": "Severity of the issue."
                },
                "recommended_human_owner": {
                    "type": "string",
                    "enum": ["user", "it_admin", "vendor", "isp"],
                    "description": "Who should handle this next."
                }
            },
            "required": ["summary", "severity"]
        })
    }

    fn safety_tier(&self) -> SafetyTier {
        SafetyTier::ReadOnly
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult> {
        let args: HandoffArgs = serde_json::from_value(input.clone())?;

        // Collect the telemetry trail as evidence
        let events = self.telemetry.events();
        let trail: Vec<Value> = events
            .iter()
            .map(|e| {
                json!({
                    "tool": e.tool,
                    "domain": e.domain,
                    "action": e.action,
                    "success": e.success,
                    "timestamp": e.timestamp.to_rfc3339(),
                })
            })
            .collect();

        let owner = args
            .recommended_human_owner
            .map(|o| format!("{:?}", o))
            .unwrap_or_else(|| "unspecified".into());

        let output = format!(
            "## Handoff Summary\n\n\
             **Severity:** {:?}\n\
             **Recommended owner:** {}\n\n\
             {}\n\n\
             **Evidence trail:** {} tool calls recorded.",
            args.severity,
            owner,
            args.summary,
            trail.len(),
        );

        // Log telemetry
        let mut event = ToolCallEvent::new("handoff");
        event.success = true;
        self.telemetry.record(event);

        Ok(ToolResult::read_only(
            output,
            json!({
                "handoff": {
                    "summary": args.summary,
                    "severity": args.severity,
                    "recommended_human_owner": args.recommended_human_owner,
                    "evidence_refs": args.evidence_refs,
                    "tool_trail": trail,
                }
            }),
        ))
    }
}
