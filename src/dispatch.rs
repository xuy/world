//! Dispatch registry — wires (domain, target_pattern, verb) to handler names.
//!
//! This is the plumbing that connects the world spec (what verbs exist)
//! to the adapter layer (which function handles them). It is NOT part of
//! the world spec and is never exposed to agents.
//!
//! When a plugin system exists, plugins will register their own dispatch
//! entries here instead of being hardcoded.

/// A dispatch entry: target pattern + verb → handler name.
pub struct Entry {
    pub target: &'static str,
    pub verb: &'static str,
    pub handler: &'static str,
}

/// All dispatch entries for a domain.
pub fn entries(domain: &str) -> &'static [Entry] {
    match domain {
        "network" => &[
            Entry { target: "dns_cache",         verb: "reset",   handler: "flush_dns" },
            Entry { target: "dhcp_lease",        verb: "reset",   handler: "renew_dhcp" },
            Entry { target: "interfaces.<name>", verb: "enable",  handler: "toggle_adapter" },
            Entry { target: "interfaces.<name>", verb: "disable", handler: "toggle_adapter" },
            Entry { target: "wifi.<ssid>",       verb: "remove",  handler: "forget_wifi" },
            Entry { target: "wifi",              verb: "restart", handler: "reconnect_wifi" },
            Entry { target: "proxy",             verb: "reset",   handler: "reset_proxy" },
        ],
        "service" => &[
            Entry { target: "<name>",              verb: "restart", handler: "restart_service" },
            Entry { target: "<name>",              verb: "enable",  handler: "start_service" },
            Entry { target: "<name>",              verb: "disable", handler: "stop_service" },
            Entry { target: "<name>.startup_mode", verb: "set",     handler: "set_startup_mode" },
        ],
        "disk" => &[
            Entry { target: "temp",   verb: "clear", handler: "clear_temp_files" },
            Entry { target: "temp",   verb: "reset", handler: "clear_temp_files" },
            Entry { target: "caches", verb: "clear", handler: "remove_large_known_caches" },
            Entry { target: "caches", verb: "reset", handler: "remove_large_known_caches" },
            Entry { target: "<path>", verb: "add",   handler: "mount_share" },
            Entry { target: "<path>", verb: "remove", handler: "unmount_share" },
        ],
        "printer" => &[
            Entry { target: "<name>.queue",  verb: "clear",   handler: "clear_queue" },
            Entry { target: "spooler",       verb: "restart", handler: "restart_spooler" },
            Entry { target: "default",       verb: "set",     handler: "set_default_printer" },
            Entry { target: "<name>.driver", verb: "reset",   handler: "reinstall_printer_driver" },
        ],
        "package" => &[
            Entry { target: "<name>", verb: "add",    handler: "install_package" },
            Entry { target: "<name>", verb: "remove", handler: "uninstall_package" },
            Entry { target: "<name>", verb: "reset",  handler: "repair_package" },
            Entry { target: "<name>", verb: "set",    handler: "update_package" },
        ],
        "share" => &[
            Entry { target: "<path>",      verb: "add",    handler: "map_share" },
            Entry { target: "<path>",      verb: "remove", handler: "disconnect_share" },
            Entry { target: "credentials", verb: "reset",  handler: "refresh_credentials" },
        ],
        "identity" => &[
            Entry { target: "credentials", verb: "clear",   handler: "clear_cached_credentials" },
            Entry { target: "credentials", verb: "reset",   handler: "clear_cached_credentials" },
            Entry { target: "account",     verb: "restart", handler: "re_authenticate_account" },
        ],
        "security" => &[
            Entry { target: "<rule>", verb: "add",    handler: "allow_firewall_rule" },
            Entry { target: "<rule>", verb: "remove", handler: "remove_firewall_rule" },
        ],
        _ => &[],
    }
}
