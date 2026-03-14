# world

**Observe · Act · Verify** — a POMDP interface for agents.

`world` gives AI agents structured, safe access to operating system state and remediation actions. Instead of shelling out blind commands and parsing unpredictable output, agents interact through a typed interface with built-in safety rails, risk classification, and audit trails.

```
world observe network --scope interfaces,internet_status
world act network dns_cache reset
world verify dns_resolves --target google.com
```

## Why

Agents that manage systems today do it through raw shell commands. This is fragile, dangerous, and unobservable. `world` replaces that with a **finite set of typed operations** — observe state, take action, verify the result — modeled as a [POMDP](https://en.wikipedia.org/wiki/Partially_observable_Markov_decision_process) so agents can reason about what they know and what they can do.

Every action is risk-classified. Every tool call is logged. Catastrophic commands are hard-blocked. High-risk operations require explicit consent. Agents can't `rm -rf /` — they can only invoke whitelisted verbs on known domains.

## The Five Tools

| Tool | Safety | Purpose |
|------|--------|---------|
| **observe** | ReadOnly | Structured state observation with progressive disclosure |
| **act** | NeedsApproval | Constrained remediation through whitelisted verbs |
| **verify** | ReadOnly | Post-remediation condition checks |
| **bash** | NeedsApproval | Escape hatch with blocked/dangerous pattern detection |
| **handoff** | ReadOnly | Structured escalation when the agent can't resolve |

### Progressive Disclosure

Call `observe` with just a domain to discover what's available:

```bash
world observe network
# → allowed_scopes: [interfaces, routes, dns, gateway, proxy, internet_status]
# → related_remediations: [flush_dns, renew_dhcp, toggle_adapter, ...]
# → related_verifications: [host_reachable, dns_resolves, internet_reachable, ...]
```

Then drill in:

```bash
world observe network --scope interfaces,dns
```

### Safety Model

```
ReadOnly ──→ SafeAction ──→ NeedsApproval
  (observe)    (flush dns)     (install pkg)
```

- **Allowlist enforcement** — only declared actions execute
- **Risk classification** — Low / Medium / High per domain+action
- **Hard blocks** — `rm -rf /`, `mkfs`, `dd if=`, fork bombs rejected outright
- **Consent gates** — high-risk actions require `--yes` or interactive confirmation
- **Dry run** — every action supports `--dry-run` to preview without executing

## Domains

| Domain | Observe | Act | Examples |
|--------|:-------:|:---:|---------|
| **network** | interfaces, routes, dns, gateway, proxy | flush_dns, renew_dhcp, toggle_adapter, forget_wifi | `world act network dns_cache reset` |
| **service** | status, startup_mode, recent_errors, deps | start, stop, restart, set_startup_mode | `world act service nginx restart` |
| **process** | processes, tree, top_cpu, top_memory, open_files, listening_ports | kill_graceful, kill_force, set_priority | `world observe process --scope top_cpu --limit 5` |
| **container** | containers, images, volumes, networks, container_logs | start, stop, restart, remove, pull, prune | `world observe container --scope containers` |
| **certificate** | remote, local, keychain, expiring_soon | install, remove, trust, untrust | `world observe certificate --target github.com --scope remote` |
| **disk** | space, mounts, temp_usage, large_paths | clear_temp, remove_caches, mount/unmount share | `world observe disk --scope space` |
| **printer** | status, queue, driver, port | clear_queue, restart_spooler, set_default | `world act printer queue clear` |
| **package** | installed, version, recent_updates | install, uninstall, repair, update | `world act package jq add` |
| **log** | recent_errors, warnings, matching, timeline | *(read-only)* | `world observe log --since 1h` |
| **share** | | map, disconnect, refresh_credentials | Stub |
| **identity** | | clear_credentials, re_authenticate | Stub |
| **security** | | allow/remove firewall rules | Stub |

Platform adapters currently implemented for **macOS**. Linux and Windows are stubbed and ready for contribution.

## Example: Network Troubleshooting

```bash
# What can we observe?
world observe network

# Get the full picture
world observe network --scope interfaces,internet_status

# Internet down? Check reachability
world verify internet_reachable

# Try flushing DNS (low risk — auto-approved)
world act network dns_cache reset

# Verify the fix
world verify dns_resolves --target google.com

# Still broken? Escalate with full context
world handoff summary "DNS broken after flush" severity high
```

Every step is logged. The handoff includes the full telemetry trail as evidence.

## Example: Process Investigation

```bash
# Top CPU consumers
world observe process --scope top_cpu --limit 5

# Find processes by name
world observe process --target postgres

# What's listening on the network?
world observe process --scope listening_ports

# Process tree from a specific PID
world observe process --scope tree --target 1234

# Graceful kill (SIGTERM, medium risk)
world act process 5678 kill --dry-run

# Force kill (SIGKILL, high risk — requires confirmation)
world act process 5678 remove --yes
```

## Example: Container Management

```bash
# List all containers (auto-detects Docker or Podman)
world observe container --scope containers

# Images and volumes
world observe container --scope images
world observe container --scope volumes

# Container logs
world observe container --scope container_logs --target my-nginx --limit 100

# Lifecycle
world act container my-nginx restart --dry-run
world act container my-nginx disable           # stop
world act container my-nginx enable            # start

# Cleanup (high risk — requires confirmation)
world act container images clear --yes
```

## Example: Certificate Monitoring

```bash
# Inspect a remote certificate
world observe certificate --target github.com --scope remote

# Check certificates expiring within 30 days
world observe certificate --scope expiring_soon

# Read a local certificate file
world observe certificate --target /path/to/cert.pem --scope local

# Browse the system keychain
world observe certificate --scope keychain
```

Certificate observations include `days_until_expiry` (negative when expired), `is_self_signed`, SAN lists, chain position, and fingerprints — all the data an agent needs to assess TLS health without parsing openssl output.

## Agent Integration

Tools expose Anthropic-compatible JSON schemas, so they plug directly into any tool-use LLM:

```rust
use world::create_tools;

let tools = create_tools();
// Each tool provides: name, description, input_schema, execute()
// Feed schemas to your LLM, dispatch calls to execute()
```

Output is always structured:

```json
{
  "output": "DNS cache flushed successfully",
  "details": { "domain": "network", "action": "flush_dns" },
  "risk": "Low",
  "next_suggested_actions": ["verify(dns_resolves, google.com)"]
}
```

## Architecture

```
Agent / CLI
    ↓
┌─────────────────────────────┐
│  5 Unified Tools            │  ← Tool trait (name, schema, execute)
├─────────────────────────────┤
│  Policy Layer               │  ← Risk classification, allowlists, consent
├─────────────────────────────┤
│  Domain Dispatch            │  ← Route to correct handler
├─────────────────────────────┤
│  Platform Adapters          │  ← macOS / Linux / Windows
├─────────────────────────────┤
│  Normalized Schemas         │  ← NetworkState, ServiceState, ...
└─────────────────────────────┘
    ↓
Telemetry Log (every call recorded)
```

**Plugin system** — domains are plugins. Native Rust for performance, external subprocesses (any language) for extensibility. Drop a plugin in `plugins/` and it's discovered automatically.

## CLI Reference

```
world observe DOMAIN [--target T] [--scope S1,S2] [--since 1h] [--limit N]
world act DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run] [--yes]
world verify CHECK [--target T] [--timeout-sec N]
world spec [DOMAIN]              # Show POMDP spec (observations + actions)
world tools                      # List all tools with schemas
world addons [NAME]              # List/inspect plugin add-ons
world completions SHELL           # Generate shell completions
```

**Output modes:** `--json` (for agents), `--pretty` (for humans, default in TTY), `-q` (exit code only).

## Building

```bash
cargo build --release
cargo test
```

## License

MIT
