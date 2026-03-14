use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, exec_shell, ExecOpts};
use crate::schemas::{CertInfo, CertSource, CertificateState, ChainPosition};

pub async fn observe(
    target: Option<&str>,
    scope: Option<&[String]>,
) -> Result<UnifiedResult> {
    let scope_str = scope.and_then(|s| s.first()).map(|s| s.as_str());

    match scope_str {
        Some("remote") => observe_remote(target).await,
        Some("local") => observe_local(target).await,
        Some("keychain") => observe_keychain(target).await,
        Some("expiring_soon") => observe_expiring_soon(target).await,
        _ => {
            if let Some(host) = target {
                // Default: probe remote host
                observe_remote(Some(host)).await
            } else {
                Ok(UnifiedResult::ok(
                    "Certificate observation requires a target (hostname for remote, path for local) or scope.",
                    json!({"hint": "Use scope: [\"remote\"] with target, or scope: [\"keychain\"] for local trust store."}),
                )
                .with_suggestions(vec![
                    "observe(certificate, target: \"example.com\", scope: [\"remote\"])".into(),
                    "observe(certificate, scope: [\"keychain\"])".into(),
                    "observe(certificate, scope: [\"expiring_soon\"])".into(),
                ]))
            }
        }
    }
}

async fn observe_remote(target: Option<&str>) -> Result<UnifiedResult> {
    let host = match target {
        Some(h) => h,
        None => return Ok(UnifiedResult::err(
            "missing_target",
            "Hostname required for remote certificate observation. Use --target <hostname>.",
        )),
    };

    // Fetch cert chain via openssl s_client
    let connect = format!("{host}:443");
    let cmd = format!(
        "echo | openssl s_client -connect {connect} -servername {host} 2>/dev/null | openssl x509 -noout -text -fingerprint -sha256 2>/dev/null"
    );
    let result = match exec_shell(&cmd, 10).await {
        Ok(r) => r,
        Err(_) => {
            return Ok(UnifiedResult::ok(
                format!("Could not connect to {host}:443 (connection timed out or refused)."),
                json!({"error": "connection_failed", "target": host}),
            ));
        }
    };

    if !result.success() || result.stdout.trim().is_empty() {
        return Ok(UnifiedResult::ok(
            format!("Could not retrieve certificate from {host}:443."),
            json!({"error": "connection_failed", "target": host}),
        ));
    }

    let cert = parse_x509_text(&result.stdout, CertSource::Remote);
    let mut warnings = Vec::new();
    if cert.is_expired {
        warnings.push(format!("Certificate for {host} is EXPIRED."));
    } else if cert.days_until_expiry <= 30 {
        warnings.push(format!(
            "Certificate for {host} expires in {} days.",
            cert.days_until_expiry
        ));
    }

    let state = CertificateState {
        certificates: vec![cert],
        warnings: if warnings.is_empty() { None } else { Some(warnings) },
    };

    Ok(UnifiedResult::ok(
        format!("Certificate for {host} retrieved."),
        serde_json::to_value(&state)?,
    )
    .with_suggestions(vec![
        format!("verify(cert_not_expired, target: \"{host}\")"),
        format!("verify(cert_chain_complete, target: \"{host}\")"),
    ]))
}

async fn observe_local(target: Option<&str>) -> Result<UnifiedResult> {
    let path = target.ok_or_else(|| anyhow::anyhow!("File path required for local certificate observation"))?;

    let result = exec(
        "openssl",
        &["x509", "-in", path, "-noout", "-text", "-fingerprint", "-sha256"],
        ExecOpts::default(),
    )
    .await?;

    if !result.success() {
        return Ok(UnifiedResult::ok(
            format!("Could not read certificate from {path}."),
            json!({"error": "read_failed", "target": path, "stderr": result.stderr.trim()}),
        ));
    }

    let cert = parse_x509_text(&result.stdout, CertSource::LocalFile);
    let state = CertificateState {
        certificates: vec![cert],
        warnings: None,
    };

    Ok(UnifiedResult::ok(
        format!("Certificate from {path} parsed."),
        serde_json::to_value(&state)?,
    ))
}

async fn observe_keychain(target: Option<&str>) -> Result<UnifiedResult> {
    let keychain = target.unwrap_or("/Library/Keychains/System.keychain");

    let result = exec(
        "security",
        &["find-certificate", "-a", "-p", keychain],
        ExecOpts { timeout_sec: 15, ..Default::default() },
    )
    .await?;

    if !result.success() {
        return Ok(UnifiedResult::ok(
            format!("Could not read keychain at {keychain}."),
            json!({"error": "keychain_read_failed", "stderr": result.stderr.trim()}),
        ));
    }

    // Split PEM blocks and parse each
    let pem_blocks: Vec<&str> = result
        .stdout
        .split("-----END CERTIFICATE-----")
        .filter(|b| b.contains("-----BEGIN CERTIFICATE-----"))
        .collect();

    let mut certs = Vec::new();
    for block in pem_blocks.iter().take(50) {
        // limit parsing to 50 certs for performance
        let pem = format!("{block}-----END CERTIFICATE-----");
        let cmd = format!(
            "echo '{}' | openssl x509 -noout -text -fingerprint -sha256 2>/dev/null",
            pem.replace('\'', "'\\''")
        );
        if let Ok(r) = exec_shell(&cmd, 5).await {
            if r.success() {
                certs.push(parse_x509_text(&r.stdout, CertSource::Keychain));
            }
        }
    }

    let count = certs.len();
    let total_pem = pem_blocks.len();
    let state = CertificateState {
        certificates: certs,
        warnings: if total_pem > 50 {
            Some(vec![format!("Showing 50 of {total_pem} certificates.")])
        } else {
            None
        },
    };

    Ok(UnifiedResult::ok(
        format!("{count} certificates found in keychain."),
        serde_json::to_value(&state)?,
    ))
}

async fn observe_expiring_soon(target: Option<&str>) -> Result<UnifiedResult> {
    // Check keychain for certs expiring within 30 days
    let days = target
        .and_then(|t| t.parse::<u32>().ok())
        .unwrap_or(30);
    let seconds = days * 86400;

    let keychain_result = exec(
        "security",
        &["find-certificate", "-a", "-p", "/Library/Keychains/System.keychain"],
        ExecOpts { timeout_sec: 15, ..Default::default() },
    )
    .await;

    let mut expiring = Vec::new();

    if let Ok(result) = keychain_result {
        if result.success() {
            let pem_blocks: Vec<&str> = result
                .stdout
                .split("-----END CERTIFICATE-----")
                .filter(|b| b.contains("-----BEGIN CERTIFICATE-----"))
                .collect();

            for block in pem_blocks.iter().take(100) {
                let pem = format!("{block}-----END CERTIFICATE-----");
                let cmd = format!(
                    "echo '{}' | openssl x509 -checkend {} -noout 2>/dev/null; echo $?",
                    pem.replace('\'', "'\\''"),
                    seconds
                );
                if let Ok(r) = exec_shell(&cmd, 5).await {
                    // exit code 1 means cert expires within the window
                    if r.stdout.trim().ends_with('1') {
                        let parse_cmd = format!(
                            "echo '{}' | openssl x509 -noout -text -fingerprint -sha256 2>/dev/null",
                            pem.replace('\'', "'\\''")
                        );
                        if let Ok(pr) = exec_shell(&parse_cmd, 5).await {
                            if pr.success() {
                                expiring.push(parse_x509_text(&pr.stdout, CertSource::Keychain));
                            }
                        }
                    }
                }
            }
        }
    }

    let count = expiring.len();
    let state = CertificateState {
        certificates: expiring,
        warnings: None,
    };

    Ok(UnifiedResult::ok(
        format!("{count} certificates expiring within {days} days."),
        serde_json::to_value(&state)?,
    ))
}

pub async fn act(
    action: &str,
    target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    let name = target.ok_or_else(|| anyhow::anyhow!("Certificate name/path required for {action}"))?;

    match action {
        "install_cert" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would install certificate '{name}' to system trust store via security add-trusted-cert."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "security",
                &["add-trusted-cert", "-d", "-r", "trustRoot", "-k", "/Library/Keychains/System.keychain", name],
                ExecOpts { elevated: true, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Certificate '{name}' installed to system trust store.")
                } else {
                    format!("Failed to install certificate: {}", result.stderr.trim())
                },
                json!({"action": "install_cert", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High)
            .with_suggestions(vec![format!("verify(cert_valid, target: \"{name}\")")]))
        }
        "remove_cert" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would remove certificate '{name}' from system keychain via security delete-certificate."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "security",
                &["delete-certificate", "-c", name],
                ExecOpts { elevated: true, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Certificate '{name}' removed from keychain.")
                } else {
                    format!("Failed to remove certificate: {}", result.stderr.trim())
                },
                json!({"action": "remove_cert", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        "trust_cert" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would mark certificate '{name}' as trusted."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "security",
                &["add-trusted-cert", "-d", "-r", "trustRoot", "-k", "/Library/Keychains/System.keychain", name],
                ExecOpts { elevated: true, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Certificate '{name}' marked as trusted.")
                } else {
                    format!("Failed to trust certificate: {}", result.stderr.trim())
                },
                json!({"action": "trust_cert", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        "untrust_cert" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    format!("Would mark certificate '{name}' as untrusted."),
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "security",
                &["add-trusted-cert", "-d", "-r", "deny", "-k", "/Library/Keychains/System.keychain", name],
                ExecOpts { elevated: true, ..Default::default() },
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    format!("Certificate '{name}' marked as untrusted.")
                } else {
                    format!("Failed to untrust certificate: {}", result.stderr.trim())
                },
                json!({"action": "untrust_cert", "target": name, "success": result.success()}),
            )
            .with_risk(Risk::High))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown certificate action: {action}"),
        )),
    }
}

pub async fn verify_valid(host: &str) -> Result<UnifiedResult> {
    let cmd = format!(
        "echo | openssl s_client -connect {host}:443 -servername {host} 2>/dev/null | openssl x509 -checkend 0 -noout 2>/dev/null"
    );
    let result = match exec_shell(&cmd, 10).await {
        Ok(r) => r,
        Err(_) => {
            return Ok(UnifiedResult::ok(
                format!("Could not connect to {host}:443 to verify certificate."),
                json!({"check": "cert_valid", "target": host, "passed": false, "error": "connection_failed"}),
            ));
        }
    };
    let valid = result.success();

    Ok(UnifiedResult::ok(
        if valid {
            format!("Certificate for {host} is valid (not expired).")
        } else {
            format!("Certificate for {host} is EXPIRED or unreachable.")
        },
        json!({
            "check": "cert_valid",
            "target": host,
            "passed": valid,
        }),
    ))
}

pub async fn verify_not_expired(host: &str, days: u32) -> Result<UnifiedResult> {
    let seconds = days * 86400;
    let cmd = format!(
        "echo | openssl s_client -connect {host}:443 -servername {host} 2>/dev/null | openssl x509 -checkend {seconds} -noout 2>/dev/null"
    );
    let result = match exec_shell(&cmd, 10).await {
        Ok(r) => r,
        Err(_) => {
            return Ok(UnifiedResult::ok(
                format!("Could not connect to {host}:443 to check certificate expiry."),
                json!({"check": "cert_not_expired", "target": host, "passed": false, "error": "connection_failed"}),
            ));
        }
    };
    let not_expiring = result.success();

    // Also get the actual expiry date for details
    let expiry_cmd = format!(
        "echo | openssl s_client -connect {host}:443 -servername {host} 2>/dev/null | openssl x509 -noout -enddate 2>/dev/null"
    );
    let expiry_result = exec_shell(&expiry_cmd, 10).await.ok();
    let end_date = expiry_result
        .as_ref()
        .map(|r| r.stdout.trim().replace("notAfter=", ""))
        .unwrap_or_default();

    Ok(UnifiedResult::ok(
        if not_expiring {
            format!("Certificate for {host} will not expire within {days} days (expires: {end_date}).")
        } else {
            format!("Certificate for {host} WILL expire within {days} days (expires: {end_date}).")
        },
        json!({
            "check": "cert_not_expired",
            "target": host,
            "passed": not_expiring,
            "days_horizon": days,
            "expires": end_date,
        }),
    ))
}

pub async fn verify_chain_complete(host: &str) -> Result<UnifiedResult> {
    let cmd = format!(
        "echo | openssl s_client -connect {host}:443 -servername {host} 2>&1"
    );
    let result = match exec_shell(&cmd, 10).await {
        Ok(r) => r,
        Err(_) => {
            return Ok(UnifiedResult::ok(
                format!("Could not connect to {host}:443 to verify certificate chain."),
                json!({"check": "cert_chain_complete", "target": host, "passed": false, "error": "connection_failed"}),
            ));
        }
    };

    // Check for "Verify return code: 0 (ok)"
    let passed = result.stdout.contains("Verify return code: 0")
        || result.stderr.contains("Verify return code: 0");

    let verify_line = result
        .combined()
        .lines()
        .find(|l| l.contains("Verify return code:"))
        .unwrap_or("")
        .trim()
        .to_string();

    Ok(UnifiedResult::ok(
        if passed {
            format!("Certificate chain for {host} is complete and valid.")
        } else {
            format!("Certificate chain for {host} has issues: {verify_line}")
        },
        json!({
            "check": "cert_chain_complete",
            "target": host,
            "passed": passed,
            "verify_result": verify_line,
        }),
    ))
}

pub async fn verify_hostname_matches(host: &str, expected: &str) -> Result<UnifiedResult> {
    let cmd = format!(
        "echo | openssl s_client -connect {host}:443 -servername {host} 2>/dev/null | openssl x509 -noout -text 2>/dev/null"
    );
    let result = match exec_shell(&cmd, 10).await {
        Ok(r) => r,
        Err(_) => {
            return Ok(UnifiedResult::ok(
                format!("Could not connect to {host}:443 to check hostname match."),
                json!({"check": "hostname_matches", "target": host, "passed": false, "error": "connection_failed"}),
            ));
        }
    };

    let sans = parse_sans(&result.stdout);
    let cn = parse_cn(&result.stdout);

    let matches = sans.iter().any(|s| hostname_matches_pattern(expected, s))
        || cn.as_ref().map(|c| hostname_matches_pattern(expected, c)).unwrap_or(false);

    Ok(UnifiedResult::ok(
        if matches {
            format!("Hostname '{expected}' matches certificate for {host}.")
        } else {
            format!("Hostname '{expected}' does NOT match certificate for {host}.")
        },
        json!({
            "check": "hostname_matches",
            "target": host,
            "passed": matches,
            "expected_hostname": expected,
            "san": sans,
            "cn": cn,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

fn parse_x509_text(output: &str, source: CertSource) -> CertInfo {
    let subject = extract_field(output, "Subject:");
    let issuer = extract_field(output, "Issuer:");
    let not_before = extract_field(output, "Not Before:");
    let not_after = extract_field(output, "Not After :");

    let is_self_signed = subject == issuer;

    let days_until_expiry = parse_days_until(&not_after);
    let is_expired = days_until_expiry < 0;

    let sans = parse_sans(output);
    let cn = parse_cn(output);

    let key_algorithm = extract_field_opt(output, "Public Key Algorithm:");
    let key_size = output
        .lines()
        .find(|l| l.contains("Public-Key:"))
        .and_then(|l| {
            l.trim()
                .trim_start_matches("Public-Key: (")
                .trim_end_matches(" bit)")
                .parse::<u32>()
                .ok()
        });

    let fingerprint = output
        .lines()
        .find(|l| l.starts_with("sha256 Fingerprint=") || l.starts_with("SHA256 Fingerprint="))
        .map(|l| l.split('=').nth(1).unwrap_or("").trim().to_string());

    let chain_position = if is_self_signed {
        Some(ChainPosition::Root)
    } else if output.contains("CA:TRUE") {
        Some(ChainPosition::Intermediate)
    } else {
        Some(ChainPosition::Leaf)
    };

    CertInfo {
        subject: cn.unwrap_or(subject),
        issuer,
        not_before,
        not_after,
        days_until_expiry,
        is_expired,
        is_self_signed,
        san: if sans.is_empty() { None } else { Some(sans) },
        key_algorithm,
        key_size,
        fingerprint_sha256: fingerprint,
        chain_position,
        source,
        trusted: None,
    }
}

fn extract_field(output: &str, prefix: &str) -> String {
    output
        .lines()
        .find(|l| l.trim().starts_with(prefix))
        .map(|l| l.trim().trim_start_matches(prefix).trim().to_string())
        .unwrap_or_default()
}

fn extract_field_opt(output: &str, prefix: &str) -> Option<String> {
    let val = extract_field(output, prefix);
    if val.is_empty() { None } else { Some(val) }
}

fn parse_cn(output: &str) -> Option<String> {
    let subject = extract_field(output, "Subject:");
    subject
        .split(',')
        .find(|s| s.trim().starts_with("CN"))
        .map(|s| {
            s.trim()
                .trim_start_matches("CN")
                .trim_start_matches(" = ")
                .trim_start_matches('=')
                .trim()
                .to_string()
        })
}

fn parse_sans(output: &str) -> Vec<String> {
    // Find the SAN line (typically indented under X509v3 Subject Alternative Name)
    let mut in_san = false;
    for line in output.lines() {
        if line.contains("X509v3 Subject Alternative Name:") {
            in_san = true;
            continue;
        }
        if in_san {
            return line
                .trim()
                .split(',')
                .filter_map(|s| {
                    let s = s.trim();
                    if s.starts_with("DNS:") {
                        Some(s.trim_start_matches("DNS:").to_string())
                    } else {
                        None
                    }
                })
                .collect();
        }
    }
    Vec::new()
}

fn parse_days_until(not_after: &str) -> i32 {
    if not_after.is_empty() {
        return -1;
    }

    // Parse openssl date format: "Mar 15 12:00:00 2026 GMT"
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let parts: Vec<&str> = not_after.split_whitespace().collect();
    if parts.len() >= 4 {
        let month = months.iter().position(|&m| m == parts[0]).map(|i| i as u32 + 1);
        let day = parts[1].parse::<u32>().ok();
        let year = parts[3].parse::<i32>().ok();

        if let (Some(month), Some(day), Some(year)) = (month, day, year) {
            let target = days_from_epoch(year, month, day);
            // Use compile-time-ish "now" — in practice the adapter runs at current time,
            // so we compute today dynamically from std::time
            let now = {
                // Seconds since Unix epoch → days
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                (secs / 86400) as i64
            };
            return (target - now) as i32;
        }
    }

    0
}

/// Days from Unix epoch (1970-01-01) for a given date. Handles leap years.
fn days_from_epoch(year: i32, month: u32, day: u32) -> i64 {
    // Compute using the algorithm from Howard Hinnant's date library
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 { month + 9 } else { month - 3 } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn hostname_matches_pattern(hostname: &str, pattern: &str) -> bool {
    if pattern.starts_with("*.") {
        let suffix = &pattern[1..]; // ".example.com"
        hostname.ends_with(suffix) || hostname == &pattern[2..]
    } else {
        hostname == pattern
    }
}
