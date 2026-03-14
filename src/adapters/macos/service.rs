use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::{ServiceState, ServiceStatus};

pub async fn observe(
    target: Option<&str>,
) -> Result<UnifiedResult> {
    if let Some(name) = target {
        return observe_single(name).await;
    }

    // List all running services
    let result = exec("launchctl", &["list"], ExecOpts::default()).await?;
    let services = parse_service_list(&result.stdout);
    let summary = format!("{} services running.", services.len());

    Ok(UnifiedResult::ok(
        summary,
        serde_json::to_value(&services)?,
    )
    .with_suggestions(vec![
        "observe(service, target: \"<service_name>\") for details".into(),
    ]))
}

async fn observe_single(name: &str) -> Result<UnifiedResult> {
    // Try launchctl list <name>
    let result = exec("launchctl", &["list", name], ExecOpts::default()).await?;

    if !result.success() {
        // Service might not exist or we lack privileges
        return Ok(UnifiedResult::ok(
            format!("Service '{name}' not found or not loaded."),
            serde_json::to_value(&ServiceState {
                name: name.to_string(),
                exists: false,
                status: ServiceStatus::Unknown,
                startup_mode: None,
                pid: None,
                recent_errors: None,
                dependencies: None,
            })?,
        ));
    }

    let state = parse_launchctl_detail(name, &result.stdout);
    let summary = format!(
        "Service '{}': {} (PID: {}).",
        name,
        match state.status {
            ServiceStatus::Running => "running",
            ServiceStatus::Stopped => "stopped",
            ServiceStatus::Degraded => "degraded",
            ServiceStatus::Unknown => "unknown",
        },
        state.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
    );

    Ok(UnifiedResult::ok(summary, serde_json::to_value(&state)?))
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    params: Option<&serde_json::Value>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    let name = target.ok_or_else(|| anyhow::anyhow!("Service name required for {action}"))?;

    match action {
        "restart_service" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would restart service '{name}' via launchctl kickstart."),
                    json!({"dry_run": true}),
                ));
            }
            // kickstart -k restarts an already-running service
            let result = exec(
                "launchctl",
                &["kickstart", "-k", &format!("system/{name}")],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Service '{name}' restarted.")
                } else {
                    format!(
                        "Restart of '{name}' may have failed: {}",
                        result.stderr.trim()
                    )
                },
                json!({"action": "restart_service", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec![format!("verify(service_healthy, target: \"{name}\")")]))
        }
        "start_service" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would start service '{name}'."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "launchctl",
                &["kickstart", &format!("system/{name}")],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Service '{name}' started.")
                } else {
                    format!("Failed to start '{name}': {}", result.stderr.trim())
                },
                json!({"action": "start_service", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec![format!("verify(service_healthy, target: \"{name}\")")]))
        }
        "stop_service" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would stop service '{name}'."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "launchctl",
                &["kill", "SIGTERM", &format!("system/{name}")],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Service '{name}' stopped.")
                } else {
                    format!("Failed to stop '{name}': {}", result.stderr.trim())
                },
                json!({"action": "stop_service", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::Medium))
        }
        "set_startup_mode" => {
            let mode = params
                .and_then(|p| p.get("mode"))
                .and_then(|m| m.as_str())
                .ok_or_else(|| anyhow::anyhow!("mode parameter required (auto, manual, disabled)"))?;

            let (launchctl_action, description) = match mode {
                "auto" => ("enable", "auto-start enabled"),
                "manual" | "disabled" => ("disable", "auto-start disabled"),
                _ => {
                    return Ok(UnifiedResult::err(
                        "invalid_param",
                        format!("Invalid mode '{mode}'. Expected: auto, manual, disabled"),
                    ));
                }
            };

            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would set startup mode for '{name}' to '{mode}' via launchctl {launchctl_action}."),
                    json!({"dry_run": true, "mode": mode}),
                ));
            }

            let result = exec(
                "launchctl",
                &[launchctl_action, &format!("system/{name}")],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Service '{name}' startup mode set to '{mode}' ({description}).")
                } else {
                    format!("Failed to set startup mode for '{name}': {}", result.stderr.trim())
                },
                json!({"action": "set_startup_mode", "target": name, "mode": mode, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown service action: {action}"),
        )),
    }
}

pub async fn verify_healthy(name: &str, _timeout_sec: u32) -> Result<UnifiedResult> {
    let result = exec("launchctl", &["list", name], ExecOpts::default()).await?;

    let _healthy = result.success() && result.stdout.contains("PID");
    // Also check by parsing - a running service shows a PID in the list
    let has_pid = result
        .stdout
        .lines()
        .any(|l| l.contains("\"PID\"") || l.trim().starts_with("PID"));

    let passed = result.success() && (has_pid || result.stdout.contains("PID"));

    Ok(UnifiedResult::ok(
        if passed {
            format!("Service '{name}' is healthy (running).")
        } else {
            format!("Service '{name}' is NOT healthy.")
        },
        json!({
            "check": "service_healthy",
            "target": name,
            "passed": passed,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

fn parse_service_list(output: &str) -> Vec<ServiceState> {
    output
        .lines()
        .skip(1) // header row
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let pid = parts[0].parse::<u32>().ok();
                let status = if pid.is_some() {
                    ServiceStatus::Running
                } else {
                    ServiceStatus::Stopped
                };
                Some(ServiceState {
                    name: parts[2].to_string(),
                    exists: true,
                    status,
                    startup_mode: None,
                    pid,
                    recent_errors: None,
                    dependencies: None,
                })
            } else {
                None
            }
        })
        .collect()
}

fn parse_launchctl_detail(name: &str, output: &str) -> ServiceState {
    let mut pid = None;
    let mut status = ServiceStatus::Unknown;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"PID\"") || trimmed.starts_with("PID") {
            if let Some(val) = trimmed.split('=').nth(1).or_else(|| trimmed.split_whitespace().nth(1)) {
                if let Ok(p) = val.trim().trim_end_matches(';').parse::<u32>() {
                    pid = Some(p);
                    status = ServiceStatus::Running;
                }
            }
        }
    }

    // If no PID found, try first line (launchctl list <name> format: PID Status Label)
    if pid.is_none() {
        if let Some(first_line) = output.lines().find(|l| l.contains(name)) {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if let Some(p) = parts.first().and_then(|s| s.parse::<u32>().ok()) {
                pid = Some(p);
                status = ServiceStatus::Running;
            } else {
                status = ServiceStatus::Stopped;
            }
        }
    }

    ServiceState {
        name: name.to_string(),
        exists: true,
        status,
        startup_mode: None,
        pid,
        recent_errors: None,
        dependencies: None,
    }
}
