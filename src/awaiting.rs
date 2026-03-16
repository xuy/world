//! Await — block until a condition becomes true.
//!
//! `world await <domain> <condition> --target T` blocks until the
//! specified verify check passes, then returns the result.
//!
//! Where possible, uses OS-native event mechanisms (kqueue EVFILT_PROC
//! for process exit) instead of polling. Falls back to polling verify
//! with exponential backoff for conditions without native event support.
//!
//! This is the missing link in the act→verify loop. Instead of:
//!   act → sleep → verify → sleep → verify → ...
//! agents can:
//!   act → await condition
//!
//! The world knows which OS mechanism to use. The agent doesn't care.

use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::verify::VerifyCheck;
use crate::contracts::UnifiedResult;
use crate::domains;
use crate::plugin::DomainPlugin;

// ─── Condition resolution ───────────────────────────────────────────────────

/// Map (domain, condition_name) → VerifyCheck.
///
/// Conditions are the domain-local names for verify checks.
/// The domain scopes which conditions are valid.
pub fn resolve_condition(domain: &str, condition: &str) -> Option<VerifyCheck> {
    match (domain, condition) {
        // Network
        ("network", "host_reachable") => Some(VerifyCheck::HostReachable),
        ("network", "dns_resolves") => Some(VerifyCheck::DnsResolves),
        ("network", "internet_reachable") => Some(VerifyCheck::InternetReachable),
        ("network", "port_open") => Some(VerifyCheck::PortOpen),

        // Service
        ("service", "healthy") => Some(VerifyCheck::ServiceHealthy),

        // Process
        ("process", "running") => Some(VerifyCheck::ProcessRunning),
        ("process", "stopped") => Some(VerifyCheck::ProcessStopped),
        ("process", "port_free") => Some(VerifyCheck::PortFree),

        // Container
        ("container", "running") => Some(VerifyCheck::ContainerRunning),
        ("container", "healthy") => Some(VerifyCheck::ContainerHealthy),
        ("container", "image_exists") => Some(VerifyCheck::ImageExists),
        ("container", "volume_exists") => Some(VerifyCheck::VolumeExists),

        // Disk
        ("disk", "writable") => Some(VerifyCheck::DiskWritable),

        // Brew
        ("brew", "installed") => Some(VerifyCheck::BrewInstalled),

        // Printer
        ("printer", "prints") => Some(VerifyCheck::PrinterPrints),

        _ => None,
    }
}

/// List valid conditions for a domain.
pub fn conditions_for(domain: &str) -> &'static [&'static str] {
    match domain {
        "network" => &["host_reachable", "dns_resolves", "internet_reachable", "port_open"],
        "service" => &["healthy"],
        "process" => &["running", "stopped", "port_free"],
        "container" => &["running", "healthy", "image_exists", "volume_exists"],
        "disk" => &["writable"],
        "brew" => &["installed"],
        "printer" => &["prints"],
        _ => &[],
    }
}

// ─── Await execution ────────────────────────────────────────────────────────

/// Configuration for an await operation.
pub struct AwaitOpts {
    /// Maximum time to wait before giving up.
    pub timeout_sec: u32,
    /// Initial polling interval (doubles up to max_interval_ms).
    pub initial_interval_ms: u64,
    /// Maximum polling interval.
    pub max_interval_ms: u64,
}

impl Default for AwaitOpts {
    fn default() -> Self {
        Self {
            timeout_sec: 60,
            initial_interval_ms: 250,
            max_interval_ms: 5000,
        }
    }
}

/// Block until a verify check passes, or timeout.
///
/// Uses OS-native event mechanisms where available (kqueue for process
/// exit on macOS), falls back to polling with exponential backoff.
pub async fn await_condition(
    platform: Platform,
    check: VerifyCheck,
    target: Option<&str>,
    params: Option<&Value>,
    opts: AwaitOpts,
) -> Result<UnifiedResult> {
    // Try native event mechanism first
    #[cfg(target_os = "macos")]
    if let Some(result) = try_native_await(check, target, &opts).await {
        return result;
    }

    // Fall back to polling with exponential backoff
    poll_until(platform, check, target, params, &opts).await
}

/// Try to use a native OS event mechanism. Returns None if no native
/// mechanism is available for this check.
#[cfg(target_os = "macos")]
async fn try_native_await(
    check: VerifyCheck,
    target: Option<&str>,
    opts: &AwaitOpts,
) -> Option<Result<UnifiedResult>> {
    match check {
        VerifyCheck::ProcessStopped => {
            let pid = target?.parse::<i32>().ok()?;
            Some(kqueue_await_exit(pid, opts.timeout_sec).await)
        }
        _ => None,
    }
}

/// Use kqueue EVFILT_PROC to await process exit — microsecond notification.
#[cfg(target_os = "macos")]
async fn kqueue_await_exit(pid: i32, timeout_sec: u32) -> Result<UnifiedResult> {
    use serde_json::json;
    use std::time::Instant;

    let start = Instant::now();

    // Run kqueue in a blocking thread to avoid tying up the async runtime
    let result = tokio::task::spawn_blocking(move || {
        unsafe {
            let kq = libc::kqueue();
            if kq < 0 {
                return Err(anyhow::anyhow!("kqueue() failed"));
            }

            // Register interest in process exit
            let mut changelist = libc::kevent {
                ident: pid as usize,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_ONESHOT,
                fflags: libc::NOTE_EXIT,
                data: 0,
                udata: std::ptr::null_mut(),
            };

            let mut eventlist = libc::kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            };

            let timeout = libc::timespec {
                tv_sec: timeout_sec as i64,
                tv_nsec: 0,
            };

            let n = libc::kevent(
                kq,
                &mut changelist as *mut _,
                1,
                &mut eventlist as *mut _,
                1,
                &timeout as *const _,
            );

            libc::close(kq);

            if n < 0 {
                // kevent error — process might already be dead
                // Check with kill -0
                if libc::kill(pid, 0) != 0 {
                    return Ok(true); // already dead
                }
                return Err(anyhow::anyhow!("kevent() failed"));
            }

            Ok(n > 0) // n > 0 means event fired (process exited)
        }
    })
    .await??;

    let elapsed_ms = start.elapsed().as_millis() as u64;

    if result {
        Ok(UnifiedResult::ok(
            format!("Process {pid} exited (detected via kqueue in {elapsed_ms}ms)."),
            json!({
                "check": "process_stopped",
                "target": pid,
                "passed": true,
                "mechanism": "kqueue",
                "elapsed_ms": elapsed_ms,
            }),
        ))
    } else {
        Ok(UnifiedResult::ok(
            format!("Timed out waiting for process {pid} to exit ({timeout_sec}s)."),
            json!({
                "check": "process_stopped",
                "target": pid,
                "passed": false,
                "mechanism": "kqueue",
                "elapsed_ms": elapsed_ms,
                "timeout": true,
            }),
        ))
    }
}

/// Poll a verify check with exponential backoff until it passes or timeout.
async fn poll_until(
    platform: Platform,
    check: VerifyCheck,
    target: Option<&str>,
    params: Option<&Value>,
    opts: &AwaitOpts,
) -> Result<UnifiedResult> {
    use serde_json::json;
    use std::time::Instant;

    let start = Instant::now();
    let deadline = std::time::Duration::from_secs(opts.timeout_sec as u64);
    let mut interval = opts.initial_interval_ms;
    let mut polls = 0u32;

    loop {
        polls += 1;

        let result = domains::dispatch_verify(
            platform,
            check,
            target,
            params,
            opts.timeout_sec,
        )
        .await?;

        // Check if the condition passed
        let passed = result
            .details
            .as_ref()
            .and_then(|d| d.get("passed"))
            .and_then(|p| p.as_bool())
            .unwrap_or(false);

        if passed {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            // Enrich the result with await metadata
            let mut details = result.details.unwrap_or(json!({}));
            if let Some(obj) = details.as_object_mut() {
                obj.insert("mechanism".into(), json!("polling"));
                obj.insert("elapsed_ms".into(), json!(elapsed_ms));
                obj.insert("polls".into(), json!(polls));
            }
            return Ok(UnifiedResult::ok(
                format!(
                    "{} (awaited {elapsed_ms}ms, {polls} polls).",
                    result.output
                ),
                details,
            ));
        }

        // Check timeout
        if start.elapsed() >= deadline {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            return Ok(UnifiedResult::ok(
                format!(
                    "Timed out after {}s waiting for {} ({polls} polls).",
                    opts.timeout_sec,
                    check.as_str()
                ),
                json!({
                    "check": check.as_str(),
                    "target": target,
                    "passed": false,
                    "timeout": true,
                    "mechanism": "polling",
                    "elapsed_ms": elapsed_ms,
                    "polls": polls,
                }),
            ));
        }

        // Exponential backoff
        tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
        interval = (interval * 2).min(opts.max_interval_ms);
    }
}

// ─── Plugin conditions ─────────────────────────────────────────────────────

/// A condition that can be checked against a plugin's observation result.
///
/// Plugin domains (browser, ssh, etc.) define conditions that are evaluated
/// by polling observe and checking the result. This avoids extending the
/// native VerifyCheck enum for every plugin.
pub enum PluginCondition {
    /// True when a field in details is non-null and non-empty.
    FieldPresent(&'static str),
    /// True when details.field contains the target string.
    FieldContains(&'static str),
}

impl PluginCondition {
    /// Check whether the condition holds for the given observation result.
    fn check(&self, details: &Value, target: Option<&str>) -> bool {
        match self {
            PluginCondition::FieldPresent(field) => {
                details.get(field).is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            }
            PluginCondition::FieldContains(field) => {
                let Some(needle) = target else { return false };
                details
                    .get(field)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.contains(needle))
            }
        }
    }
}

/// Resolve a plugin condition by domain and condition name.
/// Returns None if the condition is not recognized.
pub fn resolve_plugin_condition(domain: &str, condition: &str) -> Option<PluginCondition> {
    match (domain, condition) {
        // Browser
        ("browser", "loaded") => Some(PluginCondition::FieldPresent("url")),
        ("browser", "title_contains") => Some(PluginCondition::FieldContains("title")),

        // SSH
        ("ssh", "connected") => Some(PluginCondition::FieldPresent("host")),

        _ => None,
    }
}

/// List valid plugin conditions for a domain.
pub fn plugin_conditions_for(domain: &str) -> &'static [&'static str] {
    match domain {
        "browser" => &["loaded", "title_contains"],
        "ssh" => &["connected"],
        _ => &[],
    }
}

/// Poll a plugin's observe until a condition holds, with exponential backoff.
pub async fn await_plugin(
    plugin: &dyn DomainPlugin,
    condition: PluginCondition,
    condition_name: &str,
    target: Option<&str>,
    opts: AwaitOpts,
) -> Result<UnifiedResult> {
    use serde_json::json;
    use std::time::Instant;

    let start = Instant::now();
    let deadline = std::time::Duration::from_secs(opts.timeout_sec as u64);
    let mut interval = opts.initial_interval_ms;
    let mut polls = 0u32;

    loop {
        polls += 1;

        let result = plugin.observe(None, None, None).await?;

        if let Some(details) = &result.details {
            if condition.check(details, target) {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                return Ok(UnifiedResult::ok(
                    format!(
                        "Condition '{condition_name}' met (awaited {elapsed_ms}ms, {polls} polls)."
                    ),
                    json!({
                        "check": condition_name,
                        "target": target,
                        "passed": true,
                        "mechanism": "polling",
                        "elapsed_ms": elapsed_ms,
                        "polls": polls,
                        "details": details,
                    }),
                ));
            }
        }

        if start.elapsed() >= deadline {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            return Ok(UnifiedResult::ok(
                format!(
                    "Timed out after {}s waiting for '{condition_name}' ({polls} polls).",
                    opts.timeout_sec
                ),
                json!({
                    "check": condition_name,
                    "target": target,
                    "passed": false,
                    "timeout": true,
                    "mechanism": "polling",
                    "elapsed_ms": elapsed_ms,
                    "polls": polls,
                }),
            ));
        }

        tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
        interval = (interval * 2).min(opts.max_interval_ms);
    }
}
