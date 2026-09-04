# PLAN — per-site start/stop, and a site-type filter on the Sites page

Status: **SHIPPED 4 Sep 2026** — T1–T9, ledger #506/#507, live-proven by
`examples/site_stop_start_check.rs` (sandbox tier). Kept as the design record: the
reasoning below is why stopping a site is a serving-surface change, which is the part
the code cannot say for itself. Two user-reported gaps on the Sites
page, planned together because they land on the same screen and the second one is
only worth having once the first exists.

1. Every site is running or none is. There is no way to stop ONE site.
2. The list filters by status only (All / Running / Stopped). With twenty sites,
   "show me my Laravel ones" is a manual scan of the avatar column.

---

## 1. What "stop a site" can honestly mean here

A rexenv site is **not a process** (`docs/ARCHITECTURE.md`, request topology):

```
browser → Caddy :443 → ONE shared Nginx :18088 (vhost by server_name)
        → one php-fpm pool PER PHP VERSION (not per site) → WordPress → DB
```

So the two obvious implementations are both wrong:

- **Stop the pool.** The pool is shared by every site on that PHP minor. This is
  exactly what `restart_site` already refuses to do implicitly (`pool: true` is
  opt-in and the report says how many sites it would touch). Stopping one site
  must never take another site down — that asymmetry is the whole point.
- **Stop nginx / the edge.** That is "Stop all", which already exists in the
  footer and stops everything.

**The ruling: stopping a site removes it from the serving surface, and touches no
shared process.**

| Leg | Stopped site |
|---|---|
| Nginx vhost | no SERVING block — replaced by a **stopped block** (503 + the stop page, no PHP). See §5c: emitting nothing was a bug |
| Caddy route | **kept**, TLS and cert intact, but 503 with rexenv's own stopped PAGE (`core/stopped_page.rs`) instead of `reverse_proxy` |
| Its OWN backend (FrankenPHP / Apache override) | actually stopped — that process serves one site, so stopping it is exact |
| Shared nginx, shared php-fpm pool, DB, edge | untouched |

Why the Caddy route stays rather than disappearing: dropping the route means the
browser gets a TLS failure or a stray match from another block — a "your machine
is broken" screen for a state the user deliberately chose. A 503 that says *this
site is stopped in rexenv* is the honest answer, and it keeps the hostname from
falling through to a wildcard-multisite block that would then answer for it.

Starting is the inverse, plus one thing: if the stack is up but the site's own
PHP minor pool is down, start **starts that pool** (it serves this site; starting
it harms nobody). It never starts the whole stack behind the user's back — if the
edge is down, start records the site as enabled and says plainly that nothing
will answer until the stack is started.

## 2. Where the fact lives

New column, schema **v44**: `sites.enabled INTEGER NOT NULL DEFAULT 1`.

- **Recorded, not in-memory.** Services outlive the app; a site the user stopped
  on Tuesday must still be stopped after a relaunch, and `rebuild_configs_for` is
  reached from `start`, `reload`, `adopt_startup`, site create/edit and the
  scratch reaper. A single recorded fact is the only thing every one of those
  paths can read.
- **User-owned column** (per-column fact ownership): provisioning, config sync,
  valet import and site edits must never write it. The upsert test pins that.
- Pre-v44 rows read as enabled — every site that could exist before this column
  was serving, so `DEFAULT 1` is a fact, not a guess.

## 3. Status must stay one derivation

`site_serving` is the ONE place a site's displayed state comes from, and it grows
one leg: a disabled site is `serving: false` whatever the stack is doing. The
shape also grows `disabled: bool`, so the UI can tell the two stopped-nesses
apart instead of showing one word for both:

- **Stopped** — the stack is down, or this site's upstream is.
- **Stopped by you** — the row says so; starting the stack will not bring it back.

Guard-covers-claimed-surface: every consumer of site status reads this one
function — Sites list, SiteDetail, `mcp__rexenv__site_status`, `rex site`, tray.

## 4. Filters on the Sites page

A second segmented control beside the status one: **All / WordPress / Laravel /
PHP**, using the same `SITE_TYPE_META` letters/colours as the row avatars. The
two filters AND together.

**Counts must not lie.** Each control's counts are computed against the OTHER
control's current selection — i.e. the number on a tab is what clicking it would
actually show. (A count computed over all sites while the list is already
filtered puts "Laravel 3" above an empty list.)

**Default selection**: `Running` when at least one site is running, otherwise
`All`. Decided ONCE, on the first successful sites+serving load, and never again
— a filter that re-decides on every 2s serving poll would yank the list out from
under a user who just chose a tab, and stopping the last running site would jump
them somewhere they did not ask to be. If a filter leaves the list empty, the
empty state says so and offers "Show all sites" rather than silently switching.

## 5. Tasks — all shipped

Each was one commit, verified by `scripts/verify.sh` (the only green verdict). T4 and
T5 landed together because the tree refuses the halves: a registered command with no
caller fails `every_registered_command_is_reachable_from_a_caller`, and an IPC wrapper
with no UI fails `every_ipc_wrapper_is_actually_called`.

- **T1 — the column.** v44 migration + `Site.enabled` in `state/models.rs` and
  `store.rs` (read + upsert). Tests: pre-v44 rows read enabled; a site upsert
  from provisioning/edit never clobbers `enabled`.
- **T2 — config generation.** `rebuild_configs_for` drops disabled sites from the
  nginx vhost list; `SiteRoute { stopped }` renders `respond 503` + honest body
  instead of `reverse_proxy`, keeping `tls` and the marker header. Tests on the
  generated Caddyfile/nginx conf: a disabled site has no upstream anywhere, its
  cert block survives, neighbours are byte-identical.
- **T3 — processes and status.** `reconcile_overrides` stops a disabled site's own
  backend (and never starts one for it); `site_serving` returns
  `serving: false, disabled: true`. Test: disabling a site changes no shared
  service's state.
- **T4 — the command.** `set_site_enabled(id, enabled)` — thin IPC handler over a
  core fn that writes the row, reconciles overrides, rebuilds configs, reloads the
  web tier, and on enable starts the site's PHP minor pool if the stack is up and
  that pool is not. Returns an honest report (`enabled`, `serving`, and why not
  when false). Locking rule: spawn under the services lock, `await_ready` after
  dropping it.
- **T5 — the UI action.** IPC wrapper in `src/lib/ipc`, row-menu `Start site` /
  `Stop site`, the same action in the SiteDetail header, query invalidation, and
  the "stopped by you" pill/badge. Copy must separate this from the footer's
  "Stop all": stopping a site leaves the shared services running.
- **T6 — the type filter.** Second segmented control + the both-ways counts +
  empty-state copy.
- **T7 — the default filter.** One-shot Running/All decision on first load.
- **T8 — parity.** `site_configure` gains an `enabled` action (existing `manage`
  scope, so no new consent surface) and `site_status` reports it; `rex site start
  <domain>` / `rex site stop <domain>`; `docs/CLI-ROADMAP.md` updated.
- **T9 — docs + proof.** `ARCHITECTURE.md` (what stopping a site is and is not),
  `DESIGN.md` (two stopped-nesses, one word each), `CLAIM-LEDGER.md` rows —
  *stopping one site never stops a shared pool* and *a disabled site appears in no
  nginx server block* — with verdicts, plus `MAP.md`, `TESTING.md` and a
  fixture-owned `examples/` live check registered in `scripts/live-checks.sh`
  (sandbox tier): stop a fixture site, prove the neighbour still answers 200 and
  the stopped one answers 503, start it, prove it answers again.

## 5b. The page a stopped site answers (4 Sep 2026, after review)

The first cut was `respond "This site is stopped in rexenv…" 503`. On screen that
is one line of monospace on a white page — indistinguishable from a server that
fell over, which is precisely the reading this feature must not produce: nothing
is broken, and the way back is a button the reader already owns.

It is now a full page in rexenv's clothes (`core/stopped_page.rs`): the brand
mark inlined at build time, the app's palette with a light-theme block, the
site's own hostname, and both ways to start it (the app's menu wording and
`rex site start <domain>`). Two mechanics worth keeping:

- **A file, not a Caddyfile string.** Caddy cannot `respond` with a file, and a
  quoted Caddyfile string treats every `{` in the CSS as a placeholder. The edge
  serves it with `error 503` + `handle_errors { rewrite * /stopped.html;
  file_server }`, which keeps the status — verified against the pinned Caddy
  build before the code was written.
- **One file for every stopped site.** The hostname is the only per-site fact and
  it comes from `location.hostname`, so nothing is rewritten on a rename, and no
  site's name is put in a path on disk for no gain. The file is rewritten on
  every config rebuild rather than only when missing, so a wording change in an
  update actually reaches a machine that already has yesterday's copy.

## 5c. The correction: "no block" meant somebody else's site (5 Sep 2026)

Shipped, then found live by the owner the next day: he stopped `ea.test`, shared
it, and after a couple of reloads the public URL served **a different site**.

The cause is the half of the topology the design skipped. A tunnel does not pass
the edge — `cloudflared` proxies to the shared nginx with `--http-host-header
<domain>` — and nginx answers a name it has no server block for from its
**default server**, which is the first block in the file: another site. So
"emit nothing for a stopped site" meant "serve a neighbour's site at that
address", and the tunnel published it to the internet. It is the same
cross-site fallthrough `override_fallthrough_check` measures, and the reason
`tunnels.rs` refuses to share override sites — a refusal whose premise this
feature had quietly recreated.

The fix is a stopped site's own block: `return 503` for every path,
`error_page 503 /stopped.html` (which keeps the status while serving the page),
no `fastcgi_pass` in it at all, and every alias plus the subdomain wildcard on
its `server_name`. One page, two servers — the edge answers it too.

Two things worth keeping from this:

- **An "absent" is not a behaviour.** The question a config change has to answer
  is not "is this site's block gone" but "what does this tier do with a request
  it cannot match".
- **Probe every path that reaches the tier, not only the one you designed.**
  The live check tested the edge, because that is where the feature's 503 was
  written; the tunnel's path — straight to nginx — was never requested, and it
  is now (`site_stop_start_check`, the direct-to-nginx legs).

## 6. Known edges — how each was settled

- **Sharing a stopped site** (Tunnels): **warn, don't refuse** (owner's ruling,
  4 Sep 2026). The link works and publishes the stop page, and there are real
  reasons to want a URL standing before the site is. The warning is derived on
  every report (`core::tunnels::stopped_share_warning`, recomputed in
  `tunnels_status`), because the ordinary sequence is share first and stop the
  site later — a warning decided at start would say the opposite of what the
  link shows. Ledger #509.
- **Scratch sites**: the reaper deletes on expiry regardless of `enabled`; a
  stopped scratch site still expires. No change, but state it.
- **Site create**: new sites are enabled. A site whose provisioning never
  finished is `provisioned: false`, which is a different fact and keeps its own
  badge — a half-provisioned site is not "stopped by you".
- **`restart_site` on a stopped site**: refuses, naming the state — "… is
  stopped, so there is nothing to restart. Start it first." Rebuilding a config
  that deliberately does not serve the site and calling it a restart would be the
  least true word available.
