#!/usr/bin/env python3
"""
pip domain handler for world.

Protocol: reads a JSON request from stdin, writes a JSON response to stdout.

Request format:
  {"command": "observe", "target": null, "scope": null}
  {"command": "act", "handler": "install", "target": "requests", "params": {}, "dry_run": false}

Response format (matches UnifiedResult.details):
  {"details": {...}}
  {"error": {"code": "...", "message": "..."}}
"""

import json
import subprocess
import sys


def observe(target, scope):
    """Return structured pip state."""
    # Python + pip versions
    py_version = subprocess.check_output(
        ["python3", "--version"], text=True
    ).strip().split()[-1]

    pip_version = subprocess.check_output(
        ["python3", "-m", "pip", "--version"], text=True
    ).strip().split()[1]

    # Installed packages
    raw = subprocess.check_output(
        ["python3", "-m", "pip", "list", "--format=json"], text=True
    )
    installed = json.loads(raw)

    # Check outdated if no scope filter or scope includes "outdated"
    packages = []
    outdated_names = set()

    if scope is None or "outdated" in (scope or []):
        try:
            raw_outdated = subprocess.check_output(
                ["python3", "-m", "pip", "list", "--outdated", "--format=json"],
                text=True, stderr=subprocess.DEVNULL, timeout=30
            )
            for pkg in json.loads(raw_outdated):
                outdated_names.add(pkg["name"])
        except (subprocess.TimeoutExpired, subprocess.CalledProcessError):
            pass

    # Filter to target if specified
    for pkg in installed:
        if target and pkg["name"].lower() != target.lower():
            continue
        packages.append({
            "name": pkg["name"],
            "version": pkg["version"],
            "latest_version": None,  # only populated by --outdated
            "outdated": pkg["name"] in outdated_names,
        })

    # Detect virtualenv
    venv = None
    import os
    if os.environ.get("VIRTUAL_ENV"):
        venv = os.environ["VIRTUAL_ENV"]

    return {
        "details": {
            "packages": packages,
            "python_version": py_version,
            "pip_version": pip_version,
            "virtualenv": venv,
        }
    }


def act(handler, target, params, dry_run):
    """Execute a pip action."""
    if handler == "install":
        version = (params or {}).get("version")
        pkg = f"{target}=={version}" if version else target
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"pip install {pkg}"}}
        result = subprocess.run(
            ["python3", "-m", "pip", "install", pkg],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "install_failed", "message": result.stderr.strip()}}
        return {"details": {"installed": target, "output": result.stdout.strip()}}

    elif handler == "uninstall":
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"pip uninstall -y {target}"}}
        result = subprocess.run(
            ["python3", "-m", "pip", "uninstall", "-y", target],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "uninstall_failed", "message": result.stderr.strip()}}
        return {"details": {"uninstalled": target}}

    elif handler == "pin_version":
        version = (params or {}).get("version")
        if not version:
            return {"error": {"code": "missing_param", "message": "version= required for set"}}
        pkg = f"{target}=={version}"
        if dry_run:
            return {"details": {"dry_run": True, "would_run": f"pip install {pkg}"}}
        result = subprocess.run(
            ["python3", "-m", "pip", "install", pkg],
            capture_output=True, text=True
        )
        if result.returncode != 0:
            return {"error": {"code": "pin_failed", "message": result.stderr.strip()}}
        return {"details": {"pinned": target, "version": version}}

    elif handler == "upgrade_all":
        if dry_run:
            return {"details": {"dry_run": True, "would_run": "pip install --upgrade <all outdated>"}}
        # Get outdated, then upgrade each
        raw = subprocess.check_output(
            ["python3", "-m", "pip", "list", "--outdated", "--format=json"], text=True
        )
        outdated = json.loads(raw)
        upgraded = []
        for pkg in outdated:
            subprocess.run(
                ["python3", "-m", "pip", "install", "--upgrade", pkg["name"]],
                capture_output=True
            )
            upgraded.append(pkg["name"])
        return {"details": {"upgraded": upgraded, "count": len(upgraded)}}

    else:
        return {"error": {"code": "unknown_handler", "message": f"Unknown handler: {handler}"}}


def main():
    request = json.load(sys.stdin)
    command = request.get("command")

    if command == "observe":
        response = observe(request.get("target"), request.get("scope"))
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
