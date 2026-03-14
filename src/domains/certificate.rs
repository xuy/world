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
        Platform::MacOS => crate::adapters::macos::certificate::observe(target, scope).await,
        _ => Ok(UnifiedResult::unsupported("certificate observation")),
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
            crate::adapters::macos::certificate::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("certificate.{action}"))),
    }
}

pub async fn verify_valid(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.ok_or_else(|| anyhow::anyhow!("hostname required for cert_valid check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::certificate::verify_valid(host).await,
        _ => Ok(UnifiedResult::unsupported("cert_valid")),
    }
}

pub async fn verify_not_expired(
    platform: Platform,
    target: Option<&str>,
    params: Option<&Value>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.ok_or_else(|| anyhow::anyhow!("hostname required for cert_not_expired check"))?;
    let days = params
        .and_then(|p| p.get("days"))
        .and_then(|d| d.as_u64())
        .unwrap_or(30) as u32;
    match platform {
        Platform::MacOS => crate::adapters::macos::certificate::verify_not_expired(host, days).await,
        _ => Ok(UnifiedResult::unsupported("cert_not_expired")),
    }
}

pub async fn verify_chain_complete(
    platform: Platform,
    target: Option<&str>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.ok_or_else(|| anyhow::anyhow!("hostname required for cert_chain_complete check"))?;
    match platform {
        Platform::MacOS => crate::adapters::macos::certificate::verify_chain_complete(host).await,
        _ => Ok(UnifiedResult::unsupported("cert_chain_complete")),
    }
}

pub async fn verify_hostname_matches(
    platform: Platform,
    target: Option<&str>,
    params: Option<&Value>,
    _timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.ok_or_else(|| anyhow::anyhow!("hostname required for hostname_matches check"))?;
    let expected = params
        .and_then(|p| p.get("hostname"))
        .and_then(|h| h.as_str())
        .unwrap_or(host);
    match platform {
        Platform::MacOS => crate::adapters::macos::certificate::verify_hostname_matches(host, expected).await,
        _ => Ok(UnifiedResult::unsupported("hostname_matches")),
    }
}
