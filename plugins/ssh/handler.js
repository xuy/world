#!/usr/bin/env node
/**
 * SSH session domain handler for world.
 *
 * Protocol: reads a JSON request from stdin, writes a JSON response to stdout.
 * Session domain (session: true) — observe returns nulls when no session is
 * active, and the "open" action establishes a connection.
 *
 * State is stored in ~/.world/ssh-session.json (file-based, no daemon).
 * Uses ControlMaster for SSH connection multiplexing.
 */

const { execFileSync } = require("child_process");
const { readFileSync, writeFileSync, unlinkSync, existsSync, mkdirSync } = require("fs");
const { join } = require("path");
const os = require("os");

// ─── Paths ───────────────────────────────────────────────────────────────────

const WORLD_DIR = join(os.homedir(), ".world");
const SESSION_FILE = join(WORLD_DIR, "ssh-session.json");
const CTL_PATH = join(WORLD_DIR, "ssh-ctl-%r@%h:%p");

function ensureWorldDir() {
  if (!existsSync(WORLD_DIR)) {
    mkdirSync(WORLD_DIR, { recursive: true });
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function sshArgs(host, extra) {
  return [
    "-o", "ControlPath=" + CTL_PATH,
    ...extra,
    host,
  ];
}

function run(cmd, args) {
  try {
    return execFileSync(cmd, args, {
      encoding: "utf8",
      timeout: 30000,
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch (e) {
    return null;
  }
}

function runWithError(cmd, args) {
  try {
    const out = execFileSync(cmd, args, {
      encoding: "utf8",
      timeout: 30000,
      stdio: ["pipe", "pipe", "pipe"],
    });
    return { ok: true, stdout: out };
  } catch (e) {
    const msg = (e.stderr || e.stdout || e.message || "").trim();
    return { ok: false, message: msg };
  }
}

function readSession() {
  try {
    return JSON.parse(readFileSync(SESSION_FILE, "utf8"));
  } catch {
    return null;
  }
}

function writeSession(data) {
  ensureWorldDir();
  writeFileSync(SESSION_FILE, JSON.stringify(data, null, 2));
}

function deleteSession() {
  try {
    unlinkSync(SESSION_FILE);
  } catch {}
}

// ─── Parsers ─────────────────────────────────────────────────────────────────

function parseDf(raw) {
  const lines = raw.trim().split("\n");
  if (lines.length < 2) return [];
  // Skip header line
  return lines.slice(1).map((line) => {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 6) return null;
    return {
      filesystem: parts[0],
      size: parts[1],
      used: parts[2],
      available: parts[3],
      percent_used: parts[4],
      mount: parts.slice(5).join(" "),
    };
  }).filter(Boolean);
}

function parseLoadAverage(uptimeLine) {
  // uptime output typically ends with: load average: 0.50, 0.40, 0.35
  const match = uptimeLine.match(/load averages?:\s*(.+)/i);
  return match ? match[1].trim() : null;
}

// ─── Observe ─────────────────────────────────────────────────────────────────

const NULL_OBS = {
  details: {
    host: null,
    hostname: null,
    uptime: null,
    load_average: null,
    disk_usage: [],
    os: null,
  },
};

function observe() {
  const session = readSession();
  if (!session || !session.host) return NULL_OBS;

  const host = session.host;

  // Run combined command over the multiplexed connection
  const raw = run("ssh", sshArgs(host, [
    "-o", "BatchMode=yes",
    "-o", "ConnectTimeout=5",
  ]).concat(["hostname && uptime && df -h && uname -a"]));

  if (!raw) {
    // Connection may have died; return session host but null details
    return {
      details: {
        host,
        hostname: null,
        uptime: null,
        load_average: null,
        disk_usage: [],
        os: null,
      },
    };
  }

  const lines = raw.trim().split("\n");

  // First line: hostname
  const hostname = lines[0] || null;

  // Second line: uptime
  const uptimeLine = lines[1] || null;
  const loadAverage = uptimeLine ? parseLoadAverage(uptimeLine) : null;

  // Find where df output starts (line starting with "Filesystem")
  let dfStart = -1;
  let unameIndex = -1;
  for (let i = 2; i < lines.length; i++) {
    if (lines[i].match(/^Filesystem\s/)) {
      dfStart = i;
    }
  }

  // uname -a is the last line (after df output)
  // df output ends at the second-to-last line, uname is the last
  let dfLines = [];
  let osLine = null;

  if (dfStart >= 0) {
    // The last line should be uname -a output; detect by checking if it
    // doesn't look like a df row (df rows start with / or a device name
    // and have percentage fields).
    // Simplest: uname -a is the very last line.
    unameIndex = lines.length - 1;
    const dfBlock = lines.slice(dfStart, unameIndex).join("\n");
    dfLines = parseDf(dfBlock);
    osLine = lines[unameIndex] || null;
  }

  return {
    details: {
      host,
      hostname,
      uptime: uptimeLine ? uptimeLine.trim() : null,
      load_average: loadAverage,
      disk_usage: dfLines,
      os: osLine ? osLine.trim() : null,
    },
  };
}

// ─── Act ─────────────────────────────────────────────────────────────────────

function act(handler, target, params, dryRun) {
  params = params || {};

  switch (handler) {
    case "connect": {
      const host = params.host;
      if (!host)
        return { error: { code: "missing_param", message: "host= required for open" } };

      if (dryRun) {
        return {
          details: {
            dry_run: true,
            would_run: `ssh -o ControlMaster=auto -o ControlPath=... -o ControlPersist=600 -o ConnectTimeout=5 -o BatchMode=yes ${host} true`,
          },
        };
      }

      ensureWorldDir();

      // Establish ControlMaster connection
      const result = runWithError("ssh", [
        "-o", "ControlMaster=auto",
        "-o", "ControlPath=" + CTL_PATH,
        "-o", "ControlPersist=600",
        "-o", "ConnectTimeout=5",
        "-o", "BatchMode=yes",
        host,
        "true",
      ]);

      if (!result.ok) {
        return {
          error: {
            code: "connection_failed",
            message: result.message || "Failed to connect to " + host,
          },
        };
      }

      writeSession({ host });
      // Return observation after connecting
      return observe();
    }

    case "disconnect": {
      if (dryRun) {
        const session = readSession();
        const host = session ? session.host : "<none>";
        return {
          details: {
            dry_run: true,
            would_run: `ssh -o ControlPath=... -O exit ${host}`,
          },
        };
      }

      const session = readSession();
      if (!session || !session.host) {
        return { error: { code: "no_session", message: "No active SSH session" } };
      }

      // Kill the ControlMaster
      run("ssh", [
        "-o", "ControlPath=" + CTL_PATH,
        "-O", "exit",
        session.host,
      ]);

      deleteSession();
      return NULL_OBS;
    }

    case "exec": {
      const cmd = params.cmd;
      if (!cmd)
        return { error: { code: "missing_param", message: "cmd= required for exec" } };

      if (dryRun) {
        const session = readSession();
        const host = session ? session.host : "<none>";
        return {
          details: {
            dry_run: true,
            would_run: `ssh -o ControlPath=... ${host} ${cmd}`,
          },
        };
      }

      const session = readSession();
      if (!session || !session.host) {
        return { error: { code: "no_session", message: "No active SSH session" } };
      }

      const result = runWithError("ssh", sshArgs(session.host, [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=5",
      ]).concat([cmd]));

      if (!result.ok) {
        return {
          error: {
            code: "exec_failed",
            message: result.message || "Command execution failed",
          },
        };
      }

      return { details: { stdout: result.stdout.trimEnd() } };
    }

    default:
      return { error: { code: "unknown_handler", message: `Unknown handler: ${handler}` } };
  }
}

// ─── Main ────────────────────────────────────────────────────────────────────

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", () => {
  const request = JSON.parse(input);
  let response;

  if (request.command === "observe") {
    response = observe();
  } else if (request.command === "act") {
    response = act(
      request.handler,
      request.target,
      request.params,
      request.dry_run || false
    );
  } else {
    response = {
      error: {
        code: "unknown_command",
        message: `Unknown command: ${request.command}`,
      },
    };
  }

  process.stdout.write(JSON.stringify(response, null, 2));
});
