# world

**A POMDP interface for agents.**

`world` defines four primitives for how agents interact with the world: **observe** state, **act** to change it, **sample** over time, and **await** conditions. That's it.

Domains are pluggable. The built-in plugins cover processes, networks, containers, disks, packages, and services on macOS — but `world` is not an OS tool. Anything with observable state and a finite set of actions can be a domain: a robot, a phone, a cloud API, a database.

## The Four Primitives

### observe — read state

A point-in-time snapshot of a domain. Returns structured, normalized data — the agent never parses raw command output.

```bash
world observe process --scope top_cpu --limit 5
world observe network --scope interfaces,internet_status
world observe container --scope containers
```

Call with just a domain to discover what's available (scopes, actions, conditions):

```bash
world observe network
```

### act — change state

A finite verb on a known target. Only declared actions are accepted — there's no open-ended command execution in this interface.

```bash
world act network dns_cache reset
world act service nginx restart
world act process 5678 kill
world act container my-nginx enable
```

### sample — observe over time

A single `observe` is a snapshot. For quantities like CPU and memory, a snapshot is almost useless — you can't tell if a value is spiking, trending, or stable.

`sample` takes N observations at a fixed interval and reduces them into statistics: mean (integral), min/max (extremes), delta (differential), rate (derivative).

```bash
world sample process --scope top_cpu --limit 5 --count 5 --interval 2s
```

Fields that vary across samples become stats:
```json
"cpu_percent": {"mean": 42.3, "min": 38.1, "max": 47.2, "delta": 2.1, "rate_per_sec": 0.5}
```

Fields that don't vary stay as scalars: `"pid": 415`, `"ppid": 1`. The reducer discovers this from the data — no domain-specific configuration needed.

### await — block until a condition is true

The missing link in the act→verify loop. Instead of polling in a retry loop, agents declare what they're waiting for:

```bash
world act process 5678 kill
world await process stopped --target 5678

world act container my-nginx restart
world await container healthy --target my-nginx

world await network host_reachable --target google.com --timeout 30
```

Uses OS-native event mechanisms where available (kqueue `EVFILT_PROC` for process exit on macOS — microsecond notification). Falls back to polling with exponential backoff. Always has a timeout. Exit code 0 = condition met, 1 = timeout.

## Domains

Each domain is a plugin that declares what can be observed, what actions are available, and what conditions can be awaited.

| Domain | Observe | Act |
|--------|---------|-----|
| **process** | processes, tree, top_cpu, top_memory, open_files, listening_ports | kill_graceful, kill_force, set_priority |
| **network** | interfaces, routes, dns, gateway, proxy, internet_status | flush_dns, renew_dhcp, toggle_adapter, forget_wifi |
| **container** | containers, images, volumes, container_logs | start, stop, restart, remove, pull, prune |
| **service** | status, startup_mode, recent_errors, dependencies | start, stop, restart, set_startup_mode |
| **disk** | space, mounts, temp_usage, large_paths | clear_temp, remove_caches |
| **package** | installed, version, recent_updates | install, uninstall, repair, update |
| **printer** | status, queue, driver, port | clear_queue, restart_spooler, set_default |
| **log** | recent_errors, warnings, matching, timeline | *(read-only)* |

These are the built-in macOS adapters. Writing a new domain plugin — for any platform or any system — means implementing observe/act for your domain and dropping it into the plugin directory.

## How It Fits Together

```bash
# 1. What's going on?
world observe process --scope top_cpu --limit 5

# 2. Is it changing?
world sample process --scope top_cpu --limit 5 --count 3 --interval 2s

# 3. Do something about it
world act process 5678 kill

# 4. Wait for the result
world await process stopped --target 5678
```

Observe gives you the state. Sample gives you the trend. Act changes the state. Await confirms the change. The agent decides what to do — `world` just provides the interface.

## Structured Output

Every primitive returns the same envelope:

```json
{
  "output": "5 processes found.",
  "details": { "processes": [...], "total_count": 994 },
  "next_suggested_actions": ["observe(process, scope: [\"top_memory\"])"]
}
```

`--json` for agents, `--pretty` for humans (default in TTY), `-q` for exit code only.

## Plugin System

Domains are plugins. Native Rust plugins for performance, external subprocess plugins (any language) for extensibility.

```
plugins/
  pip/
    spec.json        # POMDP spec: observations + actions
    dispatch.json    # verb → handler mapping
    handler.py       # subprocess handler (any language)
```

## Architecture

```
Agent / CLI
    ↓
┌─────────────────────────────┐
│  Primitives                 │  ← observe, act, sample, await
├─────────────────────────────┤
│  Domain Dispatch            │  ← Route to correct domain + platform
├─────────────────────────────┤
│  Platform Adapters          │  ← macOS / Linux / Windows
├─────────────────────────────┤
│  Normalized Schemas         │  ← ProcessState, NetworkState, ...
└─────────────────────────────┘
```

## Add-ons

The core primitives are pure: observe reads, act writes, sample aggregates, await blocks. On top of this, `world` ships several add-ons that are useful but not fundamental:

- **Policy** — risk classification (Low/Medium/High), allowlists, consent gates for high-risk actions
- **Dry run** — `--dry-run` on any action to preview without executing
- **Bash** — escape hatch for commands that don't fit a domain, with blocked-pattern detection (`rm -rf /`, `mkfs`, fork bombs)
- **Handoff** — structured escalation when the agent is blocked (privilege, physical access, policy)
- **Telemetry** — every primitive call is logged with duration, success, risk, and linking (which await followed which act)

## Agent Integration

Tools expose Anthropic-compatible JSON schemas for direct use with tool-use LLMs:

```rust
use world::create_tools;

let tools = create_tools();
// Each tool: name, description, input_schema, execute()
```

## CLI Reference

```
world observe DOMAIN [--target T] [--scope S1,S2] [--since 1h] [--limit N]
world act     DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run] [--yes]
world await   DOMAIN CONDITION [--target T] [--timeout N]
world sample  DOMAIN [--scope S] [--count N] [--interval 2s] [--limit N]
world spec    [DOMAIN]
```

## Building

```bash
cargo build --release
cargo test
```

## License

MIT
