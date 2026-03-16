#!/usr/bin/env node
/**
 * Browser domain handler for world.
 *
 * Protocol: reads a JSON request from stdin, writes a JSON response to stdout.
 * Delegates to agent-browser CLI, which manages browser state via its daemon.
 *
 * Session domain (session: true) — observe returns nulls when no page is open,
 * and the "open" action populates the observation space.
 */

const { execFileSync } = require("child_process");

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

function runOrError(cmd, args) {
  try {
    execFileSync(cmd, args, {
      encoding: "utf8",
      timeout: 30000,
      stdio: ["pipe", "pipe", "pipe"],
    });
    return null;
  } catch (e) {
    const msg = (e.stderr || e.stdout || e.message || "").trim();
    return { error: { code: "browser_error", message: msg } };
  }
}

/** Strip surrounding JSON quotes from agent-browser eval output. */
function stripQuotes(s) {
  if (s && s.length >= 2 && s[0] === '"' && s[s.length - 1] === '"') {
    return s.slice(1, -1);
  }
  return s;
}

// ─── Snapshot ─────────────────────────────────────────────────────────────────

const NULL_OBS = { details: { url: null, title: null, elements: [], snapshot: null } };

function snapshot() {
  const raw = run("agent-browser", ["snapshot", "--json", "-i", "-c"]);
  if (!raw) return NULL_OBS;

  let snap;
  try {
    snap = JSON.parse(raw);
  } catch {
    return NULL_OBS;
  }

  const origin = snap && snap.data && snap.data.origin;
  if (!origin || origin === "about:blank") return NULL_OBS;

  // Get title via eval
  let title = null;
  const titleRaw = run("agent-browser", ["eval", "document.title"]);
  if (titleRaw) {
    const t = stripQuotes(titleRaw.trim());
    if (t) title = t;
  }

  // Build elements from refs
  const refs = (snap.data && snap.data.refs) || {};
  const elements = Object.entries(refs)
    .map(([ref, info]) => ({ ref, role: info.role, name: info.name }))
    .sort((a, b) => (a.ref < b.ref ? -1 : a.ref > b.ref ? 1 : 0));

  return {
    details: {
      url: origin,
      title,
      elements,
      snapshot: snap.data.snapshot || null,
    },
  };
}

// ─── Observe ──────────────────────────────────────────────────────────────────

function observe() {
  return snapshot();
}

// ─── Act ──────────────────────────────────────────────────────────────────────

function act(handler, target, params, dryRun) {
  params = params || {};

  if (dryRun) {
    return dryRunResponse(handler, target, params);
  }

  switch (handler) {
    case "navigate": {
      const url = params.url;
      if (!url)
        return { error: { code: "missing_param", message: "url= required for open" } };
      const err = runOrError("agent-browser", ["open", url]);
      if (err) return err;
      return snapshot();
    }

    case "close": {
      const err = runOrError("agent-browser", ["close"]);
      if (err) return err;
      return NULL_OBS;
    }

    case "click": {
      if (!target)
        return { error: { code: "missing_target", message: "element ref required for click" } };
      const err = runOrError("agent-browser", ["click", target]);
      if (err) return err;
      return snapshot();
    }

    case "fill": {
      const text = params.text || "";
      if (!target)
        return { error: { code: "missing_target", message: "element ref required for fill" } };
      const err = runOrError("agent-browser", ["fill", target, text]);
      if (err) return err;
      return snapshot();
    }

    case "select": {
      const val = params.value || "";
      if (!target)
        return { error: { code: "missing_target", message: "element ref required for select" } };
      const err = runOrError("agent-browser", ["select", target, val]);
      if (err) return err;
      return snapshot();
    }

    case "hover": {
      if (!target)
        return { error: { code: "missing_target", message: "element ref required for hover" } };
      const err = runOrError("agent-browser", ["hover", target]);
      if (err) return err;
      return snapshot();
    }

    case "scroll": {
      const dir = params.direction || "down";
      const px = String(params.pixels || 300);
      const err = runOrError("agent-browser", ["scroll", dir, px]);
      if (err) return err;
      return snapshot();
    }

    case "press_key": {
      const key = params.key;
      if (!key)
        return { error: { code: "missing_param", message: "key= required for press" } };
      const err = runOrError("agent-browser", ["press", key]);
      if (err) return err;
      return snapshot();
    }

    case "eval_js": {
      const js = params.js;
      if (!js)
        return { error: { code: "missing_param", message: "js= required for eval" } };
      const raw = run("agent-browser", ["eval", js]);
      if (raw === null) {
        return { error: { code: "eval_error", message: "eval failed" } };
      }
      return { details: { result: stripQuotes(raw.trim()) } };
    }

    default:
      return { error: { code: "unknown_handler", message: `Unknown handler: ${handler}` } };
  }
}

function dryRunResponse(handler, target, params) {
  switch (handler) {
    case "navigate":
      return { details: { dry_run: true, would_run: `agent-browser open ${params.url || ""}` } };
    case "close":
      return { details: { dry_run: true, would_run: "agent-browser close" } };
    case "click":
      return { details: { dry_run: true, would_run: `agent-browser click ${target}` } };
    case "fill":
      return { details: { dry_run: true, would_run: `agent-browser fill ${target} '${params.text || ""}'` } };
    case "select":
      return { details: { dry_run: true, would_run: `agent-browser select ${target} '${params.value || ""}'` } };
    case "hover":
      return { details: { dry_run: true, would_run: `agent-browser hover ${target}` } };
    case "scroll": {
      const dir = params.direction || "down";
      const px = params.pixels || 300;
      return { details: { dry_run: true, would_run: `agent-browser scroll ${dir} ${px}` } };
    }
    case "press_key":
      return { details: { dry_run: true, would_run: `agent-browser press ${params.key || ""}` } };
    case "eval_js":
      return { details: { dry_run: true, would_run: "agent-browser eval <js>" } };
    default:
      return { error: { code: "unknown_handler", message: `Unknown handler: ${handler}` } };
  }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

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
