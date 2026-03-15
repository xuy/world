---------------------------- MODULE Dispatch ----------------------------
(*
 * TLA+ specification of World's `act` dispatch state machine.
 *
 * Models the exact sequence: parse → resolve domain → match dispatch entry
 * → check allowlist → classify risk → consent gate → execute → format.
 *
 * Invariants verified:
 *   1. No high-risk action executes without consent (--yes or interactive).
 *   2. An invalid domain always terminates with error before execution.
 *   3. An unmatched verb always terminates with error before execution.
 *   4. A disallowed handler never reaches execution.
 *   5. Every path terminates (no deadlock).
 *   6. Exit codes are consistent with the state machine outcome.
 *)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Domains,            \* Set of valid domain names
    Handlers,           \* Set of valid handler names
    HighRiskHandlers,   \* Subset of Handlers classified High
    AllowedHandlers,    \* Subset of Handlers in the allowlist
    OutputModes         \* {Json, Pretty, Quiet}

VARIABLES
    state,          \* Current state in the dispatch FSM
    domain,         \* Input domain (may be valid or invalid)
    handler,        \* Resolved handler (or "none")
    risk,           \* Classified risk level
    dryRun,         \* --dry-run flag
    yesFlag,        \* --yes flag
    userConfirmed,  \* Whether user confirmed at the consent gate
    outputMode,     \* Json | Pretty | Quiet
    exitCode,       \* Exit code produced
    executed        \* Whether the handler was actually executed

vars == <<state, domain, handler, risk, dryRun, yesFlag, userConfirmed,
          outputMode, exitCode, executed>>

(* ── Initial state ─────────────────────────────────────────────────── *)

Init ==
    /\ state = "Init"
    /\ domain \in Domains \cup {"invalid_domain"}
    /\ handler = "none"
    /\ risk = "Low"
    /\ dryRun \in BOOLEAN
    /\ yesFlag \in BOOLEAN
    /\ userConfirmed \in BOOLEAN
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
    /\ UNCHANGED <<domain, handler, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, executed>>

\* Step 2: Try to resolve verb → handler via dispatch table
ResolveVerb ==
    /\ state = "ResolveDomain"
    /\ \E h \in Handlers \cup {"unmatched"}:
        /\ handler' = h
        /\ IF h = "unmatched"
           THEN /\ state' = "Done"
                /\ exitCode' = 1
           ELSE /\ state' = "CheckAllowlist"
                /\ exitCode' = exitCode
    /\ UNCHANGED <<domain, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, executed>>

\* Step 3: Check if resolved handler is in the allowlist
CheckAllowlist ==
    /\ state = "CheckAllowlist"
    /\ handler /= "none"
    /\ IF handler \in AllowedHandlers
       THEN /\ state' = "ClassifyRisk"
            /\ exitCode' = exitCode
       ELSE /\ state' = "Done"
            /\ exitCode' = 1
    /\ UNCHANGED <<domain, handler, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, executed>>

\* Step 4: Classify the risk level of the handler
ClassifyRisk ==
    /\ state = "ClassifyRisk"
    /\ risk' = IF handler \in HighRiskHandlers THEN "High" ELSE "Medium"
    /\ state' = "ConsentGate"
    /\ UNCHANGED <<domain, handler, dryRun, yesFlag, userConfirmed,
                   outputMode, exitCode, executed>>

\* Step 5: Gate high-risk actions behind consent
ConsentGate ==
    /\ state = "ConsentGate"
    /\ IF risk = "High" /\ ~dryRun /\ ~yesFlag
       THEN IF userConfirmed
            THEN /\ state' = "Execute"
                 /\ exitCode' = exitCode
            ELSE /\ state' = "Done"
                 /\ exitCode' = 4
       ELSE /\ state' = "Execute"
            /\ exitCode' = exitCode
    /\ UNCHANGED <<domain, handler, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, executed>>

\* Step 6: Execute the handler
Execute ==
    /\ state = "Execute"
    /\ executed' = TRUE
    /\ state' = "FormatResult"
    /\ UNCHANGED <<domain, handler, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, exitCode>>

\* Step 7: Format and return result
FormatResult ==
    /\ state = "FormatResult"
    /\ exitCode' = 0
    /\ state' = "Done"
    /\ UNCHANGED <<domain, handler, risk, dryRun, yesFlag, userConfirmed,
                   outputMode, executed>>

(* ── Termination ──────────────────────────────────────────────────── *)
Terminated ==
    /\ state = "Done"
    /\ UNCHANGED vars

(* ── Next-state relation ──────────────────────────────────────────── *)

Next == \/ ResolveDomain
        \/ ResolveVerb
        \/ CheckAllowlist
        \/ ClassifyRisk
        \/ ConsentGate
        \/ Execute
        \/ FormatResult
        \/ Terminated

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(* ── Safety invariants ─────────────────────────────────────────────── *)

\* INV1: A high-risk action never executes without consent or --yes
NoUnsafeExecution ==
    (state = "Execute" /\ handler \in HighRiskHandlers /\ ~dryRun)
        => (yesFlag \/ userConfirmed)

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
        /\ exitCode \in {0, 1, 4}
        /\ (exitCode = 0 => executed)
        /\ (exitCode = 4 => ~executed)

\* INV6: Exactly these exit codes are possible
ValidExitCodes ==
    state = "Done" => exitCode \in {0, 1, 4}

(* ── Liveness ─────────────────────────────────────────────────────── *)

\* Every run eventually reaches Done
EventuallyTerminates == <>(state = "Done")

====
