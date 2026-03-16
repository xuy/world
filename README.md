# world

`world` gives AI agents a structured interface to observe and act on system state.

The motivation is simple: agents that manage real systems — diagnosing why a service is down, checking what's using disk, restarting a container — need to interact with the OS. Today they do this by generating shell commands and parsing terminal output. This is fragile, unscoped, and impossible to constrain safely.

`world` treats the system as a partially observable environment. Agents observe structured state, act through a finite set of declared verbs, and await conditions instead of polling. Every action declares what it mutates. A compiled-in capability ceiling limits what any given binary can do, regardless of what the agent asks for.

This project grew out of [Noah](https://github.com/xuy/noah), an AI IT department for small businesses, where the agent needs to observe and manage machines on behalf of non-technical users.

![Architecture](docs/architecture.svg)

The system is partially observable — agents cannot see everything, only what domains expose. Each domain declares a schema (`spec`) describing its observations, actions, and what each action mutates. The agent builds a world model from structured observations, changes state through declared verbs (`act`), and waits for conditions (`await`) instead of polling.

## Why not just shell commands?

An agent with shell access can do anything — that's the problem. `world` is designed around three constraints:

1. **Structured observations.** `world observe network --json` returns a schema, not terminal output. The agent never has to parse `ifconfig` or `netstat`. Every domain returns the same shape: `{details: {...}}`.

2. **Declared mutations.** Every action says what observation paths it modifies (`mutates: ["network.interfaces"]`). This is a fact about the action, not a policy judgment.

3. **Structural safety.** The binary has a compiled-in capability ceiling — a set of observation schema paths it is allowed to mutate. An agent given a binary compiled with `CEILING: &["network.*"]` literally cannot kill processes or uninstall packages. No flag overrides this. To change it, recompile.

The combination means you can hand an agent a world binary and reason about what it can and cannot do, which is not possible with `bash`.

## Concepts

### Domains

A domain is a slice of the world that can be observed and acted on. Built-in domains cover macOS system state — processes, networks, containers, services, disks, printers, logs. External plugins extend this to package managers (brew, pip, npm) and anything else with state and actions.

### spec

Every domain declares its schema — what can be observed, what actions exist, what each action mutates. Agents use this for discovery instead of guessing.

```bash
world spec              # all domains
world spec network      # one domain
```

### observe → act → await

```bash
# What's using CPU?
world observe process top_cpu --limit 5

# Kill the offender
world act process 5678 kill

# Confirm it's gone
world await process 5678 stopped
```

`observe` reads structured state. `act` changes it through a declared verb. `await` blocks until a condition holds, using OS-native events where available (kqueue for process exit) and falling back to exponential backoff polling.

### Session domains

Most domains are ambient — processes, disks, and networks always have state to observe. Some domains are different: they start empty and must be populated by an action before observation is meaningful. A browser has no page until you open one. An SSH connection has no host until you connect.

A domain declares this with `"session": true` in its spec. The agent sees schema-conforming null observations (all fields null, arrays empty) and knows from the spec that an action like `open` will populate them. No special state values, no separate lifecycle protocol — just the same observe/act/await loop, where the initial observation happens to be empty.

```bash
world observe browser
# → { "url": null, "title": null, "elements": [], "snapshot": null }

world act browser open url=https://example.com
# → { "url": "https://example.com", "title": "Example", "elements": [...], ... }

world act browser close
# → { "url": null, "title": null, "elements": [], "snapshot": null }
```

### sample

A single observation is a snapshot. For quantities like CPU%, one snapshot is nearly useless. `sample` takes repeated observations and reduces them statistically:

```bash
world sample process top_cpu --limit 5 --count 5 --interval 2s
```

Fields that vary become `{mean, min, max, delta, rate_per_sec}`. Constant fields stay as scalars.

## Domains

| Domain | Default observation | Actions |
|--------|-------------------|---------|
| **process** | Top 20 by CPU | kill, set_priority |
| **network** | Interfaces + DNS + gateway + connectivity | flush_dns, renew_dhcp, toggle_adapter, forget_wifi |
| **container** | Running containers | start, stop, restart, remove, pull, prune |
| **service** | Running non-Apple services | start, stop, restart, set_startup_mode |
| **disk** | Mounts + space usage | clear_temp, remove_caches, mount/unmount |
| **brew** | Installed packages | install, uninstall, repair, update |
| **pip** | Installed packages + virtualenv | install, uninstall, pin, upgrade |
| **npm** | Project packages (or global) | install, uninstall, pin, update |
| **printer** | Printers + status | clear_queue, restart_spooler, set_default |
| **log** | Recent errors | *(read-only)* |
| **browser** *(session)* | Page URL + accessibility tree | open, close, click, fill, select, hover, scroll, press, eval |

Package managers are separate domains (brew, pip, npm) rather than a single "package" abstraction, because they have different scopes (system, virtualenv, node_modules) and the handler should use the runtime it observes.

## CLI

```
world COMMAND DOMAIN [TARGET] [PREDICATE] [OPTIONS]
```

```
world observe DOMAIN [TARGET] [--limit N] [--since T]
world act     DOMAIN [TARGET] VERB [ARGS...] [--dry-run]
world await   DOMAIN [TARGET] CONDITION [--timeout N]
world sample  DOMAIN [TARGET] [--count N] [--interval T] [--limit N]
world spec    [DOMAIN]
```

Every command follows the same shape: domain, then target, then what to do. TARGET is optional for targetless actions (e.g., `world act browser open https://example.com`, `world await network internet_reachable`).

Output is JSON when piped and human-readable in TTY. `--json` / `--pretty` to force. `-q` for exit code only.

## Extending

A plugin is a directory with three files:

```
plugins/npm/
  spec.json      # observations + actions + mutates
  dispatch.json  # (target, verb) → handler mapping
  handler.js     # reads JSON from stdin, writes JSON to stdout
```

The handler can be in any language (`.py` → python3, `.js` → node, `.sh` → sh, or a bare executable). The protocol is one JSON object in, one JSON object out.

Session domains (like `browser`) follow the same plugin structure. Add `"session": true` to spec.json. The handler returns null/empty observations when the session is inactive, and actions like `open`/`close` manage the lifecycle. The browser plugin delegates to [agent-browser](https://github.com/vercel-labs/agent-browser), which manages browser state via a background daemon.

## Building

```bash
cargo build --release
cargo test
```

## License

MIT
