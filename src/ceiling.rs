//! Capability ceiling — compiled-in limit on what this binary can mutate.
//!
//! The ceiling is a set of observation schema paths (e.g. "network.interfaces",
//! "process.processes") that this binary is allowed to mutate. Actions whose
//! `mutates` tags are not a subset of the ceiling are refused.
//!
//! The ceiling is structural — no CLI flag, environment variable, or runtime
//! argument can override it. To change the ceiling, recompile the binary.
//!
//! A ceiling of `&["*"]` means "allow everything" (default for development).
//! A ceiling of `&[]` means "read-only binary — observe only, no act".

/// The compiled-in capability ceiling.
///
/// Each entry is an observation schema path prefix that this binary is
/// allowed to mutate. Supports exact match and glob:
///   - "network.interfaces"  → allows mutations to network.interfaces
///   - "network.*"           → allows mutations to any network attribute
///   - "*"                   → allows all mutations (unrestricted)
///
/// To build a restricted binary, change this constant and recompile:
///   `&["network.*", "service.*"]`  → can only mutate network and service
///   `&["container.*"]`             → container-only binary
///   `&[]`                          → read-only, observe-only binary
pub const CEILING: &[&str] = &["*"];

/// Check whether a set of mutation tags is allowed by the ceiling.
///
/// Returns Ok(()) if all tags are within the ceiling, or Err with the
/// first tag that exceeds it.
pub fn check(mutates: &[String]) -> Result<(), String> {
    // Empty ceiling = read-only binary
    if CEILING.is_empty() && !mutates.is_empty() {
        return Err(mutates[0].clone());
    }

    // Wildcard ceiling = everything allowed
    if CEILING.contains(&"*") {
        return Ok(());
    }

    for tag in mutates {
        if !tag_allowed(tag) {
            return Err(tag.clone());
        }
    }
    Ok(())
}

/// Check a single mutation tag against the ceiling.
fn tag_allowed(tag: &str) -> bool {
    CEILING.iter().any(|pattern| pattern_matches(pattern, tag))
}

/// Check whether a single pattern matches a tag.
fn pattern_matches(pattern: &str, tag: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with(".*") {
        let prefix = &pattern[..pattern.len() - 2];
        if tag.starts_with(prefix)
            && tag.len() > prefix.len()
            && tag.as_bytes()[prefix.len()] == b'.'
        {
            return true;
        }
    }
    pattern == tag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_allowed_wildcard() {
        // With CEILING = &["*"], everything is allowed
        assert!(tag_allowed("network.interfaces"));
        assert!(tag_allowed("process.processes"));
        assert!(tag_allowed("anything.at.all"));
    }

    #[test]
    fn test_check_allows_empty_mutates() {
        // An action that mutates nothing is always allowed
        assert!(check(&[]).is_ok());
    }

    #[test]
    fn test_check_with_current_ceiling() {
        // Current ceiling is &["*"], so everything passes
        let tags = vec!["network.interfaces".to_string(), "process.processes".to_string()];
        assert!(check(&tags).is_ok());
    }

    #[test]
    fn test_wildcard_mutates_paths() {
        // browser.* should allow browser.url
        assert!(pattern_matches("browser.*", "browser.url"));
        // browser.* should allow browser.elements
        assert!(pattern_matches("browser.*", "browser.elements"));
        // browser.* should allow the literal wildcard tag browser.*
        assert!(pattern_matches("browser.*", "browser.*"));
        // browser.* should NOT allow process.processes
        assert!(!pattern_matches("browser.*", "process.processes"));
        // network.* should NOT allow browser.url
        assert!(!pattern_matches("network.*", "browser.url"));
    }
}
