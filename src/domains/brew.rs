use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::brew::observe(target).await,
        _ => Ok(UnifiedResult::unsupported("brew observation")),
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
            crate::adapters::macos::brew::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("brew.{action}"))),
    }
}

pub async fn verify_installed(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            let name = target.ok_or_else(|| anyhow::anyhow!("brew package name required"))?;
            crate::adapters::macos::brew::verify_installed(name).await
        }
        _ => Ok(UnifiedResult::unsupported("brew_installed")),
    }
}
