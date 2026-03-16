//! Core world spec — the Single Source of Truth for what the world looks like.
//!
//! Every domain's observation schema AND action space (target + verb + args)
//! is defined here. This is a pure declaration — no dispatch wiring.
//!
//! Dispatch wiring (which function handles which verb) lives in `dispatch.rs`.
//!
//! Future: this will be loadable from external JSON/YAML files or generated
//! by plugins in any language.

use serde_json::{json, Value};

use crate::contracts::observe::ObserveDomain;

/// Core-only spec: observations + actions.
/// Pure declaration — what the world looks like and what verbs exist.
pub fn core_spec(domain: ObserveDomain) -> Value {
    match domain {
        ObserveDomain::Network => json!({
            "domain": "network",
            "observations": {
                "interfaces": {
                    "type": "array",
                    "item": {
                        "name": "string",
                        "up": "bool",
                        "addresses": ["string"],
                        "gateway": "string | null",
                        "dns_servers": ["string"],
                        "type": "ethernet | wifi | vpn | loopback | other"
                    }
                },
                "internet_reachable": "bool | null",
                "proxy_enabled": "bool | null",
                "vpn_present": "bool | null",
                "warnings": ["string"]
            },
            "actions": [
                { "target": "dns_cache",         "verbs": ["reset"],             "mutates": ["network.interfaces"],      "description": "Flush DNS cache" },
                { "target": "dhcp_lease",        "verbs": ["reset"],             "mutates": ["network.interfaces"],      "description": "Renew DHCP lease" },
                { "target": "interfaces.<name>", "verbs": ["enable", "disable"], "mutates": ["network.interfaces"],      "description": "Toggle network interface" },
                { "target": "wifi.<ssid>",       "verbs": ["remove"],            "mutates": ["network.interfaces"],      "description": "Forget WiFi network" },
                { "target": "wifi",              "verbs": ["restart"],           "mutates": ["network.interfaces"],      "description": "Reconnect WiFi" },
                { "target": "proxy",             "verbs": ["reset"],             "mutates": ["network.interfaces"],      "description": "Reset proxy settings" }
            ]
        }),

        ObserveDomain::Service => json!({
            "domain": "service",
            "observations": {
                "name": "string",
                "exists": "bool",
                "status": "running | stopped | degraded | unknown",
                "startup_mode": "auto | manual | disabled | unknown | null",
                "pid": "integer | null",
                "recent_errors": ["string"],
                "dependencies": ["string"]
            },
            "actions": [
                { "target": "<name>",              "verbs": ["restart"],           "mutates": ["service.status"],          "description": "Restart a service" },
                { "target": "<name>",              "verbs": ["enable"],            "mutates": ["service.status"],          "description": "Start a service" },
                { "target": "<name>",              "verbs": ["disable"],           "mutates": ["service.status"],          "description": "Stop a service" },
                { "target": "<name>.startup_mode", "verbs": ["set"],              "mutates": ["service.startup_mode"],    "description": "Set startup mode", "args": { "mode": { "type": "string", "enum": ["auto", "manual", "disabled"] } } }
            ]
        }),

        ObserveDomain::Disk => json!({
            "domain": "disk",
            "observations": {
                "mounts": {
                    "type": "array",
                    "item": {
                        "path": "string",
                        "filesystem": "string",
                        "total_bytes": "integer",
                        "used_bytes": "integer",
                        "available_bytes": "integer",
                        "percent_used": "float"
                    }
                },
                "warnings": ["string"]
            },
            "actions": [
                { "target": "temp",   "verbs": ["clear", "reset"], "mutates": ["disk.mounts"],             "description": "Clear temporary files" },
                { "target": "caches", "verbs": ["clear", "reset"], "mutates": ["disk.mounts"],             "description": "Remove known large caches (brew, npm, pip)" },
                { "target": "<path>", "verbs": ["add", "remove"],  "mutates": ["disk.mounts"],             "description": "Mount/unmount a share" }
            ]
        }),

        ObserveDomain::Printer => json!({
            "domain": "printer",
            "observations": {
                "name": "string",
                "installed": "bool",
                "status": "ready | offline | error | unknown",
                "is_default": "bool | null",
                "queue_jobs": "integer | null",
                "driver": "string | null",
                "port": "string | null",
                "host_reachable": "bool | null",
                "recent_errors": ["string"]
            },
            "actions": [
                { "target": "<name>.queue",  "verbs": ["clear"],   "mutates": ["printer.queue_jobs"],      "description": "Clear print queue" },
                { "target": "spooler",       "verbs": ["restart"], "mutates": ["printer.status"],           "description": "Restart print spooler" },
                { "target": "default",       "verbs": ["set"],     "mutates": ["printer.is_default"],       "description": "Set default printer", "args": { "name": { "type": "string", "description": "printer name" } } },
                { "target": "<name>.driver", "verbs": ["reset"],   "mutates": ["printer.driver"],           "description": "Reinstall printer driver" }
            ]
        }),

        ObserveDomain::Brew => json!({
            "domain": "brew",
            "observations": {
                "name": "string",
                "installed": "bool",
                "version": "string | null",
                "latest_version": "string | null",
                "source": "string | null"
            },
            "actions": [
                { "target": "<name>", "verbs": ["add"],    "mutates": ["brew.installed", "brew.version"],  "description": "Install a package" },
                { "target": "<name>", "verbs": ["remove"], "mutates": ["brew.installed"],                   "description": "Uninstall a package" },
                { "target": "<name>", "verbs": ["reset"],  "mutates": ["brew.version"],                     "description": "Repair (reinstall) a package" },
                { "target": "<name>", "verbs": ["set"],    "mutates": ["brew.version"],                     "description": "Set version", "args": { "version": { "type": "string", "description": "version string or 'latest'" } } }
            ]
        }),

        ObserveDomain::Log => json!({
            "domain": "log",
            "observations": {
                "entries": {
                    "type": "array",
                    "item": {
                        "timestamp": "string",
                        "level": "string",
                        "source": "string",
                        "message": "string"
                    }
                },
                "total_matched": "integer",
                "truncated": "bool | null"
            },
            "actions": []
        }),

        ObserveDomain::Process => json!({
            "domain": "process",
            "observations": {
                "processes": {
                    "type": "array",
                    "item": {
                        "pid": "integer",
                        "ppid": "integer",
                        "name": "string",
                        "user": "string | null",
                        "status": "running | sleeping | zombie | stopped | idle",
                        "cpu_percent": "float | null",
                        "memory_bytes": "integer | null",
                        "memory_percent": "float | null",
                        "command": "string | null"
                    }
                },
                "total_count": "integer",
                "warnings": ["string"]
            },
            "actions": [
                { "target": "<pid>",          "verbs": ["kill"],    "mutates": ["process.processes"],       "description": "Graceful kill (SIGTERM)" },
                { "target": "<pid>",          "verbs": ["remove"],  "mutates": ["process.processes"],       "description": "Force kill (SIGKILL)" },
                { "target": "<pid>.priority", "verbs": ["set"],     "mutates": ["process.processes"],       "description": "Set process priority (renice)", "args": { "priority": { "type": "integer", "description": "nice value (-20 to 20)" } } }
            ]
        }),

        ObserveDomain::Container => json!({
            "domain": "container",
            "observations": {
                "containers": {
                    "type": "array",
                    "item": {
                        "id": "string",
                        "name": "string",
                        "image": "string",
                        "status": "created | running | paused | restarting | exited | dead",
                        "ports": "array | null",
                        "health": "healthy | unhealthy | starting | none | null"
                    }
                },
                "images": {
                    "type": "array",
                    "item": {
                        "id": "string",
                        "repository": "string",
                        "tag": "string",
                        "size_bytes": "integer"
                    }
                },
                "volumes": {
                    "type": "array",
                    "item": {
                        "name": "string",
                        "driver": "string",
                        "mountpoint": "string"
                    }
                },
                "runtime": "docker | podman",
                "warnings": ["string"]
            },
            "actions": [
                { "target": "<id>",     "verbs": ["enable"],  "mutates": ["container.containers"],     "description": "Start a container" },
                { "target": "<id>",     "verbs": ["disable"], "mutates": ["container.containers"],     "description": "Stop a container" },
                { "target": "<id>",     "verbs": ["restart"], "mutates": ["container.containers"],     "description": "Restart a container" },
                { "target": "<id>",     "verbs": ["remove"],  "mutates": ["container.containers"],     "description": "Remove a container" },
                { "target": "<image>",  "verbs": ["add"],     "mutates": ["container.images"],         "description": "Pull an image" },
                { "target": "images",   "verbs": ["clear"],   "mutates": ["container.images"],         "description": "Prune unused images" },
                { "target": "volumes",  "verbs": ["clear"],   "mutates": ["container.volumes"],        "description": "Prune unused volumes" }
            ]
        }),
    }
}

/// All domains that have specs.
pub const SPEC_DOMAINS: &[ObserveDomain] = &[
    ObserveDomain::Network,
    ObserveDomain::Service,
    ObserveDomain::Disk,
    ObserveDomain::Printer,
    ObserveDomain::Brew,
    ObserveDomain::Log,
    ObserveDomain::Process,
    ObserveDomain::Container,
];
