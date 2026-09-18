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
  composer — the vendored `wp dist-archive` tree compiled into the app: the
         packages its own `vendor/composer/installed.json` records equal the
         table's rows, with their licences, and the heading's count equals them.

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
# The Windows app links a DIFFERENT closure, not a superset: no objc2/core-foundation, and the
# windows-*/webview2-com* families instead. One arch, because that is what ships (plan §4: no
# arm64 PHP), and it is what `scripts/windows-check.sh` compiles.
TARGETS_WINDOWS = ["x86_64-pc-windows-msvc"]
RUST_HEADING = re.compile(r"^## Rust crates \(statically linked; (\d+) external crates, universal macOS graph\)$")
# NOTE the heading may not begin with "## Rust crates": `section()` matches by startswith, so a
# "## Rust crates — Windows" heading would make the macOS section ambiguous and exit.
WINDOWS_HEADING = re.compile(r"^## Windows Rust crates \(statically linked; (\d+) external crates, x86_64 Windows graph\)$")
NPM_HEADING = re.compile(r"^## npm packages \(production closure bundled by Vite; (\d+) packages\)$")
VENDORED_HEADING = re.compile(r"^## Vendored PHP \(compiled into the app binary; (\d+) packages\)$")
COMPOSER_INSTALLED = os.path.join(ROOT, "src-tauri/resources/wp-dist-archive/vendor/composer/installed.json")


def rust_graph(targets=None):
    """(name, version) -> licence for every registry crate reachable without dev edges.

    `targets` so the Windows graph is read by the SAME walk as the macOS one — two graphs built
    by two rules is how this file drifted before.
    """
    out = {}
    for manifest in MANIFESTS:
        for target in targets or TARGETS:
            raw = subprocess.run(
                ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
                 "--filter-platform", target, "--manifest-path", os.path.join(ROOT, manifest)],
                check=True, capture_output=True, encoding="utf-8",
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
        cwd=ROOT, check=True, capture_output=True, encoding="utf-8",
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


def composer_graph():
    """(name, version) -> licence for every package the vendored tree's composer installed."""
    data = json.load(open(COMPOSER_INSTALLED, encoding="utf-8"))
    packages = data["packages"] if isinstance(data, dict) else data
    return {
        (p["name"], p["version"].lstrip("v")): " OR ".join(p.get("license") or ["(none declared)"])
        for p in packages
    }


def section(prefix, heading_re, licence_col=2):
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
            rows[(cells[0], cells[1])] = cells[licence_col] if len(cells) > licence_col else ""
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
    windows = rust_graph(TARGETS_WINDOWS)
    npm = npm_graph()
    problems = compare("rust", rust, *section("## Rust crates", RUST_HEADING), rust)
    problems += compare(
        "rust (windows)", windows, *section("## Windows Rust crates", WINDOWS_HEADING), windows
    )
    problems += compare("npm", npm, *section("## npm packages", NPM_HEADING), None)
    composer = composer_graph()
    problems += compare(
        "composer", composer, *section("## Vendored PHP", VENDORED_HEADING, licence_col=3), composer
    )
    if problems:
        print(f"notices-check: THIRD-PARTY-NOTICES.md does not match what ships ({len(problems)}):")
        for p in problems:
            print(f"  {p}")
        return 1
    print(
        f"notices-check: {len(rust)} Rust crates (arm64 + x86_64, app + rex CLI) with rows and "
        f"licences; {len(windows)} for the x86_64 Windows graph; {len(npm)} npm packages with "
        f"rows; {len(composer)} vendored composer packages with rows and licences"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
