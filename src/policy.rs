//! Risk classification, action allowlists, and consent policy.

use crate::contracts::{Risk, act::ActDomain};

/// Classify the risk of an action.
pub fn classify_risk(domain: ActDomain, action: &str) -> Risk {
    match (domain, action) {
        // Low risk — read-only-like or trivially reversible
        (ActDomain::Network, "flush_dns") => Risk::Low,
        (ActDomain::Disk, "clear_temp_files") => Risk::Low,

        // Medium risk — service disruption possible but usually safe
        (ActDomain::Network, "renew_dhcp") => Risk::Medium,
        (ActDomain::Network, "toggle_adapter") => Risk::Medium,
        (ActDomain::Network, "reconnect_wifi") => Risk::Medium,
        (ActDomain::Network, "reset_proxy") => Risk::Medium,
        (ActDomain::Service, "restart_service") => Risk::Medium,
        (ActDomain::Service, "start_service") => Risk::Medium,
        (ActDomain::Service, "stop_service") => Risk::Medium,
        (ActDomain::Printer, "clear_queue") => Risk::Medium,
        (ActDomain::Printer, "restart_spooler") => Risk::Medium,
        (ActDomain::Printer, "set_default_printer") => Risk::Medium,
        (ActDomain::Disk, "remove_large_known_caches") => Risk::Medium,

        // High risk — data loss or security implications
        (ActDomain::Network, "forget_wifi") => Risk::High,
        (ActDomain::Printer, "reinstall_printer_driver") => Risk::High,
        (ActDomain::Package, _) => Risk::High,
        (ActDomain::Service, "set_startup_mode") => Risk::High,

        // Process
        (ActDomain::Process, "kill_graceful") => Risk::Medium,
        (ActDomain::Process, "kill_force") => Risk::High,
        (ActDomain::Process, "set_priority") => Risk::Low,

        // Container
        (ActDomain::Container, "start_container") => Risk::Medium,
        (ActDomain::Container, "stop_container") => Risk::Medium,
        (ActDomain::Container, "restart_container") => Risk::Medium,
        (ActDomain::Container, "remove_container") => Risk::High,
        (ActDomain::Container, "pull_image") => Risk::Low,
        (ActDomain::Container, "prune_images") => Risk::High,
        (ActDomain::Container, "prune_volumes") => Risk::High,

        // Default to medium for unknown actions
        _ => Risk::Medium,
    }
}

/// Check whether an action is in the allowlist for its domain.
pub fn is_allowed(domain: ActDomain, action: &str) -> bool {
    crate::contracts::act::allowed_actions(domain).contains(&action)
}

/// Recommended verification checks after an action.
pub fn recommended_verifications(action: &str) -> Vec<String> {
    crate::contracts::verify::recommended_verifications(action)
        .iter()
        .map(|s| s.to_string())
        .collect()
}
