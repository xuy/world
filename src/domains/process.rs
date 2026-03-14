use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;

pub async fn observe(
    platform: Platform,
    target: Option<&str>,
    scope: Option<&[String]>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => crate::adapters::macos::process::observe(target, scope, limit).await,
        _ => Ok(UnifiedResult::unsupported("process observation")),
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
            crate::adapters::macos::process::act(action, target, params, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("process.{action}"))),
    }
}

pub async fn verify_running(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let pid = target.ok_or_else(|| anyhow::anyhow!("PID required for process_running check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::process::verify_running(pid).await,
        _ => Ok(UnifiedResult::unsupported("process_running")),
    }
}

pub async fn verify_stopped(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let pid = target.ok_or_else(|| anyhow::anyhow!("PID required for process_stopped check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::process::verify_stopped(pid).await,
        _ => Ok(UnifiedResult::unsupported("process_stopped")),
    }
}

pub async fn verify_port_free(
    platform: Platform,
    target: Option<&str>,
    params: Option<&Value>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let port = target
        .and_then(|t| t.parse::<u16>().ok())
        .or_else(|| params.and_then(|p| p.get("port")).and_then(|p| p.as_u64()).map(|p| p as u16))
        .ok_or_else(|| anyhow::anyhow!("port number required for port_free check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::process::verify_port_free(port).await,
        _ => Ok(UnifiedResult::unsupported("port_free")),
    }
}
