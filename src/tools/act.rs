use anyhow::Result;
use async_trait::async_trait;
use crate::tool::{SafetyTier, Tool, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::Platform;
use crate::contracts::act::ActArgs;
use crate::domains;
use crate::policy;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

pub struct ActTool {
    platform: Platform,
    telemetry: Arc<TelemetryLog>,
}

impl ActTool {
    pub fn new(platform: Platform, telemetry: Arc<TelemetryLog>) -> Self {
        Self { platform, telemetry }
    }
}

#[async_trait]
impl Tool for ActTool {
    fn name(&self) -> &str {
        "act"
    }

    fn description(&self) -> &str {
        "Perform a constrained state-changing operation. Specify a domain, action, and optionally a target. Only whitelisted actions are allowed. After acting, use verify or await to confirm the intended outcome."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "enum": ["network", "service", "printer", "disk", "brew", "process", "container"],
                    "description": "The domain to act on."
                },
                "action": {
                    "type": "string",
                    "description": "The specific action (e.g. \"flush_dns\", \"restart_service\", \"clear_queue\")."
                },
                "target": {
                    "type": "string",
                    "description": "Target of the action (e.g. service name, printer name)."
                },
                "params": {
                    "type": "object",
                    "description": "Additional parameters for the action."
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, describe what would happen without doing it.",
                    "default": false
                }
            },
            "required": ["domain", "action"]
        })
    }

    fn safety_tier(&self) -> SafetyTier {
        // Default to NeedsApproval; input-dependent override below
        SafetyTier::NeedsApproval
    }

    fn safety_tier_for_input(&self, input: &Value) -> SafetyTier {
        let domain = input["domain"].as_str().unwrap_or("");
        let action = input["action"].as_str().unwrap_or("");
        let dry_run = input["dry_run"].as_bool().unwrap_or(false);

        if dry_run {
            return SafetyTier::ReadOnly;
        }

        // Parse domain for risk classification
        if let Ok(d) = serde_json::from_value::<crate::contracts::act::ActDomain>(
            Value::String(domain.to_string()),
        ) {
            let risk = policy::classify_risk(d, action);
            if risk.requires_consent() {
                SafetyTier::NeedsApproval
            } else {
                SafetyTier::SafeAction
            }
        } else {
            SafetyTier::NeedsApproval
        }
    }

    async fn execute(&self, input: &Value) -> Result<ToolResult> {
        let args: ActArgs = serde_json::from_value(input.clone())?;
        let start = Instant::now();

        // Check allowlist
        if !policy::is_allowed(args.domain, &args.action) {
            return Ok(ToolResult::read_only(
                format!("Action '{}' is not allowed for domain {:?}.", args.action, args.domain),
                json!({"error": "action_not_allowed", "action": args.action}),
            ));
        }

        let dry_run = args.dry_run.unwrap_or(false);
        let result = domains::dispatch_act(
            self.platform,
            args.domain,
            &args.action,
            args.target.as_deref(),
            args.params.as_ref(),
            dry_run,
        )
        .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Log telemetry
        let mut event = ToolCallEvent::new("act");
        event.domain = Some(serde_json::to_string(&args.domain).unwrap_or_default().trim_matches('"').to_string());
        event.action = Some(args.action.clone());
        event.target = args.target;
        event.duration_ms = duration_ms;
        event.success = result.error.is_none();
        
        self.telemetry.record(event);

        // Add recommended verifications to suggestions
        let verifications = policy::recommended_verifications(&args.action);
        let result = if !verifications.is_empty() && result.next_suggested_actions.is_none() {
            result.with_suggestions(
                verifications.iter().map(|v| format!("verify({v})")).collect(),
            )
        } else {
            result
        };

        let data = serde_json::to_value(&result)?;
        if dry_run {
            Ok(ToolResult::read_only(result.output, data))
        } else {
            let desc = format!("{}.{}", serde_json::to_string(&args.domain).unwrap_or_default().trim_matches('"'), args.action);
            Ok(ToolResult::with_changes(
                result.output,
                data,
                vec![crate::tool::ChangeRecord {
                    description: desc,
                    undo_tool: String::new(),
                    undo_input: json!(null),
                }],
            ))
        }
    }
}
