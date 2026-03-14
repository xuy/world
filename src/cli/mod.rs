pub mod addons;
pub mod confirm;
pub mod output;
pub mod plugins;
pub mod verbs;

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::Platform;
use crate::contracts::UnifiedResult;
use crate::plugin::PluginRegistry;
use crate::telemetry::{TelemetryLog, ToolCallEvent};

use confirm::confirm_high_risk;
use output::{format_result, format_spec, format_tools, OutputMode};

// ─── CLI definition ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "world", version, about = "Observe \u{00b7} Act — a POMDP interface for agents")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Force JSON output
    #[arg(long, global = true)]
    pub json: bool,

    /// Force pretty (human-readable) output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Quiet mode — exit code only, no output
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Observe state: world observe DOMAIN [TARGET]
    Observe {
        /// Domain (process, network, container, service, disk, ...)
        domain: String,
        /// What to observe — the domain interprets this
        /// (e.g. top_cpu, 1234, interfaces, images, my-nginx/logs)
        target: Option<String>,
        /// Time filter (e.g. 1h, 30m) — for log domain
        #[arg(long)]
        since: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },

    /// Act on the world — finite verbs on schema paths (A)
    Act {
        /// Domain (network, service, disk, ... or any plugin)
        domain: String,
        /// Target path (e.g. dns_cache, interfaces.en0, nginx, requests)
        target: String,
        /// Verb (reset, set, add, remove, restart, enable, disable, clear)
        verb: String,
        /// Verb arguments as key=value pairs (e.g. mode=auto, version=latest)
        #[arg(num_args = 0..)]
        args: Vec<String>,

        // ── Options ─────────────────────────────────────────────
        /// Preview without executing
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation for high-risk actions
        #[arg(long)]
        yes: bool,
    },

    /// Show POMDP spec — observations + actions + add-on contributions
    Spec {
        /// Domain to show (omit for all domains)
        domain: Option<String>,
        /// Show core only, no add-on contributions
        #[arg(long)]
        core: bool,
    },

    /// List registered add-ons (verify, policy, ...)
    Addons {
        /// Show spec for a specific add-on
        name: Option<String>,
        /// Limit to a specific domain
        #[arg(long)]
        domain: Option<String>,
    },

    /// Await a condition — block until a verify check passes
    ///
    /// Uses OS-native event mechanisms where available (kqueue for
    /// process exit on macOS), falls back to polling with exponential
    /// backoff. Always has a timeout.
    /// Await a condition: world await DOMAIN CONDITION [TARGET]
    Await {
        /// Domain (process, container, network, ...)
        domain: String,
        /// Condition to wait for (stopped, running, healthy, ...)
        condition: String,
        /// Target for the check (PID, hostname, container ID, ...)
        target: Option<String>,
        /// Maximum seconds to wait (default: 60)
        #[arg(long, default_value = "60")]
        timeout: u32,
    },

    /// Sample an observation over time — repeated observe + reduce
    ///
    /// Takes N snapshots at a fixed interval, then reduces numeric fields
    /// into statistics (mean, min, max, delta, rate). Domain-agnostic —
    /// works with any observable domain.
    /// Sample over time: world sample DOMAIN [TARGET]
    Sample {
        /// Domain to observe
        domain: String,
        /// What to observe (same as observe target)
        target: Option<String>,
        /// Number of samples to take
        #[arg(long, default_value = "5")]
        count: u32,
        /// Interval between samples (e.g. 2s, 500ms, 1m)
        #[arg(long, default_value = "2s")]
        interval: String,
        /// Maximum number of results per sample
        #[arg(long)]
        limit: Option<u32>,
    },

    /// List tools and universal verb set
    Tools,

    /// Generate shell completions
    Completions {
        /// Shell: bash, zsh, fish
        shell: clap_complete::Shell,
    },
}

impl Cli {
    pub fn output_mode(&self) -> OutputMode {
        if self.quiet {
            OutputMode::Quiet
        } else if self.json {
            OutputMode::Json
        } else if self.pretty {
            OutputMode::Pretty
        } else if atty::is(atty::Stream::Stdout) {
            OutputMode::Pretty
        } else {
            OutputMode::Json
        }
    }
}

// ─── Plugin discovery ───────────────────────────────────────────────────────

fn plugins_dir() -> PathBuf {
    // Look for plugins/ next to the binary, then fall back to cwd
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let dir = parent.join("plugins");
            if dir.is_dir() {
                return dir;
            }
            // Also check two levels up (for cargo run from target/debug/)
            if let Some(grandparent) = parent.parent().and_then(|p| p.parent()) {
                let dir = grandparent.join("plugins");
                if dir.is_dir() {
                    return dir;
                }
            }
        }
    }
    PathBuf::from("plugins")
}

// ─── Registry construction ──────────────────────────────────────────────────

fn build_registry(platform: Platform) -> PluginRegistry {
    let mut registry = PluginRegistry::new();

    // Native Rust plugins (performance)
    for plugin in crate::plugin::native_plugins(platform) {
        registry.register(plugin);
    }

    // External subprocess plugins (extensibility)
    for plugin in plugins::load_all(&plugins_dir()) {
        registry.register(Box::new(plugin));
    }

    registry
}

// ─── Dispatch ───────────────────────────────────────────────────────────────

pub async fn run(cli: Cli) -> ExitCode {
    let platform = Platform::current();
    let telemetry = Arc::new(TelemetryLog::new());
    let mode = cli.output_mode();
    let registry = build_registry(platform);

    match cli.command {
        Command::Observe {
            domain,
            target,
            since,
            limit,
        } => {
            run_observe(&registry, &telemetry, mode, &domain, target, since, limit).await
        }

        Command::Act {
            domain,
            target,
            verb,
            args,
            dry_run,
            yes,
        } => {
            run_act(
                &registry, &telemetry, mode, &domain, &target, &verb, &args, dry_run, yes,
            )
            .await
        }

        Command::Await {
            domain,
            condition,
            target,
            timeout,
        } => {
            run_await(mode, platform, &domain, &condition, target, timeout).await
        }

        Command::Sample {
            domain,
            target,
            count,
            interval,
            limit,
        } => {
            run_sample(&registry, mode, &domain, count, &interval, target, limit).await
        }

        Command::Spec { domain, core } => {
            format_spec(mode, domain.as_deref(), core, &registry);
            ExitCode::from(0)
        }

        Command::Addons { name, domain } => {
            output::format_addons(mode, name.as_deref(), domain.as_deref());
            ExitCode::from(0)
        }

        Command::Tools => {
            format_tools(mode);
            ExitCode::from(0)
        }

        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "world", &mut std::io::stdout());
            ExitCode::from(0)
        }
    }
}

// ─── Subcommand handlers ────────────────────────────────────────────────────

async fn run_observe(
    registry: &PluginRegistry,
    telemetry: &Arc<TelemetryLog>,
    mode: OutputMode,
    domain_str: &str,
    target: Option<String>,
    since: Option<String>,
    limit: Option<u32>,
) -> ExitCode {
    let plugin = match registry.get(domain_str) {
        Some(p) => p,
        None => {
            let r = UnifiedResult::err("invalid_domain", format!("Unknown domain: {domain_str}"));
            format_result(mode, &r);
            return ExitCode::from(1);
        }
    };

    let start = Instant::now();

    let result = plugin
        .observe(
            target.as_deref(),
            since.as_deref(),
            limit,
        )
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(r) => {
            let mut event = ToolCallEvent::new("observe");
            event.domain = Some(domain_str.to_string());
            event.target = target;
            event.duration_ms = duration_ms;
            event.success = r.error.is_none();
            event.risk = r.risk;
            telemetry.record(event);

            let has_error = r.error.is_some();
            format_result(mode, &r);
            if has_error {
                ExitCode::from(1)
            } else {
                ExitCode::from(0)
            }
        }
        Err(e) => {
            let r = UnifiedResult::err("execution_error", e.to_string());
            format_result(mode, &r);
            ExitCode::from(1)
        }
    }
}

async fn run_act(
    registry: &PluginRegistry,
    telemetry: &Arc<TelemetryLog>,
    mode: OutputMode,
    domain_str: &str,
    target: &str,
    verb: &str,
    args: &[String],
    dry_run: bool,
    yes: bool,
) -> ExitCode {
    let plugin = match registry.get(domain_str) {
        Some(p) => p,
        None => {
            let r = UnifiedResult::err("invalid_domain", format!("Unknown domain: {domain_str}"));
            format_result(mode, &r);
            return ExitCode::from(1);
        }
    };

    // Resolve verb via plugin's dispatch table
    let resolved = match verbs::resolve(domain_str, plugin.dispatch_entries(), target, verb, args) {
        Ok(r) => r,
        Err(msg) => {
            let result = UnifiedResult::err("invalid_action", msg);
            format_result(mode, &result);
            return ExitCode::from(1);
        }
    };

    // Check allowlist
    if !plugin.is_allowed(&resolved.handler) {
        let result = UnifiedResult::err(
            "action_not_allowed",
            format!(
                "Action '{}' is not allowed for domain {domain_str}.",
                resolved.handler
            ),
        );
        format_result(mode, &result);
        return ExitCode::from(1);
    }

    // Risk classification + consent gate
    let risk = plugin.classify_risk(&resolved.handler);
    if !dry_run && risk.requires_consent() && !yes {
        if !confirm_high_risk(mode, domain_str, target, verb) {
            return ExitCode::from(4);
        }
    }

    let start = Instant::now();

    let result = plugin
        .act(
            &resolved.handler,
            resolved.target.as_deref(),
            resolved.params.as_ref(),
            dry_run,
        )
        .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(r) => {
            let mut event = ToolCallEvent::new("act");
            event.domain = Some(domain_str.to_string());
            event.action = Some(format!("{target} {verb}"));
            event.target = resolved.target;
            event.duration_ms = duration_ms;
            event.success = r.error.is_none();
            event.risk = r.risk;
            telemetry.record(event);

            let has_error = r.error.is_some();
            format_result(mode, &r);
            if has_error {
                ExitCode::from(1)
            } else {
                ExitCode::from(0)
            }
        }
        Err(e) => {
            let r = UnifiedResult::err("execution_error", e.to_string());
            format_result(mode, &r);
            ExitCode::from(1)
        }
    }
}

// ─── Await ──────────────────────────────────────────────────────────────────

async fn run_await(
    mode: OutputMode,
    platform: Platform,
    domain: &str,
    condition: &str,
    target: Option<String>,
    timeout_sec: u32,
) -> ExitCode {
    use crate::awaiting;

    let check = match awaiting::resolve_condition(domain, condition) {
        Some(c) => c,
        None => {
            let conditions = awaiting::conditions_for(domain);
            let msg = if conditions.is_empty() {
                format!("Unknown domain '{domain}' or no conditions available.")
            } else {
                format!(
                    "Unknown condition '{condition}' for domain '{domain}'.\nAvailable: {}",
                    conditions.join(", ")
                )
            };
            let r = UnifiedResult::err("invalid_condition", msg);
            format_result(mode, &r);
            return ExitCode::from(1);
        }
    };

    let opts = awaiting::AwaitOpts {
        timeout_sec,
        ..Default::default()
    };

    match awaiting::await_condition(
        platform,
        check,
        target.as_deref(),
        None,
        opts,
    )
    .await
    {
        Ok(r) => {
            let passed = r
                .details
                .as_ref()
                .and_then(|d| d.get("passed"))
                .and_then(|p| p.as_bool())
                .unwrap_or(false);

            format_result(mode, &r);
            if passed {
                ExitCode::from(0)
            } else {
                ExitCode::from(1) // timeout or condition not met
            }
        }
        Err(e) => {
            let r = UnifiedResult::err("await_error", e.to_string());
            format_result(mode, &r);
            ExitCode::from(1)
        }
    }
}

// ─── Sample ─────────────────────────────────────────────────────────────────

async fn run_sample(
    registry: &PluginRegistry,
    mode: OutputMode,
    domain_str: &str,
    count: u32,
    interval: &str,
    target: Option<String>,
    limit: Option<u32>,
) -> ExitCode {
    use crate::sampling;

    let plugin = match registry.get(domain_str) {
        Some(p) => p,
        None => {
            let r = UnifiedResult::err("invalid_domain", format!("Unknown domain: {domain_str}"));
            format_result(mode, &r);
            return ExitCode::from(1);
        }
    };

    let interval_ms = match sampling::parse_duration_ms(interval) {
        Ok(ms) => ms,
        Err(msg) => {
            let r = UnifiedResult::err("invalid_interval", msg);
            format_result(mode, &r);
            return ExitCode::from(1);
        }
    };

    if count < 2 {
        let r = UnifiedResult::err("invalid_count", "Sample count must be at least 2.");
        format_result(mode, &r);
        return ExitCode::from(1);
    }

    // Collect samples
    let start = Instant::now();
    let mut samples: Vec<serde_json::Value> = Vec::with_capacity(count as usize);

    for i in 0..count {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        }

        let result = plugin
            .observe(target.as_deref(), None, limit)
            .await;

        match result {
            Ok(r) => {
                if let Some(details) = r.details {
                    samples.push(details);
                }
            }
            Err(e) => {
                let r = UnifiedResult::err("sample_error", format!("Sample {i} failed: {e}"));
                format_result(mode, &r);
                return ExitCode::from(1);
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let duration_sec = duration_ms as f64 / 1000.0;

    // Reduce
    let reduced = sampling::reduce(&samples, duration_sec, sampling::IDENTITY_KEYS);

    let sample_result = sampling::SampleResult {
        sampling: sampling::SamplingMeta {
            count,
            interval_ms,
            duration_ms,
        },
        result: reduced,
    };

    let r = UnifiedResult::ok(
        format!(
            "{count} samples over {:.1}s (interval: {interval}).",
            duration_sec
        ),
        serde_json::to_value(&sample_result).unwrap_or_default(),
    );
    format_result(mode, &r);
    ExitCode::from(0)
}
