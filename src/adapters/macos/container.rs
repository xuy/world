use std::collections::HashSet;

use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::{
    ContainerInfo, ContainerState, ContainerStatus, HealthState, ImageInfo, PortMapping, VolumeInfo,
};

/// Detect which container runtime is available.
async fn detect_runtime() -> Option<String> {
    if let Ok(r) = exec("which", &["docker"], ExecOpts::default()).await {
        if r.success() {
            return Some("docker".into());
        }
    }
    if let Ok(r) = exec("which", &["podman"], ExecOpts::default()).await {
        if r.success() {
            return Some("podman".into());
        }
    }
    None
}

pub async fn observe(
    target: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    let runtime = match detect_runtime().await {
        Some(r) => r,
        None => {
            return Ok(UnifiedResult::ok(
                "No container runtime found. Install Docker or Podman.",
                json!({"runtime": null, "containers": [], "warnings": ["Neither docker nor podman found in PATH."]}),
            ));
        }
    };

    // Target is the unified navigation:
    //   None              → list containers (default)
    //   "images"          → list images
    //   "volumes"         → list volumes
    //   "my-nginx"        → specific container
    //   "my-nginx/logs"   → logs for container
    match target {
        Some("images") => observe_images(&runtime).await,
        Some("volumes") => observe_volumes(&runtime).await,
        Some(t) if t.ends_with("/logs") => {
            let id = &t[..t.len() - 5];
            observe_logs(&runtime, Some(id), limit).await
        }
        Some(t) => observe_containers(&runtime, Some(t)).await,
        None => observe_containers(&runtime, None).await,
    }
}

async fn observe_containers(runtime: &str, target: Option<&str>) -> Result<UnifiedResult> {
    if let Some(id) = target {
        return observe_single_container(runtime, id).await;
    }

    let result = exec(
        runtime,
        &["ps", "-a", "--format", "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}"],
        ExecOpts::default(),
    )
    .await?;

    let containers: Vec<ContainerInfo> = result
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let id = parts.first().unwrap_or(&"").to_string();
            let name = parts.get(1).unwrap_or(&"").to_string();
            let image = parts.get(2).unwrap_or(&"").to_string();
            let status_str = parts.get(3).unwrap_or(&"");
            let ports_str = parts.get(4).unwrap_or(&"");

            ContainerInfo {
                id,
                name,
                image,
                status: parse_container_status(status_str),
                ports: parse_ports(ports_str),
                health: parse_health_from_status(status_str),
                created_at: None,
                started_at: None,
            }
        })
        .collect();

    let count = containers.len();
    let state = ContainerState {
        containers,
        images: None,
        volumes: None,
        runtime: runtime.into(),
        warnings: None,
    };

    Ok(UnifiedResult::ok(
        format!("{count} containers found ({runtime})."),
        serde_json::to_value(&state)?,
    )
    .with_suggestions(vec![
        "observe(container, scope: [\"images\"]) for images".into(),
        "observe(container, scope: [\"volumes\"]) for volumes".into(),
    ]))
}

async fn observe_single_container(runtime: &str, id: &str) -> Result<UnifiedResult> {
    let result = exec(
        runtime,
        &["inspect", "--format",
          "{{.Id}}\t{{.Name}}\t{{.Config.Image}}\t{{.State.Status}}\t{{.State.Health.Status}}\t{{.Created}}\t{{.State.StartedAt}}"],
        ExecOpts::default(),
    )
    .await;

    // Fall back to simpler inspect if template fails
    let result = match result {
        Ok(r) if r.success() => r,
        _ => {
            let r = exec(runtime, &["inspect", id], ExecOpts::default()).await?;
            if !r.success() {
                return Ok(UnifiedResult::ok(
                    format!("Container '{id}' not found."),
                    json!({"error": "not_found", "target": id}),
                ));
            }
            r
        }
    };

    Ok(UnifiedResult::ok(
        format!("Container '{id}' details retrieved."),
        json!({"raw_inspect": result.stdout.trim()}),
    ))
}

async fn observe_images(runtime: &str) -> Result<UnifiedResult> {
    let result = exec(
        runtime,
        &["images", "--format", "{{.ID}}\t{{.Repository}}\t{{.Tag}}\t{{.Size}}"],
        ExecOpts::default(),
    )
    .await?;

    let images: Vec<ImageInfo> = result
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            ImageInfo {
                id: parts.first().unwrap_or(&"").to_string(),
                repository: parts.get(1).unwrap_or(&"").to_string(),
                tag: parts.get(2).unwrap_or(&"").to_string(),
                size_bytes: parse_size(parts.get(3).unwrap_or(&"")),
                created_at: None,
            }
        })
        .collect();

    let count = images.len();
    Ok(UnifiedResult::ok(
        format!("{count} images found ({runtime})."),
        serde_json::to_value(&images)?,
    ))
}

async fn observe_volumes(runtime: &str) -> Result<UnifiedResult> {
    let result = exec(
        runtime,
        &["volume", "ls", "--format", "{{.Name}}\t{{.Driver}}\t{{.Mountpoint}}"],
        ExecOpts::default(),
    )
    .await?;

    let volumes: Vec<VolumeInfo> = result
        .stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            VolumeInfo {
                name: parts.first().unwrap_or(&"").to_string(),
                driver: parts.get(1).unwrap_or(&"").to_string(),
                mountpoint: parts.get(2).unwrap_or(&"").to_string(),
                size_bytes: None,
            }
        })
        .collect();

    let count = volumes.len();
    Ok(UnifiedResult::ok(
        format!("{count} volumes found ({runtime})."),
        serde_json::to_value(&volumes)?,
    ))
}

async fn observe_logs(runtime: &str, target: Option<&str>, limit: Option<u32>) -> Result<UnifiedResult> {
    let id = target.ok_or_else(|| anyhow::anyhow!("Container ID required for container_logs scope"))?;
    let tail = limit.unwrap_or(50).to_string();

    let result = exec(
        runtime,
        &["logs", "--tail", &tail, id],
        ExecOpts { timeout_sec: 10, ..Default::default() },
    )
    .await?;

    Ok(UnifiedResult::ok(
        format!("Last {tail} log lines for container '{id}'."),
        json!({
            "container": id,
            "logs": result.combined(),
            "lines": result.combined().lines().count(),
        }),
    ))
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    let runtime = match detect_runtime().await {
        Some(r) => r,
        None => {
            return Ok(UnifiedResult::err(
                "no_runtime",
                "No container runtime found. Install Docker or Podman.",
            ));
        }
    };

    match action {
        "start_container" => {
            let id = target.ok_or_else(|| anyhow::anyhow!("Container ID required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would start container '{id}' via {runtime} start."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["start", id], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Container '{id}' started.")
                } else {
                    format!("Failed to start '{id}': {}", result.stderr.trim())
                },
                json!({"action": "start_container", "target": id, "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec![format!("verify(container_running, target: \"{id}\")")]))
        }
        "stop_container" => {
            let id = target.ok_or_else(|| anyhow::anyhow!("Container ID required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would stop container '{id}' via {runtime} stop."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["stop", id], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Container '{id}' stopped.")
                } else {
                    format!("Failed to stop '{id}': {}", result.stderr.trim())
                },
                json!({"action": "stop_container", "target": id, "success": result.success()}),
            )
            .with_risk(Risk::Medium))
        }
        "restart_container" => {
            let id = target.ok_or_else(|| anyhow::anyhow!("Container ID required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would restart container '{id}' via {runtime} restart."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["restart", id], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Container '{id}' restarted.")
                } else {
                    format!("Failed to restart '{id}': {}", result.stderr.trim())
                },
                json!({"action": "restart_container", "target": id, "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec![format!("verify(container_running, target: \"{id}\")")]))
        }
        "remove_container" => {
            let id = target.ok_or_else(|| anyhow::anyhow!("Container ID required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would remove container '{id}' via {runtime} rm."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["rm", id], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Container '{id}' removed.")
                } else {
                    format!("Failed to remove '{id}': {}", result.stderr.trim())
                },
                json!({"action": "remove_container", "target": id, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        "pull_image" => {
            let image = target.ok_or_else(|| anyhow::anyhow!("Image name required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would pull image '{image}' via {runtime} pull."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                &runtime,
                &["pull", image],
                ExecOpts { timeout_sec: 120, ..Default::default() },
            )
            .await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Image '{image}' pulled.")
                } else {
                    format!("Failed to pull '{image}': {}", result.stderr.trim())
                },
                json!({"action": "pull_image", "target": image, "success": result.success()}),
            )
            .with_risk(Risk::Low)
            .with_suggestions(vec![format!("verify(image_exists, target: \"{image}\")")]))
        }
        "prune_images" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would prune unused images via {runtime} image prune -a -f."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["image", "prune", "-a", "-f"], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Unused images pruned. {}", result.stdout.trim())
                } else {
                    format!("Prune failed: {}", result.stderr.trim())
                },
                json!({"action": "prune_images", "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        "prune_volumes" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would prune unused volumes via {runtime} volume prune -f."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(&runtime, &["volume", "prune", "-f"], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Unused volumes pruned. {}", result.stdout.trim())
                } else {
                    format!("Prune failed: {}", result.stderr.trim())
                },
                json!({"action": "prune_volumes", "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown container action: {action}"),
        )),
    }
}

pub async fn verify_running(id: &str) -> Result<UnifiedResult> {
    let runtime = detect_runtime().await.unwrap_or_else(|| "docker".into());
    let result = exec(
        &runtime,
        &["inspect", "--format", "{{.State.Running}}", id],
        ExecOpts::default(),
    )
    .await?;

    let running = result.success() && result.stdout.trim() == "true";

    Ok(UnifiedResult::ok(
        if running {
            format!("Container '{id}' is running.")
        } else {
            format!("Container '{id}' is NOT running.")
        },
        json!({
            "check": "container_running",
            "target": id,
            "passed": running,
        }),
    ))
}

pub async fn verify_healthy(id: &str) -> Result<UnifiedResult> {
    let runtime = detect_runtime().await.unwrap_or_else(|| "docker".into());
    let result = exec(
        &runtime,
        &["inspect", "--format", "{{.State.Health.Status}}", id],
        ExecOpts::default(),
    )
    .await?;

    let status = result.stdout.trim().to_string();
    let healthy = result.success() && status == "healthy";

    Ok(UnifiedResult::ok(
        if healthy {
            format!("Container '{id}' is healthy.")
        } else if status.is_empty() || status == "<no value>" {
            format!("Container '{id}' has no health check configured.")
        } else {
            format!("Container '{id}' health: {status}.")
        },
        json!({
            "check": "container_healthy",
            "target": id,
            "passed": healthy,
            "health_status": status,
        }),
    ))
}

pub async fn verify_image_exists(image: &str) -> Result<UnifiedResult> {
    let runtime = detect_runtime().await.unwrap_or_else(|| "docker".into());
    let result = exec(&runtime, &["image", "inspect", image], ExecOpts::default()).await?;
    let exists = result.success();

    Ok(UnifiedResult::ok(
        if exists {
            format!("Image '{image}' exists locally.")
        } else {
            format!("Image '{image}' does NOT exist locally.")
        },
        json!({
            "check": "image_exists",
            "target": image,
            "passed": exists,
        }),
    ))
}

pub async fn verify_volume_exists(volume: &str) -> Result<UnifiedResult> {
    let runtime = detect_runtime().await.unwrap_or_else(|| "docker".into());
    let result = exec(&runtime, &["volume", "inspect", volume], ExecOpts::default()).await?;
    let exists = result.success();

    Ok(UnifiedResult::ok(
        if exists {
            format!("Volume '{volume}' exists.")
        } else {
            format!("Volume '{volume}' does NOT exist.")
        },
        json!({
            "check": "volume_exists",
            "target": volume,
            "passed": exists,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

fn parse_container_status(status: &str) -> ContainerStatus {
    let lower = status.to_lowercase();
    if lower.starts_with("up") || lower.contains("running") {
        ContainerStatus::Running
    } else if lower.starts_with("exited") {
        ContainerStatus::Exited
    } else if lower.contains("paused") {
        ContainerStatus::Paused
    } else if lower.contains("restarting") {
        ContainerStatus::Restarting
    } else if lower.contains("created") {
        ContainerStatus::Created
    } else if lower.contains("dead") {
        ContainerStatus::Dead
    } else {
        ContainerStatus::Unknown
    }
}

fn parse_health_from_status(status: &str) -> Option<HealthState> {
    let lower = status.to_lowercase();
    if lower.contains("(healthy)") {
        Some(HealthState::Healthy)
    } else if lower.contains("(unhealthy)") {
        Some(HealthState::Unhealthy)
    } else if lower.contains("(health: starting)") {
        Some(HealthState::Starting)
    } else {
        None
    }
}

fn parse_ports(ports_str: &str) -> Option<Vec<PortMapping>> {
    if ports_str.is_empty() {
        return None;
    }

    // Deduplicate by (host_port, container_port, protocol) — docker shows
    // separate entries for IPv4 (0.0.0.0:8080) and IPv6 (:::8080)
    let mut seen = HashSet::new();
    let mut mappings = Vec::new();

    for s in ports_str.split(',') {
        let s = s.trim();
        // Format: 0.0.0.0:8080->80/tcp  or  :::8080->80/tcp
        if let Some(arrow_pos) = s.find("->") {
            let host_part = &s[..arrow_pos];
            let container_part = &s[arrow_pos + 2..];

            let host_port = match host_part.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
                Some(p) => p,
                None => continue,
            };

            let (container_port, protocol) = if let Some(slash) = container_part.find('/') {
                match container_part[..slash].parse::<u16>().ok() {
                    Some(p) => (p, container_part[slash + 1..].to_string()),
                    None => continue,
                }
            } else {
                match container_part.parse::<u16>().ok() {
                    Some(p) => (p, "tcp".into()),
                    None => continue,
                }
            };

            let key = (host_port, container_port, protocol.clone());
            if seen.insert(key) {
                mappings.push(PortMapping {
                    host_port,
                    container_port,
                    protocol,
                });
            }
        }
    }

    if mappings.is_empty() {
        None
    } else {
        Some(mappings)
    }
}

fn parse_size(size_str: &str) -> u64 {
    let s = size_str.trim().to_uppercase();
    let (num_str, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1_000_000_000u64)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1_000_000u64)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1_000u64)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1u64)
    } else {
        return 0;
    };

    num_str
        .trim()
        .parse::<f64>()
        .map(|n| (n * multiplier as f64) as u64)
        .unwrap_or(0)
}
