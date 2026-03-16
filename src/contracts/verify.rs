use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Verification checks — user-visible or operationally meaningful conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyCheck {
    ServiceHealthy,
    PortOpen,
    HostReachable,
    DnsResolves,
    PrinterPrints,
    BrewInstalled,
    DiskWritable,
    InternetReachable,
    ProcessRunning,
    ProcessStopped,
    PortFree,
    ContainerRunning,
    ContainerHealthy,
    ImageExists,
    VolumeExists,
}

impl VerifyCheck {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ServiceHealthy => "service_healthy",
            Self::PortOpen => "port_open",
            Self::HostReachable => "host_reachable",
            Self::DnsResolves => "dns_resolves",
            Self::PrinterPrints => "printer_prints",
            Self::BrewInstalled => "brew_installed",
            Self::DiskWritable => "disk_writable",
            Self::InternetReachable => "internet_reachable",
            Self::ProcessRunning => "process_running",
            Self::ProcessStopped => "process_stopped",
            Self::PortFree => "port_free",
            Self::ContainerRunning => "container_running",
            Self::ContainerHealthy => "container_healthy",
            Self::ImageExists => "image_exists",
            Self::VolumeExists => "volume_exists",
        }
    }
}

/// Arguments for the `verify` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyArgs {
    pub check: VerifyCheck,
    /// Target for the check (e.g. hostname, service name, domain).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Additional parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Timeout in seconds for the check (default varies by check type).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<u32>,
}

/// Recommended verification checks after an action.
pub fn recommended_verifications(action: &str) -> &'static [&'static str] {
    match action {
        "restart_service" | "start_service" => &["service_healthy"],
        "flush_dns" => &["dns_resolves", "internet_reachable"],
        "renew_dhcp" | "toggle_adapter" | "reconnect_wifi" | "reset_proxy" => {
            &["internet_reachable"]
        }
        "clear_queue" | "restart_spooler" => &["printer_prints"],
        "repair_package" | "install_package" | "update_package" => &["brew_installed"],
        "clear_temp_files" | "remove_large_known_caches" => &["disk_writable"],
        "kill_graceful" => &["process_stopped"],
        "kill_force" => &["process_stopped", "port_free"],
        "set_priority" => &["process_running"],
        "start_container" => &["container_running", "container_healthy"],
        "restart_container" => &["container_running", "container_healthy"],
        "pull_image" => &["image_exists"],
        _ => &[],
    }
}
