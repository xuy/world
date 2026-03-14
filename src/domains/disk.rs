use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::disk::observe(target).await,
        _ => Ok(UnifiedResult::unsupported("disk observation")),
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
            crate::adapters::macos::disk::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("disk.{action}"))),
    }
}

pub async fn verify_writable(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            let path = target.unwrap_or("/tmp");
            crate::adapters::macos::disk::verify_writable(path).await
        }
        _ => Ok(UnifiedResult::unsupported("disk_writable")),
    }
}
