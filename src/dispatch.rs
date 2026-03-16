//! Dispatch registry — wires (domain, target_pattern, verb) to handler names.
//!
//! This is the plumbing that connects the world spec (what verbs exist)
//! to the adapter layer (which function handles them). It is NOT part of
//! the world spec and is never exposed to agents.
//!
//! Each entry also declares `mutates` — the observation schema paths that
//! the action will modify. This is used by the capability ceiling to
//! determine whether this binary is allowed to execute the action.

/// A dispatch entry: target pattern + verb → handler name + mutates metadata.
pub struct Entry {
    pub target: &'static str,
    pub verb: &'static str,
    pub handler: &'static str,
    pub mutates: &'static [&'static str],
}

/// All dispatch entries for a domain.
pub fn entries(domain: &str) -> &'static [Entry] {
    match domain {
        "network" => &[
            Entry { target: "dns_cache",         verb: "reset",   handler: "flush_dns",        mutates: &["network.interfaces"] },
            Entry { target: "dhcp_lease",        verb: "reset",   handler: "renew_dhcp",       mutates: &["network.interfaces"] },
            Entry { target: "interfaces.<name>", verb: "enable",  handler: "toggle_adapter",   mutates: &["network.interfaces"] },
            Entry { target: "interfaces.<name>", verb: "disable", handler: "toggle_adapter",   mutates: &["network.interfaces"] },
            Entry { target: "wifi.<ssid>",       verb: "remove",  handler: "forget_wifi",      mutates: &["network.interfaces"] },
            Entry { target: "wifi",              verb: "restart", handler: "reconnect_wifi",   mutates: &["network.interfaces"] },
            Entry { target: "proxy",             verb: "reset",   handler: "reset_proxy",      mutates: &["network.interfaces"] },
        ],
        "service" => &[
            Entry { target: "<name>",              verb: "restart", handler: "restart_service",   mutates: &["service.status"] },
            Entry { target: "<name>",              verb: "enable",  handler: "start_service",     mutates: &["service.status"] },
            Entry { target: "<name>",              verb: "disable", handler: "stop_service",      mutates: &["service.status"] },
            Entry { target: "<name>.startup_mode", verb: "set",     handler: "set_startup_mode",  mutates: &["service.startup_mode"] },
        ],
        "disk" => &[
            Entry { target: "temp",   verb: "clear", handler: "clear_temp_files",           mutates: &["disk.mounts"] },
            Entry { target: "temp",   verb: "reset", handler: "clear_temp_files",           mutates: &["disk.mounts"] },
            Entry { target: "caches", verb: "clear", handler: "remove_large_known_caches",  mutates: &["disk.mounts"] },
            Entry { target: "caches", verb: "reset", handler: "remove_large_known_caches",  mutates: &["disk.mounts"] },
            Entry { target: "<path>", verb: "add",   handler: "mount_share",                mutates: &["disk.mounts"] },
            Entry { target: "<path>", verb: "remove", handler: "unmount_share",             mutates: &["disk.mounts"] },
        ],
        "printer" => &[
            Entry { target: "<name>.queue",  verb: "clear",   handler: "clear_queue",               mutates: &["printer.queue_jobs"] },
            Entry { target: "spooler",       verb: "restart", handler: "restart_spooler",            mutates: &["printer.status"] },
            Entry { target: "default",       verb: "set",     handler: "set_default_printer",        mutates: &["printer.is_default"] },
            Entry { target: "<name>.driver", verb: "reset",   handler: "reinstall_printer_driver",   mutates: &["printer.driver"] },
        ],
        "brew" => &[
            Entry { target: "<name>", verb: "add",    handler: "install_package",    mutates: &["brew.installed", "brew.version"] },
            Entry { target: "<name>", verb: "remove", handler: "uninstall_package",  mutates: &["brew.installed"] },
            Entry { target: "<name>", verb: "reset",  handler: "repair_package",     mutates: &["brew.version"] },
            Entry { target: "<name>", verb: "set",    handler: "update_package",     mutates: &["brew.version"] },
        ],
        "process" => &[
            Entry { target: "<pid>",          verb: "kill",   handler: "kill_graceful",  mutates: &["process.processes"] },
            Entry { target: "<pid>",          verb: "remove", handler: "kill_force",     mutates: &["process.processes"] },
            Entry { target: "<pid>.priority", verb: "set",    handler: "set_priority",   mutates: &["process.processes"] },
        ],
        "container" => &[
            Entry { target: "<id>",    verb: "enable",  handler: "start_container",    mutates: &["container.containers"] },
            Entry { target: "<id>",    verb: "disable", handler: "stop_container",     mutates: &["container.containers"] },
            Entry { target: "<id>",    verb: "restart", handler: "restart_container",  mutates: &["container.containers"] },
            Entry { target: "<id>",    verb: "remove",  handler: "remove_container",   mutates: &["container.containers"] },
            Entry { target: "<image>", verb: "add",     handler: "pull_image",         mutates: &["container.images"] },
            Entry { target: "images",  verb: "clear",   handler: "prune_images",       mutates: &["container.images"] },
            Entry { target: "volumes", verb: "clear",   handler: "prune_volumes",      mutates: &["container.volumes"] },
        ],
        _ => &[],
    }
}
