#!/usr/bin/env python3
"""
npm domain handler for world.

Protocol: reads a JSON request from stdin, writes a JSON response to stdout.

Observes npm packages in the local project (node_modules) or globally (--global).
Target interpretation:
  - None / no target → list packages in nearest package.json project
  - "global"         → list globally installed packages
  - "<name>"         → show details for a specific package
"""

import json
import os
import subprocess
import sys


def observe(target):
    """Return structured npm state."""
    node_version = _run(["node", "--version"]).strip().lstrip("v")
    npm_version = _run(["npm", "--version"]).strip()

    is_global = target == "global"

    # Detect project context
    project = None
    pkg_json = _find_package_json()
    if pkg_json:
        try:
            with open(pkg_json) as f:
                project = json.load(f).get("name")
        except (json.JSONDecodeError, OSError):
            pass

    # Specific package lookup
    if target and target != "global":
        return _observe_package(target, node_version, npm_version, project)

    # List installed packages
    cmd = ["npm", "ls", "--json", "--depth=0"]
    if is_global:
        cmd.append("--global")

    installed = {}
    raw = _run(cmd, check=False)
    if raw:
        try:
            installed = json.loads(raw).get("dependencies", {})
        except json.JSONDecodeError:
            pass

    # Check outdated
    outdated_cmd = ["npm", "outdated", "--json"]
    if is_global:
        outdated_cmd.append("--global")
    outdated = {}
    raw_outdated = _run(outdated_cmd, check=False)
    if raw_outdated:
        try:
            outdated = json.loads(raw_outdated)
        except json.JSONDecodeError:
            pass

    packages = []
    for name, info in installed.items():
        current = info.get("version", "unknown")
        out_info = outdated.get(name, {})
        latest = out_info.get("latest")
        packages.append({
            "name": name,
            "version": current,
            "latest_version": latest,
            "outdated": name in outdated,
        })

    return {
        "details": {
            "packages": packages,
            "node_version": node_version,
            "npm_version": npm_version,
            "project": project if not is_global else None,
            "global": is_global,
        }
    }


def _observe_package(name, node_version, npm_version, project):
    """Observe a single package."""
    raw = _run(["npm", "ls", name, "--json", "--depth=0"], check=False)
    installed = False
    version = None
    if raw:
        try:
            deps = json.loads(raw).get("dependencies", {})
            if name in deps:
                installed = True
                version = deps[name].get("version")
        except json.JSONDecodeError:
            pass

    # Check latest from registry
    latest = None
    raw_view = _run(["npm", "view", name, "version", "--json"], check=False)
    if raw_view:
        try:
            latest = json.loads(raw_view)
        except json.JSONDecodeError:
            pass

    return {
        "details": {
            "packages": [{
                "name": name,
                "version": version,
                "latest_version": latest,
                "outdated": version != latest if version and latest else False,
            }],
            "node_version": node_version,
            "npm_version": npm_version,
            "project": project,
            "global": False,
        }
    }


def act(handler, target, params, dry_run):
    """Execute an npm action."""
    if handler == "install":
        version = (params or {}).get("version")
        pkg = f"{target}@{version}" if version else target
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"npm install {pkg}"}}
        result = subprocess.run(
            ["npm", "install", pkg],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "install_failed", "message": result.stderr.strip()}}
        return {"details": {"installed": target, "output": result.stdout.strip()}}

    elif handler == "uninstall":
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"npm uninstall {target}"}}
        result = subprocess.run(
            ["npm", "uninstall", target],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "uninstall_failed", "message": result.stderr.strip()}}
        return {"details": {"uninstalled": target}}

    elif handler == "pin_version":
        version = (params or {}).get("version")
        if not version:
            return {"error": {"code": "missing_param", "message": "version= required for set"}}
        pkg = f"{target}@{version}"
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"npm install {pkg}"}}
        result = subprocess.run(
            ["npm", "install", pkg],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "pin_failed", "message": result.stderr.strip()}}
        return {"details": {"pinned": target, "version": version}}

    elif handler == "update_all":
        if dry_run:
            return {"details": {"dry_run": True, "would_run": "npm update"}}
        result = subprocess.run(
            ["npm", "update"],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "update_failed", "message": result.stderr.strip()}}
        return {"details": {"updated": True, "output": result.stdout.strip()}}

    else:
        return {"error": {"code": "unknown_handler", "message": f"Unknown handler: {handler}"}}


def _find_package_json():
    """Walk up from cwd to find nearest package.json."""
    d = os.getcwd()
    while True:
        candidate = os.path.join(d, "package.json")
        if os.path.isfile(candidate):
            return candidate
        parent = os.path.dirname(d)
        if parent == d:
            return None
        d = parent


def _run(cmd, check=True):
    """Run a command, return stdout or empty string."""
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30
        )
        if check and result.returncode != 0:
            return ""
        return result.stdout
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return ""


def main():
    request = json.load(sys.stdin)
    command = request.get("command")

    if command == "observe":
        response = observe(request.get("target"))
    elif command == "act":
        response = act(
            request["handler"],
            request.get("target"),
            request.get("params"),
            request.get("dry_run", False),
        )
    else:
        response = {"error": {"code": "unknown_command", "message": f"Unknown: {command}"}}

    json.dump(response, sys.stdout, indent=2)


if __name__ == "__main__":
    main()
