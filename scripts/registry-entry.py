#!/usr/bin/env python3
"""Update a kumikit/registry checkout with one or more signed .kkp releases.

Reads each package's embedded `plugin.toml` and SHA-256, then creates or appends a
`[[versions]]` entry to `plugins/<publisher>/<name>.toml`. Existing versions are never
modified (idempotent). Prints `<id> <version>` for every version newly added.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tomllib
from pathlib import Path


def q(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def qlist(xs: list[str]) -> str:
    return "[" + ", ".join(q(x) for x in xs) + "]"


def read_manifest(kkp: Path) -> dict:
    res = subprocess.run(
        ["tar", "--zstd", "-xOf", str(kkp), "plugin.toml"],
        check=True,
        capture_output=True,
        text=True,
    )
    return tomllib.loads(res.stdout)


def format_version_block(m: dict, url: str, sha256: str) -> str:
    kind = m["kind"]
    is_client = bool(kind) and all(k in ("ui", "theme") for k in kind)
    lines = [
        "[[versions]]",
        f"version = {q(m['version'])}",
        f"url = {q(url)}",
        f"sha256 = {q(sha256)}",
        f"api = {q(m['api'])}",
    ]
    if is_client:
        ui = m.get("ui", {})
        iso = ui.get("isolation", "inline")
        net = sorted(ui.get("network", []))
        req_core = sorted(m.get("requires", {}).get("core", []))
        lines.append(f"isolation = {q(iso)}")
        if net:
            lines.append(f"network = {qlist(net)}")
        if req_core:
            lines.append(f"requires-core = {qlist(req_core)}")
    else:
        p = m.get("permissions", {})
        lines.append("")
        lines.append("[versions.permissions]")
        if p.get("spawn"):
            lines.append(f"spawn = {qlist(sorted(p['spawn']))}")
        if p.get("network"):
            lines.append(f"network = {qlist(sorted(p['network']))}")
        files = p.get("files", "none")
        lines.append(f"files = {q(files)}")
        if p.get("secrets"):
            lines.append(f"secrets = {qlist(sorted(p['secrets']))}")
        if p.get("tools"):
            lines.append("tools = true")
        if p.get("client"):
            lines.append(f"client = {qlist(sorted(p['client']))}")
        if p.get("workspaces") and p["workspaces"] != "none":
            lines.append(f"workspaces = {q(p['workspaces'])}")
        if p.get("schedules"):
            lines.append("schedules = true")
        limits = m.get("limits", {})
        if limits:
            lines.append("")
            lines.append("[versions.limits]")
            if "memory-mb" in limits:
                lines.append(f"memory-mb = {int(limits['memory-mb'])}")
            if "call-timeout-s" in limits:
                lines.append(f"call-timeout-s = {int(limits['call-timeout-s'])}")
    return "\n".join(lines) + "\n"


def format_plugin_header(m: dict) -> str:
    lines = [
        f"id = {q(m['id'])}",
        f"name = {q(m['name'])}",
    ]
    if m.get("description"):
        lines.append(f"description = {q(m['description'])}")
    if m.get("repository"):
        lines.append(f"repository = {q(m['repository'])}")
    if m.get("license"):
        lines.append(f"license = {q(m['license'])}")
    lines.append(f"kind = {qlist(m['kind'])}")
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--registry", required=True, type=Path, help="Path to kumikit/registry checkout")
    ap.add_argument(
        "--item",
        action="append",
        required=True,
        help="<path-to-kkp>::<download-url>",
    )
    args = ap.parse_args()

    added: list[str] = []
    for raw in args.item:
        if "::" not in raw:
            sys.exit(f"invalid --item (expected <kkp>::<url>): {raw}")
        kkp_str, url = raw.split("::", 1)
        kkp = Path(kkp_str)
        sha256 = hashlib.sha256(kkp.read_bytes()).hexdigest()
        m = read_manifest(kkp)
        pid = m["id"]
        version = m["version"]
        publisher, name = pid.split(".", 1)

        target = args.registry / "plugins" / publisher / f"{name}.toml"
        target.parent.mkdir(parents=True, exist_ok=True)
        vblock = format_version_block(m, url, sha256)

        if target.exists():
            existing_text = target.read_text()
            existing = tomllib.loads(existing_text)
            if any(v.get("version") == version for v in existing.get("versions", [])):
                continue
            sep = "\n" if existing_text.endswith("\n") else "\n\n"
            target.write_text(existing_text + sep + vblock)
        else:
            target.write_text(format_plugin_header(m) + "\n" + vblock)

        added.append(f"{pid} {version}")

    for line in added:
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
