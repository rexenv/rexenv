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
| `check.js` | `/dev/git-panel` | GitAddPanel interactive flow: URL input + hints + tool chips → Fetch → **searchable** ref picker (the full 92-branch remote list, default marked, tags grouped, filter narrows to one, picking sets the ref) + folder prefill → Add → job card (step glyphs, mapped error box with the `$`-fix line, node-version warning, disclosure line, log toggle), no horizontal overflow. **Plant-proven**: putting the old native `<select>` back fails by name — and the fixture carries ~92 branches for the same reason, because a three-branch fixture is what made the select look fine |
| `rehydrate.js` | `?rehydrate=1` | Tab-return reconnect: a live backend job is adopted with ZERO clicks — header line, running spinner, Cancel, log seeded from the tail; blank mode still starts blank |
| `repopanel.js` | `?panel=repo` (+ `clean=1`, `stashes=none`) | RepoPanel: status chips (branch / dirty counts / ↑↓ vs upstream / stash count / source), remote line, last-job log toggle; ops row (Fetch/Pull/Push/Checkout + branch dropdown) and Pull → op-job card with the deps-changed install offer + disclosure. **Working-tree row**: Reset is GATED — no op reaches the backend until the confirm is accepted, and the dialog must say both what cannot be recovered and what it does NOT delete; Restore sends the SELECTED entry (`stash@{1}`, not a hardcoded `stash@{0}` — git renumbers the list on every pop) and the picker shows each entry's message. `clean=1&stashes=none` proves the shut doors: Stash/Reset/Restore disabled, no stash chip, Status still live. **Plant-proven**: dropping the confirm, or hardcoding the stash ref, each turns it red by name |
| `uireview.js` (`view=provision`) | `/dev/ui-review?view=provision` | The site-provision card at the New Site dialog's own width with the longest real phase label: the domain must stay readable and nothing may leave the card (the shipped bug squeezed the domain to nothing) |
| `watchpanel.js` | `?panel=repo&watch=…` | Scripts row (Watch:/Run: split by the watchy heuristic, disclosure); running watcher (dot, Stop, Watch button disabled, ring-seeded output); exited watcher (`exited (code 1)` + Restart) |
| `linkpanel.js` | `?panel=link` | Link-folder flow: picker (mocked dialog) → path shown + name prefilled → Link → result line + git/header warnings; the unlink-only copy is visible before anything runs |
| `uireview.js` | `/dev/ui-review` | 28 scenarios × 2 widths (Stage 2/3 surfaces + `pills`). ASSERTS since 28 Jul 2026: pageerror/console.error fatal, horizontal overflow fatal, the Adminer-iframe height probe (the §C2 h-full collapse class, <300px = collapsed), the `deleteGate` probe (every delete variant: each destructive button dead on an empty box and on a near-miss, live on the exact domain — it types, so it runs after the shot), and pill metrics (≥92px, one line — the WKWebView "Running"-wrap fix, previously verified once by hand and never committed as a check). Screenshots to `shots-uireview/` stay the human-review artifact; `ONLY=<regex>` narrows |

| `repotoast.js` | `?panel=repo`, `&op=fail` | Every op button REPORTS: a settled Pull toasts once (naming action + asset), the post-attach snapshot re-read does not double it, pending/skipped dependency steps stay silent, and a failed op quotes the first line of git's error and no more |
| `wptoast.js` | `?panel=wp-add&plugins=list`, `&update=fail`, `&install=ok\|partial\|running` | Plugin-list actions report, and report only what is known: Activate/Deactivate once each (verb + name); Delete silent until the dialog is confirmed; Update a count on success but wp-cli's own message on failure; Install announced from the SETTLED job (`partial` keeps its own wording + wp-cli's summary, `running` says nothing). Asserted against the toaster, never a row label. The harness has no event transport, so the install toasts prove the ADOPTION path specifically. Also pins control metrics: every checkbox on the panel is the same size (the Add bar's "Activate" shipped as the browser's own ~12px default beside 18px list boxes) |
| `zipinstall.js` | `?panel=wp-add&install=zip` | The "Upload zip" source: the shared install card labels a zip job's items by FILE NAME (a full `/Users/…/Downloads/…` truncates the line to nothing) and renders NO attempt cursor — wp-cli prints no per-item header on that path, so the number could only sit frozen at "item 1 of N". Plus the tab itself: the button, the "nothing is uploaded anywhere" disclosure, and Install disabled until a file is picked. The premise (wp-cli really prints no header) is proven live in `wp_install_stream_check` job 4, not assumed here |
| `openin.js` | `/dev/ui-review?view=openin` | "Which app opens this": the header split button + the Browser/editor Quick-links tiles. Icons are real decoded PNGs in a 12–20px slot (a broken data URI still yields an `<img>`, so `naturalWidth` is the assertion), the button NAMES the browser the click resolves to, the chevron lists every app and marks the default and closes on a pick — and with one browser installed the chevron does not render AT ALL. Every chevron must carry a non-transparent `border-left` — the seam IS the affordance, and `primary` (Magic Login) draws no border of its own. `icons=none` proves the honest degrade: zero `<img>`, label intact. Each row's SECOND target (open it in that browser's private window) exists only for browsers that can really open one — present on Chrome/Firefox, absent on Safari — carries the same seam, and its click sends `open_in_browser private:true` while the row beside it sends `private:false` (the mock records the calls; the two differ by one argument and nothing else is visible from outside). No nested `<button>` anywhere in the document. **Plant-proven**: relaxing the two-app rule, growing the icon slot, routing the private icon to the ordinary open, or deleting the row divider each turn it red. WebKit specifically, because the chevron and the private target are SIBLING buttons — a nested one is invalid HTML and WKWebView drops the inner click |
| `wpsearch.js` | `?panel=wp-add&plugins=list` | The installed-plugin filter searches **what the row shows**. Reported with a screenshot: typing "loopback" against a list plainly reading "rexenv loopback DNS" answered "No plugins match" — the box matched the SLUG (`rexenv-dns`) while every row is labelled with its TITLE, which from the outside looks like a broken filter. Asserts a title-only word (`yoast` → `wordpress-seo`), a slug-only word (`wordpress` → the same row), a word in both, a matchless query (the empty state must still be reachable) and that clearing restores every row. **Plant-proven both directions**: matching only `name` fails the title case, matching only `title` fails the slug case — the second plant passed the first version of this check, which is why the mirror assertion exists |
| `wpinstallcard.js` | `?panel=wp-add&install=ok\|partial\|cancelled\|running\|blocked` | The install card's LIFECYCLE, and the asymmetry is the feature: a SUCCESSFUL install clears its card after 3s (the toast already said it, the list below now shows it), every other outcome STAYS with an × to dismiss, because a failure's card holds the only copy of the reason. Also the two traps that make an auto-hide worse than none — opening the log HOLDS the timer (three seconds is exactly long enough to click "Show log" and lose it), and a RUNNING job offers Cancel but never Dismiss. Drives **both panels** — plugins through this harness, themes through `/dev/ui-review?view=themes&install=…` with a job whose `kind` really is "theme". They share a hook and a component, which is exactly why the second is checked: a shared implementation still needs both call sites to pass the props. Also the BLOCKED zip (`install=blocked`): a re-uploaded zip of something already installed fails with wp-cli's unactionable `No plugins installed.`, so the card must name the folder from the log line that carries it and offer "Replace with the uploaded zip" — and the check reads what that button SENDS (`force: true`, same source, same zip paths), because a control that re-ran the refused command would look identical. **Plant-proven four times**: clearing on every settle fails on `partial`/`cancelled`; ignoring the hold fails the log-open leg; sending `force: false` fails the blocked leg; dropping the props from the THEMES call site alone fails `themes partial` (and not `themes ok` — the linger lives in the hook, the dismiss in the wiring, and the two legs tell them apart) |
| `wpgitchip.js` | `?panel=wp-add&plugins=list&git=1` | The two git chips on a plugin row — "git" (a real checkout) and the dashed "git?" (a directory that looks like one) — RENDER, in **both themes**. They had no fixture anywhere (`repo_assets`/`repo_unmanaged` answered `[]` in every harness), so their colours were changed in the #373 token sweep with nothing rendering them. Reads COMPUTED styles rather than classes, because the failure this catches is Tailwind emitting NO rule for a key missing from the config: the token exists in `tokens.css`, the L0 guard passes, and the element quietly has no background at all. **Plant-proven**: deleting `accent-blue-bg` from `tailwind.config.js` fails it in both themes — and the L0 token-exists guard stays green through that same plant, which is the reason this check exists at L2 |
| `uireview.js` (`view=themes`) | `/dev/ui-review?view=themes` | The themes grid labels each card with the theme's own NAME (wp-admin's label) while keeping the slug on the card — it is the directory name and the argument every theme command takes. The fixture's third theme has NO header name, so the fallback-to-slug path renders too. **Plant-proven twice**, one per half: labelling by slug again fails on the names, dropping the slug from the meta line fails on the slugs |
| `uireview.js` (`view=tunnels`) | `/dev/ui-review?view=tunnels` | The Tunnels search box, which is the only filter in this app that can hide a LIVE PUBLIC URL. Four scenarios: unfiltered (nothing amber), a query matching only an unshared site (both live cards gone → the warning names the count AND "still public until you stop sharing"), a pasted tunnel-URL fragment (resolves to exactly its site, the other shared one announced as hidden), and a matchless query (empty state AND the warning, never a blank page). The fixture runs TWO live tunnels so the plural copy is exercised — with one, a probe passes on a count of 1 where the copy has to read "2 shared sites are". **Plant-proven**: deleting the warning's call sites reddens three of the four while the unfiltered one stays green |
| `mail.js` | `/dev/ui-review?view=mail` (+ `stale=1`) | The Mail screen's unread handling. The list polls every 5s, so anything that only becomes true "on the next refetch" works somewhere between instantly and five seconds later — which is how the read flip was reported ("clicking the subject marks it read, clicking the sender doesn't"; it was the poll phase, not the click target). Asserts: opening a message flips its row with **no list refetch in between**; the Unread filter is a real backend search (`unreadOnly` on the wire), not a display trick over the fetched page; the message being READ stays listed under that filter across a full poll instead of vanishing with its own preview; Mark all read updates the screen from its own patch (`stale=1` makes the server-side mark a no-op, so only the patch can flip it) and then disables itself. **Plant-proven ×3**: dropping the read patch, filtering client-side, or un-pinning the open message each fail by name |
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
