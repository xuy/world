use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::service::observe(target).await,
        _ => Ok(UnifiedResult::unsupported("service observation")),
    }
}

pub async fn act(
    platform: Platform,
    action: &str,
    target: Option<&str>,
    params: Option<&Value>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::service::act(action, target, params, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("service.{action}"))),
    }
}

pub async fn verify_healthy(
    platform: Platform,
    target: Option<&str>,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            let name = target.ok_or_else(|| anyhow::anyhow!("service name required for service_healthy check"))?;
            crate::adapters::macos::service::verify_healthy(name, timeout_sec).await
        }
        _ => Ok(UnifiedResult::unsupported("service_healthy")),
    }
}

pub async fn verify_stopped(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            let name = target.ok_or_else(|| anyhow::anyhow!("service name required for service_stopped check"))?;
            crate::adapters::macos::service::verify_stopped(name).await
        }
        _ => Ok(UnifiedResult::unsupported("service_stopped")),
    }
}
