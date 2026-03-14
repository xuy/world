//! Add-on system — optional layers that extend the core world (O + A).
//!
//! The world is pure state and transitions:
//!   observe  → structured state  (O)
//!   act      → state transition  (A)
//!
//! Add-ons are pluggable layers on top:
//!   verify   → stored predicates (bundled observation + assertion)
//!   policy   → governance (risk classification, consent gates)
//!   ...      → open for extension (audit, cache, replay, ...)
//!
//! Each add-on implements `Addon` and contributes:
//!   1. A spec fragment per domain (what it adds to the world)
//!   2. Optionally, CLI commands or middleware behavior

pub mod policy;
pub mod verify;

use serde_json::Value;

use crate::contracts::observe::ObserveDomain;

/// An add-on that extends the core world model.
pub trait Addon: Send + Sync {
    /// Unique name (e.g., "verify", "policy").
    fn name(&self) -> &'static str;

    /// Short description of what this add-on provides.
    fn description(&self) -> &'static str;

    /// What kind of add-on this is.
    fn kind(&self) -> AddonKind;

    /// Return this add-on's spec contribution for a specific domain.
    /// Returns None if the add-on has nothing to say about this domain.
    fn domain_spec(&self, domain: ObserveDomain) -> Option<Value>;
}

/// Categorizes what an add-on does, so agents know how to use it.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AddonKind {
    /// Stored predicates — functions from observation to bool.
    Predicate,
    /// Governance — gates and classifies transitions.
    Governance,
    /// Instrumentation — observes the agent's own behavior.
    Instrumentation,
}

/// All registered add-ons. Open for extension — add new entries here.
pub fn registered() -> Vec<Box<dyn Addon>> {
    vec![
        Box::new(verify::VerifyAddon),
        Box::new(policy::PolicyAddon),
    ]
}
