------------------------- MODULE PluginProtocol -------------------------
(*
 * TLA+ specification of World's external plugin protocol.
 *
 * Models the two-party interaction between World (parent) and a plugin
 * handler (child subprocess) communicating via stdin/stdout JSON.
 *
 * Invariants verified:
 *   1. No deadlock: World and handler never both block waiting for each other.
 *   2. Every handler invocation produces a valid UnifiedResult (ok or err).
 *   3. Handler crash (non-zero exit) always produces an error UnifiedResult.
 *   4. Stdin is always closed before reading stdout (no pipe deadlock).
 *   5. Every invocation terminates.
 *   6. Malformed JSON from handler produces error, never panic.
 *)
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    HandlerOutcomes     \* {Success, ErrorResponse, Crash, MalformedJson, Hang}

VARIABLES
    worldState,         \* World process state
    handlerState,       \* Handler subprocess state
    stdinOpen,          \* Is stdin pipe still open?
    stdinWritten,       \* Has request been written to stdin?
    stdoutData,         \* What handler wrote: "ok_json" | "err_json" | "malformed" | "none"
    handlerExitCode,    \* Handler exit code: 0 | 1 | -1 (running)
    result,             \* Final UnifiedResult: "ok" | "err" | "none"
    outcome             \* Which scenario we're modeling

vars == <<worldState, handlerState, stdinOpen, stdinWritten, stdoutData,
          handlerExitCode, result, outcome>>

(* ── Initial state ─────────────────────────────────────────────────── *)

Init ==
    /\ worldState = "Spawning"
    /\ handlerState = "NotStarted"
    /\ stdinOpen = FALSE
    /\ stdinWritten = FALSE
    /\ stdoutData = "none"
    /\ handlerExitCode = -1         \* -1 means "still running"
    /\ result = "none"
    /\ outcome \in HandlerOutcomes

(* ── World actions ─────────────────────────────────────────────────── *)

WorldSpawn ==
    /\ worldState = "Spawning"
    /\ worldState' = "WritingStdin"
    /\ handlerState' = "ReadingStdin"
    /\ stdinOpen' = TRUE
    /\ UNCHANGED <<stdinWritten, stdoutData, handlerExitCode, result, outcome>>

WorldWriteStdin ==
    /\ worldState = "WritingStdin"
    /\ stdinOpen
    /\ stdinWritten' = TRUE
    /\ worldState' = "ClosingStdin"
    /\ UNCHANGED <<handlerState, stdinOpen, stdoutData, handlerExitCode, result, outcome>>

WorldCloseStdin ==
    /\ worldState = "ClosingStdin"
    /\ stdinOpen' = FALSE
    /\ worldState' = "WaitingOutput"
    /\ UNCHANGED <<handlerState, stdinWritten, stdoutData, handlerExitCode, result, outcome>>

WorldWaitOutput ==
    /\ worldState = "WaitingOutput"
    /\ handlerState = "Exited"
    /\ worldState' = "ParsingResponse"
    /\ UNCHANGED <<handlerState, stdinOpen, stdinWritten, stdoutData,
                   handlerExitCode, result, outcome>>

WorldParseResponse ==
    /\ worldState = "ParsingResponse"
    /\ worldState' = "Done"
    /\ result' = IF handlerExitCode /= 0
                 THEN "err"
                 ELSE IF stdoutData = "ok_json"
                      THEN "ok"
                      ELSE "err"    \* err_json or malformed both → err
    /\ UNCHANGED <<handlerState, stdinOpen, stdinWritten, stdoutData,
                   handlerExitCode, outcome>>

(* ── Handler actions ───────────────────────────────────────────────── *)

HandlerReadStdin ==
    /\ handlerState = "ReadingStdin"
    /\ stdinWritten
    /\ ~stdinOpen                    \* EOF received (stdin closed by World)
    /\ handlerState' = "Processing"
    /\ UNCHANGED <<worldState, stdinOpen, stdinWritten, stdoutData,
                   handlerExitCode, result, outcome>>

HandlerProcess ==
    /\ handlerState = "Processing"
    /\ IF outcome \in {"Crash", "Hang"}
       THEN \* Crash or hang (modeled as crash after timeout kill)
            /\ handlerState' = "Exited"
            /\ handlerExitCode' = 1
            /\ stdoutData' = "none"
       ELSE \* Normal processing → write stdout
            /\ handlerState' = "WritingStdout"
            /\ handlerExitCode' = handlerExitCode
            /\ stdoutData' = stdoutData
    /\ UNCHANGED <<worldState, stdinOpen, stdinWritten, result, outcome>>

HandlerWriteStdout ==
    /\ handlerState = "WritingStdout"
    /\ stdoutData' = IF outcome = "Success"
                     THEN "ok_json"
                     ELSE IF outcome = "ErrorResponse"
                          THEN "err_json"
                          ELSE "malformed"
    /\ handlerState' = "Exited"
    /\ handlerExitCode' = 0
    /\ UNCHANGED <<worldState, stdinOpen, stdinWritten, result, outcome>>

(* ── Termination ──────────────────────────────────────────────────── *)

Terminated ==
    /\ worldState = "Done"
    /\ UNCHANGED vars

(* ── Next-state relation ──────────────────────────────────────────── *)

Next == \/ WorldSpawn
        \/ WorldWriteStdin
        \/ WorldCloseStdin
        \/ WorldWaitOutput
        \/ WorldParseResponse
        \/ HandlerReadStdin
        \/ HandlerProcess
        \/ HandlerWriteStdout
        \/ Terminated

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(* ── Safety invariants ─────────────────────────────────────────────── *)

\* INV1: No deadlock — World and handler never both block on each other
NoDeadlock ==
    ~(/\ worldState = "WaitingOutput"
      /\ handlerState = "ReadingStdin"
      /\ stdinOpen)

\* INV2: Every completed invocation produces a result
AlwaysProducesResult ==
    worldState = "Done" => result \in {"ok", "err"}

\* INV3: Handler crash always produces error result
CrashProducesError ==
    (worldState = "Done" /\ handlerExitCode > 0)
        => result = "err"

\* INV4: Stdin closed before World reads stdout
StdinClosedBeforeRead ==
    worldState = "WaitingOutput" => ~stdinOpen

\* INV5: Malformed JSON produces error, not "ok"
MalformedJsonProducesError ==
    (worldState = "Done" /\ stdoutData = "malformed") => result = "err"

\* INV6: Handler reads stdin only after data is written and pipe closed
HandlerReadsComplete ==
    handlerState = "Processing" => (stdinWritten /\ ~stdinOpen)

(* ── Liveness ─────────────────────────────────────────────────────── *)

\* Every run eventually completes
EventuallyDone == <>(worldState = "Done")

\* Handler eventually exits
HandlerEventuallyExits == <>(handlerState = "Exited")

====
