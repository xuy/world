use anyhow::Result;

use crate::contracts::UnifiedResult;
use crate::execution::{exec, ExecOpts};
use crate::schemas::{LogEntries, LogEntry};

pub async fn observe(
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    let since_str = since.unwrap_or("1h");
    let max_entries = limit.unwrap_or(50);

    let mut args = vec![
        "show".to_string(),
        "--style".to_string(),
        "compact".to_string(),
        "--last".to_string(),
        since_str.to_string(),
    ];

    // Build predicate based on target
    // If target is "recent_errors", "recent_warnings", or "matching", use it as the predicate selector.
    // Otherwise treat target as a subsystem filter. Default to "recent_errors" if no target.
    let predicate = match target {
        Some("recent_errors") | None => "messageType == error".to_string(),
        Some("recent_warnings") => "messageType >= default".to_string(),
        Some("matching") => "messageType == error".to_string(),
        Some(t) if t.starts_with("matching:") => {
            let pattern = &t["matching:".len()..];
            format!("eventMessage CONTAINS \"{pattern}\"")
        }
        Some(subsystem) => {
            format!("(subsystem == \"{subsystem}\") AND (messageType == error)")
        }
    };

    args.push("--predicate".to_string());
    args.push(predicate);

    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = exec(
        "log",
        &args_refs,
        ExecOpts { timeout_sec: 15, ..Default::default() },
    )
    .await?;

    let entries = parse_log_output(&result.stdout, max_entries);
    let total = entries.entries.len() as u32;

    let summary = if total == 0 {
        "No matching log entries found.".into()
    } else {
        format!("{total} log entries found (last {since_str}).")
    };

    Ok(UnifiedResult::ok(summary, serde_json::to_value(&entries)?))
}

fn parse_log_output(output: &str, limit: u32) -> LogEntries {
    let mut entries = Vec::new();
    let total_lines = output.lines().count();

    for line in output.lines().take(limit as usize) {
        // macOS log compact format: "timestamp  thread  type  activity  pid  process: message"
        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.len() >= 2 {
            let message = parts[1].trim().to_string();
            let meta = parts[0];
            let meta_parts: Vec<&str> = meta.split_whitespace().collect();

            let timestamp = if meta_parts.len() >= 2 {
                format!("{} {}", meta_parts[0], meta_parts[1])
            } else {
                meta_parts.first().unwrap_or(&"").to_string()
            };

            let source = meta_parts.last().unwrap_or(&"").to_string();
            let level = if meta_parts.len() >= 3 {
                meta_parts[2].to_string()
            } else {
                "info".to_string()
            };

            entries.push(LogEntry {
                timestamp,
                level,
                source,
                message,
            });
        }
    }

    LogEntries {
        total_matched: total_lines as u32,
        truncated: Some(total_lines as u32 > limit),
        entries,
    }
}
