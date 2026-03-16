#!/usr/bin/env node
/**
 * npm domain handler for world.
 *
 * Protocol: reads a JSON request from stdin, writes a JSON response to stdout.
 *
 * Target interpretation:
 *   - null / no target → list packages in nearest package.json project
 *   - "global"         → list globally installed packages
 *   - "<name>"         → show details for a specific package
 */

const { execFileSync } = require("child_process");
const { readFileSync, existsSync } = require("fs");
const { join, dirname } = require("path");

// ─── Helpers ──────────────────────────────────────────────────────────────────

function run(cmd, args, { check = true } = {}) {
  try {
    return execFileSync(cmd, args, {
      encoding: "utf8",
      timeout: 30000,
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch (e) {
    if (check) return "";
    return e.stdout || "";
  }
}

function jsonRun(cmd, args, opts) {
  const raw = run(cmd, args, opts);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function findPackageJson() {
  let d = process.cwd();
  while (true) {
    const candidate = join(d, "package.json");
    if (existsSync(candidate)) return candidate;
    const parent = dirname(d);
    if (parent === d) return null;
    d = parent;
  }
}

// ─── Observe ──────────────────────────────────────────────────────────────────

function observe(target) {
  const nodeVersion = run("node", ["--version"]).trim().replace(/^v/, "");
  const npmVersion = run("npm", ["--version"]).trim();

  const isGlobal = target === "global";

  // Detect project context
  let project = null;
  const pkgJson = findPackageJson();
  if (pkgJson) {
    try {
      project = JSON.parse(readFileSync(pkgJson, "utf8")).name || null;
    } catch {}
  }

  // Single package lookup
  if (target && target !== "global") {
    return observePackage(target, nodeVersion, npmVersion, project);
  }

  // List installed
  const lsArgs = ["ls", "--json", "--depth=0"];
  if (isGlobal) lsArgs.push("--global");
  const lsData = jsonRun("npm", lsArgs, { check: false });
  const deps = (lsData && lsData.dependencies) || {};

  // Check outdated
  const outdatedArgs = ["outdated", "--json"];
  if (isGlobal) outdatedArgs.push("--global");
  const outdated = jsonRun("npm", outdatedArgs, { check: false }) || {};

  const packages = Object.entries(deps).map(([name, info]) => ({
    name,
    version: info.version || "unknown",
    latest_version: outdated[name] ? outdated[name].latest : null,
    outdated: name in outdated,
  }));

  return {
    details: {
      packages,
      node_version: nodeVersion,
      npm_version: npmVersion,
      project: isGlobal ? null : project,
      global: isGlobal,
    },
  };
}

function observePackage(name, nodeVersion, npmVersion, project) {
  // Local install check
  const lsData = jsonRun("npm", ["ls", name, "--json", "--depth=0"], {
    check: false,
  });
  const deps = (lsData && lsData.dependencies) || {};
  const version = deps[name] ? deps[name].version : null;

  // Registry latest
  const latest = jsonRun("npm", ["view", name, "version", "--json"], {
    check: false,
  });

  return {
    details: {
      packages: [
        {
          name,
          version,
          latest_version: typeof latest === "string" ? latest : null,
          outdated: version && latest ? version !== latest : false,
        },
      ],
      node_version: nodeVersion,
      npm_version: npmVersion,
      project,
      global: false,
    },
  };
}

// ─── Act ──────────────────────────────────────────────────────────────────────

function act(handler, target, params, dryRun) {
  switch (handler) {
    case "install": {
      const version = (params || {}).version;
      const pkg = version ? `${target}@${version}` : target;
      if (dryRun)
        return { details: { dry_run: true, would_run: `npm install ${pkg}` } };
      try {
        const out = execFileSync("npm", ["install", pkg], {
          encoding: "utf8",
          stdio: ["pipe", "pipe", "pipe"],
        });
        return { details: { installed: target, output: out.trim() } };
      } catch (e) {
        return {
          error: {
            code: "install_failed",
            message: (e.stderr || e.message).trim(),
          },
        };
      }
    }

    case "uninstall": {
      if (dryRun)
        return {
          details: { dry_run: true, would_run: `npm uninstall ${target}` },
        };
      try {
        execFileSync("npm", ["uninstall", target], {
          encoding: "utf8",
          stdio: ["pipe", "pipe", "pipe"],
        });
        return { details: { uninstalled: target } };
      } catch (e) {
        return {
          error: {
            code: "uninstall_failed",
            message: (e.stderr || e.message).trim(),
          },
        };
      }
    }

    case "pin_version": {
      const version = (params || {}).version;
      if (!version)
        return {
          error: { code: "missing_param", message: "version= required for set" },
        };
      const pkg = `${target}@${version}`;
      if (dryRun)
        return { details: { dry_run: true, would_run: `npm install ${pkg}` } };
      try {
        execFileSync("npm", ["install", pkg], {
          encoding: "utf8",
          stdio: ["pipe", "pipe", "pipe"],
        });
        return { details: { pinned: target, version } };
      } catch (e) {
        return {
          error: {
            code: "pin_failed",
            message: (e.stderr || e.message).trim(),
          },
        };
      }
    }

    case "update_all": {
      if (dryRun)
        return { details: { dry_run: true, would_run: "npm update" } };
      try {
        const out = execFileSync("npm", ["update"], {
          encoding: "utf8",
          stdio: ["pipe", "pipe", "pipe"],
        });
        return { details: { updated: true, output: out.trim() } };
      } catch (e) {
        return {
          error: {
            code: "update_failed",
            message: (e.stderr || e.message).trim(),
          },
        };
      }
    }

    default:
      return {
        error: {
          code: "unknown_handler",
          message: `Unknown handler: ${handler}`,
        },
      };
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
      error: {
        code: "unknown_command",
        message: `Unknown: ${request.command}`,
      },
    };
  }

  process.stdout.write(JSON.stringify(response, null, 2));
});
