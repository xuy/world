use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, ExecOpts};
use crate::schemas::PackageState;

pub async fn observe(
    target: Option<&str>,
) -> Result<UnifiedResult> {
    // Check if Homebrew is available
    let brew_check = exec("which", &["brew"], ExecOpts::default()).await;
    let has_brew = brew_check.map(|r| r.success()).unwrap_or(false);

    if !has_brew {
        return Ok(UnifiedResult::ok(
            "Homebrew not installed. Package observation limited.",
            json!({"homebrew": false, "packages": []}),
        ));
    }

    if let Some(name) = target {
        // Observe specific package
        let result = exec("brew", &["info", "--json=v2", name], ExecOpts::default()).await?;
        if result.success() {
            let info: serde_json::Value = serde_json::from_str(&result.stdout).unwrap_or(json!({}));
            let state = parse_brew_info(&info, name);
            let summary = format!(
                "Package '{}': {} (version: {}).",
                name,
                if state.installed { "installed" } else { "not installed" },
                state.version.as_deref().unwrap_or("unknown"),
            );
            return Ok(UnifiedResult::ok(summary, serde_json::to_value(&state)?));
        } else {
            return Ok(UnifiedResult::ok(
                format!("Package '{name}' not found in Homebrew."),
                serde_json::to_value(&PackageState {
                    name: name.to_string(),
                    installed: false,
                    version: None,
                    latest_version: None,
                    source: Some("homebrew".into()),
                })?,
            ));
        }
    }

    // List installed packages
    let result = exec("brew", &["list", "--versions"], ExecOpts::default()).await?;
    let packages: Vec<PackageState> = result
        .stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                Some(PackageState {
                    name: parts[0].to_string(),
                    installed: true,
                    version: Some(parts[1].to_string()),
                    latest_version: None,
                    source: Some("homebrew".into()),
                })
            } else {
                None
            }
        })
        .collect();

    let count = packages.len();
    Ok(UnifiedResult::ok(
        format!("{count} Homebrew packages installed."),
        serde_json::to_value(&packages)?,
    ))
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    let name = target.ok_or_else(|| anyhow::anyhow!("Package name required for {action}"))?;

    match action {
        "repair_package" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would reinstall package '{name}' via brew reinstall."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "brew",
                &["reinstall", name],
                ExecOpts { timeout_sec: 120, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Package '{name}' reinstalled successfully.")
                } else {
                    format!("Reinstall of '{name}' failed: {}", result.stderr.trim())
                },
                json!({"action": "repair_package", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High)
            .with_suggestions(vec![format!("verify(package_installed, target: \"{name}\")")]))
        }
        "install_package" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would install package '{name}' via brew install."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "brew",
                &["install", name],
                ExecOpts { timeout_sec: 120, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Package '{name}' installed.")
                } else {
                    format!("Install of '{name}' failed: {}", result.stderr.trim())
                },
                json!({"action": "install_package", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High)
            .with_suggestions(vec![format!("verify(package_installed, target: \"{name}\")")]))
        }
        "update_package" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would update package '{name}' via brew upgrade."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "brew",
                &["upgrade", name],
                ExecOpts { timeout_sec: 120, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Package '{name}' updated.")
                } else {
                    format!("Update of '{name}' failed: {}", result.stderr.trim())
                },
                json!({"action": "update_package", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        "uninstall_package" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would uninstall package '{name}' via brew uninstall."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "brew",
                &["uninstall", name],
                ExecOpts { timeout_sec: 60, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Package '{name}' uninstalled.")
                } else {
                    format!("Uninstall of '{name}' failed: {}", result.stderr.trim())
                },
                json!({"action": "uninstall_package", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown package action: {action}"),
        )),
    }
}

pub async fn verify_installed(name: &str) -> Result<UnifiedResult> {
    let result = exec("brew", &["list", name], ExecOpts::default()).await?;
    let installed = result.success();

    Ok(UnifiedResult::ok(
        if installed {
            format!("Package '{name}' is installed.")
        } else {
            format!("Package '{name}' is NOT installed.")
        },
        json!({
            "check": "package_installed",
            "target": name,
            "passed": installed,
        }),
    ))
}

fn parse_brew_info(info: &serde_json::Value, name: &str) -> PackageState {
    let formulae = info.get("formulae").and_then(|f| f.as_array());
    if let Some(formulae) = formulae {
        if let Some(formula) = formulae.first() {
            let installed = formula
                .get("installed")
                .and_then(|i| i.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let version = formula
                .get("installed")
                .and_then(|i| i.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let latest = formula
                .get("versions")
                .and_then(|v| v.get("stable"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            return PackageState {
                name: name.to_string(),
                installed,
                version,
                latest_version: latest,
                source: Some("homebrew".into()),
            };
        }
    }

    PackageState {
        name: name.to_string(),
        installed: false,
        version: None,
        latest_version: None,
        source: Some("homebrew".into()),
    }
}
