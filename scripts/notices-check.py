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

Dev-dependencies are excluded: they never ship. Three Rust graphs, one walk: the
universal macOS one, the x86_64 Windows one, and (since 25 Sep 2026) the Linux one
for both archs — each with its own section, because each links a different closure.

Usage: scripts/notices-check.py   (exit 0 = matches, 1 = drift)
"""
import json
import os
import re
import shutil
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
# The Linux app links a third closure (gtk/webkit2gtk -sys crates, xz2/lzma-sys for MySQL's
# .tar.xz; no objc2, no windows-*). BOTH archs, as the macOS graph: every Linux upstream
# publishes aarch64 (docs/PLAN-linux-port.md D-L5). Resolvable offline on the Mac — the lock
# file pins the crates and the registry cache has them (measured 25 Sep 2026).
TARGETS_LINUX = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
RUST_HEADING = re.compile(r"^## Rust crates \(statically linked; (\d+) external crates, universal macOS graph\)$")
# NOTE the heading may not begin with "## Rust crates": `section()` matches by startswith, so a
# "## Rust crates — Windows" heading would make the macOS section ambiguous and exit.
WINDOWS_HEADING = re.compile(r"^## Windows Rust crates \(statically linked; (\d+) external crates, x86_64 Windows graph\)$")
LINUX_HEADING = re.compile(r"^## Linux Rust crates \(statically linked; (\d+) external crates, x86_64 \+ aarch64 Linux graph\)$")
NPM_HEADING = re.compile(r"^## npm packages \(production closure bundled by Vite; (\d+) packages\)$")
VENDORED_HEADING = re.compile(r"^## Vendored PHP \(compiled into the app binary; (\d+) packages\)$")
COMPOSER_INSTALLED = os.path.join(ROOT, "src-tauri/resources/wp-dist-archive/vendor/composer/installed.json")


class GraphUnresolvable(Exception):
    """This HOST cannot resolve that target's graph offline.

    `cargo metadata --filter-platform <t> --offline` needs every crate the target pulls in to be
    in the local registry cache. A Mac has both (cargo-xwin fetches the windows-* families), a
    Windows box has only its own — so the macOS graph is unresolvable there, with `--offline`
    doing exactly what it is for. Measured 21 Sep 2026, the first time `verify.sh` ran on
    Windows: the check died with a raw traceback, which reads like a broken script rather than a
    host that cannot answer.
    """


def rust_graph(targets=None):
    """(name, version) -> licence for every registry crate reachable without dev edges.

    `targets` so the Windows graph is read by the SAME walk as the macOS one — two graphs built
    by two rules is how this file drifted before.

    Raises [`GraphUnresolvable`] when this host has no cached crates for a target.
    """
    out = {}
    for manifest in MANIFESTS:
        for target in targets or TARGETS:
            args = ["cargo", "metadata", "--format-version", "1", "--locked", "--offline",
                    "--filter-platform", target, "--manifest-path", os.path.join(ROOT, manifest)]
            done = subprocess.run(args, capture_output=True, encoding="utf-8")
            if done.returncode != 0 and os.environ.get("CI"):
                # A fresh CI runner has no registry cache for the OTHER targets' crates (a macOS
                # build never fetched windows-*, a Linux one never fetched objc2), so `--offline`
                # fails for every graph and the gate went red on all four release runners the
                # first time it ran there (v0.8.8, 27 Sep 2026). CI has the network by definition:
                # fetch that target's lockfile closure once (no build), then read offline as
                # before. A developer's machine keeps the offline-only rule — a gate that quietly
                # reaches for the network is not the same gate on a plane.
                # EVERY platform's crates, not `--target`'s: `cargo metadata --offline` resolves
                # the whole lockfile before it filters, so a fetch scoped to one target still
                # left the read red ("attempting to make an HTTP request") on ubuntu-22.04 for
                # the Windows graph, 27 Sep 2026.
                fetched = subprocess.run(["cargo", "fetch", "--locked",
                                          "--manifest-path", os.path.join(ROOT, manifest)],
                                         capture_output=True, encoding="utf-8")
                if fetched.returncode != 0:
                    print(f"notices-check: cargo fetch failed: {fetched.stderr.strip().splitlines()[-1] if fetched.stderr.strip() else 'no output'}")
                done = subprocess.run(args, capture_output=True, encoding="utf-8")
            if done.returncode != 0:
                raise GraphUnresolvable(f"{target} ({manifest}): {done.stderr.strip().splitlines()[-1] if done.stderr.strip() else 'cargo metadata failed'}")
            raw = done.stdout
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


def tool(name):
    """The executable, resolved the way THIS OS names it.

    On Windows `pnpm` is `pnpm.cmd` — a batch shim — and Python's CreateProcess does not try
    extensions, so a bare `"pnpm"` dies with `WinError 2: The system cannot find the file
    specified` even with pnpm on PATH. Measured 21 Sep 2026, the first Windows run of the bar.
    `shutil.which` applies PATHEXT, so it finds the shim on Windows and the plain binary
    everywhere else.
    """
    found = shutil.which(name)
    if not found:
        sys.exit(f"notices-check: {name} is not on PATH — install it and run the bar again")
    return found


def npm_graph():
    raw = subprocess.run(
        [tool("pnpm"), "list", "--prod", "--depth", "Infinity", "--json"],
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
    # A host checks the graphs it can RESOLVE, and says which it could not: the macOS job
    # checks both (a Mac caches the windows-* families through cargo-xwin), the Windows job
    # checks its own. Skipping is stated, never silent — and a host that can resolve NEITHER
    # is a broken checkout, not a platform difference.
    skipped = []
    try:
        rust = rust_graph()
    except GraphUnresolvable as e:
        rust, _ = None, skipped.append(f"macOS graph — {e}")
    try:
        windows = rust_graph(TARGETS_WINDOWS)
    except GraphUnresolvable as e:
        windows, _ = None, skipped.append(f"Windows graph — {e}")
    try:
        linux = rust_graph(TARGETS_LINUX)
    except GraphUnresolvable as e:
        linux, _ = None, skipped.append(f"Linux graph — {e}")
    if rust is None and windows is None:
        print("notices-check: neither Rust graph could be resolved on this host:")
        for s in skipped:
            print(f"  {s}")
        return 1
    npm = npm_graph()
    problems = []
    if rust is not None:
        problems += compare("rust", rust, *section("## Rust crates", RUST_HEADING), rust)
    if windows is not None:
        problems += compare(
            "rust (windows)", windows, *section("## Windows Rust crates", WINDOWS_HEADING), windows
        )
    if linux is not None:
        problems += compare(
            "rust (linux)", linux, *section("## Linux Rust crates", LINUX_HEADING), linux
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
    mac_says = f"{len(rust)} Rust crates (arm64 + x86_64, app + rex CLI)" if rust is not None else "macOS graph SKIPPED"
    win_says = f"{len(windows)} for the x86_64 Windows graph" if windows is not None else "Windows graph SKIPPED"
    linux_says = f"{len(linux)} for the Linux graph" if linux is not None else "Linux graph SKIPPED"
    print(
        f"notices-check: {mac_says} with rows and licences; {win_says}; {linux_says}; {len(npm)} npm packages "
        f"with rows; {len(composer)} vendored composer packages with rows and licences"
    )
    for s in skipped:
        print(f"  skipped on this host: {s} — the job on that OS checks it")
    return 0


if __name__ == "__main__":
    sys.exit(main())
