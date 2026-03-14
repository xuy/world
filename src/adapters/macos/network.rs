use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, exec_shell, ExecOpts};
use crate::schemas::{InterfaceType, NetworkInterface, NetworkState};

pub async fn observe(
    _target: Option<&str>,
    scope: Option<&[String]>,
) -> Result<UnifiedResult> {
    let scopes: Vec<&str> = scope
        .map(|s| s.iter().map(|x| x.as_str()).collect())
        .unwrap_or_else(|| vec!["interfaces", "dns", "gateway", "internet_status"]);

    let mut state = NetworkState {
        interfaces: vec![],
        internet_reachable: None,
        proxy_enabled: None,
        vpn_present: None,
        warnings: None,
    };
    let mut warnings = Vec::new();

    if scopes.contains(&"interfaces") || scopes.contains(&"gateway") || scopes.contains(&"dns") {
        // Get interface info
        let ifconfig = exec("ifconfig", &[], ExecOpts::default()).await?;
        state.interfaces = parse_interfaces(&ifconfig.stdout);

        // Get Wi-Fi info
        let wifi = exec("networksetup", &["-getinfo", "Wi-Fi"], ExecOpts::default()).await;
        if let Ok(ref wifi_res) = wifi {
            enrich_wifi_info(&mut state, &wifi_res.stdout);
        }

        // Get DNS
        let dns = exec("scutil", &["--dns"], ExecOpts::default()).await?;
        enrich_dns_info(&mut state, &dns.stdout);

        // Detect VPN
        state.vpn_present = Some(
            state.interfaces.iter().any(|i| {
                matches!(i.iface_type, Some(InterfaceType::Vpn))
                    || i.name.starts_with("utun")
                    || i.name.starts_with("ipsec")
            }),
        );
    }

    if scopes.contains(&"proxy") {
        let proxy = exec("networksetup", &["-getwebproxy", "Wi-Fi"], ExecOpts::default()).await;
        if let Ok(ref res) = proxy {
            state.proxy_enabled = Some(res.stdout.contains("Enabled: Yes"));
        }
    }

    if scopes.contains(&"internet_status") {
        let ping = exec("ping", &["-c", "1", "-W", "3", "8.8.8.8"], ExecOpts::default()).await;
        state.internet_reachable = Some(ping.map(|r| r.success()).unwrap_or(false));

        if state.internet_reachable == Some(false) {
            warnings.push("Internet appears unreachable (ping to 8.8.8.8 failed).".into());
        }
    }

    if !warnings.is_empty() {
        state.warnings = Some(warnings);
    }

    let details = serde_json::to_value(&state)?;
    let summary = format_network_summary(&state);

    Ok(UnifiedResult::ok(summary, details).with_suggestions(vec![
        "verify(internet_reachable)".into(),
        "verify(dns_resolves, target: \"google.com\")".into(),
        "act(network, flush_dns)".into(),
    ]))
}

pub async fn act(
    action: &str,
    _target: Option<&str>,
    dry_run: bool,
) -> Result<UnifiedResult> {
    match action {
        "flush_dns" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would flush DNS cache (dscacheutil -flushcache + killall mDNSResponder).",
                    json!({"dry_run": true}),
                ));
            }
            let _ = exec("dscacheutil", &["-flushcache"], ExecOpts::default()).await;
            let _ = exec("killall", &["-HUP", "mDNSResponder"], ExecOpts::default()).await;

            Ok(UnifiedResult::ok(
                "DNS cache flushed successfully.",
                json!({"action": "flush_dns", "success": true}),
            )
            .with_risk(Risk::Low)
            .with_suggestions(vec![
                "verify(dns_resolves, target: \"google.com\")".into(),
                "verify(internet_reachable)".into(),
            ]))
        }
        "renew_dhcp" => {
            if dry_run {
                return Ok(UnifiedResult::ok(
                    "Would renew DHCP lease on primary interface.",
                    json!({"dry_run": true}),
                ));
            }
            let result = exec(
                "ipconfig",
                &["set", "en0", "DHCP"],
                ExecOpts::default(),
            )
            .await?;

            Ok(UnifiedResult::ok(
                if result.success() {
                    "DHCP lease renewed on en0.".into()
                } else {
                    format!("DHCP renewal may have failed: {}", result.stderr.trim())
                },
                json!({"action": "renew_dhcp", "success": result.success()}),
            )
            .with_risk(Risk::Medium)
            .with_suggestions(vec!["verify(internet_reachable)".into()]))
        }
        _ => Ok(UnifiedResult::err(
            "unknown_action",
            format!("Unknown network action: {action}"),
        )),
    }
}

pub async fn verify_host_reachable(host: &str, timeout_sec: u32) -> Result<UnifiedResult> {
    let timeout = timeout_sec.min(10).to_string();
    let result = exec(
        "ping",
        &["-c", "1", "-W", &timeout, host],
        ExecOpts { timeout_sec: timeout_sec + 2, ..Default::default() },
    )
    .await?;

    let reachable = result.success();
    Ok(UnifiedResult::ok(
        if reachable {
            format!("{host} is reachable.")
        } else {
            format!("{host} is NOT reachable.")
        },
        json!({
            "check": "host_reachable",
            "target": host,
            "passed": reachable,
            "duration_ms": result.duration_ms,
        }),
    ))
}

pub async fn verify_dns_resolves(domain: &str, timeout_sec: u32) -> Result<UnifiedResult> {
    let result = exec(
        "dig",
        &["+short", "+time=3", domain],
        ExecOpts { timeout_sec: timeout_sec + 2, ..Default::default() },
    )
    .await?;

    let resolved = result.success() && !result.stdout.trim().is_empty();
    let addresses: Vec<&str> = result.stdout.lines().collect();

    Ok(UnifiedResult::ok(
        if resolved {
            format!("{domain} resolves to: {}", addresses.join(", "))
        } else {
            format!("{domain} does NOT resolve.")
        },
        json!({
            "check": "dns_resolves",
            "target": domain,
            "passed": resolved,
            "addresses": addresses,
        }),
    ))
}

pub async fn verify_internet_reachable(timeout_sec: u32) -> Result<UnifiedResult> {
    let result = exec(
        "curl",
        &["-o", "/dev/null", "-s", "-w", "%{http_code}", "--max-time",
          &timeout_sec.min(10).to_string(), "https://www.google.com"],
        ExecOpts { timeout_sec: timeout_sec + 2, ..Default::default() },
    )
    .await?;

    let status_code = result.stdout.trim().parse::<u16>().unwrap_or(0);
    let reachable = (200..400).contains(&status_code);

    Ok(UnifiedResult::ok(
        if reachable {
            "Internet is reachable.".into()
        } else {
            format!("Internet appears unreachable (HTTP {status_code}).")
        },
        json!({
            "check": "internet_reachable",
            "passed": reachable,
            "http_status": status_code,
            "duration_ms": result.duration_ms,
        }),
    ))
}

pub async fn verify_port_open(host: &str, port: u16, timeout_sec: u32) -> Result<UnifiedResult> {
    let cmd = format!("nc -z -w {} {} {}", timeout_sec.min(10), host, port);
    let result = exec_shell(&cmd, timeout_sec + 2).await?;

    let open = result.success();
    Ok(UnifiedResult::ok(
        if open {
            format!("Port {port} on {host} is open.")
        } else {
            format!("Port {port} on {host} is NOT open.")
        },
        json!({
            "check": "port_open",
            "target": host,
            "port": port,
            "passed": open,
        }),
    ))
}

// ── Parsing helpers ──────────────────────────────────────────────────────

fn parse_interfaces(ifconfig_output: &str) -> Vec<NetworkInterface> {
    let mut interfaces = Vec::new();
    let mut current: Option<NetworkInterface> = None;

    for line in ifconfig_output.lines() {
        if !line.starts_with('\t') && !line.starts_with(' ') && line.contains(':') {
            // New interface header, e.g. "en0: flags=8863<UP,...>"
            if let Some(iface) = current.take() {
                interfaces.push(iface);
            }
            let name = line.split(':').next().unwrap_or("").to_string();
            let up = line.contains("<UP");
            let iface_type = guess_interface_type(&name);
            current = Some(NetworkInterface {
                name,
                up,
                addresses: Some(vec![]),
                gateway: None,
                dns_servers: None,
                iface_type: Some(iface_type),
            });
        } else if let Some(ref mut iface) = current {
            let trimmed = line.trim();
            if trimmed.starts_with("inet ") {
                // IPv4 address
                if let Some(addr) = trimmed.split_whitespace().nth(1) {
                    if let Some(ref mut addrs) = iface.addresses {
                        addrs.push(addr.to_string());
                    }
                }
            } else if trimmed.starts_with("inet6 ") {
                if let Some(addr) = trimmed.split_whitespace().nth(1) {
                    // Skip link-local noise for cleaner output
                    if !addr.starts_with("fe80") {
                        if let Some(ref mut addrs) = iface.addresses {
                            addrs.push(addr.to_string());
                        }
                    }
                }
            }
        }
    }
    if let Some(iface) = current {
        interfaces.push(iface);
    }

    // Filter out uninteresting interfaces (keep en*, utun*, ipsec*, bridge*, lo0)
    interfaces.retain(|i| {
        i.name.starts_with("en")
            || i.name.starts_with("utun")
            || i.name.starts_with("ipsec")
            || i.name.starts_with("bridge")
            || i.name == "lo0"
    });

    interfaces
}

fn guess_interface_type(name: &str) -> InterfaceType {
    if name == "lo0" {
        InterfaceType::Loopback
    } else if name.starts_with("en0") || name.starts_with("en1") {
        // On modern Macs, en0 is usually Wi-Fi
        InterfaceType::Wifi
    } else if name.starts_with("en") {
        InterfaceType::Ethernet
    } else if name.starts_with("utun") || name.starts_with("ipsec") {
        InterfaceType::Vpn
    } else {
        InterfaceType::Other
    }
}

fn enrich_wifi_info(state: &mut NetworkState, wifi_output: &str) {
    for line in wifi_output.lines() {
        let line = line.trim();
        if line.starts_with("IP address:") {
            // Already captured via ifconfig
        } else if line.starts_with("Router:") {
            if let Some(gw) = line.strip_prefix("Router:").map(|s| s.trim().to_string()) {
                // Set gateway on the Wi-Fi interface
                for iface in &mut state.interfaces {
                    if matches!(iface.iface_type, Some(InterfaceType::Wifi)) {
                        iface.gateway = Some(gw.clone());
                    }
                }
            }
        }
    }
}

fn enrich_dns_info(state: &mut NetworkState, dns_output: &str) {
    let mut servers = Vec::new();
    for line in dns_output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("nameserver[") {
            if let Some(addr) = trimmed.split(':').nth(1) {
                let addr = addr.trim().to_string();
                if !servers.contains(&addr) {
                    servers.push(addr);
                }
            }
        }
    }
    if !servers.is_empty() {
        // Attach DNS to the first active interface
        for iface in &mut state.interfaces {
            if iface.up && !matches!(iface.iface_type, Some(InterfaceType::Loopback)) {
                iface.dns_servers = Some(servers.clone());
                break;
            }
        }
    }
}

fn format_network_summary(state: &NetworkState) -> String {
    let mut parts = Vec::new();

    let active_count = state.interfaces.iter().filter(|i| i.up).count();
    parts.push(format!("{active_count} active interface(s)."));

    if let Some(true) = state.internet_reachable {
        parts.push("Internet reachable.".into());
    } else if let Some(false) = state.internet_reachable {
        parts.push("Internet NOT reachable.".into());
    }

    if let Some(true) = state.vpn_present {
        parts.push("VPN adapter detected.".into());
    }

    if let Some(true) = state.proxy_enabled {
        parts.push("Web proxy enabled.".into());
    }

    if let Some(ref warnings) = state.warnings {
        for w in warnings {
            parts.push(format!("⚠ {w}"));
        }
    }

    parts.join(" ")
}
