#!/usr/bin/env python3
"""notices-check — THIRD-PARTY-NOTICES.md against the dependency graphs that ship.

The tables were hand-maintained, and a "regenerate before each release" comment
was the only thing keeping them true. `mysql_async` joined the app on 24 Aug 2026
and five releases (0.4.0 -> 0.7.0) shipped a Licenses dialog missing it and the
crates it pulls in; the `rex` CLI sidecar's graph had never been inventoried at
all. A hand count found it on 13 Sep (ledger #592). The npm table had failed the
same way before (5 Aug: 15 rows short). Prose that restates tracked state rots;
this makes the tables checked state.

Checks, BOTH directions (a one-way check is what hid the npm gap):
  Rust — every registry crate the universal macOS app links (arm64 UNION x86_64,
         normal + build edges, the app crate and the `rex` CLI) has a row; no row
         names a crate nothing links; each row's licence equals the crate's
         declared licence; the section heading's count equals the graph.
  npm  — the production closure `pnpm list --prod` reports equals the table's
         rows; the heading's count equals it. (pnpm's list carries no licence, so
         the npm licence column is not checked.)

Dev-dependencies are excluded: they never ship. The Windows graph is NOT checked
yet — no Windows build has shipped (docs/TODO.md, "Third-party notices").

Usage: scripts/notices-check.py   (exit 0 = matches, 1 = drift)
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOTICES = os.path.join(ROOT, "THIRD-PARTY-NOTICES.md")
MANIFESTS = ["src-tauri/Cargo.toml", "cli/Cargo.toml"]
TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"]
RUST_HEADING = re.compile(r"^## Rust crates \(statically linked; (\d+) external crates, universal macOS graph\)$")
NPM_HEADING = re.compile(r"^## npm packages \(production closure bundled by Vite; (\d+) packages\)$")


def rust_graph():
    """(name, version) -> licence for every registry crate reachable without dev edges."""
    out = {}
    for manifest in MANIFESTS:
        for target in TARGETS:
            raw = subprocess.run(
                ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
                 "--filter-platform", target, "--manifest-path", os.path.join(ROOT, manifest)],
                check=True, capture_output=True, text=True,
            ).stdout
            meta = json.loads(raw)
            packages = {p["id"]: p for p in meta["packages"]}
            nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
            stack, seen = list(meta["workspace_members"]), set()
            while stack:
                pid = stack.pop()
                if pid in seen:
                    continue
                seen.add(pid)
                for dep in nodes[pid]["deps"]:
                    if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                        stack.append(dep["pkg"])
            for pid in seen:
                p = packages[pid]
                if p.get("source"):
                    out[(p["name"], p["version"])] = p.get("license") or "(none declared)"
    return out


def npm_graph():
    raw = subprocess.run(
        ["pnpm", "list", "--prod", "--depth", "Infinity", "--json"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    seen = set()

    def walk(deps):
        for name, info in (deps or {}).items():
            key = (name, info.get("version"))
            if key in seen:
                continue
            seen.add(key)
            walk(info.get("dependencies"))

    for project in json.loads(raw):
        walk(project.get("dependencies"))
    return seen


def section(prefix, heading_re):
    """(heading line, stated count or None, {(name, version): licence})."""
    lines = open(NOTICES, encoding="utf-8").read().splitlines()
    starts = [i for i, l in enumerate(lines) if l.startswith(prefix)]
    if len(starts) != 1:
        sys.exit(f"notices-check: expected exactly one {prefix!r} section, found {len(starts)}")
    head = lines[starts[0]]
    m = heading_re.match(head)
    rows = {}
    for l in lines[starts[0] + 1:]:
        if l.startswith("## "):
            break
        if l.startswith("| ") and not l.startswith("|---"):
            cells = [c.strip() for c in l.strip().strip("|").split("|")]
            if cells[0] in ("Crate", "Package"):
                continue
            rows[(cells[0], cells[1])] = cells[2] if len(cells) > 2 else ""
    return head, (int(m.group(1)) if m else None), rows


def norm(licence):
    return re.sub(r"\s+", "", licence).replace("/", "OR")


def compare(label, graph, head, stated, rows, licences):
    problems = []
    for name, version in sorted(set(graph) - set(rows)):
        lic = f" {licences[(name, version)]} |" if licences else ""
        problems.append(f"{label}: missing row: | {name} | {version} |{lic}")
    for name, version in sorted(set(rows) - set(graph)):
        problems.append(f"{label}: row for something nothing ships: {name} {version}")
    if licences:
        for key in sorted(set(rows) & set(graph)):
            if norm(rows[key]) != norm(licences[key]):
                problems.append(
                    f"{label}: licence differs: {key[0]} {key[1]} — row says {rows[key]!r}, "
                    f"the crate declares {licences[key]!r}"
                )
    if stated is None:
        problems.append(f"{label}: heading does not state its count in the checked form: {head!r}")
    elif stated != len(graph):
        problems.append(f"{label}: heading says {stated}, the graph has {len(graph)}")
    return problems


def main():
    rust = rust_graph()
    npm = npm_graph()
    problems = compare("rust", rust, *section("## Rust crates", RUST_HEADING), rust)
    problems += compare("npm", npm, *section("## npm packages", NPM_HEADING), None)
    if problems:
        print(f"notices-check: THIRD-PARTY-NOTICES.md does not match what ships ({len(problems)}):")
        for p in problems:
            print(f"  {p}")
        return 1
    print(
        f"notices-check: {len(rust)} Rust crates (arm64 + x86_64, app + rex CLI) with rows and "
        f"licences; {len(npm)} npm packages with rows"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
