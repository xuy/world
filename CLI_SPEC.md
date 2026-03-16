# world CLI — Strict Behavioral Specification

**Version:** 0.1.0
**Status:** Normative
**Governing principle:** This document is the single source of truth for CLI behavior. Any behavior not specified here is undefined and MUST NOT be relied upon. Any behavior that contradicts this document is a bug.

---

## 1. First Principles

These are axiomatic. Every rule in this spec derives from one or more of them.

### 1.1 The world is partially observable

The CLI exposes structured snapshots of system state. A snapshot is a point-in-time reading — not the truth, but the best available evidence. The CLI never claims completeness. Fields may be `null`. Arrays may be truncated. Observations may fail. The consumer (human or agent) owns belief formation; the CLI owns measurement.

### 1.2 Observe and act are the only primitives

There are exactly two interactions with the world: reading state (`observe`) and changing state (`act`). Every other command — `await`, `sample`, `spec`, `tools`, `addons`, `completions` — is a *derived operation* or *meta-query*. Derived operations compose the primitives; they do not introduce new ways to touch the world.

### 1.3 Domains are the unit of modularity

The world is partitioned into non-overlapping domains (network, service, disk, process, package, container, printer, log). Each domain owns a schema (what can be observed) and an action space (what verbs exist on what targets). Domains are plugins. The CLI is a neutral dispatch framework — it has no domain-specific knowledge in its core loop.

### 1.4 The CLI has two consumers: humans and machines

Output format is chosen by consumer type, never by command semantics. A human gets colored text. A machine gets structured JSON. The information content is identical; only the representation differs. The CLI MUST never produce output that is useful to one consumer but not the other.

### 1.5 Actions are classified by risk; risk gates consent

Every action has a risk classification (low, medium, high). High-risk actions require explicit consent before execution. This is a policy concern, not a UX preference — it exists to prevent irreversible damage when the CLI is operated by an autonomous agent.

### 1.6 The CLI is silent by default about its own internals

The CLI reports world state, not its own state. Telemetry, dispatch tables, plugin loading errors, and internal timing are never part of the primary output stream. Errors about the world (domain not found, action failed) go to the appropriate output channel. Errors about the CLI itself (plugin load failure) go to stderr and do not affect exit codes for the requested operation.

---

## 2. Command Grammar

### 2.1 Invocation syntax

```
world [GLOBAL_OPTIONS] <COMMAND> [COMMAND_ARGS] [COMMAND_OPTIONS]
```

Global options MUST appear before the command. Command-specific options MUST appear after command arguments. This is enforced by the parser; violations are parse errors.

### 2.2 Commands

The CLI defines exactly 8 commands. No more. Every command falls into one of three categories:

| Category | Commands | Property |
|---|---|---|
| **Primitives** | `observe`, `act` | Touch the world |
| **Derived** | `await`, `sample` | Compose primitives with time |
| **Meta** | `spec`, `tools`, `addons`, `completions` | Describe the CLI itself |

#### 2.2.1 `observe` — Read state

```
world observe <domain> [target] [--since <duration>] [--limit <n>]
```

- `domain` (required): The domain to observe. MUST match a registered plugin name exactly.
- `target` (optional): Domain-specific selector. The domain interprets this opaquely.
- `--since <duration>`: Time filter. Only meaningful for domains that support temporal queries (e.g., `log`). Domains that do not support it MUST ignore it silently.
- `--limit <n>`: Maximum number of results. Applied by the domain, not the CLI.

**Semantics:** Reads structured state from the domain. MUST be side-effect-free. MUST NOT modify any system state. The result is a `UnifiedResult` with `details` containing domain-specific structured data.

**Risk:** Always `Low`. Never requires consent.

#### 2.2.2 `act` — Change state

```
world act <domain> <target> <verb> [key=value ...] [--dry-run] [--yes]
```

- `domain` (required): The domain to act on.
- `target` (required): The target path (e.g., `dns_cache`, `interfaces.en0`, `nginx.startup_mode`).
- `verb` (required): The action verb (e.g., `reset`, `set`, `enable`, `disable`, `add`, `remove`, `restart`, `kill`, `clear`).
- `key=value` (optional, variadic): Verb arguments. NOT flags — they are positional key=value pairs.
- `--dry-run`: Preview the action without executing it. MUST NOT modify any system state.
- `--yes`: Skip the interactive consent gate for high-risk actions.

**Verb argument rules:**
- Every verb argument MUST be in `key=value` format. Bare values are rejected.
- Arguments starting with `--` are silently ignored (to tolerate accidental flag-style input).
- The `set` verb MUST have at least one key=value argument. Invoking `set` with zero arguments is an error.
- Key=value pairs are converted to a JSON object and passed to the handler as `params`.

**Verb resolution:**
1. Parse verb arguments into a key=value map.
2. Look up `(target, verb)` in the domain's dispatch table using pattern matching.
3. If no match: error with code `invalid_action` listing available `(target, verb)` pairs.
4. If the resolved handler is not in the domain's allowlist: error with code `action_not_allowed`.
5. If the handler's risk classification is `High`, `--dry-run` is false, and `--yes` is not set: trigger the consent gate (see section 5).
6. Execute the handler.

**Target pattern matching** (dispatch table resolution):

| Pattern | Example | Matches | Extracted target |
|---|---|---|---|
| Exact literal | `dns_cache` | `dns_cache` only | `None` |
| Full wildcard | `<name>`, `<pid>` | Any string | `Some(input)` |
| Prefix wildcard | `interfaces.<name>` | `interfaces.en0` | `Some("en0")` |
| Suffix wildcard | `<name>.queue` | `hp_printer.queue` | `Some("hp_printer")` |

Patterns are matched in dispatch table order. First match wins.

#### 2.2.3 `await` — Block until condition

```
world await <domain> <condition> [target] [--timeout <seconds>]
```

- `domain` (required): The domain context for the condition.
- `condition` (required): The condition name (e.g., `host_reachable`, `stopped`, `healthy`).
- `target` (optional): The target to check (e.g., a PID, hostname, container ID).
- `--timeout <seconds>`: Maximum wait time. Default: 60. MUST be positive.

**Semantics:** Blocks until the named condition evaluates to true, or timeout is reached.

**Execution strategy:**
1. Attempt OS-native event mechanism (e.g., kqueue `EVFILT_PROC` for `process.stopped` on macOS).
2. If no native mechanism: poll with exponential backoff (initial: 250ms, max: 5000ms, doubling).
3. On each poll: execute the domain's verify check and inspect `details.passed`.

**Result:**
- If condition met: exit 0. `details.passed = true`.
- If timeout: exit 1. `details.passed = false`, `details.timeout = true`.
- Result always includes `mechanism` (`"kqueue"` | `"polling"`), `elapsed_ms`, and `polls` (if polling).

**Error on invalid condition:** Error code `invalid_condition` with the list of available conditions for that domain.

#### 2.2.4 `sample` — Observe over time

```
world sample <domain> [target] [--count <n>] [--interval <duration>] [--limit <n>]
```

- `domain` (required): The domain to observe.
- `target` (optional): Same as `observe` target.
- `--count <n>`: Number of samples. Default: 5. MUST be >= 2.
- `--interval <duration>`: Time between samples. Default: `2s`. Supports: `Ns`, `Nms`, `Nm`, bare `N` (treated as seconds).
- `--limit <n>`: Passed through to each `observe` call.

**Semantics:** Calls `observe` N times at fixed intervals, then reduces numeric fields into statistics.

**Reduction rules:**
- Numeric fields that vary across samples: replaced with `Stats { mean, min, max, first, last, delta, rate_per_sec, samples }`.
- Numeric fields that are constant across all samples: kept as scalar (no stats noise).
- Identity fields (`pid`, `id`, `name`, `subject`, `path`, `port`): kept as-is, used as grouping keys for arrays.
- String/boolean fields: last value wins.
- Arrays of objects: grouped by identity key, each group reduced independently.
- Arrays of primitives: last value wins.

**If any sample fails:** Abort immediately with error code `sample_error`. Partial results are not returned.

#### 2.2.5 `spec` — Show domain schema

```
world spec [domain] [--core]
```

- `domain` (optional): If provided, show spec for that domain only. If omitted, show all domains.
- `--core`: Exclude add-on contributions from the spec.

**Semantics:** Pure meta-query. Returns the declared observation schema and action space for one or all domains.

#### 2.2.6 `tools` — List CLI commands

```
world tools
```

**Semantics:** Lists the 5 primary tools (observe, act, await, sample, spec) with their descriptions and usage syntax.

#### 2.2.7 `addons` — List add-ons

```
world addons [name] [--domain <domain>]
```

- `name` (optional): Show details for a specific add-on.
- `--domain <domain>`: Filter to a specific domain's contributions.

**Semantics:** Lists registered add-ons (e.g., `verify`, `policy`) and their per-domain specs.

#### 2.2.8 `completions` — Shell completions

```
world completions <shell>
```

- `shell` (required): One of `bash`, `zsh`, `fish`.

**Semantics:** Writes shell completion script to stdout. Exit 0.

---

## 3. Global Options

Exactly three global options exist. They control representation, not semantics.

| Option | Short | Type | Default | Description |
|---|---|---|---|---|
| `--json` | — | flag | false | Force JSON output mode |
| `--pretty` | — | flag | false | Force Pretty output mode |
| `--quiet` | `-q` | flag | false | Suppress all output; exit code only |

### 3.1 Output mode resolution

Output mode is resolved exactly once per invocation, in this order:

1. If `--quiet`: `Quiet`
2. Else if `--json`: `Json`
3. Else if `--pretty`: `Pretty`
4. Else if stdout is a TTY: `Pretty`
5. Else: `Json`

This means: pipes and redirects default to JSON. Terminals default to Pretty. Explicit flags override both. `--quiet` overrides everything.

`--json` and `--pretty` are mutually exclusive by intent. If both are provided, `--json` wins (per the precedence above, `--json` is checked first after `--quiet`).

---

## 4. Output Contracts

### 4.1 The `UnifiedResult` envelope

Every primitive and derived command produces a `UnifiedResult`:

```
{
  "output":                 string,          // Human-readable summary (Pretty mode only)
  "details":                object | null,   // Structured domain state
  "artifacts":              [Artifact] | null,
  "risk":                   "low" | "medium" | "high" | null,
  "next_suggested_actions": [string] | null,
  "error":                  ToolError | null
}
```

### 4.2 JSON mode output

JSON mode writes to stdout a JSON object containing ONLY:
- `"error"`: present if and only if the operation failed
- `"details"`: present if and only if the operation produced structured state

No other fields from `UnifiedResult` are emitted. The `output`, `artifacts`, `risk`, and `next_suggested_actions` fields are representation concerns and MUST NOT appear in JSON mode.

JSON is always pretty-printed (indented). Output is a single JSON value — no NDJSON, no streaming.

### 4.3 Pretty mode output

Pretty mode writes to stdout:
1. The `output` summary line (if non-empty).
2. Structured `details` formatted as indented key-value pairs with color:
   - Keys: cyan
   - Boolean true: green
   - Boolean false: red
   - Arrays: show count, then up to 10 items; remaining as `...N more...`
   - Objects in arrays: lead with identity field (`name`, `pid`, `id`, `path`, `subject`) bolded, then remaining fields as `key=value`
   - Strings longer than 60 characters: truncated with `...`

Errors in Pretty mode are written to stderr: `ERROR [code] message` in red+bold, with optional dimmed `Blocked by:` line.

### 4.4 Quiet mode output

Quiet mode writes nothing to stdout or stderr. The exit code is the only signal.

### 4.5 Error output

Errors follow one of two paths:

**Domain/operation errors** (invalid domain, invalid action, execution failure, timeout):
- JSON mode: `{ "error": { "code": "...", "message": "...", ... } }` on stdout
- Pretty mode: `ERROR [code] message` on stderr
- Quiet mode: nothing

**CLI infrastructure errors** (plugin load failure):
- Always stderr via `eprintln!`
- Prefixed with `Warning:` for non-fatal errors
- Do not affect the exit code of the requested command

### 4.6 ToolError structure

```
{
  "code":       string,                    // Machine-readable (e.g., "invalid_domain")
  "message":    string,                    // Human-readable
  "retryable":  boolean | null,            // Hint: can this be retried?
  "blocked_by": "privilege" | "policy" | "physical" | "unsupported" | "unknown" | null
}
```

**Defined error codes:**

| Code | When | Exit code |
|---|---|---|
| `invalid_domain` | Domain name not found in registry | 1 |
| `invalid_action` | Target+verb not in dispatch table | 1 |
| `action_not_allowed` | Handler not in domain allowlist | 1 |
| `invalid_condition` | Condition not recognized for domain | 1 |
| `invalid_interval` | Unparseable duration string | 1 |
| `invalid_count` | Sample count < 2 | 1 |
| `execution_error` | Handler raised an unhandled error | 1 |
| `plugin_error` | External plugin process failed | 1 |
| `sample_error` | One of N observe calls failed during sample | 1 |
| `await_error` | Await mechanism failed (not timeout — timeout is exit 1 with `passed=false`) | 1 |
| `unsupported` | Domain not supported on this platform | 1 |

---

## 5. Consent Gate

The consent gate enforces policy for high-risk actions. It is NOT a UX feature — it is a safety mechanism.

### 5.1 When consent is required

Consent is required when ALL of the following are true:
1. The command is `act`
2. The resolved handler's risk classification is `High`
3. `--dry-run` is NOT set
4. `--yes` is NOT set

### 5.2 Consent behavior by output mode

**Pretty mode (interactive):**
1. Write to stderr: `Policy: {domain} {target} {verb} is classified HIGH risk. Proceed? [y/N]`
2. Read one line from stdin.
3. If input is `y` or `yes` (case-insensitive): proceed.
4. Otherwise: exit 4.

**JSON mode (non-interactive):**
1. Write to stdout:
```json
{
  "policy": "confirmation_required",
  "classification": "high",
  "domain": "...",
  "target": "...",
  "verb": "...",
  "message": "Policy: this action is classified HIGH risk. Re-run with --yes to confirm."
}
```
2. Exit 4.

**Quiet mode:**
1. Write nothing.
2. Exit 4.

### 5.3 Exit code 4

Exit code 4 means exactly one thing: **consent required but not given**. It is the machine-readable signal for "re-run with `--yes`". It is never used for any other purpose.

---

## 6. Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success. Operation completed and produced a valid result. |
| 1 | Failure. Operation failed, condition not met, timeout reached, or invalid input. |
| 4 | Consent required. High-risk action blocked pending `--yes` flag. |

No other exit codes are defined. The CLI MUST NOT exit with any code other than 0, 1, or 4.

`await` with timeout exits 1 (condition not met), NOT a separate timeout code. The timeout is reported in `details.timeout = true`.

---

## 7. Plugin System

### 7.1 Plugin types

Two types exist. Both implement the same `DomainPlugin` trait. The CLI dispatches through the trait — there is no separate code path.

| Type | Location | Performance | Language |
|---|---|---|---|
| **Native** | Compiled into binary | In-process | Rust |
| **External** | `plugins/<name>/` directory | Subprocess | Any (Python, shell, binary) |

### 7.2 External plugin structure

A plugin is a directory containing:

```
plugins/<domain>/
  spec.json       # Domain spec (observations + actions)
  dispatch.json   # Dispatch table + risk policy
  handler.py      # Handler executable (or handler.sh, or handler)
```

**spec.json:** Must contain a `"domain"` field (string). Otherwise, same schema as native specs.

**dispatch.json:**
```json
{
  "entries": [
    { "target": "<pattern>", "verb": "<verb>", "handler": "<handler_name>" }
  ],
  "policy": {
    "<handler_name>": "low" | "medium" | "high"
  }
}
```

**Handler invocation:**
- `.py` extension: invoked as `python3 handler.py`
- `.sh` extension: invoked as `sh handler.sh`
- No extension or other: invoked directly (must be executable)

### 7.3 External plugin protocol

**Request** (JSON on stdin):
```json
{
  "command": "observe" | "act",
  "target": "string | null",
  "handler": "string",         // only for "act"
  "params": "object | null",   // only for "act"
  "dry_run": "boolean"         // only for "act"
}
```

**Response** (JSON on stdout):
```json
{ "details": { ... } }
```
or
```json
{ "error": { "code": "string", "message": "string" } }
```

If the handler process exits with non-zero status, the CLI wraps stderr into an error with code `plugin_error`.

### 7.4 Plugin discovery

Plugin directories are searched in this order:
1. `plugins/` adjacent to the binary
2. Two levels up from the binary (supports `cargo run` from `target/debug/`)
3. `plugins/` in the current working directory (fallback)

First match wins. Only directories containing `spec.json` are considered.

### 7.5 Plugin loading failures

If a plugin fails to load (missing files, invalid JSON, no handler), a warning is printed to stderr. The remaining plugins continue loading. The failed plugin is simply not registered.

---

## 8. Risk Classification

### 8.1 Risk levels

| Level | Consent required? | Semantics |
|---|---|---|
| `Low` | No | Read-only-like or trivially reversible |
| `Medium` | No | Service disruption possible but generally safe |
| `High` | Yes | Data loss, security implications, or hard to reverse |

### 8.2 Classification source

- **Native plugins:** Hardcoded in `policy::classify_risk()` per (domain, handler) pair.
- **External plugins:** Declared in `dispatch.json` `"policy"` map. Missing entries default to `Medium`.

### 8.3 Allowlist

Every native handler MUST appear in the domain's allowlist (`contracts::act::allowed_actions`). A handler that exists in the dispatch table but not in the allowlist is rejected with `action_not_allowed`. This is a defense-in-depth mechanism — it prevents dispatch table entries from accidentally enabling unimplemented or dangerous handlers.

External plugins bypass the allowlist check — they manage their own authorization.

---

## 9. Domains

### 9.1 Native domains

The following 8 domains are compiled into the binary:

| Domain | Observe | Act | Notes |
|---|---|---|---|
| `network` | interfaces, internet reachability, proxy, VPN | DNS flush, DHCP renew, adapter toggle, WiFi forget/reconnect, proxy reset | |
| `service` | service status, startup mode, errors, deps | restart, start, stop, set startup mode | |
| `disk` | mount points, usage, warnings | clear temp, clear caches, mount/unmount shares | |
| `printer` | printer status, queue, driver, reachability | clear queue, restart spooler, set default, reinstall driver | |
| `package` | installed status, versions, source | install, uninstall, repair, set version | All actions are High risk |
| `log` | system log entries | — (read-only) | Supports `--since` |
| `process` | process list with CPU/memory/status | kill (SIGTERM), force kill (SIGKILL), set priority | |
| `container` | containers, images, volumes | start, stop, restart, remove, pull image, prune | |

### 9.2 Platform support

Native domains use platform-specific adapters. If a domain is not supported on the current platform, `observe` and `act` return error code `unsupported` with `blocked_by: "unsupported"` and `retryable: false`.

---

## 10. Add-ons

Add-ons are cross-cutting concerns that contribute specifications and behavior to domains. They are NOT plugins — they do not own domains. They layer on top.

### 10.1 Registered add-ons

| Add-on | Kind | Description |
|---|---|---|
| `verify` | Observation | Stored predicates (bundled observation + assertion). Used by `await`. |
| `policy` | Governance | Risk classification and consent gates. |

### 10.2 Add-on contributions

Add-ons may contribute per-domain specs. These are layered into the domain spec when `world spec` is called (unless `--core` is set).

---

## 11. Telemetry

### 11.1 Event logging

Every `observe` and `act` call records a `ToolCallEvent` with:
- Unique ID, timestamp
- Tool name, domain, action, target
- Duration (ms)
- Risk classification
- Success/failure

### 11.2 Storage

Events are stored in-memory for the duration of the process. They are NOT persisted. They are NOT exposed via any CLI command. They exist solely for programmatic consumers using `world` as a library.

### 11.3 Non-interference

Telemetry recording MUST NOT affect the exit code, output, or behavior of any command. If telemetry recording fails (e.g., mutex poisoned), the failure is silently ignored.

---

## 12. Duration Parsing

The following duration formats are accepted wherever a duration argument is expected (`--interval`, `--since`):

| Format | Example | Meaning |
|---|---|---|
| `Ns` | `2s`, `0.5s` | N seconds |
| `Nms` | `500ms` | N milliseconds |
| `Nm` | `1m` | N minutes |
| Bare `N` | `3` | N seconds (implicit) |

Invalid formats produce error code `invalid_interval`.

---

## 13. Invariants

These properties MUST hold at all times. Violation of any invariant is a bug.

1. **Observe is pure.** `world observe` MUST NOT cause side effects on the system.
2. **JSON output is valid JSON.** Every byte sequence written to stdout in JSON mode MUST parse as valid JSON.
3. **Exit codes are consistent.** Exit 0 if and only if `error` is `None` and (for `await`) `passed` is `true`. Exit 1 for all failures. Exit 4 only for consent-required.
4. **Quiet mode is silent.** In Quiet mode, nothing is written to stdout or stderr. Ever.
5. **Plugin symmetry.** Native and external plugins are dispatched through the same code path. Adding a domain as a native plugin vs. external plugin MUST NOT change CLI behavior for the end user.
6. **Output mode is consumer-independent of command.** The same command produces the same information in all output modes — only the representation differs.
7. **Risk gates are unconditional.** If an action is classified High, it MUST trigger the consent gate. There is no "trusted mode" or "admin bypass" other than `--yes`.
8. **Errors are structured.** Every error has a machine-readable `code` and a human-readable `message`. Errors without codes are bugs.
9. **No partial output on failure.** If a command fails, it produces an error result — not a partial success. The `sample` command aborts on first failure.
10. **Dispatch table is the single authority for verb resolution.** There is no hardcoded verb-to-handler mapping outside the dispatch table. The CLI core does not know what verbs exist.

---

## 14. What This Spec Does NOT Cover

The following are explicitly out of scope:

- **Configuration files.** The CLI has no config file. All behavior is determined by command-line arguments and plugin discovery.
- **Environment variables.** The CLI does not read environment variables.
- **Persistent state.** The CLI is stateless across invocations. No cache, no history, no session.
- **Authentication.** The CLI operates with the permissions of the invoking user. There are no credentials, tokens, or auth mechanisms.
- **Network communication.** The CLI does not phone home, check for updates, or communicate with any remote service on its own behalf. (Domains may access the network as part of their observations.)
- **Signal handling.** The CLI does not install custom signal handlers. SIGINT and SIGTERM cause default process termination.
- **Concurrency.** Commands execute sequentially within a single invocation. There is no parallel execution of observations or actions within one CLI call.
- **Logging verbosity.** There is no `--verbose` or `--debug` flag. The CLI does not support user-facing log levels.
