# world

`world` is a contrarian tool that gives AI agents a proper interface to interact with the world. The fundamental model is to treat the world as a partially observable environment — agents observe what they can, act on what they're allowed to, and maintain their own internal belief and world models from the observations.

## Key Concepts

### Domain

A domain is a slice of the world that can be observed and acted on. Each domain declares what state is observable, what actions are available, and what conditions can be awaited.

The built-in domains cover system state on macOS — processes, networks, containers, services, disks, packages, printers, logs — but a domain can be anything: a robot arm, a phone, a cloud API, a database. If it has state and actions, it's a domain.

### observe

Read state. Returns structured, normalized data — the agent never parses raw command output.

```bash
world observe process top_cpu --limit 5
world observe network interfaces
world observe container images
```

Call with just a domain to discover what's available:

```bash
world observe network
```

### act

Change state. A finite verb on a known target.

```bash
world act network dns_cache reset
world act service nginx restart
world act process 5678 kill
world act container my-nginx enable
```

### await

Block until a condition becomes true. This is the link between act and observe — instead of polling in a retry loop, the agent declares what it's waiting for.

```bash
world await process stopped 5678
world await container healthy my-nginx
world await network host_reachable google.com --timeout 30
```

Uses OS-native event mechanisms where available (kqueue `EVFILT_PROC` for process exit — microsecond notification). Falls back to polling with exponential backoff. Always has a timeout.

## Examples

### Diagnose and fix

```bash
# What's using CPU?
world observe process top_cpu --limit 5

# Kill the offender
world act process 5678 kill

# Confirm it's dead
world await process stopped 5678
```

### Observe over time

A single observe is a snapshot. For ephemeral quantities like CPU%, one snapshot is nearly useless. `sample` takes repeated observations and reduces them:

```bash
world sample process top_cpu --limit 5 --count 5 --interval 2s
```

Fields that vary become statistics (`cpu_percent: {mean: 42.3, delta: 2.1, rate_per_sec: 0.5}`). Constant fields stay as scalars (`pid: 415`).

### Containers

```bash
world observe container
world act container my-nginx restart
world await container healthy my-nginx
```

Auto-detects Docker or Podman. Degrades gracefully when neither is installed.

### Network

```bash
world observe network interfaces
world act network dns_cache reset
world await network dns_resolves google.com
```

### What's listening?

```bash
world observe process listening_ports
```

## Domains

| Domain | Targets | Act |
|--------|---------|-----|
| **process** | *(default: top by CPU)*, top_cpu, top_memory, processes, listening_ports, `<pid>`, `<name>`, `<pid>/tree`, `<pid>/open_files` | kill_graceful, kill_force, set_priority |
| **network** | *(default: all)*, interfaces, dns, gateway, internet_status | flush_dns, renew_dhcp, toggle_adapter, forget_wifi |
| **container** | *(default: containers)*, images, volumes, `<name>`, `<name>/logs` | start, stop, restart, remove, pull, prune |
| **service** | *(default: list)*, `<name>` | start, stop, restart, set_startup_mode |
| **disk** | *(default: mounts + space)*, temp_usage | clear_temp, remove_caches |
| **package** | *(default: list)*, `<name>` | install, uninstall, repair, update |
| **printer** | *(default: list)*, `<name>` | clear_queue, restart_spooler, set_default |
| **log** | *(default: recent errors)*, recent_warnings, `<subsystem>` | *(read-only)* |

## CLI

```
world observe DOMAIN [TARGET] [--limit N] [--since T]
world act     DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run] [--yes]
world await   DOMAIN CONDITION [TARGET] [--timeout N]
world sample  DOMAIN [TARGET] [--count N] [--interval T] [--limit N]
world spec    [DOMAIN]
```

`--json` for agents. `--pretty` for humans (default in TTY). `-q` for exit code only.

## Building

```bash
cargo build --release
cargo test
```

## License

MIT
