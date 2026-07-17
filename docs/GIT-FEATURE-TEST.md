# Manual test — Add plugin/theme from Git (phases 1–5)

Everything below is what could NOT be fully self-verified without you — mostly
things that only show up in the **packaged app (WKWebView)**, your SSH keys,
or a real tunnel. Run in the packaged app unless a step says otherwise.

What was already machine-verified (don't re-prove, just spot-check): URL
parsing, ls-remote probe, streamed clone + **cancel leaves zero orphans**
(ps-proven: 3 live processes in the clone's group → empty after cancel,
partial dir removed), composer/npm/build against fixture repos (vendor/ +
build artifacts produced on the bundled PHP + your nvm node), failure
mapping (php>=9 fixture, missing tool), 307 lib tests, and a Playwright
**WebKit** drive of the panel (mocked IPC — layout/steps/error/warning/log
render clean, no overflow). What WebKit-mocked testing can NOT prove: real
event streaming into the packaged webview, real toasts, and anything
involving your keychain/agent — that's this checklist.

Before starting: **restart services once** (`rex restart`) — the dotfile-deny
lands in the generated configs on the next stack start.

---

## 1. Security — dotfile deny (verify yourself; the tunnel case is the risk)

### 1.1 `.git` and `.env` are 404 on ALL THREE servers

Why: pre-existing hole — any existing dotfile in a docroot was served as
static (source/secret disclosure). Cloning repos plants `.git/` (and often
`.env`) into served docroots at scale, and tunnels make docroots public.

For **one nginx site, one Apache site, one FrankenPHP site** (switch a
throwaway site's server in Settings if needed):

```
cd <docroot>/wp-content/plugins && mkdir -p .probe && echo top-secret > .probe/config
echo APP_KEY=oops > <docroot>/.env
curl -sk https://<domain>/wp-content/plugins/.probe/config   # expect 404 page, NEVER "top-secret"
curl -sk https://<domain>/.env                               # expect 404, NEVER "APP_KEY"
curl -sk -o /dev/null -w '%{http_code}\n' https://<domain>/.probe/x.php   # 404 (ordering: deny must beat the .php handler)
```

Expected: 404 (page or status) on all three servers, all three paths.
FrankenPHP is the one I'm least certain about in single-site mode (see §7) —
please hit it explicitly.

### 1.2 `.well-known` still serves

```
mkdir -p <docroot>/.well-known && echo ok > <docroot>/.well-known/ping.txt
curl -sk https://<domain>/.well-known/ping.txt               # expect: ok
```

Why: ACME/app probes live there; the exemption must not have broken.

### 1.3 The tunnel case (the real risk)

On a site with a cloned plugin (after §2): start its tunnel, then from the
public URL:

```
curl -s https://<tunnel-host>/wp-content/plugins/<dir>/.git/config   # expect 404
curl -s https://<tunnel-host>/wp-content/plugins/<dir>/.git/HEAD     # expect 404
```

Why: locally, `.rex → 127.0.0.1` limits exposure; through a tunnel a served
`.git/` is republished to the internet (full source + remote URLs).

Clean up the probes (`.probe/`, `.env`, `ping.txt`) after.

---

## 2. The Git add flow end-to-end in the ACTUAL app UI

Why this group exists: this session's bug class (badge/select/redirect/
confirm/auto-login) hid ONLY in the packaged WKWebView. Every rendering
claim below was verified in Playwright WebKit with mocked IPC — the packaged
app with real IPC + real event streaming is the part only you can see.

Steps (a WP site, Plugins tab):

1. Add bar shows two small tabs: **WordPress.org | From Git**. wp.org flow
   unchanged (search, tags, batch install still work — regression check).
2. **From Git** → paste `https://github.com/WordPress/theme-check` (real
   plugin repo, no build) → **Fetch**.
   - Expect ≤ a few seconds: branch select appears with the default marked
     `(default)`, tags in a group, folder prefilled `theme-check`.
   - The hint line "`owner/repo` means github.com · private repos use your
     own SSH keys/agent" is visible BEFORE anything runs, plus a
     `node vXX` chip (should match your terminal's node).
3. **Add plugin** → job card appears:
   - Steps "Clone repository" → "Detect dependencies" tick green with a
     spinner while running; the log pane opens itself and **streams** git
     output live (not frozen, then all-at-once — that's the point).
   - Log auto-scrolls, no blank pane, no overlapping text (WKWebView!).
   - After detect: `✓ plugin header: Theme Check` in the log.
4. The plugin list behind the panel refreshes — the row shows the **sky
   "git" badge** next to its name.
5. Now a **build** repo: paste `https://github.com/10up/insert-special-characters`
   (wp-scripts plugin: npm + build). After clone+detect, buttons appear:
   `npm install`, `npm run build` — with the disclosure line above them
   (§4). Click install → streamed npm output → ✓; click build → ✓.
   - Verify on disk: `node_modules/` and `build/` exist in the plugin dir.
6. **Activate** button appears only after every step is ✓ (never before,
   never auto). Click → toast, plugin active in the list.
7. "Done — add another" resets the panel.
8. Themes side: same flow on the Themes tab with a block theme repo, e.g.
   `https://github.com/WordPress/twentytwentyfive` → clone → theme appears
   in the grid with the git badge → Activate. (The theme path is
   code-identical to plugins except destination + activate call, but it was
   NOT live-run end-to-end — flagging honestly.)
9. Quit the app while the panel shows a finished job; reopen → panel is
   gone (jobs are per-session; the log file remains: Logs tab → source
   "Git job — <dir>").

## 3. Private repo via YOUR SSH key (I can't test this)

Why: the whole private flow rides your `~/.ssh` + agent; there is no token
storage in rexenv by design.

1. Pick one of your private repos. Paste its **SSH form**
   (`git@github.com:you/private-thing.git`) → Fetch.
   - Expect: branch list appears — this alone proves auth (probe runs over
     your agent; `SSH_AUTH_SOCK` rides the login-shell snapshot).
2. Add → clone succeeds → detection/steps as normal.
3. Negative check: paste the same repo's **https** form → Fetch → expect the
   mapped error, roughly: "asked for credentials — this is a PRIVATE repo
   (or the URL is wrong…) … use the SSH form … `$ gh auth login`" — and it
   must fail in ~1s, not hang (GIT_TERMINAL_PROMPT=0).
4. If you have a host you've never SSH'd to (first contact), Fetch should
   fail with the host-key message and `$ ssh -T git@<host>` as the fix.

## 4. Disclosure/consent copy is visible BEFORE code runs

- The line "These run the repo's own scripts (npm postinstall, composer
  scripts) as your user — same as running them in Terminal. Install only
  code you trust." must be visible **above** the composer/install/build
  buttons, before any of them is clicked (screenshot-verified in WebKit;
  confirm in the packaged app at your window sizes — it must not clip or
  vanish at narrow widths).
- Clone/Fetch carry no such warning — correct: they execute no repo code.
- The log echoes every command it runs (`$ git clone …`, `$ npm install`).

## 5. Failure cases to try (each should be an HONEST mapped error)

| Try | Expected |
|---|---|
| Fetch `https://github.com/acme/definitely-not-real-xyz` | "Repository not found / private → SSH form" class message (GitHub answers missing+private identically over https), in ~1s |
| Fetch `git://old.example.com/x.git` | Parse-time refusal: git:// unencrypted, use https/git@ |
| Fetch a GitHub **archive** link (`…/archive/main.zip`) | "That's an archive, not a repository" |
| Add the same repo twice (same folder) | Refused BEFORE any network: "already exists — pick another folder name" |
| A repo whose **build fails** (e.g. add `https://github.com/10up/insert-special-characters`, then delete `node_modules` dir and click build without install) | Build step flips red, mapped or raw-tail error shown under the step, full output in the log — never a silent spinner, never a stack trace |
| Turn wifi OFF, click an install step | "registry unreachable — you look offline" class message |

## 6. Node-version warning

Clone a repo pinning an old Node — e.g.
`https://github.com/WordPress/gutenberg` at some older tag, or any repo of
yours with `.nvmrc` `18`. After detect: amber box "This repo wants Node 18 —
you have v22.x… `nvm install 18` … Re-detect." It's display-only (buttons
stay enabled). Also try **Re-detect tools** after switching node in a
terminal (`nvm use`): the chip should update — note: Re-detect re-reads your
LOGIN shell init, so it picks up your default nvm alias, not a per-terminal
`nvm use` (expected behavior, not a bug — flag if it confuses).

## 7. Cancel from the UI leaves no orphans (spot-check the machine proof)

Machine-proven at the core layer (ps evidence in the phase-2 report). UI
spot-check: paste `https://github.com/WordPress/gutenberg` (big) → Add →
while "Clone repository" spins, click **Cancel**:

- Step flips to "cancelled", log gets the cancel line.
- `ps -ax | grep -i 'git.*gutenberg'` → nothing.
- The partial `wp-content/plugins/gutenberg` dir is GONE.
- Repeat mid-`npm install` on a build repo: `ps -ax | grep npm` → nothing,
  then click install again → re-runs cleanly (half-done install heals).
- Quit the app mid-install → same ps check (exit hook kills job groups).
  **Force-kill** (`kill -9` the app) is the known gap: children survive
  (nothing can run our exit hook) — they finish or fail harmlessly; noted
  as accepted.

## 8. Tab-return reconnect (added after QA round 1)

Why: the backend job registry survives a tab switch; the panel's state did
not — returning showed a blank panel over a live clone, inviting a dangerous
second run (same class as the mount-frozen DNS tile).

1. Start a big clone (`WordPress/gutenberg`) → while "Clone repository"
   spins, switch to another SiteDetail tab (or another site) and come back
   to WordPress → From Git.
   - Expect: the SAME job card, current step still spinning, log seeded with
     the earlier output (from the job's log file) and streaming live — not a
     blank form. Header line names the target (`gutenberg · <url>`).
2. While it runs, stay on the **WordPress.org** tab: the "From Git" tab
   label carries a small spinner (the job is visible without entering the
   panel).
3. Try to Add the same repo/folder again while it runs (fresh URL → Fetch →
   Add): refused with "a job for <dir> is already running — reconnect…".
4. Cancel from the reconnected card → step flips cancelled, partial dir
   cleaned (same §7 guarantees).

---

## Flagged: what I could NOT self-verify / am least sure about

1. **FrankenPHP single-site dotfile guard ordering** — the multisite variant
   places the guard inside `route {}` (literal order, safe); single-site
   relies on Caddy's canonical directive order (`respond` before
   `php_server`). Reasoned from Caddy docs, not runtime-proven → §1.1 on a
   FrankenPHP site is the proof.
2. **Real event streaming in WKWebView** — line events + state snapshots
   were only proven with mocked IPC in Playwright WebKit. If the packaged
   log pane freezes or batches oddly during a big npm install, that's the
   spot to look (event rate; we emit per line, no backend coalescing).
3. **Theme path end-to-end** (§2.8) — code-identical to plugins, not
   live-run.
4. **Multisite sites**: Activate uses the single-site call; on a multisite
   the correct action may be network-enable (themes) — expect rough edges,
   listed as follow-up, not v1.
5. **repo_tools first paint**: the first "From Git" open triggers the
   login-shell snapshot (~0.5–2s, your rc files) — chips may pop in late.
   If your shell prints on init, worst case is a failed detect → Re-detect.
6. **Log file size**: unbounded within one job (a huge build could write
   MBs; file truncates on the next job for that dir). Accepted for v1.
7. **`.probe` cleanup**: §1's probes are manual — remember to remove them.
