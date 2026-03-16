use anyhow::Result;
use serde_json::json;

use crate::contracts::{Risk, UnifiedResult};
use crate::execution::{exec, exec_shell, ExecOpts};
use crate::schemas::{InterfaceType, NetworkInterface, NetworkState, VpnConnection, VpnStatus};

pub async fn observe(
    target: Option<&str>,
) -> Result<UnifiedResult> {
    // Target interpretation:
    //   None               → everything
    //   "interfaces"       → all interfaces
    //   "dns"              → DNS config
    //   "gateway"          → gateway/routing
    //   "internet_status"  → reachability check
    //   "proxy"            → proxy settings
    //   "en0" / anything else → search interfaces by name, return match
    match target {
        Some("dns") => return observe_aspect_dns().await,
        Some("internet_status") => return observe_aspect_internet().await,
        Some("proxy") => return observe_aspect_proxy().await,
        Some("vpn") => return observe_aspect_vpn().await,
        _ => {} // "interfaces", specific name, or None — all need interface data
    }

    // Fetch full interface state (needed for "interfaces", specific name, and default)
    let mut state = NetworkState {
        interfaces: vec![],
        internet_reachable: None,
        proxy_enabled: None,
        vpns: None,
        warnings: None,
    };
    let mut warnings = Vec::new();

    let ifconfig = exec("ifconfig", &[], ExecOpts::default()).await?;
    state.interfaces = parse_interfaces(&ifconfig.stdout);

    let wifi = exec("networksetup", &["-getinfo", "Wi-Fi"], ExecOpts::default()).await;
    if let Ok(ref wifi_res) = wifi {
        enrich_wifi_info(&mut state, &wifi_res.stdout);
    }

    let dns = exec("scutil", &["--dns"], ExecOpts::default()).await?;
    enrich_dns_info(&mut state, &dns.stdout);

    // Rich VPN observation via scutil --nc
    let vpns = observe_vpns().await;
    if !vpns.is_empty() {
        state.vpns = Some(vpns);
    }

    // If target is a specific name (not "interfaces" and not None), filter to it
    if let Some(t) = target {
        if t != "interfaces" && t != "gateway" {
            // Search interfaces by name
            state.interfaces.retain(|i| i.name == t);
            if state.interfaces.is_empty() {
                return Ok(UnifiedResult::err(
                    "not_found",
                    format!("Interface '{t}' not found. Use 'world observe network interfaces' to list all."),
                ));
            }
        }
    }

    // For default (no target), also check internet
    if target.is_none() {
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

async fn observe_aspect_dns() -> Result<UnifiedResult> {
    let dns = exec("scutil", &["--dns"], ExecOpts::default()).await?;
    // Extract resolver entries
    let servers: Vec<String> = dns
        .stdout
        .lines()
        .filter(|l| l.contains("nameserver"))
        .map(|l| l.trim().replace("nameserver[0] : ", "").replace("nameserver[1] : ", "").trim().to_string())
        .collect();
    Ok(UnifiedResult::ok(
        format!("{} DNS servers configured.", servers.len()),
        json!({"dns_servers": servers}),
    ))
}

async fn observe_aspect_internet() -> Result<UnifiedResult> {
    let ping = exec("ping", &["-c", "1", "-W", "3", "8.8.8.8"], ExecOpts::default()).await;
    let reachable = ping.map(|r| r.success()).unwrap_or(false);
    Ok(UnifiedResult::ok(
        if reachable { "Internet is reachable." } else { "Internet appears unreachable." },
        json!({"internet_reachable": reachable}),
    ))
}

async fn observe_aspect_vpn() -> Result<UnifiedResult> {
    let vpns = observe_vpns().await;
    let summary = if vpns.is_empty() {
        "No VPN configurations found.".to_string()
    } else {
        let connected = vpns.iter().filter(|v| matches!(v.status, VpnStatus::Connected)).count();
        format!("{} VPN(s) configured, {} connected.", vpns.len(), connected)
    };
    Ok(UnifiedResult::ok(summary, json!({"vpns": vpns})))
}

async fn observe_aspect_proxy() -> Result<UnifiedResult> {
    let proxy = exec("networksetup", &["-getwebproxy", "Wi-Fi"], ExecOpts::default()).await;
    let enabled = proxy.map(|r| r.stdout.contains("Enabled: Yes")).unwrap_or(false);
    Ok(UnifiedResult::ok(
        if enabled { "Web proxy is enabled." } else { "No web proxy configured." },
        json!({"proxy_enabled": enabled}),
    ))
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

// ── VPN observation ─────────────────────────────────────────────────────

/// Observe all VPN connections via scutil --nc.
/// Returns structured VPN info: name, status, protocol, addresses, DNS, uptime.
async fn observe_vpns() -> Vec<VpnConnection> {
    let nc_list = match exec("scutil", &["--nc", "list"], ExecOpts::default()).await {
        Ok(r) => r.stdout,
        Err(_) => return vec![],
    };

    let mut vpns = Vec::new();

    for line in nc_list.lines().skip(1) {
        // Format: * (Status) UUID TYPE "Name" [Protocol:xxx]
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (status_str, rest) = parse_nc_list_line(line);

        // Extract name from quotes
        let name = match rest.find('"') {
            Some(start) => match rest[start + 1..].find('"') {
                Some(end) => rest[start + 1..start + 1 + end].to_string(),
                None => continue,
            },
            None => continue,
        };

        // Extract protocol from [VPN:xxx] or [com.xxx]
        let protocol = extract_protocol(rest);

        let status = match status_str {
            "Connected" => VpnStatus::Connected,
            "Disconnected" => VpnStatus::Disconnected,
            "Connecting" => VpnStatus::Connecting,
            "Disconnecting" => VpnStatus::Disconnecting,
            _ => VpnStatus::Unknown,
        };

        // Get detailed info for connected VPNs
        let mut vpn = VpnConnection {
            name: name.clone(),
            status,
            protocol,
            local_address: None,
            server_address: None,
            interface: None,
            dns_servers: None,
            search_domains: None,
            connect_time_sec: None,
        };

        if matches!(vpn.status, VpnStatus::Connected) {
            enrich_vpn_details(&mut vpn).await;
        }

        vpns.push(vpn);
    }

    vpns
}

/// Parse a line from `scutil --nc list` into (status, rest).
fn parse_nc_list_line(line: &str) -> (&str, &str) {
    // Line format: "* (Connected)  UUID ..." or "  (Disconnected)  UUID ..."
    if let Some(start) = line.find('(') {
        if let Some(end) = line.find(')') {
            let status = &line[start + 1..end];
            let rest = &line[end + 1..];
            return (status, rest);
        }
    }
    ("Unknown", line)
}

/// Extract protocol from the [VPN:xxx] bracket at end of nc list line.
fn extract_protocol(line: &str) -> Option<String> {
    // [VPN:io.tailscale.ipn.macsys] → "Tailscale"
    // [VPN:com.wireguard.macos] → "WireGuard"
    // [VPN:L2TP] → "L2TP"
    // [VPN:IKEv2] → "IKEv2"
    // [VPN:com.cisco.anyconnect] → "Cisco AnyConnect"
    let bracket_start = line.rfind('[')?;
    let bracket_end = line.rfind(']')?;
    let inner = &line[bracket_start + 1..bracket_end];

    // Strip "VPN:" prefix if present
    let proto_raw = inner.strip_prefix("VPN:").unwrap_or(inner);

    // Map known bundle IDs to friendly names
    let friendly = match proto_raw {
        s if s.contains("tailscale") => "Tailscale",
        s if s.contains("wireguard") => "WireGuard",
        s if s.contains("cisco") || s.contains("anyconnect") => "Cisco AnyConnect",
        s if s.contains("openvpn") || s.contains("tunnelblick") => "OpenVPN",
        s if s.contains("mullvad") => "Mullvad",
        s if s.contains("nordvpn") => "NordVPN",
        s if s.contains("expressvpn") => "ExpressVPN",
        s if s.contains("cloudflare") || s.contains("warp") => "Cloudflare WARP",
        _ => proto_raw,
    };
    Some(friendly.to_string())
}

/// Fetch detailed status for a connected VPN via scutil --nc status.
async fn enrich_vpn_details(vpn: &mut VpnConnection) {
    let result = match exec(
        "scutil",
        &["--nc", "status", &vpn.name],
        ExecOpts::default(),
    )
    .await
    {
        Ok(r) => r.stdout,
        Err(_) => return,
    };

    // Parse the plist-like output from scutil --nc status.
    // The output uses nested dictionaries/arrays with indentation.
    // We track depth to know when a top-level section ends.
    let mut dns_servers = Vec::new();
    let mut search_domains = Vec::new();
    let mut section_stack: Vec<&str> = Vec::new();

    for line in result.lines() {
        let trimmed = line.trim();

        // Track brace depth
        if trimmed == "}" {
            section_stack.pop();
            continue;
        }

        // New named section with opening brace
        if trimmed.contains(" : <") && trimmed.ends_with('{') {
            let key = trimmed.split(':').next().unwrap_or("").trim();
            let tag = match key {
                "IPv4" => "ipv4",
                "Addresses" if section_stack.last() == Some(&"ipv4") => "ipv4_addr",
                "DNSServers" => "dns",
                "DNSSearchDomains" => "search",
                "VPN" => "vpn",
                _ => "_",
            };
            section_stack.push(tag);
            continue;
        }

        let current = section_stack.last().copied().unwrap_or("");

        // Extract values based on current section
        match current {
            "ipv4" => {
                if trimmed.starts_with("InterfaceName :") {
                    vpn.interface = extract_value(trimmed);
                } else if trimmed.starts_with("ServerAddress :") {
                    let addr = extract_value(trimmed);
                    if addr.as_deref() != Some("127.0.0.1") {
                        vpn.server_address = addr;
                    }
                }
            }
            "ipv4_addr" => {
                if let Some(addr) = extract_array_value(trimmed) {
                    vpn.local_address = Some(addr);
                }
            }
            "dns" => {
                if let Some(server) = extract_array_value(trimmed) {
                    dns_servers.push(server);
                }
            }
            "search" => {
                if let Some(domain) = extract_array_value(trimmed) {
                    search_domains.push(domain);
                }
            }
            "vpn" => {
                if trimmed.starts_with("ConnectTime :") {
                    if let Some(val) = extract_value(trimmed) {
                        vpn.connect_time_sec = val.parse::<u64>().ok();
                    }
                } else if trimmed.starts_with("RemoteAddress :") {
                    let addr = extract_value(trimmed);
                    if addr.as_deref() != Some("127.0.0.1") {
                        vpn.server_address = vpn.server_address.clone().or(addr);
                    }
                }
            }
            _ => {}
        }
    }

    if !dns_servers.is_empty() {
        vpn.dns_servers = Some(dns_servers);
    }
    if !search_domains.is_empty() {
        vpn.search_domains = Some(search_domains);
    }
}

/// Extract value from "Key : Value" format.
fn extract_value(line: &str) -> Option<String> {
    let val = line.split(':').nth(1)?.trim().to_string();
    if val.is_empty() || val.starts_with('<') {
        None
    } else {
        Some(val)
    }
}

/// Extract value from array entry like "0 : 100.100.100.100".
fn extract_array_value(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match lines like "0 : value" (array entries)
    let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
    if parts.len() == 2 && parts[0].trim().parse::<u32>().is_ok() {
        let val = parts[1].trim().to_string();
        if !val.is_empty() && !val.starts_with('<') {
            return Some(val);
        }
    }
    None
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

    if let Some(ref vpns) = state.vpns {
        for vpn in vpns {
            let proto = vpn.protocol.as_deref().unwrap_or("VPN");
            let status = match vpn.status {
                VpnStatus::Connected => {
                    let addr = vpn.local_address.as_deref().unwrap_or("?");
                    format!("{proto} ({}) connected, IP {addr}", vpn.name)
                }
                VpnStatus::Disconnected => format!("{proto} ({}) disconnected", vpn.name),
                VpnStatus::Connecting => format!("{proto} ({}) connecting...", vpn.name),
                VpnStatus::Disconnecting => format!("{proto} ({}) disconnecting...", vpn.name),
                VpnStatus::Unknown => format!("{proto} ({}) unknown", vpn.name),
            };
            parts.push(status);
        }
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
