pub mod bash;
pub mod handoff;
pub mod observe;
pub mod act;
pub mod verify;

use crate::tool::Tool;

use crate::adapters::Platform;
use crate::telemetry::TelemetryLog;
use std::sync::Arc;

/// Create all 5 unified tools, ready to register with a ToolRouter.
pub fn create_unified_tools(
    platform: Platform,
    telemetry: Arc<TelemetryLog>,
) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(observe::ObserveTool::new(platform, telemetry.clone())),
        Box::new(act::ActTool::new(platform, telemetry.clone())),
        Box::new(verify::VerifyTool::new(platform, telemetry.clone())),
        Box::new(bash::BashTool::new(platform, telemetry.clone())),
        Box::new(handoff::HandoffTool::new(telemetry)),
    ]
}
