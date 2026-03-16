//! Verb resolution — the virtual dispatch table.
//!
//! Maps (domain, target, verb, args) → (handler, extracted_target, params).
//!
//! The spec declares what verbs exist (the interface).
//! The plugin carries dispatch entries mapping verbs to handlers (the vtable).
//! This module does the pattern matching and resolution (the dispatch).
//!
//! Verb arguments use key=value syntax:
//!   world act service nginx.startup_mode set mode=auto
//!   world act brew jq set version=latest

use std::collections::BTreeMap;

use serde_json::Value;

use crate::plugin::DispatchEntry;

/// The result of resolving a verb + target via a plugin's dispatch table.
#[derive(Debug)]
pub struct ResolvedAction {
    pub handler: String,
    pub target: Option<String>,
    pub params: Option<Value>,
    /// Observation schema paths this action mutates.
    pub mutates: Vec<String>,
}

/// Parse verb arguments as key=value pairs.
fn parse_args(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for arg in args {
        if arg.starts_with("--") {
            continue;
        }
        if let Some((key, value)) = arg.split_once('=') {
            if key.is_empty() {
                return Err(format!("Invalid argument: '{arg}'. Use key=value syntax."));
            }
            map.insert(key.to_string(), value.to_string());
        } else {
            return Err(format!(
                "Invalid argument: '{arg}'. Verb arguments must be key=value pairs (e.g. mode=auto, version=latest)."
            ));
        }
    }
    Ok(map)
}

/// Resolve (domain, target, verb, args) into a handler call via dispatch entries.
///
/// The `domain_name` is only used for error messages.
/// The `entries` come from the plugin's dispatch table.
pub fn resolve(
    domain_name: &str,
    entries: &[DispatchEntry],
    target: &str,
    verb: &str,
    args: &[String],
) -> Result<ResolvedAction, String> {
    // Parse verb arguments as key=value pairs
    let kv_args = parse_args(args)?;

    // Validate: `set` requires at least one key=value
    if verb == "set" && kv_args.is_empty() {
        return Err(
            "Verb 'set' requires key=value arguments (e.g. set mode=auto, set version=latest)."
                .into(),
        );
    }

    // Look up in dispatch table
    for entry in entries {
        if entry.verb != verb {
            continue;
        }

        if let Some(extracted) = match_target(&entry.target, target) {
            let params = if kv_args.is_empty() {
                None
            } else {
                let obj: serde_json::Map<String, Value> = kv_args
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                Some(Value::Object(obj))
            };

            return Ok(ResolvedAction {
                handler: entry.handler.clone(),
                target: extracted,
                params,
                mutates: entry.mutates.clone(),
            });
        }
    }

    // Build error with available actions from dispatch table
    let mut seen = std::collections::HashSet::new();
    let available: Vec<String> = entries
        .iter()
        .filter_map(|e| {
            let key = format!("{} {}", e.target, e.verb);
            if seen.insert(key.clone()) {
                Some(key)
            } else {
                None
            }
        })
        .collect();

    Err(format!(
        "No mapping for: {domain_name} {target} {verb}\nAvailable: {}",
        available.join(", ")
    ))
}

/// Match a concrete target against a pattern.
/// Returns Some(extracted_name) on match, None otherwise.
///
/// Patterns:
///   "dns_cache"              — exact match, returns None (no extracted part)
///   "<name>"                 — wildcard, returns Some(target)
///   "interfaces.<name>"      — prefix match, returns Some(suffix)
///   "<name>.queue"           — suffix match, returns Some(prefix)
fn match_target(pattern: &str, target: &str) -> Option<Option<String>> {
    // Exact match (no wildcards)
    if !pattern.contains('<') {
        if pattern == target {
            return Some(None);
        }
        return None;
    }

    // Pure wildcard: "<name>", "<path>", "<rule>", etc.
    if pattern.starts_with('<') && pattern.ends_with('>') && !pattern.contains('.') {
        return Some(Some(target.to_string()));
    }

    // Prefix.<wildcard>  e.g. "interfaces.<name>" matches "interfaces.en0"
    if let Some(prefix) = pattern
        .strip_suffix(".<name>")
        .or_else(|| pattern.strip_suffix(".<ssid>"))
    {
        if let Some(suffix) = target.strip_prefix(prefix).and_then(|s| s.strip_prefix('.')) {
            if !suffix.is_empty() {
                return Some(Some(suffix.to_string()));
            }
        }
        return None;
    }

    // <wildcard>.suffix  e.g. "<name>.queue" matches "hp_printer.queue"
    if let Some(dot_pos) = pattern.find(">.") {
        let suffix = &pattern[dot_pos + 2..];
        if let Some(prefix) = target.strip_suffix(&format!(".{suffix}")) {
            if !prefix.is_empty() {
                return Some(Some(prefix.to_string()));
            }
        }
        return None;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries_for(domain: &str) -> Vec<DispatchEntry> {
        crate::dispatch::entries(domain)
            .iter()
            .map(|e| DispatchEntry {
                target: e.target.to_string(),
                verb: e.verb.to_string(),
                handler: e.handler.to_string(),
                mutates: e.mutates.iter().map(|s| s.to_string()).collect(),
            })
            .collect()
    }

    #[test]
    fn test_exact_match() {
        assert_eq!(match_target("dns_cache", "dns_cache"), Some(None));
        assert_eq!(match_target("dns_cache", "other"), None);
    }

    #[test]
    fn test_wildcard() {
        assert_eq!(
            match_target("<name>", "nginx"),
            Some(Some("nginx".to_string()))
        );
        assert_eq!(
            match_target("<path>", "/tmp/share"),
            Some(Some("/tmp/share".to_string()))
        );
    }

    #[test]
    fn test_prefix_wildcard() {
        assert_eq!(
            match_target("interfaces.<name>", "interfaces.en0"),
            Some(Some("en0".to_string()))
        );
        assert_eq!(match_target("interfaces.<name>", "wifi.home"), None);
    }

    #[test]
    fn test_suffix_wildcard() {
        assert_eq!(
            match_target("<name>.queue", "hp_printer.queue"),
            Some(Some("hp_printer".to_string()))
        );
        assert_eq!(
            match_target("<name>.startup_mode", "nginx.startup_mode"),
            Some(Some("nginx".to_string()))
        );
        assert_eq!(match_target("<name>.queue", "hp_printer"), None);
    }

    #[test]
    fn test_parse_args_valid() {
        let args = vec!["mode=auto".to_string()];
        let kv = parse_args(&args).unwrap();
        assert_eq!(kv.get("mode").unwrap(), "auto");
    }

    #[test]
    fn test_parse_args_multiple() {
        let args = vec!["version=1.2.3".to_string(), "source=npm".to_string()];
        let kv = parse_args(&args).unwrap();
        assert_eq!(kv.get("version").unwrap(), "1.2.3");
        assert_eq!(kv.get("source").unwrap(), "npm");
    }

    #[test]
    fn test_parse_args_naked_value_rejected() {
        let args = vec!["latest".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_set_requires_args() {
        let entries = entries_for("network");
        let r = resolve("network", &entries, "dns_cache", "set", &[]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("requires key=value"));
    }

    #[test]
    fn test_dispatch_resolve() {
        // Network dns_cache reset
        let entries = entries_for("network");
        let r = resolve("network", &entries, "dns_cache", "reset", &[]).unwrap();
        assert_eq!(r.handler, "flush_dns");
        assert!(r.target.is_none());
        assert!(r.params.is_none());

        // Service nginx restart
        let entries = entries_for("service");
        let r = resolve("service", &entries, "nginx", "restart", &[]).unwrap();
        assert_eq!(r.handler, "restart_service");
        assert_eq!(r.target.as_deref(), Some("nginx"));

        // Brew jq add
        let entries = entries_for("brew");
        let r = resolve("brew", &entries, "jq", "add", &[]).unwrap();
        assert_eq!(r.handler, "install_package");
        assert_eq!(r.target.as_deref(), Some("jq"));

        // Service nginx.startup_mode set mode=auto
        let entries = entries_for("service");
        let r = resolve(
            "service",
            &entries,
            "nginx.startup_mode",
            "set",
            &["mode=auto".to_string()],
        )
        .unwrap();
        assert_eq!(r.handler, "set_startup_mode");
        assert_eq!(r.target.as_deref(), Some("nginx"));
        assert_eq!(r.params.unwrap()["mode"], "auto");
    }

    #[test]
    fn test_invalid_verb() {
        let entries = entries_for("network");
        let r = resolve("network", &entries, "dns_cache", "enable", &[]);
        assert!(r.is_err());
    }
}
