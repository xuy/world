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
    /// Default arg key for bare positional values.
    pub default_arg: Option<&'static str>,
}

impl Entry {
    const fn new(target: &'static str, verb: &'static str, handler: &'static str, mutates: &'static [&'static str]) -> Self {
        Self { target, verb, handler, mutates, default_arg: None }
    }
}

static NETWORK: &[Entry] = &[
    Entry::new("dns_cache",         "reset",   "flush_dns",      &["network.interfaces"]),
    Entry::new("dhcp_lease",        "reset",   "renew_dhcp",     &["network.interfaces"]),
    Entry::new("interfaces.<name>", "enable",  "toggle_adapter", &["network.interfaces"]),
    Entry::new("interfaces.<name>", "disable", "toggle_adapter", &["network.interfaces"]),
    Entry::new("wifi.<ssid>",       "remove",  "forget_wifi",    &["network.interfaces"]),
    Entry::new("wifi",              "restart", "reconnect_wifi", &["network.interfaces"]),
    Entry::new("proxy",             "reset",   "reset_proxy",    &["network.interfaces"]),
];

static SERVICE: &[Entry] = &[
    Entry::new("<name>",              "restart", "restart_service",  &["service.status"]),
    Entry::new("<name>",              "enable",  "start_service",    &["service.status"]),
    Entry::new("<name>",              "disable", "stop_service",     &["service.status"]),
    Entry::new("<name>.startup_mode", "set",     "set_startup_mode", &["service.startup_mode"]),
];

static DISK: &[Entry] = &[
    Entry::new("temp",   "clear",  "clear_temp_files",          &["disk.mounts"]),
    Entry::new("temp",   "reset",  "clear_temp_files",          &["disk.mounts"]),
    Entry::new("caches", "clear",  "remove_large_known_caches", &["disk.mounts"]),
    Entry::new("caches", "reset",  "remove_large_known_caches", &["disk.mounts"]),
    Entry::new("<path>", "add",    "mount_share",               &["disk.mounts"]),
    Entry::new("<path>", "remove", "unmount_share",             &["disk.mounts"]),
];

static PRINTER: &[Entry] = &[
    Entry::new("<name>.queue",  "clear",   "clear_queue",              &["printer.queue_jobs"]),
    Entry::new("spooler",       "restart", "restart_spooler",          &["printer.status"]),
    Entry::new("default",       "set",     "set_default_printer",      &["printer.is_default"]),
    Entry::new("<name>.driver", "reset",   "reinstall_printer_driver", &["printer.driver"]),
];

static BREW: &[Entry] = &[
    Entry::new("<name>", "add",    "install_package",   &["brew.installed", "brew.version"]),
    Entry::new("<name>", "remove", "uninstall_package", &["brew.installed"]),
    Entry::new("<name>", "reset",  "repair_package",    &["brew.version"]),
    Entry::new("<name>", "set",    "update_package",    &["brew.version"]),
];

static PROCESS: &[Entry] = &[
    Entry::new("<pid>",          "kill",   "kill_graceful", &["process.processes"]),
    Entry::new("<pid>",          "remove", "kill_force",    &["process.processes"]),
    Entry::new("<pid>.priority", "set",    "set_priority",  &["process.processes"]),
];

static CONTAINER: &[Entry] = &[
    Entry::new("<id>",    "enable",  "start_container",   &["container.containers"]),
    Entry::new("<id>",    "disable", "stop_container",    &["container.containers"]),
    Entry::new("<id>",    "restart", "restart_container", &["container.containers"]),
    Entry::new("<id>",    "remove",  "remove_container",  &["container.containers"]),
    Entry::new("<image>", "add",     "pull_image",        &["container.images"]),
    Entry::new("images",  "clear",   "prune_images",      &["container.images"]),
    Entry::new("volumes", "clear",   "prune_volumes",     &["container.volumes"]),
];

/// All dispatch entries for a domain.
pub fn entries(domain: &str) -> &'static [Entry] {
    match domain {
        "network"   => NETWORK,
        "service"   => SERVICE,
        "disk"      => DISK,
        "printer"   => PRINTER,
        "brew"      => BREW,
        "process"   => PROCESS,
        "container" => CONTAINER,
        _ => &[],
    }
}
