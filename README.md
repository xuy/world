# world

**Observe · Act · Await** — a POMDP interface for agents.

`world` gives AI agents structured, safe access to operating system state and remediation actions. Instead of shelling out blind commands and parsing unpredictable output, agents interact through a typed interface with built-in safety rails, risk classification, and audit trails.

```
world observe network --scope interfaces,internet_status
world act network dns_cache reset
world await network dns_resolves --target google.com
```

## Why

Agents that manage systems today do it through raw shell commands. This is fragile, dangerous, and unobservable. `world` replaces that with a **finite set of typed operations** — observe state, take action, await the result — modeled as a [POMDP](https://en.wikipedia.org/wiki/Partially_observable_Markov_decision_process) so agents can reason about what they know and what they can do.

Every action is risk-classified. Every tool call is logged. Catastrophic commands are hard-blocked. High-risk operations require explicit consent. Agents can't `rm -rf /` — they can only invoke whitelisted verbs on known domains.

## The Primitives

| Primitive | Purpose |
|-----------|---------|
| **observe** | Structured state observation — point-in-time snapshot |
| **act** | Constrained remediation through whitelisted verbs |
| **sample** | Temporal observation — repeated observe + statistical reduction |
| **await** | Block until a condition becomes true (kqueue/polling) |

Plus escape hatches for agents: **bash** (with blocked/dangerous pattern detection) and **handoff** (structured escalation when the agent can't resolve).

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
| **disk** | space, mounts, temp_usage, large_paths | clear_temp, remove_caches, mount/unmount share | `world observe disk --scope space` |
| **printer** | status, queue, driver, port | clear_queue, restart_spooler, set_default | `world act printer queue clear` |
| **package** | installed, version, recent_updates | install, uninstall, repair, update | `world act package jq add` |
| **log** | recent_errors, warnings, matching, timeline | *(read-only)* | `world observe log --since 1h` |

Platform adapters currently implemented for **macOS**. Linux and Windows are stubbed and ready for contribution.

## Example: Network Troubleshooting

```bash
# What can we observe?
world observe network

# Get the full picture
world observe network --scope interfaces,internet_status

# Try flushing DNS (low risk — auto-approved)
world act network dns_cache reset

# Block until DNS is working again
world await network dns_resolves --target google.com

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

# Block until the process is confirmed dead
world await process stopped --target 5678
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
world act container my-nginx restart
world await container running --target my-nginx
world await container healthy --target my-nginx

# Cleanup (high risk — requires confirmation)
world act container images clear --yes
```

## Example: Temporal Sampling

Single `observe` calls are point-in-time snapshots. For ephemeral quantities like CPU and memory, use `sample` to get temporal context:

```bash
# Take 5 samples at 2s intervals — get mean, min, max, delta, rate
world sample process --scope top_cpu --limit 5 --count 5 --interval 2s
```

Numeric fields that vary become statistics (`cpu_percent: {mean: 42.3, delta: 2.1, rate: 0.5/sec}`). Constant fields stay as scalars (`pid: 415`, `ppid: 1`). Works with any domain.

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
│  Primitives                 │  ← observe, act, sample, await
├─────────────────────────────┤
│  Policy Layer               │  ← Risk classification, allowlists, consent
├─────────────────────────────┤
│  Domain Dispatch            │  ← Route to correct handler
├─────────────────────────────┤
│  Platform Adapters          │  ← macOS / Linux / Windows
├─────────────────────────────┤
│  Normalized Schemas         │  ← NetworkState, ServiceState, ProcessState, ...
└─────────────────────────────┘
    ↓
Telemetry Log (every call recorded)
```

**Plugin system** — domains are plugins. Native Rust for performance, external subprocesses (any language) for extensibility. Drop a plugin in `plugins/` and it's discovered automatically.

## CLI Reference

```
world observe DOMAIN [--target T] [--scope S1,S2] [--since 1h] [--limit N]
world act DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run] [--yes]
world await DOMAIN CONDITION [--target T] [--timeout N]
world sample DOMAIN [--scope S] [--count N] [--interval 2s] [--limit N]
world spec [DOMAIN]              # Show POMDP spec (observations + actions)
world tools                      # List all tools with schemas
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
