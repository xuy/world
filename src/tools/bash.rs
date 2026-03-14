use anyhow::Result;
use async_trait::async_trait;
use crate::tool::{SafetyTier, Tool, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::Platform;
use crate::contracts::bash::BashArgs;
use crate::execution::exec_shell;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

/// Blocked command patterns that should never be executed.
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "mkfs",
    "dd if=",
    "shutdown",
    "reboot",
    "halt",
    "> /dev/sda",
    "chmod -R 777",
    ":(){ :|:& };:",
];

/// Patterns that require explicit approval.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm ",
    "sudo ",
    "chmod ",
    "chown ",
    "killall ",
    "pkill ",
    "launchctl unload",
    "systemctl stop",
    "net stop",
];

pub struct BashTool {
    #[allow(dead_code)]
    platform: Platform,
    telemetry: Arc<TelemetryLog>,
}

impl BashTool {
    pub fn new(platform: Platform, telemetry: Arc<TelemetryLog>) -> Self {
        Self { platform, telemetry }
    }

    fn is_blocked(command: &str) -> bool {
        let lower = command.to_lowercase();
        BLOCKED_PATTERNS.iter().any(|p| lower.contains(p))
    }

    fn is_dangerous(command: &str) -> bool {
        let lower = command.to_lowercase();
        DANGEROUS_PATTERNS.iter().any(|p| lower.contains(p))
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. This is an escape hatch for cases not covered by observe/act/verify. Prefer typed tools when possible. You must provide a reason explaining why bash is needed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                },
                "timeout_sec": {
                    "type": "integer",
                    "description": "Timeout in seconds (default 30).",
                    "default": 30
                },
                "reason": {
                    "type": "string",
                    "description": "Plain-language reason for using bash instead of typed tools."
                }
            },
            "required": ["command", "reason"]
        })
    }

    fn safety_tier(&self) -> SafetyTier {
        SafetyTier::NeedsApproval
    }

    fn safety_tier_for_input(&self, input: &Value) -> SafetyTier {
        let command = input["command"].as_str().unwrap_or("");

        if Self::is_blocked(command) {
            return SafetyTier::NeedsApproval; // Will be rejected in execute
        }

        if Self::is_dangerous(command) {
            SafetyTier::NeedsApproval
        } else {
            // Read-only-like commands can auto-approve
            SafetyTier::SafeAction
        }
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult> {
        let args: BashArgs = serde_json::from_value(input.clone())?;
        let start = Instant::now();

        // Hard block dangerous patterns
        if Self::is_blocked(&args.command) {
            return Ok(ToolResult::read_only(
                format!("Command blocked by safety policy: {}", args.command),
                json!({"error": "blocked", "command": args.command}),
            ));
        }

        let timeout = args.timeout_sec.unwrap_or(30);
        let result = exec_shell(&args.command, timeout).await?;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Log telemetry
        let mut event = ToolCallEvent::new("bash");
        event.action = Some(args.command.clone());
        event.duration_ms = duration_ms;
        event.success = result.success();
        self.telemetry.record(event);

        let output = if result.success() {
            result.combined()
        } else {
            format!(
                "Command exited with code {}.\nstdout: {}\nstderr: {}",
                result.exit_code,
                result.stdout.trim(),
                result.stderr.trim(),
            )
        };

        Ok(ToolResult::read_only(
            output,
            json!({
                "exit_code": result.exit_code,
                "stdout": result.stdout.trim(),
                "stderr": result.stderr.trim(),
                "duration_ms": result.duration_ms,
                "reason": args.reason,
            }),
        ))
    }
}
