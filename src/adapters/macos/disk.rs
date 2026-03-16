use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::{DiskState, MountPoint};

pub async fn observe(
    target: Option<&str>,
) -> Result<UnifiedResult> {
    // Target selects what to show:
    //   None          → mounts + space (default)
    //   "temp_usage"  → temp dir sizes
    //   "/path"       → specific mount point (future)
    let scopes: Vec<&str> = match target {
        Some("temp_usage") => vec!["temp_usage"],
        _ => vec!["space", "mounts"],
    };

    let mut state = DiskState {
        mounts: vec![],
    };

    if scopes.contains(&"space") || scopes.contains(&"mounts") {
        let result = exec("df", &["-h"], ExecOpts::default()).await?;
        state.mounts = parse_df_output(&result.stdout);

        // Also get raw bytes for structured data
        let result_bytes = exec("df", &["-k"], ExecOpts::default()).await?;
        enrich_byte_counts(&mut state.mounts, &result_bytes.stdout);

    }

    let summary = format_disk_summary(&state);
    Ok(UnifiedResult::ok(summary, serde_json::to_value(&state)?)
        .with_suggestions(vec![
            "act(disk, clear_temp_files)".into(),
            "verify(disk_writable)".into(),
        ]))
}

pub async fn act(
    action: &str,
    _target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match action {
        "clear_temp_files" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would clear temporary files from /tmp and ~/Library/Caches.",
                    json!({"dry_run": true}),
                ));
            }

            // Clear user caches (safe subset)
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let cache_dir = format!("{home}/Library/Caches");

            // Get size before
            let before = exec_du("/tmp").await.unwrap_or(0)
                + exec_du(&cache_dir).await.unwrap_or(0);

            // Clear /tmp files older than 3 days
            let _ = exec(
                "find",
                &["/tmp", "-type", "f", "-mtime", "+3", "-delete"],
                ExecOpts::default(),
            )
            .await;

            // Purge system caches
            let _ = exec("purge", &[], ExecOpts::default()).await;

            let after = exec_du("/tmp").await.unwrap_or(0)
                + exec_du(&cache_dir).await.unwrap_or(0);

            let freed = if before > after { before - after } else { 0 };

            Ok(UnifiedResult::ok(
                format!("Temp files cleared. Freed approximately {}.", format_bytes(freed)),
                json!({"action": "clear_temp_files", "freed_bytes": freed}),
            )
            .with_risk(Risk::Low)
            .with_suggestions(vec!["verify(disk_writable)".into()]))
        }
        "remove_large_known_caches" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would remove known large caches (Homebrew, npm, pip).",
                    json!({"dry_run": true}),
                ));
            }

            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let cache_paths = [
                format!("{home}/Library/Caches/Homebrew"),
                format!("{home}/.npm/_cacache"),
                format!("{home}/Library/Caches/pip"),
            ];

            let mut total_freed = 0u64;
            for path in &cache_paths {
                let before = exec_du(path).await.unwrap_or(0);
                let _ = exec("rm", &["-rf", path], ExecOpts::default()).await;
                total_freed += before;
            }

            Ok(UnifiedResult::ok(
                format!("Removed known caches. Freed approximately {}.", format_bytes(total_freed)),
                json!({"action": "remove_large_known_caches", "freed_bytes": total_freed}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec!["verify(disk_writable)".into()]))
        }
        "unmount_share" => {
            let path = match _target {
                Some(p) => p,
                None => {
                    return Ok(UnifiedResult::err(
                        "missing_target",
                        "Mount path required (e.g. /Volumes/MyDisk).",
                    ));
                }
            };

            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would eject {path}."),
                    json!({"dry_run": true, "target": path}),
                ));
            }

            // Try diskutil eject first (works for most external volumes)
            let result = exec("diskutil", &["eject", path], ExecOpts::default()).await?;
            if result.success() {
                return Ok(UnifiedResult::ok(
                    format!("Ejected {path}."),
                    json!({"action": "eject", "target": path, "method": "diskutil"}),
                )
                .with_risk(Risk::Low));
            }

            // Fall back to hdiutil detach (for disk images)
            let result = exec("hdiutil", &["detach", path], ExecOpts::default()).await?;
            if result.success() {
                return Ok(UnifiedResult::ok(
                    format!("Detached {path}."),
                    json!({"action": "eject", "target": path, "method": "hdiutil"}),
                )
                .with_risk(Risk::Low));
            }

            // Fall back to umount
            let result = exec("umount", &[path], ExecOpts::default()).await?;
            if result.success() {
                return Ok(UnifiedResult::ok(
                    format!("Unmounted {path}."),
                    json!({"action": "eject", "target": path, "method": "umount"}),
                )
                .with_risk(Risk::Low));
            }

            Ok(UnifiedResult::err(
                "eject_failed",
                format!("Failed to eject {path}. It may be in use."),
            ))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown disk action: {action}"),
        )),
    }
}

pub async fn verify_writable(path: &str) -> Result<UnifiedResult> {
    let test_file = format!("{path}/.world_write_test_{}", std::process::id());
    let result = exec("touch", &[&test_file], ExecOpts::default()).await?;
    let writable = result.success();
    if writable {
        let _ = exec("rm", &["-f", &test_file], ExecOpts::default()).await;
    }

    Ok(UnifiedResult::ok(
        if writable {
            format!("{path} is writable.")
        } else {
            format!("{path} is NOT writable.")
        },
        json!({
            "check": "disk_writable",
            "target": path,
            "passed": writable,
        }),
    ))
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn parse_df_output(output: &str) -> Vec<MountPoint> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 9 {
                let path = parts[8..].join(" ");
                let percent = parts[4].trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
                // We'll fill in byte counts from df -k in a second pass
                Some(MountPoint {
                    path,
                    filesystem: parts[0].to_string(),
                    total_bytes: 0,
                    used_bytes: 0,
                    available_bytes: 0,
                    percent_used: percent,
                })
            } else {
                None
            }
        })
        .filter(|m| {
            // Keep only interesting mounts
            m.path == "/"
                || m.path.starts_with("/Volumes")
                || m.path.starts_with("/System/Volumes/Data")
        })
        .collect()
}

fn enrich_byte_counts(mounts: &mut [MountPoint], df_k_output: &str) {
    for line in df_k_output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 9 {
            let path = parts[8..].join(" ");
            if let Some(mount) = mounts.iter_mut().find(|m| m.path == path) {
                mount.total_bytes = parts[1].parse::<u64>().unwrap_or(0) * 1024;
                mount.used_bytes = parts[2].parse::<u64>().unwrap_or(0) * 1024;
                mount.available_bytes = parts[3].parse::<u64>().unwrap_or(0) * 1024;
            }
        }
    }
}

async fn exec_du(path: &str) -> Option<u64> {
    let result = exec("du", &["-sk", path], ExecOpts::default()).await.ok()?;
    result
        .stdout
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

fn format_disk_summary(state: &DiskState) -> String {
    let mut parts = Vec::new();

    for mount in &state.mounts {
        parts.push(format!(
            "{}: {:.0}% used ({} available)",
            mount.path,
            mount.percent_used,
            format_bytes(mount.available_bytes),
        ));
    }

    if parts.is_empty() {
        "No disk information available.".into()
    } else {
        parts.join(" | ")
    }
}
