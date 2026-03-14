use anyhow::Result;
use async_trait::async_trait;
use crate::tool::{SafetyTier, Tool, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::Platform;
use crate::contracts::observe::ObserveArgs;
use crate::domains;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

pub struct ObserveTool {
    platform: Platform,
    telemetry: Arc<TelemetryLog>,
}

impl ObserveTool {
    pub fn new(platform: Platform, telemetry: Arc<TelemetryLog>) -> Self {
        Self { platform, telemetry }
    }
}

#[async_trait]
impl Tool for ObserveTool {
    fn name(&self) -> &str {
        "observe"
    }

    fn description(&self) -> &str {
        "Read-only observation of live state. Specify a domain and optionally a target. If called with just a domain and no target, returns capability metadata showing available scopes, actions, and verification checks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "enum": ["system", "network", "service", "process", "disk", "printer", "package", "log", "container"],
                    "description": "The system domain to observe."
                },
                "target": {
                    "type": "string",
                    "description": "Specific target within the domain (e.g. service name, printer name, hostname)."
                },
                "since": {
                    "type": "string",
                    "description": "Time filter for log-like observations (e.g. \"1h\", \"30m\")."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return."
                }
            },
            "required": ["domain"]
        })
    }

    fn safety_tier(&self) -> SafetyTier {
        SafetyTier::ReadOnly
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult> {
        let args: ObserveArgs = serde_json::from_value(input.clone())?;
        let start = Instant::now();

        // Progressive disclosure: if no target specified, return capabilities
        if args.target.is_none() {
            let caps = domains::domain_capabilities(args.domain);
            let data = serde_json::to_value(&caps)?;
            return Ok(ToolResult::read_only(caps.output, data));
        }

        let result = domains::dispatch_observe(
            self.platform,
            args.domain,
            args.target.as_deref(),
            args.since.as_deref(),
            args.limit,
        )
        .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Log telemetry
        let mut event = ToolCallEvent::new("observe");
        event.domain = Some(args.domain.as_str().to_string());
        event.target = args.target;
        event.duration_ms = duration_ms;
        event.success = result.error.is_none();
        event.risk = result.risk;
        self.telemetry.record(event);

        let data = serde_json::to_value(&result)?;
        Ok(ToolResult::read_only(result.output, data))
    }
}
