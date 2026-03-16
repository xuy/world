# world

`world` gives AI agents a structured interface to observe and act on system state.

The motivation is simple: agents that manage real systems — diagnosing why a service is down, checking what's using disk, restarting a container — need to interact with the OS. Today they do this by generating shell commands and parsing terminal output. This is fragile, unscoped, and impossible to constrain safely.

`world` treats the system as a partially observable environment. Agents observe structured state, act through a finite set of declared verbs, and await conditions instead of polling. Every action declares what it mutates. A compiled-in capability ceiling limits what any given binary can do, regardless of what the agent asks for.

This project grew out of [Noah](https://github.com/xuy/noah), an AI IT department for small businesses, where the agent needs to observe and manage machines on behalf of non-technical users.

```
                          ┌─────────────────────────────────┐
                          │           AI Agent               │
                          │   (Noah, or any LLM agent)       │
                          │                                  │
                          │   Holds a world model built      │
                          │   from structured observations   │
                          └──────────┬──────────────────┬────┘
                                     │                  │
                            observe / await          act (verb)
                           ← structured JSON    → declared mutates
                                     │                  │
                    ┌────────────────┴──────────────────┴────────────────┐
                    │                    world CLI                        │
                    │                                                     │
                    │   ┌───────────────────────────────────────────┐     │
                    │   │          capability ceiling                │     │
                    │   │   compiled-in, no runtime override         │     │
                    │   │   e.g. ["network.*", "service.*"]         │     │
                    │   └───────────────────────────────────────────┘     │
                    │                                                     │
                    │   spec ──→ schema for each domain:                  │
                    │            observations, actions, mutates           │
                    ├─────────────────────────────────────────────────────┤
                    │                     domains                         │
                    │                                                     │
                    │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐  │
                    │  │ network │ │ process │ │ service │ │  disk   │  │
                    │  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘  │
                    │  ┌────┴────┐ ┌────┴────┐ ┌────┴────┐ ┌────┴────┐  │
                    │  │  brew  │ │   pip   │ │   npm   │ │   ...   │  │
                    │  └────┬────┘ └────┬────┘ └────┬────┘ └─────────┘  │
                    │       │          │          │        plugins/      │
                    │    native      python3      node    (any language) │
                    └───────┴──────────┴──────────┴──────────────────────┘
                            │          │          │
                    ┌───────┴──────────┴──────────┴──────────────────────┐
                    │                 actual system                       │
                    │   processes, interfaces, containers, packages, ...  │
                    └────────────────────────────────────────────────────┘
```

The agent never touches the system directly. It reads structured state through `observe`, changes state through `act` with declared verbs, and waits for conditions through `await`. The capability ceiling ensures the binary itself is structurally limited — an agent cannot exceed what the binary was compiled to allow.

## Why not just shell commands?

An agent with shell access can do anything — that's the problem. `world` is designed around three constraints:

1. **Structured observations.** `world observe network --json` returns a schema, not terminal output. The agent never has to parse `ifconfig` or `netstat`. Every domain returns the same shape: `{details: {...}}`.

2. **Declared mutations.** Every action says what observation paths it modifies (`mutates: ["network.interfaces"]`). This is a fact about the action, not a policy judgment.

3. **Structural safety.** The binary has a compiled-in capability ceiling — a set of observation schema paths it is allowed to mutate. An agent given a binary compiled with `CEILING: &["network.*"]` literally cannot kill processes or uninstall packages. No flag overrides this. To change it, recompile.

The combination means you can hand an agent a world binary and reason about what it can and cannot do, which is not possible with `bash`.

## Concepts

### Domains

A domain is a slice of the world that can be observed and acted on. Built-in domains cover macOS system state — processes, networks, containers, services, disks, printers, logs. External plugins extend this to package managers (brew, pip, npm) and anything else with state and actions.

### observe → act → await

```bash
# What's using CPU?
world observe process top_cpu --limit 5

# Kill the offender
world act process 5678 kill

# Confirm it's gone
world await process stopped 5678
```

`observe` reads structured state. `act` changes it through a declared verb. `await` blocks until a condition holds, using OS-native events where available (kqueue for process exit) and falling back to exponential backoff polling.

### sample

A single observation is a snapshot. For quantities like CPU%, one snapshot is nearly useless. `sample` takes repeated observations and reduces them statistically:

```bash
world sample process top_cpu --limit 5 --count 5 --interval 2s
```

Fields that vary become `{mean, min, max, delta, rate_per_sec}`. Constant fields stay as scalars.

### spec

Every domain declares its schema — what can be observed, what actions exist, what each action mutates. Agents use this for discovery instead of guessing.

```bash
world spec              # all domains
world spec network      # one domain
```

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

Package managers are separate domains (brew, pip, npm) rather than a single "package" abstraction, because they have different scopes (system, virtualenv, node_modules) and the handler should use the runtime it observes.

## CLI

```
world observe DOMAIN [TARGET] [--limit N] [--since T]
world act     DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run]
world await   DOMAIN CONDITION [TARGET] [--timeout N]
world sample  DOMAIN [TARGET] [--count N] [--interval T] [--limit N]
world spec    [DOMAIN]
```

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

## Building

```bash
cargo build --release
cargo test
```

## License

MIT
