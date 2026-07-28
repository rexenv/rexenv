# wk-checks — WebKit regression checks for the add-from-Git UI

**Dev/test tooling only. Nothing here ships.**

Why these exist: this project's UI bug class hides specifically in
WKWebView (badge overlap, select rendering, redirect handling, confirm
dialogs, streaming panes) — Chrome-side dev never shows it. Playwright's
WebKit is the same engine family as the packaged app's webview, and these
scripts are the only automated way to catch that class. Re-run them after
any change to the Git/asset panels.

They drive the **dev-only harness route** `/dev/git-panel`
(`src/routes/DevGitPanel.tsx`): it mocks the Tauri IPC layer (`mockIPC`)
with canned `repo_*` responses, so every visual state renders in a plain
browser with **zero backend and zero contact with the real app** (the
no-synthetic-clicks-on-the-live-machine rule). The route is mounted only
when `import.meta.env.DEV` and is tree-shaken out of production bundles.

## One-time setup

```
cd scripts/wk-checks
npm install                     # playwright (local to this dir)
npx playwright install webkit   # the WebKit browser build
```

## Run

```
# terminal 1 — vite dev server on the port the checks expect:
npx vite --port 5199 --strictPort

# terminal 2:
cd scripts/wk-checks
npm run check                   # all six, exits non-zero on any failure
node repopanel.js               # or any single scenario
```

`WK_BASE_URL` overrides `http://localhost:5199` if you run vite elsewhere.
Each script drops full-page `shot-*.png` screenshots beside itself
(gitignored) — eyeball them when something fails.

## Scenarios

| Script | Harness URL | Covers |
|---|---|---|
| `check.js` | `/dev/git-panel` | GitAddPanel interactive flow: URL input + hints + tool chips → Fetch → branch select (default marked, tags grouped) + folder prefill → Add → job card (step glyphs, mapped error box with the `$`-fix line, node-version warning, disclosure line, log toggle), no horizontal overflow |
| `rehydrate.js` | `?rehydrate=1` | Tab-return reconnect: a live backend job is adopted with ZERO clicks — header line, running spinner, Cancel, log seeded from the tail; blank mode still starts blank |
| `repopanel.js` | `?panel=repo` | RepoPanel: status chips (branch / dirty counts / ↑↓ vs upstream / source), remote line, last-job log toggle; ops row (Fetch/Pull/Push/Checkout + branch dropdown) and Pull → op-job card with the deps-changed install offer + disclosure |
| `watchpanel.js` | `?panel=repo&watch=…` | Scripts row (Watch:/Run: split by the watchy heuristic, disclosure); running watcher (dot, Stop, Watch button disabled, ring-seeded output); exited watcher (`exited (code 1)` + Restart) |
| `linkpanel.js` | `?panel=link` | Link-folder flow: picker (mocked dialog) → path shown + name prefilled → Link → result line + git/header warnings; the unlink-only copy is visible before anything runs |
| `uireview.js` | `/dev/ui-review` | 28 scenarios × 2 widths (Stage 2/3 surfaces + `pills`). ASSERTS since 28 Jul 2026: pageerror/console.error fatal, horizontal overflow fatal, the Adminer-iframe height probe (the §C2 h-full collapse class, <300px = collapsed), and pill metrics (≥92px, one line — the WKWebView "Running"-wrap fix, previously verified once by hand and never committed as a check). Screenshots to `shots-uireview/` stay the human-review artifact; `ONLY=<regex>` narrows |

What these deliberately do NOT cover (backend truth lives in
`src-tauri/examples/repo_*_check.rs`, run those instead): real cloning,
process-group cancel/orphans, git ops against a remote, watcher process
lifecycle, symlink/unlink safety.
