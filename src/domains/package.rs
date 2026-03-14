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
        Platform::MacOS => crate::adapters::macos::package::observe(target, scope).await,
        _ => Ok(UnifiedResult::unsupported("package observation")),
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
            crate::adapters::macos::package::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("package.{action}"))),
    }
}

pub async fn verify_installed(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            let name = target.ok_or_else(|| anyhow::anyhow!("package name required"))?;
            crate::adapters::macos::package::verify_installed(name).await
        }
        _ => Ok(UnifiedResult::unsupported("package_installed")),
    }
}
