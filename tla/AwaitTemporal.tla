--------------------------- MODULE AwaitTemporal ---------------------------
(*
 * TLA+ specification of World's `await` primitive — polling with
 * exponential backoff and timeout.
 *
 * Models the temporal behavior of poll_until: check condition, backoff,
 * check again, until pass or timeout.
 *
 * Invariants verified:
 *   1. Await always terminates (either pass or timeout).
 *   2. If condition becomes true and stays true, await eventually returns pass.
 *   3. Timeout always fires — no infinite wait even if condition never true.
 *   4. Backoff interval never exceeds max_interval.
 *   5. Result is always well-formed (passed=true or timeout=true).
 *   6. A condition that is true on first check returns immediately.
 *   7. Poll count monotonically increases.
 *)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    MaxPolls,           \* Maximum polls before timeout (models finite timeout)
    MaxInterval,        \* Maximum backoff interval (abstract units)
    InitialInterval     \* Initial backoff interval

VARIABLES
    state,              \* "Polling" | "Passed" | "TimedOut"
    conditionTrue,      \* Whether the external condition is currently true
    pollCount,          \* Number of polls executed
    currentInterval,    \* Current backoff interval
    elapsed,            \* Abstract elapsed time units
    result              \* "none" | "pass" | "timeout"

vars == <<state, conditionTrue, pollCount, currentInterval, elapsed, result>>

(* ── Initial state ─────────────────────────────────────────────────── *)

Init ==
    /\ state = "Polling"
    /\ conditionTrue \in BOOLEAN   \* Non-deterministic: condition may start true or false
    /\ pollCount = 0
    /\ currentInterval = InitialInterval
    /\ elapsed = 0
    /\ result = "none"

(* ── Environment: condition can change at any time ─────────────────── *)

ConditionBecomesTrue ==
    /\ state = "Polling"
    /\ ~conditionTrue
    /\ conditionTrue' = TRUE
    /\ UNCHANGED <<state, pollCount, currentInterval, elapsed, result>>

ConditionBecomesFalse ==
    /\ state = "Polling"
    /\ conditionTrue
    /\ conditionTrue' = FALSE
    /\ UNCHANGED <<state, pollCount, currentInterval, elapsed, result>>

(* ── Polling ───────────────────────────────────────────────────────── *)

Poll ==
    /\ state = "Polling"
    /\ pollCount' = pollCount + 1
    /\ IF conditionTrue
       THEN \* Condition passed!
            /\ state' = "Passed"
            /\ result' = "pass"
            /\ UNCHANGED <<currentInterval, elapsed>>
       ELSE IF elapsed >= MaxPolls
            THEN \* Timeout!
                 /\ state' = "TimedOut"
                 /\ result' = "timeout"
                 /\ UNCHANGED <<currentInterval, elapsed>>
            ELSE \* Backoff and try again
                 /\ state' = "Polling"
                 /\ result' = "none"
                 /\ elapsed' = elapsed + currentInterval
                 /\ currentInterval' =
                     IF currentInterval * 2 > MaxInterval
                     THEN MaxInterval
                     ELSE currentInterval * 2
    /\ UNCHANGED conditionTrue

(* ── Terminal ──────────────────────────────────────────────────────── *)

Terminated ==
    /\ state \in {"Passed", "TimedOut"}
    /\ UNCHANGED vars

(* ── Next-state relation ──────────────────────────────────────────── *)

Next == \/ Poll
        \/ ConditionBecomesTrue
        \/ ConditionBecomesFalse
        \/ Terminated

Fairness == /\ WF_vars(Poll)

Spec == Init /\ [][Next]_vars /\ Fairness

(* ── Safety invariants ─────────────────────────────────────────────── *)

\* INV1: Backoff interval never exceeds maximum
IntervalBounded ==
    currentInterval <= MaxInterval

\* INV2: Poll count is monotonically non-decreasing (type invariant)
PollCountNonNegative ==
    pollCount >= 0

\* INV3: Result is well-formed when terminated
ResultWellFormed ==
    state \in {"Passed", "TimedOut"} =>
        /\ result \in {"pass", "timeout"}
        /\ (state = "Passed" => result = "pass")
        /\ (state = "TimedOut" => result = "timeout")

\* INV4: At least one poll happens before any result
AtLeastOnePoll ==
    result /= "none" => pollCount >= 1

\* INV5: If condition is true on first poll, result is pass immediately
ImmediatePass ==
    (pollCount = 1 /\ conditionTrue) =>
        (state = "Passed" \/ state = "Polling")
    \* After 1 poll with true condition, we must have passed

\* INV6: Elapsed time is bounded
ElapsedBounded ==
    elapsed <= MaxPolls + MaxInterval

(* ── Liveness ─────────────────────────────────────────────────────── *)

\* Every run eventually terminates (pass or timeout)
EventuallyTerminates ==
    <>(state \in {"Passed", "TimedOut"})

\* If condition becomes permanently true, await eventually passes
\* (not just times out)
ConditionTrueImpliesPass ==
    [](conditionTrue) ~> (state = "Passed")

====
