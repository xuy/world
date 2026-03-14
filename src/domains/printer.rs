use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
    scope: Option<&[String]>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::printer::observe(target, scope).await,
        _ => Ok(UnifiedResult::unsupported("printer observation")),
    }
}

pub async fn act(
    platform: Platform,
    action: &str,
    target: Option<&str>,
    _params: Option<&Value>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::printer::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("printer.{action}"))),
    }
}

pub async fn verify_prints(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::printer::verify_prints(target).await,
        _ => Ok(UnifiedResult::unsupported("printer_prints")),
    }
}
