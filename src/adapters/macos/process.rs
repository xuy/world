use std::collections::HashSet;

use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::{ProcessInfo, ProcessState, ProcessStatus};

pub async fn observe(
    target: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    // Target is the unified navigation:
    //   None             → top 20 by CPU (default)
    //   "top_cpu"        → top by CPU
    //   "top_memory"     → top by memory
    //   "processes"      → full process list
    //   "listening_ports" → listening ports
    //   "1234"           → specific PID
    //   "postgres"       → name filter
    //   "1234/tree"      → tree rooted at PID
    //   "1234/open_files" → open files for PID
    match target {
        Some("top_cpu") => observe_top(limit, "pcpu").await,
        Some("top_memory") => observe_top(limit, "rss").await,
        Some("processes") => observe_processes(None, limit).await,
        Some("listening_ports") => observe_listening_ports().await,
        Some(t) if t.contains('/') => {
            let (id, sub) = t.split_once('/').unwrap();
            match sub {
                "tree" => observe_tree(Some(id)).await,
                "open_files" => observe_open_files(Some(id)).await,
                _ => Ok(UnifiedResult::err(
                    "unknown_target",
                    format!("Unknown sub-target '{sub}'. Available: tree, open_files"),
                )),
            }
        }
        Some(t) => observe_processes(Some(t), limit).await,
        None => observe_top(limit.or(Some(20)), "pcpu").await,
    }
}

async fn observe_processes(target: Option<&str>, limit: Option<u32>) -> Result<UnifiedResult> {
    let result = exec(
        "ps",
        &["-eo", "pid,ppid,pcpu,pmem,rss,stat,user,comm"],
        ExecOpts::default(),
    )
    .await?;

    let mut processes = parse_ps_output(&result.stdout);

    // Filter by target (name or PID) if specified
    if let Some(t) = target {
        if let Ok(pid) = t.parse::<u32>() {
            processes.retain(|p| p.pid == pid);
        } else {
            let lower = t.to_lowercase();
            processes.retain(|p| p.name.to_lowercase().contains(&lower));
        }
    }

    let total = processes.len() as u32;

    if let Some(lim) = limit {
        processes.truncate(lim as usize);
    }

    let state = ProcessState {
        processes,
        total_count: total,
        warnings: Some(vec!["CPU/memory values are point-in-time snapshots.".into()]),
    };

    Ok(UnifiedResult::ok(
        format!("{total} processes found."),
        serde_json::to_value(&state)?,
    )
    .with_suggestions(vec![
        "observe(process, scope: [\"top_cpu\"], limit: 10) for CPU hogs".into(),
        "observe(process, scope: [\"listening_ports\"]) for listening ports".into(),
    ]))
}

async fn observe_top(limit: Option<u32>, sort_key: &str) -> Result<UnifiedResult> {
    let result = exec(
        "ps",
        &["-eo", "pid,ppid,pcpu,pmem,rss,stat,user,comm", "-r"],
        ExecOpts::default(),
    )
    .await?;

    let mut processes = parse_ps_output(&result.stdout);

    if sort_key == "rss" {
        processes.sort_by(|a, b| {
            b.memory_bytes.unwrap_or(0).cmp(&a.memory_bytes.unwrap_or(0))
        });
    }

    let total = processes.len() as u32;
    let lim = limit.unwrap_or(10) as usize;
    processes.truncate(lim);

    let label = if sort_key == "pcpu" { "CPU" } else { "memory" };
    let state = ProcessState {
        processes,
        total_count: total,
        warnings: Some(vec!["CPU/memory values are point-in-time snapshots.".into()]),
    };

    Ok(UnifiedResult::ok(
        format!("Top {lim} processes by {label} (of {total} total)."),
        serde_json::to_value(&state)?,
    )
    .with_suggestions(vec![
        "observe(process, scope: [\"processes\"]) for full list".into(),
        "observe(process, scope: [\"listening_ports\"]) for listening ports".into(),
    ]))
}

async fn observe_tree(target: Option<&str>) -> Result<UnifiedResult> {
    let result = exec(
        "ps",
        &["-eo", "pid,ppid,pcpu,pmem,rss,stat,user,comm"],
        ExecOpts::default(),
    )
    .await?;

    let all = parse_ps_output(&result.stdout);

    // Build tree: find root PID and attach children
    let root_pid = target
        .and_then(|t| t.parse::<u32>().ok())
        .unwrap_or(1);

    let tree = build_tree(&all, root_pid);
    let count = count_tree(&tree);

    let state = ProcessState {
        processes: tree,
        total_count: count,
        warnings: None,
    };

    Ok(UnifiedResult::ok(
        format!("Process tree from PID {root_pid}: {count} processes."),
        serde_json::to_value(&state)?,
    ))
}

async fn observe_open_files(target: Option<&str>) -> Result<UnifiedResult> {
    let pid = match target {
        Some(p) => p,
        None => return Ok(UnifiedResult::err(
            "missing_target",
            "PID required for open_files scope. Use --target <pid>.",
        )),
    };

    let result = exec("lsof", &["-p", pid], ExecOpts::default()).await?;
    let count = result.stdout.lines().count().saturating_sub(1); // minus header

    Ok(UnifiedResult::ok(
        format!("PID {pid} has {count} open file descriptors."),
        json!({
            "pid": pid,
            "open_files_count": count,
            "sample": result.stdout.lines().skip(1).take(20).collect::<Vec<_>>(),
        }),
    ))
}

async fn observe_listening_ports() -> Result<UnifiedResult> {
    let result = exec(
        "lsof",
        &["-i", "-P", "-n", "-sTCP:LISTEN"],
        ExecOpts::default(),
    )
    .await?;

    // Deduplicate by (pid, port) — lsof shows IPv4 and IPv6 as separate rows
    let mut seen = HashSet::new();
    let mut entries: Vec<serde_json::Value> = Vec::new();

    for line in result.stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        let name = parts[0];
        let pid = parts[1];
        let addr = parts[8];

        // Skip ESTABLISHED connections (contain "->")
        if addr.contains("->") {
            continue;
        }

        // Extract port from addr like *:8080 or 127.0.0.1:3000 or [::1]:5432
        let port = addr.rsplit(':').next().unwrap_or("");

        // Skip wildcard-only entries (*:*)
        if port == "*" {
            continue;
        }

        // Deduplicate by (pid, port)
        let key = format!("{pid}:{port}");
        if !seen.insert(key) {
            continue;
        }

        // Resolve full process name from PID (lsof truncates to ~9 chars)
        let full_name = resolve_process_name(pid).await.unwrap_or_else(|| name.to_string());

        // Normalize bind address for display
        let bind = if addr.starts_with('*') {
            "0.0.0.0".to_string()
        } else if addr.starts_with("[::") {
            "[::]".to_string()
        } else {
            addr.rsplit_once(':').map(|(h, _)| h.to_string()).unwrap_or_default()
        };

        entries.push(json!({
            "process": full_name,
            "pid": pid.parse::<u32>().unwrap_or(0),
            "port": port.parse::<u16>().unwrap_or(0),
            "bind": bind,
        }));
    }

    let count = entries.len();
    Ok(UnifiedResult::ok(
        format!("{count} listening ports found."),
        json!({ "listening": entries }),
    ))
}

/// Resolve full process name from PID via ps (lsof truncates names).
async fn resolve_process_name(pid: &str) -> Option<String> {
    let r = exec("ps", &["-p", pid, "-o", "comm="], ExecOpts::default()).await.ok()?;
    let name = r.stdout.trim();
    if name.is_empty() {
        None
    } else {
        Some(basename(name).to_string())
    }
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    params: Option<&serde_json::Value>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    let pid = target.ok_or_else(|| anyhow::anyhow!("PID required for {action}"))?;

    match action {
        "kill_graceful" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would send SIGTERM to PID {pid}."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec("kill", &["-TERM", pid], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("SIGTERM sent to PID {pid}.")
                } else {
                    format!("Failed to signal PID {pid}: {}", result.stderr.trim())
                },
                json!({"action": "kill_graceful", "target": pid, "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec![format!("verify(process_stopped, target: \"{pid}\")")]))
        }
        "kill_force" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would send SIGKILL to PID {pid}."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec("kill", &["-9", pid], ExecOpts::default()).await?;
            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("SIGKILL sent to PID {pid}.")
                } else {
                    format!("Failed to kill PID {pid}: {}", result.stderr.trim())
                },
                json!({"action": "kill_force", "target": pid, "success": result.success()}),
            )
            .with_risk(Risk::High)
            .with_suggestions(vec![format!("verify(process_stopped, target: \"{pid}\")")]))
        }
        "set_priority" => {
            let priority = params
                .and_then(|p| p.get("priority"))
                .and_then(|p| p.as_i64())
                .unwrap_or(0);

            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would renice PID {pid} to priority {priority}."),
                    json!({"dry_run": true, "priority": priority}),
                ));
            }

            let pri_str = priority.to_string();
            let result = exec(
                "renice",
                &[&pri_str, "-p", pid],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("PID {pid} priority set to {priority}.")
                } else {
                    format!("Failed to renice PID {pid}: {}", result.stderr.trim())
                },
                json!({"action": "set_priority", "target": pid, "priority": priority, "success": result.success()}),
            )
            .with_risk(Risk::Low)
            .with_suggestions(vec![format!("verify(process_running, target: \"{pid}\")")]))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown process action: {action}"),
        )),
    }
}

pub async fn verify_running(pid: &str) -> Result<UnifiedResult> {
    let result = exec("kill", &["-0", pid], ExecOpts::default()).await?;
    let running = result.success();

    Ok(UnifiedResult::ok(
        if running {
            format!("Process {pid} is running.")
        } else {
            format!("Process {pid} is NOT running.")
        },
        json!({
            "check": "process_running",
            "target": pid,
            "passed": running,
        }),
    ))
}

pub async fn verify_stopped(pid: &str) -> Result<UnifiedResult> {
    let result = exec("kill", &["-0", pid], ExecOpts::default()).await?;
    let stopped = !result.success();

    Ok(UnifiedResult::ok(
        if stopped {
            format!("Process {pid} is stopped.")
        } else {
            format!("Process {pid} is still running.")
        },
        json!({
            "check": "process_stopped",
            "target": pid,
            "passed": stopped,
        }),
    ))
}

pub async fn verify_port_free(port: u16) -> Result<UnifiedResult> {
    let port_str = format!(":{port}");
    let result = exec(
        "lsof",
        &["-i", &port_str, "-P", "-n", "-sTCP:LISTEN"],
        ExecOpts::default(),
    )
    .await?;

    // Port is free if lsof returns nothing (exit code 1 = no matches)
    let free = !result.success() || result.stdout.lines().count() <= 1;

    Ok(UnifiedResult::ok(
        if free {
            format!("Port {port} is free.")
        } else {
            format!("Port {port} is still in use.")
        },
        json!({
            "check": "port_free",
            "target": port,
            "passed": free,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

/// Extract basename from a full path: "/usr/libexec/logd" → "logd"
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Round to 1 decimal place to avoid serialization noise
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn parse_ps_output(output: &str) -> Vec<ProcessInfo> {
    output
        .lines()
        .skip(1) // header
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                let pid = parts[0].parse::<u32>().ok()?;
                let ppid = parts[1].parse::<u32>().ok()?;
                let cpu = parts[2].parse::<f64>().ok();
                let mem_pct = parts[3].parse::<f64>().ok();
                let rss_kb = parts[4].parse::<u64>().ok();
                let stat = parts[5];
                let user = parts[6].to_string();
                let full_cmd = parts[7..].join(" ");

                Some(ProcessInfo {
                    pid,
                    ppid,
                    name: basename(&full_cmd).to_string(),
                    user: Some(user),
                    status: parse_stat(stat),
                    cpu_percent: cpu.map(round1),
                    memory_bytes: rss_kb.map(|k| k * 1024),
                    memory_percent: mem_pct.map(round1),
                    command: Some(full_cmd),
                    started_at: None,
                    children: None,
                    open_files_count: None,
                    listening_ports: None,
                })
            } else {
                None
            }
        })
        .collect()
}

fn parse_stat(stat: &str) -> ProcessStatus {
    match stat.chars().next() {
        Some('R') => ProcessStatus::Running,
        Some('S') => ProcessStatus::Sleeping,
        Some('Z') => ProcessStatus::Zombie,
        Some('T') => ProcessStatus::Stopped,
        Some('I') => ProcessStatus::Idle,
        _ => ProcessStatus::Unknown,
    }
}

fn build_tree(all: &[ProcessInfo], root_pid: u32) -> Vec<ProcessInfo> {
    all.iter()
        .filter(|p| p.pid == root_pid)
        .map(|p| {
            let mut node = p.clone();
            let children: Vec<ProcessInfo> = all
                .iter()
                .filter(|c| c.ppid == root_pid && c.pid != root_pid)
                .flat_map(|c| build_tree(all, c.pid))
                .collect();
            if !children.is_empty() {
                node.children = Some(children);
            }
            node
        })
        .collect()
}

fn count_tree(nodes: &[ProcessInfo]) -> u32 {
    nodes.iter().map(|n| {
        1 + n.children.as_ref().map(|c| count_tree(c)).unwrap_or(0)
    }).sum()
}
