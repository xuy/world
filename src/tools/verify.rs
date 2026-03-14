use anyhow::Result;
use async_trait::async_trait;
use crate::tool::{SafetyTier, Tool, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::Platform;
use crate::contracts::verify::VerifyArgs;
use crate::domains;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

pub struct VerifyTool {
    platform: Platform,
    telemetry: Arc<TelemetryLog>,
}

impl VerifyTool {
    pub fn new(platform: Platform, telemetry: Arc<TelemetryLog>) -> Self {
        Self { platform, telemetry }
    }
}

#[async_trait]
impl Tool for VerifyTool {
    fn name(&self) -> &str {
        "verify"
    }

    fn description(&self) -> &str {
        "Check whether a target condition holds. Use after remediation to confirm the fix worked. Checks include: service_healthy, port_open, host_reachable, dns_resolves, printer_prints, share_accessible, package_installed, disk_writable, login_works, internet_reachable."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "check": {
                    "type": "string",
                    "enum": [
                        "service_healthy", "port_open", "host_reachable",
                        "dns_resolves", "printer_prints", "share_accessible",
                        "package_installed", "disk_writable", "login_works",
                        "internet_reachable"
                    ],
                    "description": "The condition to verify."
                },
                "target": {
                    "type": "string",
                    "description": "Target for the check (e.g. hostname, service name, package name)."
                },
                "params": {
                    "type": "object",
                    "description": "Additional parameters (e.g. {\"port\": 443} for port_open)."
                },
                "timeout_sec": {
                    "type": "integer",
                    "description": "Timeout in seconds for the check (default 10).",
                    "default": 10
                }
            },
            "required": ["check"]
        })
    }

    fn safety_tier(&self) -> SafetyTier {
        SafetyTier::ReadOnly
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult> {
        let args: VerifyArgs = serde_json::from_value(input.clone())?;
        let start = Instant::now();
        let timeout = args.timeout_sec.unwrap_or(10);

        let result = domains::dispatch_verify(
            self.platform,
            args.check,
            args.target.as_deref(),
            args.params.as_ref(),
            timeout,
        )
        .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Log telemetry, linking to last remediation if applicable
        let mut event = ToolCallEvent::new("verify");
        event.action = Some(args.check.as_str().to_string());
        event.target = args.target;
        event.duration_ms = duration_ms;
        event.success = result.error.is_none();
        event.risk = result.risk;
        event.verification_of = self.telemetry.last_remediation_id();
        self.telemetry.record(event);

        let data = serde_json::to_value(&result)?;
        Ok(ToolResult::read_only(result.output, data))
    }
}
