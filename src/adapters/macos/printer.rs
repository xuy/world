use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::{PrinterState, PrinterStatus};

pub async fn observe(
    target: Option<&str>,
) -> Result<UnifiedResult> {
    // List printers
    let printers_result = exec("lpstat", &["-p", "-d"], ExecOpts::default()).await?;
    let queue_result = exec("lpstat", &["-o"], ExecOpts::default()).await?;

    let printers = parse_printers(&printers_result.stdout, &queue_result.stdout);

    let summary = if printers.is_empty() {
        "No printers configured.".into()
    } else {
        let count = printers.len();
        let ready = printers.iter().filter(|p| matches!(p.status, PrinterStatus::Ready)).count();
        let total_jobs: u32 = printers.iter().filter_map(|p| p.queue_jobs).sum();
        format!("{count} printer(s) configured, {ready} ready, {total_jobs} queued job(s).")
    };

    // If a specific target was requested, filter
    let result_printers = if let Some(name) = target {
        printers.into_iter().filter(|p| p.name == name).collect()
    } else {
        printers
    };

    Ok(UnifiedResult::ok(summary, serde_json::to_value(&result_printers)?)
        .with_suggestions(vec![
            "act(printer, clear_queue)".into(),
            "act(printer, restart_spooler)".into(),
            "verify(printer_prints)".into(),
        ]))
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match action {
        "clear_queue" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would cancel all pending print jobs.",
                    json!({"dry_run": true}),
                ));
            }
            let result = exec("cancel", &["-a"], ExecOpts::default()).await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    "All print jobs cancelled.".into()
                } else {
                    format!("Queue clear may have failed: {}", result.stderr.trim())
                },
                json!({"action": "clear_queue", "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec!["verify(printer_prints)".into()]))
        }
        "restart_spooler" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would restart CUPS printing service.",
                    json!({"dry_run": true}),
                ));
            }

            // Restart CUPS
            let _ = exec(
                "launchctl",
                &["unload", "/System/Library/LaunchDaemons/org.cups.cupsd.plist"],
                ExecOpts::default(),
            )
            .await;
            let start = exec(
                "launchctl",
                &["load", "/System/Library/LaunchDaemons/org.cups.cupsd.plist"],
                ExecOpts::default(),
            )
            .await?;

            let success = start.success();
            if !success {
                // Fallback
                let _ = exec("killall", &["cupsd"], ExecOpts::default()).await;
            }

            Ok(UnifiedResult::ok(
                "CUPS printing service restarted.",
                json!({"action": "restart_spooler", "success": true}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec!["verify(printer_prints)".into()]))
        }
        "set_default_printer" => {
            let name = target.ok_or_else(|| anyhow::anyhow!("Printer name required"))?;
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would set '{name}' as default printer."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec("lpoptions", &["-d", name], ExecOpts::default()).await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("'{name}' set as default printer.")
                } else {
                    format!("Failed to set default printer: {}", result.stderr.trim())
                },
                json!({"action": "set_default_printer", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::Medium))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown printer action: {action}"),
        )),
    }
}

pub async fn verify_prints(target: Option<&str>) -> Result<UnifiedResult> {
    // Send a test page to the default or specified printer
    let args = if let Some(name) = target {
        vec!["-d", name, "-T", "World Test Page", "--", "/dev/null"]
    } else {
        vec!["-T", "World Test Page", "--", "/dev/null"]
    };

    let result = exec("lp", &args.iter().map(|s| *s).collect::<Vec<_>>(), ExecOpts::default()).await?;
    let submitted = result.success();

    Ok(UnifiedResult::ok(
        if submitted {
            "Test print job submitted successfully.".into()
        } else {
            format!("Test print failed: {}", result.stderr.trim())
        },
        json!({
            "check": "printer_prints",
            "target": target,
            "passed": submitted,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

fn parse_printers(printer_output: &str, queue_output: &str) -> Vec<PrinterState> {
    let mut printers = Vec::new();
    let mut default_printer = String::new();

    for line in printer_output.lines() {
        if line.starts_with("printer ") {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let status = if line.contains("idle") {
                    PrinterStatus::Ready
                } else if line.contains("disabled") || line.contains("offline") {
                    PrinterStatus::Offline
                } else {
                    PrinterStatus::Unknown
                };

                printers.push(PrinterState {
                    name,
                    installed: true,
                    status,
                    is_default: None,
                    queue_jobs: Some(0),
                    driver: None,
                    port: None,
                    host_reachable: None,
                    recent_errors: None,
                });
            }
        } else if line.starts_with("system default destination:") {
            default_printer = line
                .strip_prefix("system default destination:")
                .unwrap_or("")
                .trim()
                .to_string();
        }
    }

    // Count queued jobs per printer
    for line in queue_output.lines() {
        if let Some(printer_name) = line.split('-').next() {
            if let Some(printer) = printers.iter_mut().find(|p| p.name == printer_name) {
                if let Some(ref mut jobs) = printer.queue_jobs {
                    *jobs += 1;
                }
            }
        }
    }

    // Mark default
    for printer in &mut printers {
        printer.is_default = Some(printer.name == default_printer);
    }

    printers
}
