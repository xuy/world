//! Plugin trait and registry — the core abstraction for world domains.
//!
//! Every domain is a plugin. Native Rust plugins offer performance;
//! external subprocess plugins offer extensibility in any language.
//! World itself is a neutral framework.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::adapters::Platform;
use crate::{dispatch, spec};
use crate::contracts::observe::ObserveDomain;
use crate::contracts::act::ActDomain;
use crate::contracts::{Risk, UnifiedResult};
use crate::domains;
use crate::policy;

// ─── Core types ──────────────────────────────────────────────────────────────

/// A dispatch entry: target pattern + verb → handler name.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DispatchEntry {
    pub target: String,
    pub verb: String,
    pub handler: String,
}

/// The unified plugin interface.
///
/// Every domain — native Rust or external subprocess — implements this trait.
/// The CLI dispatches through this trait exclusively; there is no separate
/// code path for "built-in" vs "external" domains.
#[async_trait]
pub trait DomainPlugin: Send + Sync {
    /// Domain name (e.g., "network", "pip").
    fn domain(&self) -> &str;

    /// POMDP spec: observations + actions.
    fn spec(&self) -> &Value;

    /// Dispatch table: (target_pattern, verb) → handler.
    fn dispatch_entries(&self) -> &[DispatchEntry];

    /// Risk classification for a handler.
    fn classify_risk(&self, handler: &str) -> Risk;

    /// Whether a handler is in the allowlist.
    fn is_allowed(&self, handler: &str) -> bool;

    /// Observe structured state.
    async fn observe(
        &self,
        target: Option<&str>,
        since: Option<&str>,
        limit: Option<u32>,
    ) -> Result<UnifiedResult>;

    /// Execute an action.
    async fn act(
        &self,
        handler: &str,
        target: Option<&str>,
        params: Option<&Value>,
        dry_run: bool,
    ) -> Result<UnifiedResult>;
}

// ─── Registry ────────────────────────────────────────────────────────────────

/// Registry of all loaded plugins (native + external).
pub struct PluginRegistry {
    plugins: Vec<Box<dyn DomainPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn DomainPlugin>) {
        self.plugins.push(plugin);
    }

    pub fn get(&self, domain: &str) -> Option<&dyn DomainPlugin> {
        self.plugins
            .iter()
            .find(|p| p.domain() == domain)
            .map(|p| p.as_ref())
    }

    pub fn all(&self) -> &[Box<dyn DomainPlugin>] {
        &self.plugins
    }
}

// ─── Native plugin ──────────────────────────────────────────────────────────

/// A native Rust plugin wrapping existing domain implementations.
///
/// Native plugins offer the same interface as external plugins but
/// execute in-process for performance. They wrap the existing domain
/// modules (network, service, disk, etc.).
struct NativePlugin {
    name: &'static str,
    observe_domain: ObserveDomain,
    act_domain: Option<ActDomain>,
    platform: Platform,
    entries: Vec<DispatchEntry>,
    spec_value: Value,
}

impl NativePlugin {
    fn new(
        name: &'static str,
        observe_domain: ObserveDomain,
        act_domain: Option<ActDomain>,
        platform: Platform,
    ) -> Self {
        let spec_value = spec::core_spec(observe_domain);
        let static_entries = dispatch::entries(name);
        let entries = static_entries
            .iter()
            .map(|e| DispatchEntry {
                target: e.target.to_string(),
                verb: e.verb.to_string(),
                handler: e.handler.to_string(),
            })
            .collect();

        Self {
            name,
            observe_domain,
            act_domain,
            platform,
            entries,
            spec_value,
        }
    }
}

#[async_trait]
impl DomainPlugin for NativePlugin {
    fn domain(&self) -> &str {
        self.name
    }

    fn spec(&self) -> &Value {
        &self.spec_value
    }

    fn dispatch_entries(&self) -> &[DispatchEntry] {
        &self.entries
    }

    fn classify_risk(&self, handler: &str) -> Risk {
        match self.act_domain {
            Some(rd) => policy::classify_risk(rd, handler),
            None => Risk::Low,
        }
    }

    fn is_allowed(&self, handler: &str) -> bool {
        match self.act_domain {
            Some(rd) => policy::is_allowed(rd, handler),
            None => false,
        }
    }

    async fn observe(
        &self,
        target: Option<&str>,
        since: Option<&str>,
        limit: Option<u32>,
    ) -> Result<UnifiedResult> {
        domains::dispatch_observe(
            self.platform,
            self.observe_domain,
            target,
            since,
            limit,
        )
        .await
    }

    async fn act(
        &self,
        handler: &str,
        target: Option<&str>,
        params: Option<&Value>,
        dry_run: bool,
    ) -> Result<UnifiedResult> {
        match self.act_domain {
            Some(rd) => {
                domains::dispatch_act(self.platform, rd, handler, target, params, dry_run)
                    .await
            }
            None => Ok(UnifiedResult::unsupported(&format!("act on {}", self.name))),
        }
    }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/// Create all native plugins for a platform.
pub fn native_plugins(platform: Platform) -> Vec<Box<dyn DomainPlugin>> {
    vec![
        Box::new(NativePlugin::new(
            "network",
            ObserveDomain::Network,
            Some(ActDomain::Network),
            platform,
        )),
        Box::new(NativePlugin::new(
            "service",
            ObserveDomain::Service,
            Some(ActDomain::Service),
            platform,
        )),
        Box::new(NativePlugin::new(
            "disk",
            ObserveDomain::Disk,
            Some(ActDomain::Disk),
            platform,
        )),
        Box::new(NativePlugin::new(
            "printer",
            ObserveDomain::Printer,
            Some(ActDomain::Printer),
            platform,
        )),
        Box::new(NativePlugin::new(
            "package",
            ObserveDomain::Package,
            Some(ActDomain::Package),
            platform,
        )),
        Box::new(NativePlugin::new(
            "log",
            ObserveDomain::Log,
            None,
            platform,
        )),
        Box::new(NativePlugin::new(
            "process",
            ObserveDomain::Process,
            Some(ActDomain::Process),
            platform,
        )),
        Box::new(NativePlugin::new(
            "container",
            ObserveDomain::Container,
            Some(ActDomain::Container),
            platform,
        )),
    ]
}
