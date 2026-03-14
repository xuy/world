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
    ShareAccessible,
    PackageInstalled,
    DiskWritable,
    LoginWorks,
    InternetReachable,
    ProcessRunning,
    ProcessStopped,
    PortFree,
    ContainerRunning,
    ContainerHealthy,
    ImageExists,
    VolumeExists,
    CertValid,
    CertNotExpired,
    CertChainComplete,
    HostnameMatches,
}

impl VerifyCheck {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ServiceHealthy => "service_healthy",
            Self::PortOpen => "port_open",
            Self::HostReachable => "host_reachable",
            Self::DnsResolves => "dns_resolves",
            Self::PrinterPrints => "printer_prints",
            Self::ShareAccessible => "share_accessible",
            Self::PackageInstalled => "package_installed",
            Self::DiskWritable => "disk_writable",
            Self::LoginWorks => "login_works",
            Self::InternetReachable => "internet_reachable",
            Self::ProcessRunning => "process_running",
            Self::ProcessStopped => "process_stopped",
            Self::PortFree => "port_free",
            Self::ContainerRunning => "container_running",
            Self::ContainerHealthy => "container_healthy",
            Self::ImageExists => "image_exists",
            Self::VolumeExists => "volume_exists",
            Self::CertValid => "cert_valid",
            Self::CertNotExpired => "cert_not_expired",
            Self::CertChainComplete => "cert_chain_complete",
            Self::HostnameMatches => "hostname_matches",
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

/// Recommended verification checks after a remediation action.
pub fn recommended_verifications(action: &str) -> &'static [&'static str] {
    match action {
        "restart_service" | "start_service" => &["service_healthy"],
        "flush_dns" => &["dns_resolves", "internet_reachable"],
        "renew_dhcp" | "toggle_adapter" | "reconnect_wifi" | "reset_proxy" => {
            &["internet_reachable"]
        }
        "clear_queue" | "restart_spooler" => &["printer_prints"],
        "repair_package" | "install_package" | "update_package" => &["package_installed"],
        "clear_temp_files" | "remove_large_known_caches" => &["disk_writable"],
        "map_share" | "refresh_credentials" => &["share_accessible"],
        "kill_graceful" => &["process_stopped"],
        "kill_force" => &["process_stopped", "port_free"],
        "set_priority" => &["process_running"],
        "start_container" => &["container_running", "container_healthy"],
        "restart_container" => &["container_running", "container_healthy"],
        "pull_image" => &["image_exists"],
        "install_cert" => &["cert_valid", "cert_not_expired"],
        "trust_cert" => &["cert_chain_complete", "hostname_matches"],
        _ => &[],
    }
}
