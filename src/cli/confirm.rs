use std::io::{self, Write};

use super::output::OutputMode;

/// Policy gate: prompt for confirmation on high-risk actions.
/// Returns true if the user confirmed, false otherwise.
/// In JSON/piped mode, always returns false (exit code 4 signals confirmation_required).
pub fn confirm_high_risk(mode: OutputMode, domain: &str, target: &str, verb: &str) -> bool {
    match mode {
        OutputMode::Json | OutputMode::Quiet => {
            if mode == OutputMode::Json {
                let err = serde_json::json!({
                    "policy": "confirmation_required",
                    "classification": "high",
                    "domain": domain,
                    "target": target,
                    "verb": verb,
                    "message": "Policy: this action is classified HIGH risk. Re-run with --yes to confirm."
                });
                println!("{}", serde_json::to_string_pretty(&err).unwrap_or_default());
            }
            false
        }
        OutputMode::Pretty => {
            eprint!(
                "\x1b[1;33mPolicy\x1b[0m: {domain} {target} {verb} is classified \x1b[1;31mHIGH\x1b[0m risk. Proceed? [y/N] "
            );
            io::stderr().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let trimmed = input.trim().to_lowercase();
                trimmed == "y" || trimmed == "yes"
            } else {
                false
            }
        }
    }
}
