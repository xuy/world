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
        Platform::MacOS => crate::adapters::macos::network::observe(target, scope).await,
        _ => Ok(UnifiedResult::unsupported("network observation")),
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
            crate::adapters::macos::network::act(action, target, dry_run).await
        }
        _ => Ok(UnifiedResult::unsupported(&format!("network.{action}"))),
    }
}

pub async fn verify_host_reachable(
    platform: Platform,
    target: Option<&str>,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.unwrap_or("8.8.8.8");
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::network::verify_host_reachable(host, timeout_sec).await
        }
        _ => Ok(UnifiedResult::unsupported("host_reachable")),
    }
}

pub async fn verify_dns_resolves(
    platform: Platform,
    target: Option<&str>,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    let domain = target.unwrap_or("google.com");
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::network::verify_dns_resolves(domain, timeout_sec).await
        }
        _ => Ok(UnifiedResult::unsupported("dns_resolves")),
    }
}

pub async fn verify_internet_reachable(
    platform: Platform,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    match platform {
        Platform::MacOS => {
            crate::adapters::macos::network::verify_internet_reachable(timeout_sec).await
        }
        _ => Ok(UnifiedResult::unsupported("internet_reachable")),
    }
}

pub async fn verify_port_open(
    platform: Platform,
    target: Option<&str>,
    params: Option<&Value>,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    let host = target.unwrap_or("localhost");
    let port = params
        .and_then(|p| p.get("port"))
        .and_then(|p| p.as_u64())
        .unwrap_or(80) as u16;

    match platform {
        Platform::MacOS => {
            crate::adapters::macos::network::verify_port_open(host, port, timeout_sec).await
        }
        _ => Ok(UnifiedResult::unsupported("port_open")),
    }
}
