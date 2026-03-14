//! Verify add-on — stored predicates.
//!
//! A predicate bundles: observation query + assertion over the result.
//! The agent calls `world verify <predicate>` instead of manually
//! observing and checking fields. Like a stored procedure in a DB.
//!
//! Example: `dns_resolves` = observe(network, scope=dns, target=X)
//!          then assert addresses.len() > 0.

use serde_json::{json, Value};

use crate::contracts::observe::ObserveDomain;
use super::{Addon, AddonKind};

pub struct VerifyAddon;

impl Addon for VerifyAddon {
    fn name(&self) -> &'static str {
        "verify"
    }

    fn description(&self) -> &'static str {
        "Stored predicates — bundled observation + assertion. Like a stored procedure: the agent calls verify(name) instead of manually observing and checking fields."
    }

    fn kind(&self) -> AddonKind {
        AddonKind::Predicate
    }

    fn domain_spec(&self, domain: ObserveDomain) -> Option<Value> {
        let predicates = match domain {
            ObserveDomain::Network => json!([
                {
                    "name": "dns_resolves",
                    "params": { "target": { "type": "string", "description": "hostname to resolve", "default": "google.com" } },
                    "observes": "network.dns",
                    "asserts": "addresses is non-empty",
                    "returns": { "passed": "bool", "addresses": "[string]" }
                },
                {
                    "name": "internet_reachable",
                    "params": {},
                    "observes": "network.internet_status",
                    "asserts": "HTTP 2xx from external endpoint",
                    "returns": { "passed": "bool", "http_status": "integer", "duration_ms": "integer" }
                },
                {
                    "name": "host_reachable",
                    "params": { "target": { "type": "string", "description": "hostname or IP" } },
                    "observes": "network.connectivity",
                    "asserts": "ICMP ping succeeds",
                    "returns": { "passed": "bool" }
                },
                {
                    "name": "port_open",
                    "params": {
                        "target": { "type": "string", "description": "hostname or IP" },
                        "port": { "type": "integer", "description": "port number" }
                    },
                    "observes": "network.connectivity",
                    "asserts": "TCP connection succeeds",
                    "returns": { "passed": "bool" }
                }
            ]),

            ObserveDomain::Service => json!([
                {
                    "name": "service_healthy",
                    "params": { "target": { "type": "string", "description": "service name" } },
                    "observes": "service.status",
                    "asserts": "service is running with a PID",
                    "returns": { "passed": "bool" }
                }
            ]),

            ObserveDomain::Disk => json!([
                {
                    "name": "disk_writable",
                    "params": { "target": { "type": "string", "description": "path to test", "default": "/tmp" } },
                    "observes": "disk.mounts",
                    "asserts": "can create and delete a temp file at path",
                    "returns": { "passed": "bool" }
                }
            ]),

            ObserveDomain::Printer => json!([
                {
                    "name": "printer_prints",
                    "params": { "target": { "type": "string", "description": "printer name", "default": "default" } },
                    "observes": "printer.status",
                    "asserts": "test print job submits successfully",
                    "returns": { "passed": "bool" }
                }
            ]),

            ObserveDomain::Package => json!([
                {
                    "name": "package_installed",
                    "params": { "target": { "type": "string", "description": "package name" } },
                    "observes": "package.installed",
                    "asserts": "package exists in package manager",
                    "returns": { "passed": "bool", "version": "string | null" }
                }
            ]),

            ObserveDomain::Share => json!([
                {
                    "name": "share_accessible",
                    "params": { "target": { "type": "string", "description": "share path" } },
                    "observes": "share.connectivity",
                    "asserts": "share is mounted and readable",
                    "returns": { "passed": "bool" }
                }
            ]),

            ObserveDomain::Identity => json!([
                {
                    "name": "login_works",
                    "params": { "target": { "type": "string", "description": "account name" } },
                    "observes": "identity.credentials",
                    "asserts": "cached credentials are valid",
                    "returns": { "passed": "bool" }
                }
            ]),

            _ => return None,
        };

        Some(json!({ "predicates": predicates }))
    }
}
