# world CLI Specification

## Grammar

```
world observe DOMAIN [TARGET] [--limit N] [--since T]
world act     DOMAIN TARGET VERB [KEY=VALUE ...] [--dry-run]
world await   DOMAIN [TARGET] CONDITION [--timeout N]
world sample  DOMAIN [TARGET] [--count N] [--interval T] [--limit N]
world spec    [DOMAIN]
```

All arguments after DOMAIN are positional unless prefixed with `--`.

## Output Modes

- **TTY** (default): pretty mode — human-readable, colored
- **Pipe/redirect**: JSON mode — structured, machine-readable
- `--json`: force JSON
- `--pretty`: force pretty
- `-q`: quiet — exit code only, no output

## observe

**Purpose**: Read state from a domain.

**Grammar**: `world observe DOMAIN [TARGET] [--limit N] [--since T]`

### Target interpretation

The domain interprets the target string. There are three cases:

1. **No target** → default view. Every domain defines a useful default that is not overwhelming.
2. **Known view name** → that view. Each domain has a fixed set of named views (e.g. `top_cpu`, `images`, `dns`).
3. **Anything else** → instance lookup. The domain searches its data by name/ID and returns the match. If nothing matches, return an error with guidance.

The domain does NOT need a hardcoded list to distinguish case 2 from case 3. It checks its known view names first, then falls through to search.

### Hierarchical targets

Some domains support `parent/child` navigation:

```
world observe process 1234/tree        # tree rooted at PID 1234
world observe process 1234/open_files  # open files for PID 1234
world observe container my-nginx/logs  # logs for container my-nginx
```

The `/` is a convention, not a framework rule. The domain splits on `/` if it makes sense. Domains where target is naturally a path (e.g. disk with `/System/Volumes/Data`) do NOT split.

### Default views

Every domain must define a sensible default when no target is given. The default should:
- Show the most useful information, not everything
- Be bounded in size (not dump 500+ items)
- Include a hint about how to see more

| Domain | Default (no target) | How to see more |
|---|---|---|
| process | Top 20 by CPU | `processes` for full list, `<name>` to filter |
| network | All interfaces + DNS + gateway + internet check | `<iface_name>` to drill in |
| container | List containers | `images`, `volumes` for other resources |
| service | Running non-Apple services | `all` for full list, `<name>` for details |
| disk | Mounts + space usage | `temp_usage` for temp dirs |
| brew | First 20 installed (alphabetical) | `all` for full list, `<name>` for details |
| pip | Installed packages with versions | `<name>` for details |
| npm | Packages in nearest project | `global` for global, `<name>` for details |
| printer | List printers | `<name>` for details |
| log | Recent errors | `recent_warnings`, `<subsystem>` |

### --limit

Caps the number of items returned. Applies to list/array results. Domain-specific behavior:
- Process: limits number of processes in the list
- Log: limits number of log entries

### --since

Time filter. Only meaningful for log domain. Format: `1h`, `30m`, `2d`.

### Output format

Every observe returns a `UnifiedResult`:

```json
{
  "output": "Human-readable summary line",
  "details": { /* structured data — the actual observation */ }
}
```

JSON mode shows only `details` (and `error` if present).
Pretty mode shows `output` as the header, then renders `details`.

#### Pretty rendering rules

- **Object**: render each field with `key value` formatting
- **Array of objects**: render as a list. Each item leads with the **identity field** (name, pid, id, path — first match), bolded. Skip noise fields (installed=true, exists=true).
- **Array of strings**: render as a bullet list
- **Scalars**: render inline
- Arrays longer than 10 items: show first 10, then `...N more...`

## act

**Purpose**: Change state in a domain.

**Grammar**: `world act DOMAIN [TARGET] VERB [KEY=VALUE ...] [--dry-run]`

Two forms:
- **Targeted**: `world act process 1234 kill` — act on a specific target
- **Targetless**: `world act browser open https://...` — session actions without a target

### Verb resolution

The dispatch table maps `(target_pattern, verb)` → handler. Target patterns can be:
- Exact: `dns_cache` matches only `dns_cache`
- Wildcard: `<name>` matches any string
- Prefix: `interfaces.<name>` matches `interfaces.en0`
- Suffix: `<name>.queue` matches `hp_printer.queue`

### mutates metadata

Every action declares which observation schema paths it modifies via `mutates` tags. These are self-referential — they reference the same schema that `observe` returns. Examples:

| Action | mutates |
|---|---|
| `act network dns_cache reset` | `network.interfaces` |
| `act service <name> restart` | `service.status` |
| `act brew <name> add` | `brew.installed`, `brew.version` |
| `act pip <name> add` | `pip.installed`, `pip.version` |
| `act npm <name> add` | `npm.installed`, `npm.version` |
| `act process <pid> kill` | `process.processes` |

An action with `mutates: []` is read-only and always allowed. The `mutates` metadata is a world-property (a fact about what the action does), not an actor-property (not about who is allowed to do it).

### Capability ceiling

The binary has a compiled-in capability ceiling — a set of observation schema paths it is allowed to mutate. The ceiling is structural: no CLI flag, environment variable, or runtime argument can override it. To change the ceiling, recompile the binary.

Before executing any action, the dispatch pipeline checks that the action's `mutates` tags are a subset of the ceiling. If any tag exceeds the ceiling, the action is refused with error code `exceeds_capability`.

Ceiling examples:
- `["*"]` — unrestricted (default for development builds)
- `["network.*", "service.*"]` — can only mutate network and service domains
- `["container.*"]` — container-only binary
- `[]` — read-only binary, observe only, no act can execute

### --dry-run

Describe what would happen without doing it. Output must include `"dry_run": true` in details. Dry-run does NOT bypass the capability ceiling — if the action exceeds the ceiling, even `--dry-run` is refused.

## await

**Purpose**: Block until a condition becomes true.

**Grammar**: `world await DOMAIN [TARGET] CONDITION [--timeout N]`

Two forms:
- **Targeted**: `world await process 1234 stopped` — wait for PID 1234 to stop
- **Targetless**: `world await network internet_reachable` — wait for internet

### resolves

Actions that produce async effects declare what condition confirms them:

```json
{ "target": "<pid>", "verbs": ["kill"], "mutates": ["process.processes"], "resolves": "stopped" }
```

The agent reads `resolves` and knows to `await stopped` after killing. No `resolves` = synchronous — the exit code is sufficient.

### Conditions

Each domain declares its valid conditions:

| Domain | Conditions |
|---|---|
| process | running, stopped, port_free |
| network | host_reachable, dns_resolves, internet_reachable, port_open |
| container | running, stopped, healthy, image_exists, volume_exists |
| service | healthy, stopped |
| disk | writable, mounted, unmounted |
| brew | installed, uninstalled |
| npm | installed |
| pip | installed |
| printer | prints |
| browser | loaded, title_contains |
| ssh | connected |
| home | connected |

Invalid conditions return an error listing available ones.

### Mechanism

1. Try OS-native event mechanism (e.g. kqueue EVFILT_PROC for process exit)
2. Fall back to polling with exponential backoff (250ms → 500ms → 1s → 2s → 5s max)

### Timeout

Default: 60 seconds. `--timeout N` overrides. On timeout: exit code 1, `"passed": false, "timeout": true` in details.

### Exit code

- 0: condition met
- 1: timeout or error

### Output

```json
{
  "check": "process_stopped",
  "target": "1234",
  "passed": true,
  "mechanism": "kqueue",
  "elapsed_ms": 1693
}
```

## sample

**Purpose**: Observe over time. Repeated observe + statistical reduction.

**Grammar**: `world sample DOMAIN [TARGET] [--count N] [--interval T] [--limit N]`

TARGET is interpreted exactly as in observe — same domain, same rules.

### Parameters

- `--count N`: number of samples (default: 5, minimum: 2)
- `--interval T`: time between samples (e.g. `2s`, `500ms`, `1m`, default: `2s`)
- `--limit N`: passed through to each observe call

### Reduction rules

The reducer walks the observation JSON tree:

- **Numeric field that varies across samples** → `{mean, min, max, first, last, delta, rate_per_sec, samples}`
- **Numeric field that is constant** → kept as scalar (e.g. pid, ppid)
- **Identity fields** (pid, id, name, subject, path, port) → kept as scalar, never reduced
- **Arrays of objects** → grouped by identity field, then reduced per-group
- **Strings, bools, non-reducible** → last value

### Output

```json
{
  "sampling": { "count": 5, "interval_ms": 2000, "duration_ms": 10234 },
  "result": { /* reduced observation */ }
}
```

## spec

**Purpose**: Show the domain specification — what can be observed, what actions exist.

**Grammar**: `world spec [DOMAIN]`

With no argument: show all domains. With a domain name: show that domain only.

## Error handling

All errors use `UnifiedResult::err`:

```json
{
  "error": {
    "code": "not_found",
    "message": "Interface 'bogus' not found. Use 'world observe network interfaces' to list all."
  }
}
```

Error codes:
- `not_found` — target doesn't exist in the domain
- `missing_target` — required target not provided
- `unknown_action` — action not recognized
- `action_not_allowed` — action not in allowlist
- `exceeds_capability` — action's `mutates` tags exceed this binary's capability ceiling
- `invalid_domain` — domain doesn't exist
- `invalid_condition` — await condition not recognized
- `execution_error` — underlying command failed

Exit codes:
- 0: success
- 1: error (all error types, including capability ceiling violations)

Errors should include guidance: what to do instead, what's available.
