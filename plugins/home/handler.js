#!/usr/bin/env node
/**
 * Home domain handler for world.
 *
 * Protocol: reads a JSON request from stdin, writes a JSON response to stdout.
 * Delegates to HomeAssistant REST API.
 *
 * Session domain (session: true) — stores HA connection info in
 * ~/.world/home-session.json. Observe returns null fields when disconnected.
 */

const { execFileSync } = require("child_process");
const { readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync } = require("fs");
const { join } = require("path");

const HOME_DIR = join(require("os").homedir(), ".world");
const SESSION_FILE = join(HOME_DIR, "home-session.json");

// ─── Session helpers ───────────────────────────────────────────────────────

function ensureDir() {
  if (!existsSync(HOME_DIR)) mkdirSync(HOME_DIR, { recursive: true });
}

function loadSession() {
  try {
    return JSON.parse(readFileSync(SESSION_FILE, "utf8"));
  } catch {
    return null;
  }
}

function saveSession(session) {
  ensureDir();
  writeFileSync(SESSION_FILE, JSON.stringify(session, null, 2));
}

function clearSession() {
  try { unlinkSync(SESSION_FILE); } catch {}
}

// ─── HA API helpers ────────────────────────────────────────────────────────

function haGet(session, path) {
  try {
    const url = `${session.url}/api${path}`;
    const out = execFileSync("curl", [
      "-s", "-f",
      "-H", `Authorization: Bearer ${session.token}`,
      url,
    ], { encoding: "utf8", timeout: 10000 });
    return JSON.parse(out);
  } catch (e) {
    return null;
  }
}

function haPost(session, path, data) {
  try {
    const url = `${session.url}/api${path}`;
    const out = execFileSync("curl", [
      "-s", "-f",
      "-X", "POST",
      "-H", `Authorization: Bearer ${session.token}`,
      "-H", "Content-Type: application/json",
      "-d", JSON.stringify(data),
      url,
    ], { encoding: "utf8", timeout: 10000 });
    return JSON.parse(out);
  } catch (e) {
    return null;
  }
}

// ─── Entity classification ─────────────────────────────────────────────────

/** Slugify a friendly name: "Living Room Light" → "living_room_light" */
function slugify(name) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function isLightLike(entityId, attrs) {
  const domain = entityId.split(".")[0];
  if (domain === "light" || domain === "switch") return true;
  if (domain === "input_boolean") {
    const icon = attrs.icon || "";
    const name = (attrs.friendly_name || "").toLowerCase();
    return icon.includes("light") || name.includes("light");
  }
  return false;
}

function isLockLike(entityId, attrs) {
  const domain = entityId.split(".")[0];
  if (domain === "lock") return true;
  if (domain === "input_boolean") {
    const icon = attrs.icon || "";
    const name = (attrs.friendly_name || "").toLowerCase();
    return icon.includes("lock") || name.includes("lock");
  }
  return false;
}

function isCoverLike(entityId, attrs) {
  const domain = entityId.split(".")[0];
  if (domain === "cover") return true;
  if (domain === "input_boolean") {
    const icon = attrs.icon || "";
    const name = (attrs.friendly_name || "").toLowerCase();
    return icon.includes("garage") || icon.includes("cover") || name.includes("garage") || name.includes("door");
  }
  return false;
}

// ─── Observe ───────────────────────────────────────────────────────────────

const NULL_OBS = { lights: [], climate: [], sensors: [], locks: [], covers: [] };

/**
 * Fetch all HA states, classify them, and return observations with clean IDs.
 * Also returns a slug→entityId lookup map for act resolution.
 */
function fetchStates(session) {
  const states = haGet(session, "/states");
  if (!states) return { obs: NULL_OBS, lookup: {} };

  const lights = [];
  const climate = [];
  const sensors = [];
  const locks = [];
  const covers = [];
  const lookup = {}; // slug → HA entity_id

  for (const entity of states) {
    const entityId = entity.entity_id;
    const attrs = entity.attributes || {};
    const name = attrs.friendly_name || entityId;
    const id = slugify(name);
    const domain = entityId.split(".")[0];

    lookup[id] = entityId;

    if (isLockLike(entityId, attrs)) {
      locks.push({
        id,
        name,
        state: entity.state === "on" || entity.state === "locked" ? "locked" : "unlocked",
      });
    } else if (isCoverLike(entityId, attrs)) {
      covers.push({
        id,
        name,
        state: entity.state === "on" || entity.state === "open" ? "open" : "closed",
      });
    } else if (isLightLike(entityId, attrs)) {
      lights.push({
        id,
        name,
        state: entity.state === "on" ? "on" : "off",
      });
    } else if (domain === "climate" || domain === "input_number") {
      climate.push({
        id,
        name,
        target_temp: parseFloat(entity.state) || null,
        unit: attrs.unit_of_measurement || null,
      });
    } else if (domain === "sensor") {
      if (entityId.includes("backup") || entityId.includes("sun_next")) continue;
      sensors.push({
        id,
        name,
        state: entity.state,
        unit: attrs.unit_of_measurement || null,
      });
    }
  }

  return { obs: { lights, climate, sensors, locks, covers }, lookup };
}

function observe() {
  const session = loadSession();
  if (!session) return { details: NULL_OBS };
  const { obs } = fetchStates(session);
  return { details: obs };
}

/** Resolve a slug target to an HA entity_id. */
function resolveTarget(session, target) {
  const { lookup } = fetchStates(session);
  return lookup[target] || null;
}

// ─── Act helpers ───────────────────────────────────────────────────────────

function resolveService(entityId, action) {
  const domain = entityId.split(".")[0];

  switch (domain) {
    case "input_boolean":
      if (action === "turn_on" || action === "lock" || action === "open")
        return "input_boolean/turn_on";
      if (action === "turn_off" || action === "unlock" || action === "close")
        return "input_boolean/turn_off";
      break;
    case "input_number":
      return "input_number/set_value";
    case "light":
      return action === "turn_on" ? "light/turn_on" : "light/turn_off";
    case "switch":
      return action === "turn_on" ? "switch/turn_on" : "switch/turn_off";
    case "climate":
      return "climate/set_temperature";
    case "lock":
      return action === "lock" ? "lock/lock" : "lock/unlock";
    case "cover":
      return action === "turn_on" ? "cover/open_cover" : "cover/close_cover";
  }
  return null;
}

// ─── Act ───────────────────────────────────────────────────────────────────

function act(handler, target, params, dryRun) {
  // ── Session lifecycle ──
  if (handler === "connect") {
    const url = (params || {}).url;
    const token = (params || {}).token;
    if (!url) return { error: { code: "missing_param", message: "url= required for open" } };
    if (!token) return { error: { code: "missing_param", message: "token= required for open" } };

    if (dryRun) return { details: { dry_run: true, would_run: `Connect to ${url}` } };

    // Verify connectivity
    const cleanUrl = url.replace(/\/$/, "");
    const testSession = { url: cleanUrl, token };
    const test = haGet(testSession, "/");
    if (!test) return { error: { code: "connection_failed", message: `Cannot reach ${cleanUrl}/api/` } };

    saveSession(testSession);
    return observe();
  }

  if (handler === "disconnect") {
    if (dryRun) return { details: { dry_run: true, would_run: "Disconnect from HomeAssistant" } };
    clearSession();
    return { details: NULL_OBS };
  }

  // ── Entity actions ──
  const session = loadSession();
  if (!session) return { error: { code: "no_session", message: "Not connected. Use: world act home open <url> token=<token>" } };

  if (!target) return { error: { code: "missing_target", message: "Target required (e.g. living_room_light)" } };

  // Resolve slug → HA entity_id
  const entityId = resolveTarget(session, target);
  if (!entityId) {
    return { error: { code: "unknown_target", message: `Unknown target '${target}'. Use 'world observe home' to see available targets.` } };
  }

  if (dryRun) {
    return { details: { dry_run: true, would_run: `${handler} on ${target} (${entityId})` } };
  }

  const service = resolveService(entityId, handler);
  if (!service) {
    return { error: { code: "unsupported", message: `Cannot ${handler} on ${target}` } };
  }

  const data = { entity_id: entityId };
  if (handler === "set_value") {
    const temp = (params || {}).temperature;
    if (!temp) return { error: { code: "missing_param", message: "temperature= required for set" } };
    if (entityId.startsWith("input_number.")) {
      data.value = parseFloat(temp);
    } else {
      data.temperature = parseFloat(temp);
    }
  }

  const result = haPost(session, `/services/${service}`, data);
  if (!result) {
    return { error: { code: "action_failed", message: `Failed to ${handler} on ${target}` } };
  }

  return observe();
}

// ─── Main ──────────────────────────────────────────────────────────────────

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", () => {
  const request = JSON.parse(input);
  let response;

  if (request.command === "observe") {
    response = observe(request.target);
  } else if (request.command === "act") {
    response = act(
      request.handler,
      request.target,
      request.params,
      request.dry_run || false
    );
  } else {
    response = {
      error: { code: "unknown_command", message: `Unknown: ${request.command}` },
    };
  }

  process.stdout.write(JSON.stringify(response, null, 2));
});
