use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::container::observe(target, limit).await,
        _ => Ok(UnifiedResult::unsupported("container observation")),
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
            crate::adapters::macos::container::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("container.{action}"))),
    }
}

pub async fn verify_running(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let id = target.ok_or_else(|| anyhow::anyhow!("container ID required for container_running check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::container::verify_running(id).await,
        _ => Ok(UnifiedResult::unsupported("container_running")),
    }
}

pub async fn verify_healthy(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let id = target.ok_or_else(|| anyhow::anyhow!("container ID required for container_healthy check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::container::verify_healthy(id).await,
        _ => Ok(UnifiedResult::unsupported("container_healthy")),
    }
}

pub async fn verify_image_exists(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let image = target.ok_or_else(|| anyhow::anyhow!("image name required for image_exists check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::container::verify_image_exists(image).await,
        _ => Ok(UnifiedResult::unsupported("image_exists")),
    }
}

pub async fn verify_volume_exists(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let volume = target.ok_or_else(|| anyhow::anyhow!("volume name required for volume_exists check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::container::verify_volume_exists(volume).await,
        _ => Ok(UnifiedResult::unsupported("volume_exists")),
    }
}
