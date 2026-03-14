use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Domains that support actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActDomain {
    Network,
    Service,
    Printer,
    Disk,
    Package,
    Process,
    Container,
}

/// Arguments for the `act` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActArgs {
    pub domain: ActDomain,
    /// The specific action to perform (e.g. "flush_dns", "restart_service").
    pub action: String,
    /// Target of the action (e.g. service name, printer name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Additional parameters for the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// If true, describe what would happen without doing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

/// All whitelisted actions, organized by domain.
pub fn allowed_actions(domain: ActDomain) -> &'static [&'static str] {
    match domain {
        ActDomain::Network => &[
            "flush_dns",
            "renew_dhcp",
            "toggle_adapter",
            "forget_wifi",
            "reconnect_wifi",
            "reset_proxy",
        ],
        ActDomain::Service => &[
            "start_service",
            "stop_service",
            "restart_service",
            "set_startup_mode",
        ],
        ActDomain::Printer => &[
            "clear_queue",
            "restart_spooler",
            "set_default_printer",
            "reinstall_printer_driver",
        ],
        ActDomain::Disk => &[
            "clear_temp_files",
            "remove_large_known_caches",
            "mount_share",
            "unmount_share",
        ],
        ActDomain::Package => &[
            "install_package",
            "uninstall_package",
            "repair_package",
            "update_package",
        ],
        ActDomain::Process => &[
            "kill_graceful",
            "kill_force",
            "set_priority",
        ],
        ActDomain::Container => &[
            "start_container",
            "stop_container",
            "restart_container",
            "remove_container",
            "pull_image",
            "prune_images",
            "prune_volumes",
        ],
    }
}
