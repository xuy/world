//! Policy add-on — governance over world transitions.
//!
//! Policy does not change the world — it gates access to transitions.
//! The world defines what CAN happen; policy defines what SHOULD happen
//! given organizational constraints.
//!
//! Policy speaks the spec vocabulary: targets and verbs.
//! Internally it looks up risk via the dispatch table's handler names.

use serde_json::{json, Value};

use crate::dispatch;
use crate::contracts::observe::ObserveDomain;
use crate::contracts::act::ActDomain;
use crate::contracts::Risk;
use crate::policy;

use super::{Addon, AddonKind};

pub struct PolicyAddon;

impl Addon for PolicyAddon {
    fn name(&self) -> &'static str {
        "policy"
    }

    fn description(&self) -> &'static str {
        "Governance — risk classification and consent gates over world transitions. Does not change the world; gates access to it."
    }

    fn kind(&self) -> AddonKind {
        AddonKind::Governance
    }

    fn domain_spec(&self, domain: ObserveDomain) -> Option<Value> {
        let act_domain = match domain {
            ObserveDomain::Network => ActDomain::Network,
            ObserveDomain::Service => ActDomain::Service,
            ObserveDomain::Disk => ActDomain::Disk,
            ObserveDomain::Printer => ActDomain::Printer,
            ObserveDomain::Brew => ActDomain::Brew,
            _ => return None,
        };

        // Read from dispatch table — it knows the handler names for risk lookup
        let entries = dispatch::entries(domain.as_str());
        if entries.is_empty() {
            return None;
        }

        let mut rules = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for entry in entries {
            // Deduplicate (same target+verb can appear in dispatch but we show once)
            let key = format!("{}.{}", entry.target, entry.verb);
            if !seen.insert(key) {
                continue;
            }

            let risk = policy::classify_risk(act_domain, entry.handler);
            let needs_consent = risk.requires_consent();

            let mut rule = json!({
                "target": entry.target,
                "verb": entry.verb,
                "classification": risk_str(risk),
            });
            if needs_consent {
                rule.as_object_mut()
                    .unwrap()
                    .insert("requires_consent".into(), json!(true));
            }
            rules.push(rule);
        }

        Some(json!({
            "rules": rules,
            "classifications": ["low", "medium", "high"],
            "consent_required_for": ["high"],
            "observe_classification": "low (all observations are read-only)"
        }))
    }
}

fn risk_str(risk: Risk) -> &'static str {
    match risk {
        Risk::Low => "low",
        Risk::Medium => "medium",
        Risk::High => "high",
    }
}
