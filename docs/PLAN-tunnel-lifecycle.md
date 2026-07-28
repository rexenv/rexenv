# PLAN — Tunnel lifecycle decision (AWAITING RULING)

Status: **proposed, not ruled on**. Nothing below is implemented. Follow-up fixes
(status honesty, override-site block, double-start race, Bedrock paths, leftovers,
cross-guards) are sequenced separately and deliberately NOT planned here.

## The problem being decided

A tunnel child (cloudflared) currently inherits **neither** lifecycle model:

- Not a repo job: `RunEvent::Exit` (`lib.rs:717`) kills repo jobs only — a quit
  leaves cloudflared running, publicly serving the site.
- Not a service: adoption (`service_manager::adopt_startup`) is port-based
  (`owned_master(port, marker)`), and cloudflared binds no local port — it is
  structurally unadoptable as-is.

Result: after a quit or crash the tunnel is invisible (registry is in-memory),
unstoppable from the UI, and still publicly live; the mu-plugin in the docroot
holds a live origin while `wp_tunnel.rs:21` claims a leftover copy is inert.

## Options

### (a) Dies with the app — kill on `RunEvent::Exit`, sweep orphans at next launch

A tunnel is a session ("while I'm working"), not infrastructure. Quit = stop
sharing, remove the mu-plugin. A crash is repaired at the next launch by a sweep
over recorded pids (below).

- **Pro:** matches the actual meaning of a quick tunnel (attended, ephemeral,
  URL is random per start anyway — nothing durable is lost); removes the
  invisible-public-URL security hole; makes the crash-path safety claim true
  again (after restating); simplest identification story (we hold the `Child`).
- **Con:** quitting kills a share someone may be relying on, silently. (Mitigable
  later with a quit-time warning when shares are active — see Add-ons.)
- **Con:** `RunEvent::Exit` only fires on an orderly quit. Crash, force-quit,
  SIGKILL, and plain SIGTERM (no handler installed) all bypass it → the crash
  story below is REQUIRED, not optional, for (a) to be honest.

### (b) Outlives and is adopted — recorded pid + positive cmdline identification

Feasible: cloudflared's argv happens to carry BOTH halves of the established
ownership doctrine ("our fixed port + app-data marker on the cmdline") — the
binary path lives under app-data (`paths().bin_dir()`) and the args include
`--url http://127.0.0.1:18088 --http-host-header <domain>`. The public URL is
recoverable from the persisted `tunnel-<domain>.log`. So adoption CAN be built
without a port.

- **Pro:** shares survive restarts; consistent with services-outlive-the-app.
- **Con:** wrong semantics — a service outliving the app serves the *developer*;
  a tunnel outliving the app serves the *public*, unattended. An invisible
  public URL surviving quit is a liability, not a feature.
- **Con:** real work: a new non-port identification path, adopted-state handling
  (miss tolerance à la B29), re-parsing the URL from a log we'd now have to
  trust across sessions — and it lands on top of a status layer that already
  can't tell live from dead (finding #2). Adoption would widen the honesty gap
  before step 2 narrows it.
- **Con:** quick tunnels are ephemeral by design (no SLA, random URL). If
  persistent shares ever become a feature, the right vehicle is cloudflared
  *named* tunnels (credentialed, stable hostname) — a product decision, not a
  lifecycle patch.

### (c) Outlives, not adopted — discovered at launch and offered for stopping

Same recorded-pid + positive-ID machinery as (a)'s sweep, plus consent UI, and
the exposure persists until the user notices and answers a prompt.

- **Pro:** never kills anything without consent.
- **Con:** strictly more work than (a) for a worse security posture; the prompt
  is answerable only AFTER the exposure window; prompt fatigue.

## Recommendation: (a), with the launch sweep as the mandatory crash story

Agreed with the instinct in the ruling request, and with one sharpening: **(a)
without the sweep would be dishonest** — it fixes the quit story only. The full
shape is "dies with the app; a crash is repaired at the next launch."

Direct answer to the question asked: **yes, (a) leaves a crash window.** The
exit hook never fires on crash/force-quit/SIGKILL/SIGTERM. The sweep bounds that
window at "until rexenv next launches." Between a crash and the next launch the
tunnel stays live and unattended; that residual gap is closable only by a
parent-death watcher process (macOS has no `PR_SET_PDEATHSIG`; it would take a
small helper using kqueue `EVFILT_PROC`/`NOTE_EXIT` on the app pid). Proposed:
accept the bounded gap now, note the watcher as future hardening (Add-ons).

## Design (scoped to step 1 only)

### 1. Spawn-time record (SQLite — new migration)

New table `tunnels(domain TEXT PRIMARY KEY, pid INTEGER NOT NULL, docroot TEXT
NOT NULL, started_at TEXT NOT NULL)`. Row written **immediately after spawn,
before the URL poll**; deleted on clean stop, on every start-failure kill path,
and by the sweep.

Recording at spawn (not at registry insert) is deliberate: it also covers the
"app quits during the 30s URL poll" window, where a child exists but the
registry doesn't know it yet. (Same structural move later closes the
double-start race — finding #3 — by making in-flight starts visible; that fix
stays in step 4, but the record is designed so step 4 can reuse it.)

### 2. Quit path — `RunEvent::Exit`

Mirror `repo::cancel_all_on_exit` (`repo.rs:1765`): a synchronous
`tunnels::kill_all_on_exit(app)` that

- kills every recorded tunnel (DB rows, not just registry — covers in-flight
  starts): supervisor `stop` (SIGTERM → bounded grace → SIGKILL). cloudflared
  exits promptly on SIGTERM (graceful tunnel close), so the grace window is
  rarely consumed; exit latency stays sub-second in the normal case.
- removes each row's mu-plugin via `wp_tunnel::disable(docroot)` (best-effort,
  logged) — quit must not leave a live-origin file in a possibly-linked repo.
- clears the table.

No `.process_group(0)` needed: cloudflared is a single process and spawns no
worker tree; the recorded pid + held `Child` suffice. `spawn_logged` unchanged.

### 3. Crash story — launch-time sweep

At startup (alongside `adopt_startup`), read all `tunnels` rows. Per row:

- **Positive identification, never a bare pid** (§5 doctrine): the pid's argv
  must contain BOTH the app-data cloudflared binary path AND
  `--http-host-header <domain>`. Pid recycling therefore cannot cause a wrong
  kill — a recycled pid fails the argv match and only the row + file are
  cleaned. (Small platform seam: an argv probe on the supervisor/proc layer;
  `core/proc.rs` is the likely home — exact seam settled at implementation.)
- Identified live → supervisor `stop` (TERM → KILL, bounded), then
  `wp_tunnel::disable(row.docroot)` best-effort, delete row.
- Not identified (exited, or pid recycled) → `disable` + delete row only.

The sweep uses the row's recorded docroot, not a site lookup — a site deleted
or renamed between crash and relaunch must still get its file removed.

This also self-heals the crash-leftover `rexenv-tunnel.php` case from the
review's Group 2. (The never-removed `rexenv-login.php` and the delete/rename
leftovers remain step 6 — different code paths, not touched here.)

### 4. Restate the `wp_tunnel.rs:21` claim

Current claim ("a stale copy (crash) is inert — its dead tunnel receives no
requests") is false today and stays partially false even after (a): between a
crash and the next launch the tunnel is alive and the file is *active* — which
during that window is correct behavior, not a hazard; the hazard is the
unattended exposure, which the sweep ends. Draft replacement:

> The file is removed on tunnel stop and at app quit (tunnels die with the
> app); a launch-time sweep kills any tunnel a crash left behind and removes
> this file with it. Between a crash and the next launch the tunnel may still
> be live — the file is then still doing its job for that live tunnel; the
> sweep ends both together. A copy orphaned some other way is inert: its
> tunnel is gone, and tunnel-marked requests can no longer arrive.

Final wording at implementation; the invariant it must state: **file lifetime
is bounded by tunnel lifetime plus at most one app relaunch.**

### Verification sketch ("done when")

- `cargo test --lib`: table migration; sweep decision matrix (identified-live /
  dead / recycled-pid → kill+clean / clean / clean-only); argv-match unit test.
- Live checks (`examples/*`, fixture-owned per `examples/common/mod.rs`):
  spawn a fixture tunnel → orderly exit path kills it and removes the
  mu-plugin; simulate crash (kill -9 the app-side fixture, keep child) →
  relaunch sweep kills the child, removes file, clears row. `Reaped` guards on
  every spawned child.
- `cargo build --examples` before commit (standing rule).

## Add-ons (explicitly deferred, listed for the ruling)

1. **Quit-time warning** when shares are active (`WindowEvent::CloseRequested`:
   "Quitting stops N public share(s)") — addresses (a)'s one real cost. Cheap;
   recommend as a later UI nicety after step 2.
2. **Parent-death watcher helper** (kqueue NOTE_EXIT) to close the residual
   crash gap entirely. Not recommended now: a new long-lived helper process to
   get right vs. a rare window already bounded by next launch.
3. **Named-tunnel persistence** as a future feature if quit-surviving shares
   are ever actually wanted — the honest home for option (b)'s use case.

## Out of scope here (sequenced separately per the ruling request)

Status honesty (`try_wait` + the URL-probe question) · override-site refusal ·
double-start race + toggle UI · Bedrock content-dir resolution (and the wider
hardcoded-`wp-content` audit) · Group-2 leftover removal on delete/rename +
`rexenv-login.php` ownership · tunnel↔job cross-guards · disclosure copy for
linked-docroot exposure deltas (symlinks followed; stray dev PHP publicly
executable).
