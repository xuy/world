pub mod container;
pub mod disk;
pub mod log;
pub mod network;
pub mod brew;
pub mod printer;
pub mod process;
pub mod service;

use anyhow::Result;
use serde_json::Value;

use crate::adapters::Platform;
use crate::contracts::observe::ObserveDomain;
use crate::contracts::act::ActDomain;
use crate::contracts::verify::VerifyCheck;
use crate::contracts::UnifiedResult;

/// Dispatch an observe call to the correct domain + platform adapter.
pub async fn dispatch_observe(
    platform: Platform,
    domain: ObserveDomain,
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<u32>,
) -> Result<UnifiedResult> {
    match domain {
        ObserveDomain::Network => network::observe(platform, target).await,
        ObserveDomain::Service => service::observe(platform, target).await,
        ObserveDomain::Disk => disk::observe(platform, target).await,
        ObserveDomain::Printer => printer::observe(platform, target).await,
        ObserveDomain::Brew => brew::observe(platform, target).await,
        ObserveDomain::Log => log::observe(platform, target, since, limit).await,
        ObserveDomain::Process => process::observe(platform, target, limit).await,
        ObserveDomain::Container => container::observe(platform, target, limit).await,
        _ => Ok(UnifiedResult::unsupported(domain.as_str())),
    }
}

/// Dispatch an act call.
pub async fn dispatch_act(
    platform: Platform,
    domain: ActDomain,
    action: &str,
    target: Option<&str>,
    params: Option<&Value>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match domain {
        ActDomain::Network => network::act(platform, action, target, params, dry_run).await,
        ActDomain::Service => service::act(platform, action, target, params, dry_run).await,
        ActDomain::Disk => disk::act(platform, action, target, params, dry_run).await,
        ActDomain::Printer => printer::act(platform, action, target, params, dry_run).await,
        ActDomain::Brew => brew::act(platform, action, target, params, dry_run).await,
        ActDomain::Process => process::act(platform, action, target, params, dry_run).await,
        ActDomain::Container => container::act(platform, action, target, params, dry_run).await,
    }
}

/// Dispatch a verify call.
pub async fn dispatch_verify(
    platform: Platform,
    check: VerifyCheck,
    target: Option<&str>,
    params: Option<&Value>,
    timeout_sec: u32,
) -> Result<UnifiedResult> {
    match check {
        VerifyCheck::HostReachable => network::verify_host_reachable(platform, target, timeout_sec).await,
        VerifyCheck::DnsResolves => network::verify_dns_resolves(platform, target, timeout_sec).await,
        VerifyCheck::InternetReachable => network::verify_internet_reachable(platform, timeout_sec).await,
        VerifyCheck::PortOpen => network::verify_port_open(platform, target, params, timeout_sec).await,
        VerifyCheck::ServiceHealthy => service::verify_healthy(platform, target, timeout_sec).await,
        VerifyCheck::PrinterPrints => printer::verify_prints(platform, target, timeout_sec).await,
        VerifyCheck::DiskWritable => disk::verify_writable(platform, target, timeout_sec).await,
        VerifyCheck::BrewInstalled => brew::verify_installed(platform, target, timeout_sec).await,
        VerifyCheck::ProcessRunning => process::verify_running(platform, target, timeout_sec).await,
        VerifyCheck::ProcessStopped => process::verify_stopped(platform, target, timeout_sec).await,
        VerifyCheck::PortFree => process::verify_port_free(platform, target, params, timeout_sec).await,
        VerifyCheck::ContainerRunning => container::verify_running(platform, target, timeout_sec).await,
        VerifyCheck::ContainerHealthy => container::verify_healthy(platform, target, timeout_sec).await,
        VerifyCheck::ImageExists => container::verify_image_exists(platform, target, timeout_sec).await,
        VerifyCheck::VolumeExists => container::verify_volume_exists(platform, target, timeout_sec).await,
    }
}

/// Progressive disclosure: return capability metadata for a domain.
pub fn domain_capabilities(domain: ObserveDomain) -> UnifiedResult {
    let (scopes, actions, verifications, privilege_notes) = match domain {
        ObserveDomain::Network => (
            vec!["interfaces", "routes", "dns", "gateway", "proxy", "internet_status"],
            vec!["flush_dns", "renew_dhcp", "toggle_adapter", "reconnect_wifi", "reset_proxy"],
            vec!["host_reachable", "dns_resolves", "internet_reachable", "port_open"],
            vec!["toggle_adapter may require administrator privileges"],
        ),
        ObserveDomain::Service => (
            vec!["status", "startup_mode", "recent_errors", "dependencies"],
            vec!["start_service", "stop_service", "restart_service"],
            vec!["service_healthy"],
            vec!["Service management may require administrator privileges"],
        ),
        ObserveDomain::Disk => (
            vec!["space", "mounts", "temp_usage", "large_paths"],
            vec!["clear_temp_files", "remove_large_known_caches"],
            vec!["disk_writable"],
            vec![],
        ),
        ObserveDomain::Printer => (
            vec!["status", "queue", "driver", "port", "recent_errors"],
            vec!["clear_queue", "restart_spooler", "set_default_printer"],
            vec!["printer_prints", "host_reachable"],
            vec!["reinstall_printer_driver may require administrator privileges"],
        ),
        ObserveDomain::Brew => (
            vec!["installed", "version", "recent_updates"],
            vec!["repair_package", "install_package", "update_package"],
            vec!["brew_installed"],
            vec![],
        ),
        ObserveDomain::Log => (
            vec!["recent_errors", "recent_warnings", "matching", "timeline"],
            vec![],
            vec![],
            vec![],
        ),
        ObserveDomain::Process => (
            vec!["processes", "tree", "top_cpu", "top_memory", "open_files", "listening_ports"],
            vec!["kill_graceful", "kill_force", "set_priority"],
            vec!["process_running", "process_stopped", "port_free"],
            vec!["kill_force and set_priority may require administrator privileges"],
        ),
        ObserveDomain::Container => (
            vec!["containers", "images", "volumes", "networks", "container_logs"],
            vec!["start_container", "stop_container", "restart_container", "remove_container", "pull_image", "prune_images", "prune_volumes"],
            vec!["container_running", "container_healthy", "image_exists", "volume_exists"],
            vec!["Requires Docker or Podman runtime"],
        ),
        _ => (vec![], vec![], vec![], vec![]),
    };

    UnifiedResult::ok(
        format!("{} observation available.", domain.as_str()),
        serde_json::json!({
            "allowed_scopes": scopes,
            "related_actions": actions,
            "related_verifications": verifications,
            "privilege_notes": privilege_notes,
        }),
    )
}

