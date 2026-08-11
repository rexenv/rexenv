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
npm run check                   # all of them, exits non-zero on any failure
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
| `uireview.js` (`view=provision`) | `/dev/ui-review?view=provision` | The site-provision card at the New Site dialog's own width with the longest real phase label: the domain must stay readable and nothing may leave the card (the shipped bug squeezed the domain to nothing) |
| `watchpanel.js` | `?panel=repo&watch=…` | Scripts row (Watch:/Run: split by the watchy heuristic, disclosure); running watcher (dot, Stop, Watch button disabled, ring-seeded output); exited watcher (`exited (code 1)` + Restart) |
| `linkpanel.js` | `?panel=link` | Link-folder flow: picker (mocked dialog) → path shown + name prefilled → Link → result line + git/header warnings; the unlink-only copy is visible before anything runs |
| `uireview.js` | `/dev/ui-review` | 28 scenarios × 2 widths (Stage 2/3 surfaces + `pills`). ASSERTS since 28 Jul 2026: pageerror/console.error fatal, horizontal overflow fatal, the Adminer-iframe height probe (the §C2 h-full collapse class, <300px = collapsed), the `deleteGate` probe (every delete variant: each destructive button dead on an empty box and on a near-miss, live on the exact domain — it types, so it runs after the shot), and pill metrics (≥92px, one line — the WKWebView "Running"-wrap fix, previously verified once by hand and never committed as a check). Screenshots to `shots-uireview/` stay the human-review artifact; `ONLY=<regex>` narrows |

| `repotoast.js` | `?panel=repo`, `&op=fail` | Every op button REPORTS: a settled Pull toasts once (naming action + asset), the post-attach snapshot re-read does not double it, pending/skipped dependency steps stay silent, and a failed op quotes the first line of git's error and no more |
| `wptoast.js` | `?panel=wp-add&plugins=list`, `&update=fail`, `&install=ok\|partial\|running` | Plugin-list actions report, and report only what is known: Activate/Deactivate once each (verb + name); Delete silent until the dialog is confirmed; Update a count on success but wp-cli's own message on failure; Install announced from the SETTLED job (`partial` keeps its own wording + wp-cli's summary, `running` says nothing). Asserted against the toaster, never a row label. The harness has no event transport, so the install toasts prove the ADOPTION path specifically. Also pins control metrics: every checkbox on the panel is the same size (the Add bar's "Activate" shipped as the browser's own ~12px default beside 18px list boxes) |
| `zipinstall.js` | `?panel=wp-add&install=zip` | The "Upload zip" source: the shared install card labels a zip job's items by FILE NAME (a full `/Users/…/Downloads/…` truncates the line to nothing) and renders NO attempt cursor — wp-cli prints no per-item header on that path, so the number could only sit frozen at "item 1 of N". Plus the tab itself: the button, the "nothing is uploaded anywhere" disclosure, and Install disabled until a file is picked. The premise (wp-cli really prints no header) is proven live in `wp_install_stream_check` job 4, not assumed here |
| `openin.js` | `/dev/ui-review?view=openin` | "Which app opens this": the header split button + the Browser/editor Quick-links tiles. Icons are real decoded PNGs in a 12–20px slot (a broken data URI still yields an `<img>`, so `naturalWidth` is the assertion), the button NAMES the browser the click resolves to, the chevron lists every app and marks the default and closes on a pick — and with one browser installed the chevron does not render AT ALL. Every chevron must carry a non-transparent `border-left` — the seam IS the affordance, and `primary` (Magic Login) draws no border of its own. `icons=none` proves the honest degrade: zero `<img>`, label intact. **Plant-proven**: relaxing the two-app rule, or growing the icon slot, turns it red. WebKit specifically, because the chevron is a SIBLING button — a nested one is invalid HTML and WKWebView drops the inner click |
| `focusrefresh.js` | `?panel=repo` | Freshness wiring, counted rather than seen: a window focus event must re-read `repo_asset_status` + `repo_branches` (a branch changed in a terminal used to sit stale until you left the tab), and must NOT fire the lazy network read `repo_pull_refs`. It reads the `window.__ipcCalls` tally the harness keeps |

**A mock is a fixture, and a fixture can be wrong.** `repo_job_state` answered
ONE canned job for every id, so RepoPanel's post-attach snapshot re-read (real,
correct behaviour: a short job can finish before the listeners are up) replaced
the pull job the click had just started with the ADD job — and `repopanel.js`
went red for months on a UI bug that was never in the UI. A harness that answers
without looking at its arguments is a red check waiting to happen; mock by id.

What these deliberately do NOT cover (backend truth lives in
`src-tauri/examples/repo_*_check.rs`, run those instead): real cloning,
process-group cancel/orphans, git ops against a remote, watcher process
lifecycle, symlink/unlink safety.
