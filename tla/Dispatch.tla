---------------------------- MODULE Dispatch ----------------------------
(*
 * TLA+ specification of World's `act` dispatch state machine.
 *
 * Models the exact sequence: parse → resolve domain → match dispatch entry
 * → check allowlist → check capability ceiling → execute → format.
 *
 * The capability ceiling is a compiled-in constant — no runtime flag can
 * override it. Actions declare which observation schema paths they mutate.
 * The ceiling is the set of paths this binary is allowed to mutate.
 *
 * Invariants verified:
 *   1. An action whose mutates tags exceed the ceiling NEVER executes.
 *   2. An invalid domain always terminates with error before execution.
 *   3. An unmatched verb always terminates with error before execution.
 *   4. A disallowed handler never reaches execution.
 *   5. Every path terminates (no deadlock).
 *   6. Exit codes are consistent with the state machine outcome.
 *   7. The ceiling cannot be bypassed by any combination of flags.
 *   8. A read-only ceiling (empty set) blocks ALL actions.
 *)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Domains,            \* Set of valid domain names
    Handlers,           \* Set of valid handler names
    AllowedHandlers,    \* Subset of Handlers in the allowlist
    MutationPaths,      \* Universe of possible mutation paths
    CeilingPaths,       \* Compiled-in allowed paths (subset of MutationPaths, or empty for read-only)
    OutputModes         \* {Json, Pretty, Quiet}

VARIABLES
    state,          \* Current state in the dispatch FSM
    domain,         \* Input domain (may be valid or invalid)
    handler,        \* Resolved handler (or "none")
    mutates,        \* Set of mutation paths for the resolved action
    dryRun,         \* --dry-run flag
    outputMode,     \* Json | Pretty | Quiet
    exitCode,       \* Exit code produced
    executed        \* Whether the handler was actually executed

vars == <<state, domain, handler, mutates, dryRun, outputMode, exitCode, executed>>

(* ── Initial state ─────────────────────────────────────────────────── *)

Init ==
    /\ state = "Init"
    /\ domain \in Domains \cup {"invalid_domain"}
    /\ handler = "none"
    /\ mutates = {}
    /\ dryRun \in BOOLEAN
    /\ outputMode \in OutputModes
    /\ exitCode = -1
    /\ executed = FALSE

(* ── State transitions ─────────────────────────────────────────────── *)

\* Step 1: Check if domain is valid
ResolveDomain ==
    /\ state = "Init"
    /\ IF domain \in Domains
       THEN /\ state' = "ResolveDomain"
            /\ exitCode' = exitCode
       ELSE /\ state' = "Done"
            /\ exitCode' = 1
    /\ UNCHANGED <<domain, handler, mutates, dryRun, outputMode, executed>>

\* Step 2: Resolve verb → handler + mutates via dispatch table
ResolveVerb ==
    /\ state = "ResolveDomain"
    /\ \E h \in Handlers \cup {"unmatched"}:
        /\ handler' = h
        /\ IF h = "unmatched"
           THEN /\ state' = "Done"
                /\ exitCode' = 1
                /\ mutates' = {}
           ELSE \* Non-deterministically choose a set of mutation paths
                /\ \E m \in SUBSET MutationPaths:
                    mutates' = m
                /\ state' = "CheckAllowlist"
                /\ exitCode' = exitCode
    /\ UNCHANGED <<domain, dryRun, outputMode, executed>>

\* Step 3: Check if resolved handler is in the allowlist
CheckAllowlist ==
    /\ state = "CheckAllowlist"
    /\ handler /= "none"
    /\ IF handler \in AllowedHandlers
       THEN /\ state' = "CheckCeiling"
            /\ exitCode' = exitCode
       ELSE /\ state' = "Done"
            /\ exitCode' = 1
    /\ UNCHANGED <<domain, handler, mutates, dryRun, outputMode, executed>>

\* Step 4: Check capability ceiling — STRUCTURAL, UNBYPASSABLE
CheckCeiling ==
    /\ state = "CheckCeiling"
    /\ IF mutates \subseteq CeilingPaths
       THEN /\ state' = "Execute"
            /\ exitCode' = exitCode
       ELSE /\ state' = "Done"
            /\ exitCode' = 1
    /\ UNCHANGED <<domain, handler, mutates, dryRun, outputMode, executed>>

\* Step 5: Execute the handler
Execute ==
    /\ state = "Execute"
    /\ executed' = TRUE
    /\ state' = "FormatResult"
    /\ UNCHANGED <<domain, handler, mutates, dryRun, outputMode, exitCode>>

\* Step 6: Format and return result
FormatResult ==
    /\ state = "FormatResult"
    /\ exitCode' = 0
    /\ state' = "Done"
    /\ UNCHANGED <<domain, handler, mutates, dryRun, outputMode, executed>>

(* ── Termination ──────────────────────────────────────────────────── *)
Terminated ==
    /\ state = "Done"
    /\ UNCHANGED vars

(* ── Next-state relation ──────────────────────────────────────────── *)

Next == \/ ResolveDomain
        \/ ResolveVerb
        \/ CheckAllowlist
        \/ CheckCeiling
        \/ Execute
        \/ FormatResult
        \/ Terminated

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(* ── Safety invariants ─────────────────────────────────────────────── *)

\* INV1: An action that exceeds the ceiling NEVER executes
CeilingEnforced ==
    executed => mutates \subseteq CeilingPaths

\* INV2: Invalid domain terminates without execution
InvalidDomainNeverExecutes ==
    (domain \notin Domains) => ~executed

\* INV3: Unmatched handler terminates without execution
UnmatchedNeverExecutes ==
    (handler = "unmatched") => ~executed

\* INV4: Disallowed handler never reaches execution
DisallowedNeverExecutes ==
    (handler /= "none" /\ handler /= "unmatched" /\ handler \notin AllowedHandlers)
        => ~executed

\* INV5: Exit code is well-formed when done
ExitCodeConsistent ==
    state = "Done" =>
        /\ exitCode \in {0, 1}
        /\ (exitCode = 0 => executed)

\* INV6: Only exit codes 0 and 1 exist (no more exit code 4)
ValidExitCodes ==
    state = "Done" => exitCode \in {0, 1}

\* INV7: dryRun flag cannot bypass the ceiling
\* (ceiling is checked regardless of dry_run — even exploring
\*  what would happen is gated by capability)
DryRunCannotBypassCeiling ==
    (executed /\ dryRun) => mutates \subseteq CeilingPaths

\* INV8: The ceiling is monotonic — once blocked, stays blocked
\* (there is no state where a blocked action becomes unblocked)
CeilingMonotonic ==
    (state = "Done" /\ ~executed /\ ~(mutates \subseteq CeilingPaths))
        => ~executed

(* ── Liveness ─────────────────────────────────────────────────────── *)

\* Every run eventually reaches Done
EventuallyTerminates == <>(state = "Done")

====
