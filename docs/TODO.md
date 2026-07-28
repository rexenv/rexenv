# TODO — the single active-work file

Everything open lives here. Completed phases + audit history: `docs/archive/`
(historical — don't trust as current). When you finish an item, tick it here with a
one-line ✓ evidence note (same convention as the archived TASKS files).

## Actionable now

- [ ] **Testing strategy — proofs at the right level (plan RULED 28 Jul 2026:
  `docs/PLAN-testing-strategy.md`; claim inventory: `docs/CLAIM-LEDGER.md`, 194 claims,
  117 proven at compile time)**. Metric = ledger tally, never line coverage. Tasks
  (T-numbers from the plan):
  - [x] **T1. Plan + ledger written** ✓ this entry's two docs.
  - [x] **T2. Wrong assertions fixed/deleted** ✓ 28 Jul 2026 — the four `len() >`
    byte-count checks now assert structure (second sentence / names the actionable
    datum); terminal.rs unfailable PATH check → exact equality both branches; macos
    RunAtLoad split assert → contiguous; mail bounds echo test deleted (wedged-server
    test is the behavioral proof); `len() >= 2` const tautology dropped. verify.sh
    green, 534 tests.
  - [x] **T3. Lying names renamed; needle tests reduced** ✓ 28 Jul 2026 — 16
    `manifest_resolves_*` → `manifest_pins_*` (a HashMap lookup is not a fetch);
    dbcompat's two "imports cleanly"/"land without drama" → verdict-level names;
    `…parse_live_…` → `…parse_captured_…` ×2; wp_tunnel/wp_login needle tests reduced
    to tripwires that NAME their behavioral example, vacuous needles (".test",
    "'exp'") dropped. verify.sh green.
  - [x] **T4. `db_dump_flags_check` closes the mysqldump incident class** ✓ 28 Jul
    2026 — feeds the EXACT production argv (`dbdump::dump_tool_flags` extracted pub,
    `client_base_args` made pub — one source of truth, no drift) to all 4 cached dump
    tools + both clients. PASS. Its first run FALSIFIED the "every dump tool
    hard-errors on --connect-timeout" comment: mariadb-dump warns-and-ignores (exit 0);
    both prose sites corrected, ledger #195 added. verify.sh green.
  - [ ] T5. `frankenphp_subdir_validate` actually validates (real binary, asserts).
  - [ ] T6. examples/common fixture library (fixture_db, port allocator, Check
    reporter, wp_fixture); two examples migrated.
  - [ ] T7. De-fang the three incident-3-shaped examples (nginx_php_serve,
    php_fpm_serve, php_pools_serve → sandbox + fixture ports).
  - [ ] T8. `scripts/live-checks.sh` tiered runner (declared-tier enforcement).
  - [ ] T9. wk-checks: heights probe (h-full class), uireview.js can fail, StatusPill
    all-states scenario.
  - [ ] T10. Ledger quick wins: wp_login injection-point test (#36), DNS loopback-bind
    assert (#44), autostart guard tests (#175 L0).
  - [ ] T11. Dotfile-guard live 404 example (#103, nginx template first).
  - [ ] T12. `scripts/verify-full.sh` (fast/full gate split).
  - [ ] T13. SMOKE-TEST/PUBLISH-TESTING §5 additions + ledger re-tally.

- [ ] **Tunnel hardening — end-to-end review 28 Jul 2026, fix order RULED same day**
  (report + lifecycle decision: `docs/PLAN-tunnel-lifecycle.md`). Root finding: tunnels
  inherited NEITHER lifecycle model (not killed with the app like jobs, structurally
  unadoptable like services — cloudflared binds no port). Fix in this order:
  - [x] **1. Lifecycle: dies with the app + crash swept at next launch** ✓ SHIPPED
    28 Jul 2026 — `0aae71a` v23 tunnels table (spawn-time rows, docroot recorded);
    `d22cfa0` record before URL poll, failed starts settle their own row, stop/delete/
    rename delete the row; `92d6daf` `RunEvent::Exit` kill from rows (in-flight starts
    covered, reaped pids never re-signalled); `311bb57` launch sweep killing ONLY on
    positive argv identity (`ProcessSupervisor::pid_command`; recycled pid ⇒ cleanup,
    no signal — live-verified `examples/tunnel_sweep.rs`: foreign process survives,
    identified dies, dead cleans); mu-plugin claim restated (lifetime = tunnel lifetime
    + at most one relaunch), crash-gap disclosure added to the Tunnels page copy.
    ✓ 521 lib tests, examples build+run, tsc via next UI task.
  - [x] **2. Status honesty** ✓ SHIPPED 28 Jul 2026 — `4d259d1` dead children settled
    (`try_wait`, `Err` reads alive — death claims need positive evidence) before every
    status snapshot AND the already-sharing return (a crashed tunnel re-click now starts
    fresh); `e8b8bc2` the URL IS probeable, so process-liveness is NOT the ceiling:
    bounded HEAD / 30s / outside locks; ≠530 proves the path (origin errors rode the
    tunnel — origin health stays the Services page's fact), 530×3 consecutive = Broken
    (edge error 1033, reconnect-tolerant), transport error = Unverified, never Broken
    (strikes carry through); `7f5cc9b` badge/border/status-line read tunnel.health
    alone: Live / Unverified / Broken / absent. ✓ 523 lib tests (fold matrix +
    take_dead with real children), tsc. Human verify: share → kill -9 the cloudflared
    pid → card leaves Live within ~5s; share → watch Unverified→Live within seconds.
  - [x] **3. Override-site refusal** ✓ SHIPPED 28 Jul 2026 — `f70a350`
    `core::tunnels::ensure_tunnelable` refuses ahead of every path (UI + CLI land in
    `start_tunnel`) reading the SAME predicate as the nginx config generator
    (`is_nginx_served` — eligibility can't drift from reality). Message names the
    site, its server, and the default-vhost consequence; "yet" is truthful (per-
    backend-port origin is a real later fix, recorded ports exist). UI mirrors:
    disabled toggle + plain hint. ✓ unit test asserts specificity per server.
    QUIT WARNING also shipped (`7a7da37`, unblocked by step 2's honest count):
    liveness-settled N, native confirm off the main thread, no shares = no dialog.
    Human verify: quit while sharing (dialog + both buttons), quit while not
    sharing (no dialog), share attempt for an Apache site via `rex tunnel start`
    (refusal text). `e434fe4` scripts/verify.sh is now the pre-commit bar.
  - [x] **4. Double-start race** ✓ SHIPPED 28 Jul 2026 — `87044fe` the v23 row IS the
    guard: atomic claim (`INSERT..DO NOTHING`) BEFORE resolve/spawn, so the loser
    errors before it can spawn or truncate the winner's log; sentinel pid `u32::MAX`
    (inert even unguarded) until the child exists, exit hook + sweep skip it.
    Stop-vs-start closed BOTH ways: stop kills an in-flight recorded pid on positive
    argv ID only and deletes the row = claim REVOKED (`set_tunnel_pid` false ⇒
    pre-spawn start cancels itself; post-spawn kill caught by the poll's new
    `try_wait` early-exit in ≤300ms, which also ends the burn-30s-on-a-dead-spawn
    bug). Exit-during-in-flight: covered — the claim exists before any child, the
    exit hook reads rows. `4055c01` toggle disabled (busy) during Starting — no
    stop-shaped control requesting a second start. ✓ verify.sh green, sweep example
    + pending-claim branch PASS, claim/revoke store tests.
  - [x] **5. Bedrock content-dir (writers)** ✓ SHIPPED 28 Jul 2026 — `29a247a` v24
    `sites.content_dir` recorded ONCE (creation + backfill, same fs markers as
    detection; poison case tested — a bug-written `web/wp-content` can't flip the
    answer); both mu-plugin writers take the recorded rel; tunnel `disable` sweeps
    every known layout so pre-v24 strays clean up. "Log in as" was silently broken
    on Bedrock (wp-cli succeeded, file never loaded). ✓ verify.sh, 527 tests.
    - [x] **5b. Audit remainder** ✓ SHIPPED 28 Jul 2026 — `50d0de3` (plan
      `docs/PLAN-content-dir-assets.md`, `e8061e3`): `asset_dest` takes the recorded
      rel (12 callers); the unlink-delete guard's OWN path build (missed by the first
      audit) was silently defeated on Bedrock — restored, zero change on stock
      layouts; provenance rows store names so no migration exists to do, stranded
      pre-fix clones fail loudly; `theme_screenshot` threaded; debug-log panel gains
      honest `indeterminate` ("can't determine", never a wrong "off") — FULL Bedrock
      config parsing still rides the future wp-config reader work; Radicle
      `public/content` flagged unverified in code. ✓ verify.sh (which caught a
      missing mock field mid-work), 528 lib tests.
    - [ ] **Debug-log full fix (deferred with the wp-config reader):** parse
      Bedrock's `config/application.php` env defines so WP_DEBUG/WP_DEBUG_LOG read
      truthfully on non-stock layouts; today's honest state is "can't determine".
  - [x] **6. Leftover files, linked-repo lens** ✓ SHIPPED 28 Jul 2026 — `f4790f4`
    delete (preserved docroots) + rename remove BOTH mu-plugins
    (`cleanup_muplugin_artifacts`, every-layout sweep; delete's false
    goes-away-with-the-docroot comment corrected). `rexenv-login.php` owner ruled:
    lives while rexenv manages the site (eager removal = git-status flapping on the
    repeated-login workflow), rewritten per issue, removed at delete/rename. Empty
    dirs: v25 `mu_dir_created` set-once when a writer creates `mu-plugins/`; delete
    removes the dir only when RECORDED ours + empty (benign `.DS_Store`/`._*` swept,
    real files sacred) — recorded, never inferred. ✓ verify.sh, 530 lib tests
    (ownership/noise/real-content matrix). Human verify: "Log in as" on a real
    linked BEDROCK site must land in wp-admin (own proof — the path fix should
    cover it, but it rides nothing).
  - [x] **7. Cross-guards** ✓ SHIPPED 28 Jul 2026 — `f8439ba` a tunnel EXPOSES where
    jobs MUTATE, so all six pairs REFUSE with the exposure named (what a visitor
    would see), never "busy". Jobs read the v23 row (live OR starting share,
    dead-settled first); tunnel start checks after its claim and releases on
    refusal. Ordering: claim/set-then-check on tunnel + rewrite + db-import (can't
    cross); provision keeps the family idiom (accepted microsecond class). NEVER
    auto-stop a share or cancel a job — messages say where to act; re-share = new
    link. Valet-import inherits. ✓ verify.sh, 530 tests.
  - [x] **Disclosure copy** ✓ SHIPPED 28 Jul 2026 — `4d92ceb` the Tunnels info box
    states the linked-docroot deltas before the click: stray dev PHP publicly
    runnable, symlinks followed out of the project, dotfiles blocked (verified).
    **TUNNEL HARDENING COMPLETE** — steps 1–7 + 5b + disclosure all shipped
    28 Jul 2026; only the deliberate deferrals remain (watcher helper, quit-warning
    shipped early, debug-log full fix with the wp-config reader, stranded-asset
    badge, per-backend tunnel origins for override sites).
  - [x] **Post-completion fresh audit + rulings** ✓ SHIPPED 28 Jul 2026 — the
    sharpened pattern: a one-time check on a MUTABLE fact is a snapshot; lifetime
    guards or propagate-on-commit, usually both. `4815ff1` A1 web-server switch +
    third door (multisite convert) refuse under a live share; `7bebd11` A2 move
    refused while shared AND row docroot re-pointed on commit; `c8b7399` A3
    backfill leaves NULL for unreachable docroots (set-once poison); `679cdfc`
    A4+A5 probe semantics honest to the live-verified DNS reality (NO wildcard on
    trycloudflare.com ⇒ Unverified is the terminal state for most drops; Broken
    sticky, Reachable decays, redirects unfollowed, UI copy says what Unverified
    means); `9d8b2f3` A6 quit-dialog drop guard + A7 api-host never the URL;
    `ac2300c` clippy in verify.sh at zero. A4 shipped as the ASYMMETRY (ruled
    better than the original ruling): Broken sticky (530s are positive evidence,
    only an HTTP answer clears), Reachable decays (a freshness claim expires).
    Live-measurement items, none blocking (human + real network), adjusted 28 Jul
    post-diagnosis: ONE probe session covering both fault shapes — kill -9 (death
    path: Broken window minute by minute) AND a wifi blip while sharing (recovery
    path: does the URL survive the reconnect, does the badge return to Live);
    Bedrock "Log in as" landing in wp-admin; Radicle first-link verify (the
    `public/content` rel is flagged UNVERIFIED in code — nobody has seen it work).
    MEASURED ✓: cloudflared is single-process (`pgrep -P` empty on live pids,
    28 Jul diagnosis). CLOSED deliberately, not a gap: registration longevity of
    an unattended quick tunnel — the prober made the number irrelevant to users
    (if Cloudflare reaps a long-lived share, the badge says so honestly, which is
    the property we needed, not the number).
  - [ ] **Per-backend tunnel origins (deferred — RE-SCOPED 28 Jul post-audit,
    smaller than the original estimate):** the lifetime vhost guard it needed now
    EXISTS (`4815ff1` refuses any web-server switch while shared — so "handle a
    mid-share switch" collapses to "already refused"; only its message needs
    generalizing off nginx-specific wording when this ships). Remaining work:
    resolve the RECORDED override port at tunnel start (`recorded_override_port`,
    B20 — never re-derive) as the `--url` origin; refuse start when that backend
    isn't up; replace `ensure_tunnelable`'s refusal (its "yet" message is the
    seam). Health probe, mu-plugin, claim/row machinery are all origin-agnostic —
    no changes there.
  - [x] **Rowless-orphan backstop** ✓ SHIPPED 28 Jul 2026 — `502c514` (ruled after
    the live diagnosis: four pre-v23 fossils had served publicly 9–15 days,
    invisible to every record-driven mechanism; survival of the PROCESSES is
    measured, registration longevity was lost with them). Launch, after the row
    sweep: `pids_named("cloudflared")` → the sweep's exact argv identity (domain
    read from argv — no record exists) → no row ⇒ stop + weighty WARN ("a public
    share this app had no record of"). Class = "DB and process table disagree"
    (app-data reset, restore, future record loss) — permanent, not an upgrade
    patch. Live-checked beside a REAL running share: rowless-ours killed,
    outside-app-data lookalike survived, real tunnel provably untouched.
    Follow-up option (not built): a user-visible launch notice needs a queued
    notices channel — the sweep runs in setup before the webview mounts, so a
    plain event would be lost; WARN log is the honest current surface.
  - [x] **Three-check diagnosis** ✓ SHIPPED 28 Jul 2026 — `683f961` (design ruled
    same day). Failure-gated (healthy = zero extra traffic forever): 1.1.1.1-only
    resolve (no new party learns anything — Cloudflare already carries the
    tunnel), then edge connect to a LIVE-resolved IP with true SNI/Host (never a
    constant). Badge meaning unchanged; the Unverified line becomes four precise
    sentences (local-dns-behind / dns-propagating / edge-gone / offline), all
    scoped, all pointing at the second device. The half that adds TRUTH: edge
    530 feeds strikes, making Broken reachable after DNS death — tested as its
    own claim (same ticks: Unverified without the edge answer, Broken with it;
    edge 200 pinned to never upgrade). Anycast limit documented at the claim
    site. Probe-session note: the kill -9 mapping can now also watch the
    diagnosis line, not just the badge.
  - [x] **Phase A/B do-no-harm probing** ✓ SHIPPED 28 Jul 2026 — `009cbf9`. The
    best finding of the tunnel effort, out of a question that looked like a
    misunderstanding: OUR immediate post-start probe (step 2's latency
    optimisation) asked the system resolver ~1s after the banner, inside the
    propagation window, and trycloudflare's SOA MINIMUM = 1800s (MEASURED via
    1.1.1.1 + 8.8.8.8, identical) negative-caches the LAN's resolver for 30
    minutes — every device behind the router, which is why the phone worked
    only on cellular. We made the race universal. Phase A: 1.1.1.1 + pinned-
    address edge checks only; Phase B once the record is provably public (or
    the 5-min escape-hatch cap for 1.1.1.1-blocked networks — reasoning at the
    const), system probe run the SAME tick the gate opens. Pinned loudly:
    `phase_a_never_plans_a_system_dns_query`. Copy: dns-propagating nudges
    against the user's own too-early click — the only remaining poisoning path.
    PROBE SESSION addition: after a fresh share, watch `dig @1.1.1.1`
    second-by-second — the banner→authoritative gap decides how often a
    human-speed click races on its own; the last unknown in this chain.
  - [ ] **DNS agent answers ARBITRARY names when queried directly** (found during the
    28 Jul live tunnel diagnosis): `dig -p 15353 @127.0.0.1 <any-hostname>` returns
    `127.0.0.1` — the hickory agent is a catch-all wildcard, not per-TLD zones.
    Harmless today (only `/etc/resolver/{rex,sb,test}` route queries to it), but
    "answers anything" is unintended: anything ever pointed more broadly at :15353
    would black-hole all DNS to localhost. Scope it to configured TLDs; NXDOMAIN
    the rest.
  - **Deferred (deliberate):** parent-death watcher helper (kqueue `NOTE_EXIT`) to close
    the crash→relaunch exposure gap entirely — a new long-lived helper process to get
    right vs. a rare window already bounded by next launch; revisit only if crash
    reports show the gap mattering. Quit-time warning ("quitting stops N public
    shares", `CloseRequested`) lands AFTER step 2 — only an honest count is worth
    confirming.

- [x] **Migrate an existing Valet / Herd environment into rexenv** ✓ **ALL FOUR
  STAGES SHIPPED** — Stage 3's §J packaged pass (the last gate) passed 28 Jul 2026.
  APPROVED 26 Jul 2026, all four stages ship BEFORE the release. Plan + research:
  `docs/PLAN-valet-herd-migration.md` (Q0–Q7, verified against the live Valet 4.12.0 +
  Herd 1.29.0 install on this Mac). Feasibility gate PASSED — the serving plane already
  handles an arbitrary docroot end to end; only `provision()` (`core/sites.rs:761-778`)
  and the create entry points overwrite the caller's path, so import-in-place needs no
  copy. Two pre-existing bugs are folded IN, not handled separately (plan §12):
  `ensure_resolver` silently overwrites a FOREIGN `/etc/resolver/<tld>` → Stage 1 with the
  full consent-gated takeover (root-op care class, B2/B10 family); provisioning claims
  "serving at …" while a shadow-binding Herd answers → wherever import reports success.
  All decisions settled — the last (`mysqli.default_socket` pool defaults, plan §11.3)
  decided 27 Jul 2026 as Stage 3 D5: MySQL's socket on every pool.
  - [x] **Stage 0 — Link an existing folder** ✓ SHIPPED (all commits ticked below;
    plan `docs/PLAN-linked-sites.md`) (first-class feature, not migration
    plumbing): serve a docroot outside `~/rexenv/Sites`. **Planned, awaiting 3 decisions:
    `docs/PLAN-linked-sites.md`** — v17 `sites.docroot_managed` marker (NOT `linked`: it
    must also cover docroots moved outside the sites dir, or `move_site_docroot`'s "kept —
    not deleted" promise breaks) + Rust backfill recording today's guard answer once,
    structural delete guard, linked-sites-never-provision, `$HOME`/`/` refused, 7 commits.
    All three design decisions approved 26 Jul 2026.
    - [x] **Commit 1 — examples orphan-worker class fix** ✓ `2c1d59e`: `examples/common::Reaped`
      (Drop guard owning the child + production `Proc::terminate` + fixture-port sweep
      matching on the program NAME, since php-fpm workers rewrite their title). Both
      leakers converted; explicit `reap()` on the `exit(1)` paths because
      `std::process::exit` skips destructors. Reaped the 4 live orphans first (:9998,
      :9799, SIGTERM sufficed; live stack untouched). ✓ `cargo build --examples` clean,
      `cargo test --lib` 396 passed, no new clippy lints.
    - [x] **Commits 2–7 — SHIPPED 26 Jul 2026.** `cd5946c` v17 `sites.docroot_managed`
      + startup backfill (nullable, no DEFAULT — a default would have to GUESS for
      moved-out rows); `8579447` provision honors a caller path, `validate_linked_docroot`,
      teardown reads the marker and returns `{existed, docroot_removed}`; `4338d21`
      `detect_project` + linked phase-set skip + move refusal/downgrade; `7e34a14` New Site
      "Existing folder" + disclosure + ownership-aware copy + `external` badge; `4df738a`
      `rex site create --path` + honest delete prompt; `aa3c4ba` `linked_site_check`.
      ✓ `cargo test --lib` 405 passed (11 new), `cargo build --examples` clean,
      `tsc --noEmit` + `npm run build` clean, clippy at the 6 pre-existing lints.
      ✓ **Live-verified on this machine with the stack running**
      (`cargo run --example linked_site_check`): path stored as given,
      `docroot_managed=false`, folder untouched by provisioning, move refused, their file
      served through the vhost, and after delete the row + certificate are gone while the
      folder and all its files remain. Stack pools 9780–9785 + nginx 18088 untouched.
      Audit finding that made commit 4 mandatory (plan §14.2): `move_site_docroot` deletes
      the old tree with NO ownership check (`commands/sites.rs:446-452`, gated only on the
      cross-volume `copied` flag) — more permissive than teardown, so a linked folder on
      another volume would have been relocated and its original deleted.
      **Still open, deliberately:** "install WordPress into an empty linked folder" is out
      of Stage 0 (own flow, own disclosure); `teardown` still never removes the Apache
      per-site config/log (pre-existing, plan §3).
  - [x] **Stage 1 — sites-only import + resolver consent** ✓ SHIPPED 26 Jul 2026,
    commits `42af4fc` → `7d7c340` (9). `42af4fc` ensure_resolver refuses a foreign file
    (the silent-overwrite bug); `732d394` v18 `resolver_takeovers` + 0600 backup +
    restore-aware teardown (all six decision rows) + startup orphan sweep; `159de11`
    `core/valet.rs` read-only discovery; `37b1b4d` scan/takeover/hand-back/drift IPC;
    `528e5ce` build_plan linked-awareness (~600 MB of MySQL + wp-cli no longer fetched for
    phases a linked WP import never runs) + `servingBlocked` field; `8f3cf7a` sequential
    continue-on-failure import reusing `site_provision::start`; `411cdc8` the `/import`
    screen + consent panel + dismissible Sites banner + Settings hand-back; `7d7c340`
    `valet_scan_check`; `aa789fc`/`b6f4190` the plan.
    ✓ `cargo test --lib` 414 passed (13 new), examples build, tsc + `npm run build` clean,
    clippy at the 6 pre-existing lints.
    ✓ **Live-verified on this machine**: `cargo run --example valet_scan_check` — both
    Valet and Herd trees BYTE-IDENTICAL after a full scan (fingerprinted path+size+mtime
    before/after), 32 rows reconciled: 19 importable, 8 dangling links each naming their
    missing target, 3 leftover confs, Herd winning 11 duplicate domains, `.dev` + `.test`
    both surfaced, both marker formats read.
    ✓ **Import chain live-verified** (`cargo run --example valet_import_check`, on `.rex`
    against a fixture Valet tree so the machine's real `.test` setup is never involved):
    bare-digit isolate marker read as 8.3, a Laravel project resolved to its `public/`
    docroot (not the project root), imported as a LINK with `docroot_managed=false`, their
    file served through the vhost, and after delete their project AND their Valet tree
    byte-identical. Live stack untouched, no stray cert or fixture left behind.
    ⚠ **NOT live-verified — clean-VM items in `docs/PUBLISH-TESTING.md` §F**: resolver
    TAKEOVER, hand-back, restore-on-uninstall and drift. This machine has no
    `/etc/resolver/test`, and creating a root-owned foreign file to test against was
    deliberately refused; those paths are fixture- and unit-tested only.
  - [x] **Stage 2 — database import** ✓ SHIPPED — §I passed 27 Jul 2026 (all 12
    steps, live DBngin source, packaged app) — plan `docs/PLAN-valet-herd-db-import.md`
    (APPROVED 26 Jul 2026, D1–D6 settled). Two halves, each verifiable alone.
    ✓ **Step 0 — the pre-auth handshake read is VERIFIED live** (`cabc749`): MySQL
    8.4.6 → `proto=10 version='8.4.6'`, MariaDB 12.3.2 → `'12.3.2-MariaDB'`, both
    without authenticating, so engine identification never touches the
    client-pairing trap; a stopped server refuses instantly. MariaDB 10.x's
    `5.5.5-` prefix is handled but unverified here; DBngin 8.0.27 stays open until
    the live check.
    - *Half A — dump to artifact (their side, read-only)*
      - [x] 1. v19 `sites.db_created` + teardown guard `bcf3a1f` — nullable, no
        DEFAULT (the v17 reasoning); `set_site_db_created` refuses `false`→`true`
        in SQL so a pre-existing database can never become ours to drop;
        `may_drop_database` also skips the engine bring-up for a linked site that
        never imported one. 419 lib tests.
      - [x] 2. `core::dbimport` config mapping (WP defines + conservative `.env`)
        `022c969` — `core::phpconf` is the ONE wp-config reader: a real scan
        (comments, strings, multi-line defines, heredoc stop) replacing the old
        line-local one in `core/logs.rs`, which now calls it. A password
        containing `)` or `;` survives whole — the old "find the next paren"
        reader truncated it. Refusals are first-class (`Unreadable`, one sentence
        each, asserted not to read as blame): conditional duplicates, computed
        defines, `${VAR}`, multi-line `.env` values. `DbConnection` redacts its
        password in `Debug` and the IPC shape has no password field to forget.
        `DbSiteStatus` keeps unreachable / refused / not-found apart. 444 tests.
      - [x] 3. engine discovery + handshake identification `49a5bac` —
        `core::dbsource`. "Plists label, listeners decide" is structural, not a
        convention: `SourceServer` has a private `Answered` witness minted only
        on the probe branch that got an answer, so no caller can turn a config
        claim into a server. Same shape for vendor: `Identity::Declared` carries
        a private `DeclarationGuard`, so `resolve_vendor` is the only way to
        reach it — a declaration fills an unknown and is REFUSED when it
        contradicts the wire. Greeting parser pinned to the captured bytes of
        both live servers, plus MariaDB 10.x `5.5.5-`, MySQL derivatives, ERR
        packets and silent (non-MySQL) servers. 455 tests.
        ✓ **Live-verified** (`cargo run --example db_source_check`): DBngin's
        plist says `Status = started` for MySQL 8.0.27 on 3306 → reported as a
        claim in `silent`, never in `servers`, because nothing answered. Free
        Herd has no services config; that hint source degrades to nothing.
      - [x] 4. `compat()` matrix + per-site verdict `90aeafd` —
        `core::dbcompat`, pure over (vendor, version) × (vendor, version). Three
        TYPES, not three strings: `Proceed{cautions}` (runs; cautions inform and
        never block), `NeedsOverride{reason, consequence, better}` (refused by
        default, user may accept the stated cost, safer route named when we ship
        one), `Blocked{reason, fix}` (no override; a test asserts every block
        carries a way forward). Exhaustive over 14 sources × all 4 targets we
        ship, asserting each explains itself and none reads as blame. The two
        cross-vendor directions differ on evidence: MariaDB→MySQL is Blocked
        (verified Aria options fail on the first table) with a one-setting fix;
        MySQL→MariaDB is NeedsOverride (drifts later, not immediately).
        Unidentified vendor is Blocked-with-a-resolution, never overridable —
        proceeding would pick client tools by coin flip. 464 tests; the printed
        table is `cargo run --example db_compat_matrix`.
      - [x] 5. preflight (connect bound, size, disk) + dump to a 0600 artifact
        `62101e3` — `core::dbdump`. Preflight order is STRUCTURAL: the
        no-connection gate (is-this-us via `server_is_ours` two-facts check +
        `classify_self_import` three-way split + the verdict) is the only mint
        of the `Cleared` witness that `preflight_live` and `dump` require, and
        `dump` also requires the `Preflight` only `check_disk` produces. A
        partial dump is unrepresentable: `.partial` → rename on exit 0 →
        manifest last; `load_manifest` (Half B's only door) refuses no-manifest
        and size-mismatch. Manifest type has no credential fields; test pins the
        exact key set. Disk estimate 2×data+16MB, stated as an estimate. Cancel
        = SIGTERM the client, delete `.partial` — a read that stopped
        (`--single-transaction`, never FTWRL/`--master-data`). 472 tests.
        ✓ **Live-verified in the sandbox** (`cargo run --example db_dump_check`,
        own mysqld on :13399): pre-auth identify → gate refusals → missing-vs-
        present preflight → dump (artifact AND manifest mode 600 — proves
        mysqldump truncates in place, the plan §3 open question) → cancel leaves
        nothing → `.partial`/tampered artifact refused.
    - *Half B — restore into ours (+ credential mirroring)*
      - [x] 6. collision + provenance + create + restore `4141e9d` —
        `core::dbrestore`. Three orderings structural: provenance-before-create
        (`Recorded` witness, minted only by `record_provenance` which writes
        `db_created` FIRST); recorded-truth-outranks-rederivation on Retry (a
        crash between create and feed would otherwise demote our half-made db
        to "pre-existing" and strand it); and settle-through-`Verified`-only
        (`verify_complete` checks MEMBERSHIP of the manifest's table names —
        not count equality, which a pre-existing target's extra tables would
        break — and `finish` won't compile without its proof). Retry is
        drop-and-refeed, never resume (a dump's INSERTs aren't idempotent);
        `prepare_target` IS the retry path. Manifest now carries table names
        read from the artifact by the scan. 481 tests.
      - [x] 7. credential mirroring `590e997` — `core::dbmirror`. Reserved
        accounts refused as an OUTCOME before any spawn; loopback-only by
        construction (SQL built from a two-host list, `'%'` in no statement,
        grant on the one db); idempotent because Retry reruns it (IF NOT
        EXISTS + ALTER + GRANT); password inside SQL over stdin, never argv.
        ✓ **Live-verified in the sandbox** (`cargo run --example
        db_restore_check`, own mysqld :13398): restore asserted as ROW COUNTS
        (500/20); crafted mid-dump failure names its missing table and cannot
        verify; **Retry recovers unaided through the same code path to full
        row counts**; a pre-existing database's extra table survives failure
        AND retry; mirrored user connects with a quote-and-backslash password,
        hosts = {localhost, 127.0.0.1} only; root → RefusedReserved; our root
        still passwordless after everything.
      - [x] 8. job wiring + both entry points `2aebe8e` —
        `commands/db_import.rs`: streamed job (registry, events, per-job log,
        cancel, honest progress — real bytes both directions), sequencing IS
        the witness chain. Both directions of the concurrency guard: db-import
        checks `ProvisionJobs::busy_for`, provision/retry check
        `AppState::db_import_active`. Batch: `importDatabases` opt-in runs the
        SAME job per site after its provision settles (no parallel
        implementation), continue-on-failure, per-row `db` outcome + summary
        counts. Typed-confirm required to overwrite an unclaimed database.
      - [x] 9. interim state from ONE fact `2aebe8e` — v20 `db_imports`
        row (`state` closed set, no "connected" value until Stage 3);
        summary, Sites badge and copy-paste block all render the same
        `DbImportRecord`. Copy says the three things: the copy exists, the
        site still reads the OLD database, and the two DRIFT from now on.
        Root case states the 3-key change. Settings lists leftover dumps
        (their data — named, sized, deletable); success deletes its own.
      - [x] 10. recovery + end-to-end examples — covered by `db_dump_check`
        (cancel leaves nothing; interrupted dump unrestorable) and
        `db_restore_check` (partial restore honest + named; **Retry recovers
        unaided to full row counts**; pre-existing survives everything;
        mirroring live). Unreachable-source is unit-level (`probe` refused
        port) + §I step 4 live.
      ✓ **§I PASSED 27 Jul 2026** — all 12 steps on the packaged app against
        live DBngin: the stopped-engine refusal contradicted the plist, the
        drift step demonstrated itself, and DBngin's 8.0.27 handshake closed
        the last open identification case. **Stage 2 SHIPPED.**
      - [x] 11. `PUBLISH-TESTING.md` §I `2aebe8e` — 12 steps incl. the
        DBngin precondition (user starts it; step 4 checks the honest refusal
        while it's stopped), the drift demonstration (step 7), the mid-copy
        kill for the kept-artifact path, and both concurrency directions.
  - [x] **Stage 3 — opt-in connection-config rewrite ✓ SHIPPED — §J passed
    28 Jul 2026** (all 12 steps, packaged app, real ea.test; every deviation:
    none reported). (per-site, backup, diff first).
    **Plan APPROVED 27 Jul 2026: `docs/PLAN-valet-herd-rewrite.md`** — all five
    decisions settled (§9): D1 dedicated per-SITE user `rex_<slug>` (password
    unrepresentable in `RewritePlan`), D2 revert-then-delete as the default button
    naming both outcomes, D3 revert leaves the mirrored user, D4 HTTP check
    supplementary-only, D5 MySQL's socket on every pool.
    - [x] 1. Decisions recorded (§8 step 1) ✓ 27 Jul 2026, plan §9 rewritten from
      "needed" to "settled" in the same commit as this tick
    - [x] 2. v21 + the closed-set extension ✓ `e179439` — `config_rewrites`
      (site_id+file PK; INSERT-only so FIRST BACKUP WINS is the PK, not a
      convention), `db_imports.verified` (nullable, no DEFAULT), and the door
      closed: `NewDbImport` has no state field, `set_db_import_connected` is
      the sole 'connected' writer and demands a `ConnectedVerified` witness
      with no production constructor until the verification path exists.
      Migration test in the v15/v17/v19 shape + a serde pin of the TS wire
      contract. 484 lib tests, tsc clean, examples build.
    - [x] 3. `core::confedit` ✓ `359e69f` — closed `RewriteKey` enum + private
      plan fields (password/Port-in-wp unrepresentable), diff derived from the
      produced bytes, `.env` editor on a shared `dotenv_lines` scanner (reader
      rebuilt on it, Stage 2 tests unchanged), wp value-span edits, writes
      refuse more than reads (whole-file on unclosed quote, equal-value dups,
      heredoc, commented-out keys), DB_PORT append copies the DB_HOST line's
      conventions. 19 new tests incl. the no-secret-in-any-diff pin.
      wp-config edits are OUR span editor, not `wp config set` — SETTLED
      28 Jul 2026 (plan §3): wp-cli would reopen the approved-vs-written gap.
    - [x] 4. dedicated user + drop-by-record ✓ `a4c7eb3` — `rex_<slug>` capped
      at 32 with full-domain FNV disambiguation (two same-truncation domains
      proven distinct), `rex_` prefix structurally never reserved; delete
      drops `db_imports.mirrored_user` (the record, never re-derived),
      `DROP USER IF EXISTS` over exactly the two loopback scopes, reserved
      hard-refused even on a corrupt record; teardown removes the db_imports
      row (closes Stage 2's orphan + dangling-cleanup gap). 507 lib tests.
    - [x] 5. the rewrite job ✓ `f72b2b7` (5a: v22 `written_digest`, nullable/
      no-default, NULL = conservative FileEdited branch; `core::confrewrite`
      file mechanics, mode-preserving atomic write, revert classifier),
      `1f1fb6d` (5b: `core::confverify` — the proof's ONLY production mint is
      verify_signin's success path against the re-read file; HTTP probe can
      only upgrade a proof, never create one), `e0df48f` (5c: preview/apply/
      revert commands, whole-file fingerprint, record-first always-converge
      mirror, backup→row→write order, named revert states incl. BackupMissing
      keeping `connected`, cross-guards both ways via `rewrite_active`,
      teardown cleans rewrite rows+backups, boot-to-drop comment).
    - [x] 6. UI ✓ `9285d91` — consent card (diff from the written bytes,
      consent voided on fingerprint change, backup note carries the
      password-in-backup limit), fileChanged as a normal state, verifyFailed
      distinct from write-failure, Verify-connection for already-pointing
      configs, tell-only floor with the reason, D5 MariaDB note in the site's
      panel, §5 cache warning leads, connected panel + revert (+force), D2
      delete confirm naming both outcomes with revert-then-delete default.
      Pending HIS ruling: collision-rename tell-only permanence, Laravel DB
      survival at delete (writeups delivered in session).
    - [x] 7. socket pool default ✓ `34973ed` — `mysqli.default_socket` only:
      compiled default verified EMPTY on the cached binaries (strictly
      additive), while `pdo_mysql`'s compiled default is `/tmp/mysql.sock`
      (Homebrew's spot) so setting it could redirect an existing PDO site —
      deliberately omitted, pinned by test, PENDING his ruling. Provisioning
      confirmed to write `127.0.0.1:<port>` (never localhost). Live
      served-and-connecting check folds into step 8/§J. Rulings landed this
      round: collision-rename tell-only FINAL `abff030`; non-WP imported DB
      drops with its site (Some(1)-only gate + NULL pinned) `120aea0`.
    - [x] 8. `config_rewrite_check` ✓ `287e42f` — 11 live assertions against a
      sandbox mysqld, ALL PASSED 28 Jul 2026: root-case end to end (dedicated
      user + their password + sign-in as it against the re-read file),
      no-secret diff, 0600 backup, mode-preserving write, denied-write leaves
      previous bytes, ThisSite re-scan, FileEdited classification,
      byte-identical revert, reverted file can't mint a proof.
    - [x] 9. `PUBLISH-TESTING.md` §J packaged pass ✓ PASSED 28 Jul 2026 —
      human-verified on the packaged app (`44b6a6d`), all 12 steps, no
      deviations reported. pdo_mysql ruling: OUT permanently `44b6a6d`
      (silent-redirect of working Homebrew-socket sites — worse than a
      regression; the pin test is intent).

- [x] **Add plugin/theme from Git — clone → detect → install → build** ✓
  **HUMAN-VERIFIED 18 Jul 2026** — full docs/GIT-FEATURE-TEST.md pass (all
  sections incl. tunnel dotfile case, private-repo SSH, WKWebView streaming,
  cancel/orphans, force-kill gap accepted). (plan
  green-lit 17 Jul 2026: system git/node resolved via the user's login-shell
  env, composer.phar on the site's PHP as fallback, per-step streamed jobs
  with cancel, repo scripts NEVER auto-run — explicit click + disclosure).
  Phases, each gated on lib tests + a live check:
  - [x] **Phase 1 — hardening + foundations** (17 Jul, 3 commits). (1)
    `00716bb` dotfile deny in ALL THREE vhost templates — nginx regex location
    BEFORE the `.php` location, Apache mod_rewrite `[R=404,L]` before the WP
    routing (also covers the previously unprotected `.htaccess`), FrankenPHP
    two-RE2-matcher guard (no lookahead in Go); root `/.well-known/` exempt.
    Pre-existing hole, tunnel-exposed — any docroot `.git`/`.env` was served.
    (2) `b44cbcf` `core/repo.rs` git source parser: https/ssh/scp/shorthand,
    GitHub `/tree/` + GitLab `/-/tree/` ref candidates, folder-name validated
    at the source. (3) `1ebbe62` `ShellRunner::login_shell_env` (macOS
    `$SHELL -ilc`, NUL-marker protocol, 10s cap) + `git_preflight` (quiet CLT
    probe — no GUI dialog) + `core/devtools` resolution with `$`-fix errors.
    ✓ 297 lib tests, clippy no new, examples build; devtools_check live on the
    dev machine: node → `~/.nvm/versions/node/v22.23.1/bin/node` (the nvm
    case), SSH_AUTH_SOCK present. **Awaiting human verify:** `.git/config` →
    404 on nginx AND Apache AND FrankenPHP with `/.well-known/` still served
    (needs a config regen — restart services once); `devtools_check` output
    matches your terminal's `which node`.
  - [x] **Phase 2 — probe + streamed cancellable clone** (17 Jul, 1 commit).
    `probe_remote` (ls-remote --symref, 30s cap, reader-thread capture — big
    repos overflow a pipe), `run_step_streamed` (own process GROUP via new
    `ProcessSupervisor::spawn_streamed`/`stop_group`, pkill/pgrep -g GROUP
    liveness), `clone_repo` (collision refused pre-network; partial dir
    removed on fail/cancel only if we created it), `map_git_error` house-
    style. ✓ **Live** (`examples/repo_clone_check` ALL PASS): real probe,
    missing-repo mapped in 0.5s, clone lands .git + 18 streamed lines,
    collision leaves existing content, and THE cancel proof — gutenberg
    clone cancelled mid-transfer, ps showed 3 procs in the group before →
    EMPTY after, partial checkout gone. 301 lib tests.
  - [x] **Phase 3 — detect + install/build + mapped errors** (17 Jul, 1
    commit). `inspect_repo` (packageManager > lockfile > npm; scripts.build;
    .nvmrc beats engines; WP header 8KB window), `node_version_warning`
    (display-only), composer = ALWAYS the pinned phar (new `composer` 2.10.2
    manifest pin, sha == getcomposer.org published sum, run-tested) executed
    by the SITE's bundled PHP — resolves the Herd-wrapper note; user env
    rides along (COMPOSER auth). ✓ **Live** (`examples/repo_install_check`
    ALL PASS): fixture repos through the real pipeline — vendor/psr/log +
    autoload via PHP 8.3.31, npm install + build artifacts via the nvm node,
    .nvmrc=18 warning vs v22, php>=9 fixture → "switch the site's PHP"
    mapped error, missing-bun mapped. 306 lib tests.
  - [x] **Phase 4 — UI/IPC/provenance** (17 Jul, 1 commit). commands/repo.rs
    (probe/add/run_step/cancel/state/assets/tools; raw URL re-parsed
    server-side; one step at a time), events `repo-job://state|output/<id>`
    + flat `logs/repo-<domain>-<dir>.log`, v12 `site_git_assets` + badge,
    RunEvent::Exit kills job groups (jobs die WITH the app — deliberate
    opposite of services-outlive-the-app), GitAddPanel (Fetch → ref select
    → steps + streamed log + cancel + disclosure + Activate-only-after-
    all-green), source tabs in both panels (wp.org flow untouched),
    DEV-only mockIPC harness `/dev/git-panel` (tree-shaken from prod —
    verified absent in dist). ✓ 306 lib tests, tsc, vite build, Playwright
    **WebKit** drive of the harness ALL PASS (no-synthetic-clicks rule).
  - [x] **Phase 5 — polish + docs** (17 Jul, 1 commit). Site → Logs tab
    lists "Git job — <dir>" sources (`targets_for_site` scans
    `repo-<domain>-*.log`; +1 test), ARCHITECTURE §9 subsection, PORTS.md
    composer pin row, `docs/GIT-FEATURE-TEST.md` — the manual checklist
    (packaged-app WKWebView flow, tunnel dotfile case, private-repo SSH
    steps, failure matrix, cancel/orphan spot-checks, flagged unknowns).
    ✓ 307 lib tests. ✓ **Human-verified 18 Jul** (full test-doc run — all
    green; packaging/QA-handoff unblocked). Post-v1 ladder: link-existing-folder (symlink), watch mode (dies
    with app BY DESIGN — not a ServiceManager service), update/pull via
    provenance, multisite network-activate variants.

- [x] **Supervisor-aware fix-it commands + status-pill ghost fix** (`2fc0b37`/`d55f3dd`).
  (1) Port-conflict messages carry a copyable command matched to how the holder is
  MANAGED (toast CommandBlock): app-supervised → `osascript -e 'quit app "Herd"'`
  (live-tested: quits Herd, frees 127.0.0.1:443); brew binary → `brew services stop F
  || sudo …` (Valet's nginx lands here); else `sudo kill <master-pid>`; wired into
  ensure_free, Start-all wire gate, watchdog edge-blocked, bind-conflict error.
  (2) Status-badge overlap root-caused in two layers: exact-fit pill widths (86px <
  natural "Running" 88.3/"Stopped" 89.9 in WebKit metrics — SF Pro, NOT an
  Inter/WKWebView delta; the font stack puts -apple-system first) AND a WKWebView
  compositing ghost — `transition-opacity` layerizes the row while the label swaps,
  and →Idle stops all animation so the stale "Running" snapshot lingers till the next
  poll (→Running mounts the ping dot, forcing recomposition — hence the asymmetry).
  Fixed: row dims instantly (no opacity transition), all pills `min-w-[92px]` +
  nowrap + flex-none markers (incl. the DNS pill still at 86px), so no future label
  can wrap or clip. ✓ WebKit-measured (8 pills × 92.0px, single line); ✓
  **live-verified in the packaged app** (Jul 13): Caddy Running↔Idle toggled
  repeatedly, clean both directions.
- [x] **Herd/port-conflict honesty + opt-in login-start** (post-reboot 502 report,
  Jul 13). (1) `friendly_holder` attributes app-bundled listeners to the owning app
  ("Herd (nginx, pid 1234)"); `start_edge_daemon` timeout and the watchdog edge-down
  give-up now NAME the :443/:80 holder instead of a vague "socket never came up"
  (`c3c1236`). (2) New default-off setting `start_services_on_launch`: app launch runs
  Start-all after adoption — with app-autostart on, the stack returns after reboot
  promptlessly (boot daemon has the edge up → adopted). Login-safe: never downloads
  (cold cache → honest service-health event), never prompts (privileged edge plan →
  skipped + surfaced). Settings UI: old "Start services on login" toggle renamed to
  the truthful "Open rexenv at login" (it only installed the login item); new real
  toggle added (`3fa1753`). ✓ 234 lib tests, examples, tsc. **Live test of (1) found the
  REAL failure mode** (`d4e5892`): Herd binds `127.0.0.1:443` SPECIFICALLY, coexisting
  with our wildcard `*:443` bind — NO bind error anywhere (netstat: both listeners;
  caddy-start.log clean), kernel routes loopback to the most-specific listener = Herd →
  all-green UI while Herd 404s every site (TLS issuer: Laravel Valet CA). Process
  identity (admin unix socket) cannot catch this — fixed with positive WIRE identity:
  every site block stamps `header X-Rexenv-Edge "1"`; `proxy::edge_answers_as_ours`
  probes 127.0.0.1:443 (DNS-free, marker-checked; `Server: Caddy` fallback); watchdog
  flips `edge_blocked` on transitions (`edge-blocked`/`edge-unblocked` events naming
  the holder); `status()` folds it in (Caddy + site dots read NOT running while
  shadowed); Start-all/auto-start gate on the probe and fail naming the interceptor.
  ✓ **Live-verified (A)** (Jul 13): with Herd running — Start-all fails naming Herd,
  Caddy row flips red ≤10s, recovery on Herd quit; holder attribution fixed en route
  (`c6200c7`: master pid not worker, real exe via txt FD not the rewritten ps title,
  `.app`/Application-Support attribution → "quit Herd", honest name+pid+path degrade).
  ✓ **Live-verified (B)** (Jul 13): reboot with both
  toggles on, untouched — edge up 16s after boot (pre-login), DNS agent at login,
  auto-start spawned MySQL/fpm/nginx promptlessly, `https://tr.rex`/`lm.rex` → 200,
  wire identity ours (`x-rexenv-edge: 1`). health.log also captured the whole Herd
  suite firing in production: `edge-blocked` naming "Herd (nginx-arm64, pid …)" +
  copyable quit command, `edge-unblocked` on Herd quit.
- [x] **Sites die 1–2h after quitting the app — DNS must survive the app.** Root cause
  (evidence-first diagnosis): the data plane (nginx/fpm/MySQL/edge, all detached or
  launchd-owned) survives a quit indefinitely — but the resolver was an IN-PROCESS tokio
  task that died the instant the app quit. Sites coasted ~1h40m on client caches +
  persistent H2 connections (access log: steady 60s polls 18:37→20:17, flapping
  revivals, final 58-min outage ending 34s after app relaunch with ZERO server
  restarts — same pids throughout). Sleep exonerated (`pmset -g log`: awake through all
  outages); idle timeouts exonerated (fpm/MySQL kill workers/connections, never
  masters); macOS never reaps orphans. ✓ **Fixed:** DNS now served by a per-user
  LaunchAgent `dev.rexenv.rexenv.dns` (`<binary> --dns-agent`, KeepAlive + RunAtLoad,
  all unprivileged — new 11th trait `DnsAgentManager`); app launch = adopt
  (`answers_as_ours` wire probe, H2) or install/refresh (plist tracks current binary,
  dev↔installed hand off; busy-port retry every 10s hands off from an old in-process
  holder) or IN-PROCESS fallback (never regress); watchdog kickstarts a dead agent
  (bounded 3) then one in-process fallback; `dns_status.mode` = agent | in-process |
  down; teardown uninstalls the agent. ✓ 233 lib tests green (plist invariants +
  `answers_as_ours` live probe), examples build, tsc clean. ✓ **Live-verified with the
  app CLOSED** (Jul 12): OS-chain resolution (dscacheutil → /etc/resolver/rex → agent)
  answers 127.0.0.1; `https://tr.rex`/`lm.rex` → 200; SIGTERM'd agent relaunched by
  launchd in ~3s and kept resolving. Follow-up: Settings doesn't RENDER `dns_status.mode`
  yet (backend field + TS type only).
- [x] **Xdebug per-site toggle (§8.2) — UNBLOCKED 16 Jul 2026, built.** The
  original blocker (no hosted Xdebug for static PHP) dissolved: shivammathur's
  homebrew-extensions ghcr bottles (`xdebug@8.1`–`@8.5`, Xdebug 3.5.3 — the same
  content-addressed digest-pinned bottle class as Redis/MariaDB/Apache; the tap
  powering GitHub Actions setup-php) **dlopen directly into our EXISTING static-php
  binaries** — no debug PHP build, no extension-parity problem, extension set stays
  identical by construction. Feasibility proven first (cli+fpm load on 8.1–8.5,
  `xdebug_info()` functional, DBGp init on a live socket; sonoma bottles load on
  Tahoe; `.so` links only system libs; old tags retained → pins 404-but-never-
  drift). **8.0.30 excluded** (see Blocked). ✓ **Built** in 5 commits `617d050`/
  `c424322`/`a731311`/`cb28b13`/`99e9fce` + example: per-minor one-part bundles
  (all 10 digests downloaded + load-tested at pin time); per-minor DEBUG pools
  (same fpm binary, `-d zend_extension` + `xdebug.mode=debug,develop`, ports
  `9981–9985`, spawned only for toggled sites, adopted/watchdog-respawned/orphan-
  swept/settings-restarted like normal pools, status row `PHP-FPM x.y (Xdebug)`);
  **every debug spawn gated on a real load probe** (PHP treats a bad
  zend_extension as a warning — the gate makes it an error, never a silently
  Xdebug-less pool); v11 `sites.xdebug`; `pool_port_for_site` = the single
  routing seam (nginx vhost, Apache override, site_serving; stale flag on 8.0
  falls back to the normal pool); core-validated toggle (FrankenPHP + 8.0
  refused with real reasons, disable always allowed); toggle carries across a
  PHP-version switch; Start-all prefetches toggled minors' bundles; SiteDetail
  Settings card with IDE hints. ✓ 266 lib tests, clippy (no new), examples,
  tsc, vite. ✓ **Live-verified via `examples/xdebug_pool_check`** (real cache,
  production paths): bundle download→verify→relink→sign→publish, loads-clean +
  codesign pass, load-probe gate Ok, **full DBGp handshake**, debug pool via
  `start_fpm_xdebug` accepting. ✓ **Human-verified in-app** (16 Jul 2026): the
  per-site toggle works on a real site.
- [x] **Watchdog races an in-flight Start-all.** Observed live (health.log 12:25:46Z,
  during the edge-daemon verification): a watchdog tick landed between `start_core`
  spawning MySQL/fpm and their readiness, saw "port closed", and killed + respawned
  them mid-start (benign outcome, but a needless kill of a healthy starting child —
  and a slow-to-boot MySQL could be respawn-looped into `gave-up`). The edge branch
  was already race-free (bootout-first stop, `aa79c93`). ✓ **Fixed** with the
  per-spawn-timestamp option (self-clearing — no caller has to remember to clear a
  flag on error paths, and it covers EVERY spawn path incl. the watchdog's own
  respawns, so a slow starter can't be respawn-looped): `Proc::Child` now carries
  its spawn `Instant` (stamped in `From<Child>`, compile-enforced everywhere);
  `Proc::starting()` = within `START_GRACE` (30s = 2× the longest readiness budget
  of 30×500ms); adopted survivors get NO grace (they were already serving). All
  five non-edge watchdog branches (DB engines, fpm pools incl. debug, override
  backends, Mailpit, nginx) now treat port-closed as dead only when the master is
  gone OR the grace has lapsed — a dead master is still reaped instantly, grace or
  not (a crash during start must restart), so the orphaned-workers detection is
  untouched. ✓ 268 lib tests (+2: grace semantics in proc.rs; a reap_dead sweep
  proving a live just-spawned pool with a closed port survives while a dead master
  is reaped despite grace), clippy (no new), examples build. Observe on the next
  few Start-alls: health.log should show no "restarted … port closed" events
  during startup.

- [x] **Configurable TLD (v1: default-TLD-for-new-sites)** — stored `default_tld`
  setting; policy-driven `validate_domain` (hard-block `.local`/gTLDs/2-letter in CORE —
  refused even via direct invoke; warn tier for non-RFC-2606 TLDs); answer-all DNS
  handler + one `/etc/resolver/<tld>` per TLD (first-use prompt, no DNS restart);
  uninstall sweeps all rexenv resolver files by content signature; `.test` backbone
  permanent; wp-login mu-plugin allow-list now per-site domain; Settings picker +
  TLD-aware dialogs. ✓ **Done** in 5 commits `bb1ba89`/`509ec95`/`7dad2de`/`dc6cb57`/
  `b1e103d`. **Corrected 12 Jul** (`e9a87d9`/`9b14cbe` + docs): `.rex` is the backbone —
  onboarding installs ONLY `/etc/resolver/rex`, v9 migration flips the default,
  `ADMINER_HOST` → `adminer.rexenv.rex`; `.test` is an ordinary safe-set choice whose
  resolver installs on demand. All self-checks green (225 lib tests, clippy, examples,
  tsc, vite); **awaiting human runtime verification** — Done-when checklist in
  `docs/TLD-FEATURE-REPORT.md`.
- [x] **Edge "stops by itself" — adopt a live edge, never kill it.** Root cause of the
  `health.log` `edge-down` entries: every edge death was rexenv's own `caddy stop` —
  (a) `prepare_edge` treated a LIVE edge as a stale leftover (`recover_stale_edge`)
  whenever the manager's state said stopped, so Start-all killed the healthy root edge
  and re-prompted for the password; (b) the watchdog was one-way (running→stopped,
  never back), so one stale edge-down left the UI lying and invited exactly that
  Start-all; (c) live-check examples share the real app-data dir and stop the edge via
  the shared admin socket (`health_watchdog_check` artifacts match the Jul 7 event
  exactly — see item below). ✓ **Done:** `prepare_edge` adopts+reloads a live edge
  (fresh-start path only if the reload is refused); `reconcile_health` re-adopts a live
  edge (`"adopted"` event, surfaced as an info toast); verified live against the running
  root edge — pid unchanged through both flows (`examples/edge_adopt_reload_check.rs`),
  `cargo test --lib` 153 green, site 200 through the edge after.
- [x] **Edge stays up unless explicitly stopped — root LaunchDaemon KeepAlive.** The
  remaining "Caddy stopped by itself" case was NOT rexenv: an external OS `SIGTERM` to
  the root edge (graceful `exit_code 0`, confirmed via `caddy-start.log`; not a crash,
  not sleep/wake, not a port conflict). The edge was the ONE service the health watchdog
  never auto-restarts (a privileged `:443` start needs an admin prompt), so any external
  kill left every site unreachable until a manual Start-all — and the `edge-down` toast
  only showed if the app window was open. ✓ **Done:** new `EdgeSupervisor` trait (10th
  platform trait) → macOS root LaunchDaemon `dev.rexenv.rexenv.edge` (`KeepAlive=true` +
  `RunAtLoad=true`); launchd relaunches the edge on any death/sleep/reboot.
  `proxy::start_edge_daemon` (stage unprivileged → one privileged `cp`-into-root-tree +
  `bootstrap`), `stop_edge_daemon` (`disable`+`bootout`, out of lock — Stop-all now costs
  one prompt), `CaddyHandle::Daemon` across start/adopt/stop/status, watchdog
  `edge-restarting` (info) instead of `edge-down` for a Daemon edge. **Security:** the
  daemon execs a `root:wheel 0755` COPY of caddy (never the user-writable cache — LPE
  guard); plist `root:wheel 0644`. `cargo test --lib` 229 green (+4 edge-daemon tests),
  examples build, `tsc` clean. Commits `4f45ff7`/`c15680d`. NOTE: the daemon installs on
  the next Start-all (first run re-prompts once); supersedes the osascript start path
  (kept only for the `caddy_443`/`service_manager_demo` examples). Partially overlaps the
  deferred **SMAppService** item below (that would fold the remaining setup prompts into
  one registration). **Follow-up (live Stop-all exposed 3 state-machine defects, fixed):**
  (1) Stop-all raced the watchdog — `stop_all` cleared the handle, the watchdog re-adopted
  the still-serving edge during the auth prompt, the bootout then landed → stale `Daemon`
  handle (health.log 11:56:08 `adopted` + 11:56:11 SIGTERM) → `stop_services` now boots
  out BEFORE touching manager state; (2) `edge-restarting` was an unbounded every-10s
  reassurance even for an edge never coming back → bounded: announce once, 3-poll grace,
  then DIAGNOSED `edge-down` (uninstalled/disabled via new `EdgeSupervisor::is_enabled`/
  blocked) + handle→Stopped; (3) `prepare_edge` trusted a stale non-Stopped handle → every
  Start-all silently skipped the edge — now liveness-checked (H2), stale handle falls
  through to a fresh start. Also `chown -h` in the launcher loop (root chown on a
  user-controlled path must not follow symlinks). ✓ 231 lib tests green incl. 2 new
  state-machine regression tests (mock platform). ✓ **Live-verified full cycle**
  (Jul 12): Start-all from a `disabled` label (re-enable+bootstrap) → external
  `sudo kill` self-healed in ~1s with ZERO health events → Stop-all prompt-first,
  edge stayed down, zero events across 35s (3+ polls) → Start-all-after-stop restored
  the edge, sites 200 over `:443` (`lm.rex`/`tr.rex`/`adminer.rexenv.rex`).
- [x] **Isolate live-check examples from the real stack.** `examples/*.rs` use
  `platform::current()` → the REAL app-data dir: their `start_all`/`stop_all`/
  `recover_stale_edge` stop the USER'S running edge over the shared admin socket (and
  restart shared services). Adopt-don't-kill removed the worst path, but an example's
  explicit `stop_all` still tears the stack down. Options: env-var app-data override for
  example runs, or a guard that refuses `stop_all` when the edge wasn't started by the
  example. ✓ **Done (guard option)** — sharing the real app-data is the POINT of a
  live check (cache + adopt paths), so the fix is provenance, not isolation:
  `core::stack_guard` — a non-app process may stop only what it SPAWNED
  (`Proc::Child`). Guarded chokepoints: `proxy::stop_edge` (skip+log),
  `recover_stale_edge` (refuse with a real error), `stop_stale_owned` orphan sweep
  (skip), and adopted-`Proc` skips in `stop_all` + `PhpFpmPools::stop_all` (Drop
  paths were already adopted-safe). App opens the guard via `mark_app_process()`
  in `lib::run`; deliberate utilities (`stack_stop`, `adopt_check`,
  `caddy_recovery_demo`, `service_manager_demo`, `mail_adopt_settings_check`)
  call `allow_real_stack_control()`; ad-hoc override `REXENV_CONTROL_REAL_STACK=1`.
  ✓ 269 lib tests, examples build. ✓ **Live-verified** (16 Jul,
  `examples/stack_guard_check` — no opt-in, real running stack): with edge +
  MySQL + MariaDB + 6 fpm pools serving, `recover_stale_edge` refused naming the
  guard, `stop_edge` skipped (admin socket stayed live), adopt (11 services) +
  `stop_all` left everything serving; `https://tr.rex` → 200 with
  `x-rexenv-edge: 1` after.
- [x] **L7 — move shared Nginx off 8088** (from BACKLOG). `8088` collides with Hadoop
  YARN / common dev proxies; moved to `18088`. Low risk: loopback-only, bind-tested at
  start. ✓ **Done:** `core/services.rs` `NGINX_HTTP_PORT` = 18088; configs regenerate
  from the constant on every stack start (`rebuild_configs_for`); tests/examples/mock/
  docs updated; `cargo test --lib` green. Transition note: a stack left running by a
  pre-change build keeps its old nginx on 8088 — it is not adopted (adoption keys on the
  new port) and not auto-reaped; kill it manually or via the old build's Stop all.
- [x] **In-flight download dedup** (download-manager follow-up, NOT a release blocker):
  add a per-(name,version) async once-lock (single-flight map) in `core/binaries.rs`
  `resolve*` so concurrent callers await the same download instead of racing — today the
  same bytes download twice and the hub progress bar jitters between the two streams.
  Pre-existing race (any two commands resolving the same binary); onboarding's
  auto-prefetch makes it easier to trigger. Correctness is fine (atomic staging/publish
  keeps one winner) — this is efficiency/polish. ✓ **Done:** `in_flight(name, version)`
  single-flight guard acquired first in `resolve`/`resolve_file`/`resolve_dir` — the
  second caller waits, then hits the cached-path early return; a failed first attempt
  lets the waiter download (natural retry). New serialization unit test; 167 lib tests
  green, clippy clean.
- [x] **Release 1.5 — cold first run on a second Mac / clean account** (from TASKS-RELEASE).
  Hands-on: install the .dmg on a machine that has never seen rexenv, run the full first-run
  flow. Pairs with the next item. ✓ **Done:** verified by a real fresh-account cold run on
  10 July 2026 — onboarding system setup, live binary downloads, WordPress site over HTTPS
  with a valid lock, rest of the app all worked end to end.
- [x] **`rex` CLI v1 — remote control for the running app** (design green-lit 16 Jul).
  Separate `cli/` crate (bin `rex`) that CANNOT link the app lib (structural
  second-brain guarantee — the examples-stop-the-edge class is impossible at compile
  time); one JSON line per request over `<config>/rexenv-cli.sock` (`0600`, unlinked
  at bind, connect-probed); app-side `cli_server.rs` dispatches to the SAME
  `commands::*` fns the UI calls. Surface: `status`, `start`/`stop`/`restart`,
  `site list`, `site create <domain> [--name --type --php --server --db]`,
  `site delete <domain> [--yes]` (confirm-gated), global `--json`; app not running →
  exit 2 with a clear message. ✓ **Done** in 4 commits `d08d157`/`9b8e01e`/`a9e4963`/
  `164fed9`: 273 lib tests (+4), clippy no new, examples build; **live-verified** via
  `examples/cli_socket_check` against the real stack (13 adopted services, real DNS
  probe, `rex status`/`--json`, not-running exit path) and a full site round-trip
  (`rex site create clitest.rex --type php --php 8.4` → listed serving + 200 over
  HTTPS through the real edge; delete → row gone, wire dead, docroot removed).
  **Human-verify remaining:** `rex start/stop/restart` need the NEW app build running
  (the socket server ships with it) — exercise on the next app run.
- [x] **Orphaned httpd blocks Start-all + "sudo kill our own process" message**
  (found by the first live `rex start`, 16 Jul). ROOT CAUSE: adoption picked the
  lowest listener pid on the claim "masters fork first" — false under worker churn
  + pid recycling (live: Apache worker 71063 sat below master 95274) → a WORKER got
  adopted → Stop-all killed the worker → the surviving master held `:8320` → the
  port gate told the user to sudo-kill rexenv's own orphan. Latent for every
  adopted service (nginx/fpm/DB/Mailpit workers share the listen socket too).
  ✓ **Fixed** in 3 commits: `af1d248` `owned_master` (parent-based master selection,
  pure `select_master` + 5 tests incl. wraparound; adoption switched); `e8d31ff`
  override stops resolve the CURRENT master + `ports::wait_free` drain (stopped ==
  port free), spawn_override self-heals (our leftover reaped, foreign → honest
  error), plus a pre-existing guard gap closed (reconcile's stale/mismatch stops
  now refuse adopted backends outside the app — the likely source of the orphan);
  `372f1f0` marker-aware `ensure_free` (ours → plain `kill`, no sudo; foreign →
  Herd-style help unchanged). 280 lib tests (+7 across the round), clippy clean.
  ✓ **Human-verified:** rex stop frees `:8320` (master gone, no survivors), rex
  start/restart clean, Apache serves, health.log quiet.
- [x] **Apache reconcile bounce check** (observation, low): the 16 Jul log showed
  two Apache restarts 19s apart across app launches. After the fix round, run
  Start-all twice with no stop between — the Apache pid should be STABLE on the
  second run. If it changes every time, reconcile's config diff has a
  session-dependent input; chase `desired_override_config` vs the on-disk conf.
  ✓ **Closed 16 Jul (human-verified):** back-to-back Start-alls, Apache pid STABLE
  — reconcile does not bounce; the earlier double-restart was one-time conf drift
  from the pre-fix build's session, not a persistent diff.
- [x] **`rex` CLI packaging** — ship the `rex` binary in the app bundle + a
  Settings/onboarding "install CLI" step (symlink into PATH, Herd/Docker-style).
  v1 builds from `cli/` only; not release-blocking until the CLI is user-facing.
  ✓ **Done** in 3 commits: `0a85cec` sidecar bundling (`scripts/build-cli.sh`
  stages aarch64+x86_64+universal as `externalBin`; build.rs self-stages so a
  fresh clone's bare cargo build works — verified broken without; debug bundle
  contains a signed, working `Contents/MacOS/rex`); `40e2630` install backend
  (`core/cli.rs` status/install — unprivileged symlink first, one-prompt
  `PrivilegeManager` fallback, read_link-verified; `Paths::cli_symlink_path` =
  `/usr/local/bin/rex`; teardown removes OUR link only, content-checked;
  283 lib tests, +3); `4037b71` Settings General-tab card (status-aware
  Install/Reinstall, link+target shown). **Human-verify:** Settings → Install
  (expect one admin prompt) → `rex status` from a fresh terminal.
- [x] **`rex` CLI roadmap — work through `docs/CLI-ROADMAP.md`** (created 16 Jul).
  ✓ **Cheap tier COMPLETE, same day** — 42 commands shipped across 12 commits
  (`b05f09d` … `23403f1`), each live-verified against the real stack where the
  example-harness stack guard allows (per-command evidence in the roadmap doc):
  full site lifecycle incl. `--blueprint`/`--multisite`, logs `--follow`,
  `doctor`, db export/import/reset/versions, PHP + Xdebug + ini settings, the
  whole WP plugin/theme/user manager + singles (maintenance wire-proven
  503→200), service/mail/tunnel/tld/version, zsh+bash completions.
- [ ] **`rex` CLI — remaining** (see the roadmap doc's Status section):
  (a) in-app verifies owed for the guard-blocked passthrough halves
  (`php install/uninstall`, `php settings set`, `db versions --set`,
  `site server/domain/move`, `mail clear`, `tunnel start`, `wp core
  update/switch`) — fold into the next deep test; (b) `config get|set` parked
  on an allow-list decision; (c) the 🔴 design-first set (single-site restart,
  web-tier singles, wp passthrough, `wp_user_delete`, progress streaming).
- [ ] **Release 5.4 — execute the clean-Mac smoke test** — checklist already written:
  `docs/SMOKE-TEST.md`. First pass (fresh-account, 10 Jul 2026) all green except the
  multisite-convert item, untestable because the convert UI didn't exist — fixed below;
  re-verify converted-multisite items + onboarding fixes on the next cold run.

## Asset follow-ups — plan green-lit 18 Jul 2026 (phases A-D; build in order)

- [x] **Phase A — asset foundation + adopt + status-driven delete safety**
  (18 Jul, 1 commit). v13 `site_git_assets.source` (cloned|adopted|linked —
  linked deletes must UNLINK; backfill exact). `core/repo.rs`:
  `parse_status_v2` (PURE; tested against detached HEAD, no-upstream,
  dirty+ahead, unborn), `loss_warning` (exact sentences, Rust-tested —
  "3 changed files, 2 untracked files, and 2 unpushed commits will be
  lost."), `read_git_status`/`read_remote_url` (local, capped, no repo
  code), `scan_unmanaged` (.git dir OR file — worktrees; symlink detection;
  tested incl. symlink fixture). IPC: `repo_asset_status` (+lossWarning,
  +logKey), `repo_unmanaged`, `repo_adopt` (metadata only). UI: git badge →
  button opening the per-asset RepoPanel (branch/clean-dirty/↑↓ vs
  upstream/remote/source + last-job log inline; row expansion, themes
  col-span-full); dashed "git?" adopt chips; plugin single+bulk delete and
  theme delete confirms show the loss warning verbatim for git assets.
  ✓ 310 lib tests (+3), tsc, vite build, examples, clippy no new; WebKit
  harness `?panel=repo` ALL PASS + rehydrate/interactive re-run green.
  ✓ **Human-verified 18 Jul** (§9: exact counts named in single+bulk
  confirms, no cry-wolf on clean+pushed, no-upstream caveat, adopt
  round-trip survives restart, Refresh tracks dirtying).
- [x] **Phase B — git ops as jobs in the RepoPanel** (18 Jul, 1 commit).
  core: validate_ref (argv-trick guard, tested), git_fetch/--prune,
  git_pull_ff (--ff-only ONLY — diverged = honest error), git_checkout
  (DWIM remote-tracking), git_push (auto --set-upstream when missing,
  never force), map_git_op_error (diverged/dirty-overwrite/pathspec/
  non-ff → house messages, else falls through to the clone-era auth/
  offline mapping), lockfile_fingerprint (lock+manifest files only,
  tested), run_git_lines (branch listings). commands: repo_git_op (same
  registry/events/cancel/one-job-per-dest as add; checkout updates
  provenance git_ref; changed fingerprint → install/build steps OFFERED
  on the job — explicit clicks), repo_branches; op field on RepoJobState
  (add panel adopts only op=="add"; RepoPanel reconnects to its own op
  jobs with tail-seeded log). UI: ops row (Fetch/Pull/Push + branch
  dropdown + Checkout), op-job card with mapped errors + deps-changed
  offer + disclosure. ✓ 313 lib tests (+3), tsc, vite build; **live**
  `examples/repo_git_ops_check` ALL PASS against a LOCAL bare origin
  (ff-pull file arrival, DIVERGED + dirty + non-ff all mapped, checkout
  lockfile flip, push seen at origin, auto-upstream stuck); WebKit
  `?panel=repo` extended (ops row + Pull → op card + offer + disclosure)
  ALL PASS, rehydrate/interactive re-run green. **Awaiting human verify —
  test-doc §10** (real remotes/agent + packaged-app panel).
- [x] **Phase C — scripts + watch** (18 Jul, 1 commit). core: list_scripts
  (pathological names dropped, command shown; tested), is_watchy heuristic
  (dev/watch/start/serve/hot + prefixes + contains-watch; wp-scripts start
  IS watch — tested), node_run_script (shared by one-shot jobs + watchers —
  the watcher IS run_step_streamed on a dedicated thread, zero new exec
  machinery). commands: repo_scripts, repo_script_job (op=="script" job,
  same registry/busy/cancel; re-run = fresh job — repo_run_step flipped to
  an ALLOW-list {composer,install,build}), RepoWatches registry (uuid ids
  for event names — dir names can contain dots; one watcher per asset
  under one lock; 400-line ring + repo-<domain>-<dir>-watch.log which the
  Logs tab picks up automatically; exited keeps code + Restart, NEVER
  auto-restart), repo_watch_start/stop/list/log, repo-watch-global event,
  exit hook extended (watchers die with the app). UI: scripts row under
  the disclosure (Watch:/Run: split), watch card (dot/Stop/exited-code/
  Restart/ring-seeded output), StatusFooter chip ("watching <dir>" / "N
  watchers running"). ✓ 314 lib tests (+1), tsc, vite build; live
  `examples/repo_watch_check` ALL PASS (ticks streamed, group 2→0 procs on
  stop, crash exit=3 surfaced); WebKit watch scenario ALL PASS (+3 prior
  re-run green). **Awaiting human verify — test-doc §11** (real wp-scripts
  watcher, footer chip, die-with-app on quit, manual-kill → Restart).
- [x] **Phase D — link folder + the unlink-only delete guard** (18 Jul,
  1 commit). traits: ShellRunner::symlink_dir / remove_symlink (macOS impls;
  remove_symlink itself REFUSES non-symlinks — defense in depth). core:
  validate_link_target (canonicalized; refuses self-link, docroot-containing
  CYCLE, collision incl. dangling links — tested), partition_symlink_deletes
  (FILESYSTEM truth via symlink_metadata, NOT provenance — a never-adopted
  manual `ln -s` is protected too; tested). commands: wp_plugin_delete /
  wp_theme_delete now partition: symlinks → best-effort deactivate + unlink
  (active linked THEME refused via wp option get stylesheet), rest → wp-cli;
  provenance rows cleaned for everything deleted — covers UI single, UI
  bulk, and the rex CLI (same command fns). repo_link (canonical target,
  provenance source=linked only for git checkouts w/ best-effort remote/
  branch; non-git folders link fine, guarded by fs truth), linkTarget on
  asset status. UI: third source tab "Link folder" (picker via the existing
  pickFolder, name prefill, honest copy), calm linked-delete confirm
  ("removes only the link"), RepoPanel "→ target" line. ✓ 316 lib tests
  (+2), tsc, vite build, examples, clippy no new; **live**
  `examples/repo_link_check` ALL PASS — link lands + detects through the
  link, partition correct, UNLINK LEFT THE TARGET BYTE-INTACT (uncommitted
  work + .git survived), remove_symlink refused a real dir, all three
  validations refused; WebKit link scenario ALL PASS (+4 prior re-run
  green). **Awaiting human verify — test-doc §12** (the delete test through
  real WordPress, bulk path, active linked theme, serving on all three).
- [x] **Ref picker — searchable branches + tags + PR refs, honest detached
  HEAD** (23 Jul, 4 commits: 8477fca → 50ae273 → f2d0f62 → daa9d76). UI: the
  ~100-branch `<select>` replaced by RefPicker (cmdk combobox in a Menu-style
  portal — filter input, arrow/Enter/Esc, groups local/Remote/Tags/Pull
  Requests; picking only SETS the target, Checkout still fires the op; cmdk
  onSelect arg is normalized so handlers close over the case-sensitive name).
  Detached honesty: `?? 0` ahead/behind false-zero → "?", Pull/Push disabled
  when detached with reason, "no upstream" chip suppressed, "detached @
  <tag|sha>" via read_detached_at (describe --tags --exact-match, NOT --all
  — would name a branch while detached), pull's "not currently on a branch"
  mapped honestly. Tags: repo_branches += for-each-ref -creatordate; fetch →
  --prune --tags --force (moved rolling tag otherwise fails EVERY fetch,
  git ≥2.20; never --prune-tags); checkout target refs/tags/<name> (exact,
  no DWIM, detached). PRs: refs-only, NO host API/token — parse/list_pull_refs
  (both refs/pull/*/head + refs/merge-requests/*/head in ONE ls-remote, 30s
  probe cap, numeric sort); checkout = one-shot argv-refspec fetch (no config
  write, plain fetch drags zero PR refs) + checkout --detach FETCH_HEAD
  (hardcoded, never crosses IPC). CLI: branches prints tags:, new `rex repo
  prs`, checkout <ref>; roadmap rows. ✓ 377 lib tests (+4: fetch_args,
  pull-ref parser/routing, detached-pull mapping), tsc, examples + cli build;
  real-git fixture (tag sort incl. lightweight, refs/tags checkout →
  (detached) + describe names it, moved-tag reject/--force, PR-ref fetch →
  FETCH_HEAD detached at PR sha); WebKit `?panel=repo` (+`&detached=1`,
  `&prs=none`): filter/keyboard matrix, lazy PR load, honest notes ALL PASS.
  **Awaiting human verify** — packaged app on a real many-branch repo: tag +
  PR checkout land in the detached view, `rex repo prs`, Fetch on a repo
  with a moved rolling tag.
- [x] **Deps: zero-exec "Check deps" + "Run all" steps** (23 Jul, 2 commits:
  ae88c11 → cfce970). Check = detect-only job (op=check, own -check.log slot
  so casual checks never wipe an op/build log): presence probes (vendor-dir
  override honored) + stored per-manager fingerprints — pinned FNV-1a with
  "fnv1a:1:" prefix (DefaultHasher unstable across toolchains; foreign
  prefix → unverified, never false-stale), v15 migration two NULLABLE
  columns, NULL = "present (unverified)" NOT stale (existing-user bar,
  test-covered on a pre-v15 row); only Missing/Stale offer steps; mtime
  staleness rejected (checkout rewrites mtimes). Run all = cli_server's
  --install loop PROMOTED to run_offered_steps (one impl, panel + CLI;
  old poll loop deleted): whole-sequence step_running hold closes the
  between-step busy-guard races (sequence-start window remains, stated),
  stop-at-first-failure, never-ran steps = NEW "skipped" status ("»",
  distinct from pending/cancelled, still re-runnable), cancel → current
  cancelled + rest skipped, cancelled flag persistence makes "nothing
  else starts" structural. CLI `repo check [--install]`. ✓ 382 lib tests
  (+7 across both), examples + cli build, tsc; **live**
  `examples/repo_run_all_check` ALL PASS (composer ✓ / install ✕ / build
  SKIPPED; cancel-mid-install → cancelled + skipped; failed install
  records NO fingerprint); WebKit harness (check card offers/clean,
  ✓✕» dots, honest suffix) ALL PASS. ✓ Human-verified 23 Jul (packaged):
  fp write→up-to-date→stale live flip, ✓/✕/» card, cancel case.
- [x] **wp.org add flow: chips above input + honest streamed installs**
  (24 Jul, 3 commits: 5455ed0 → 52bd390 → dfcca77). Chips: SlugTag row
  lifted ABOVE the search input (both panels; input keeps flex-1 width;
  queue/remove refocus the input; dropdown anchor unchanged). Streamed
  installs (B25 constraint: wp-cli opaque mid-download, NO byte signal →
  NO percentage, no invented phases): thin WpInstallJobs registry (seq-
  ordered) over the repo streaming primitives (run_step_streamed
  idle=None — a 300s idle guard would tie-race wp-cli's own 300s
  download_url bound; B25 outer cap 120s+900s·N survives as a record-
  reason-then-cancel timer), per-JOB logs (keep 5/domain), events
  wp-install://state|output. Honesty: phase label = last line VERBATIM
  (only two pinned-phar literals parsed: per-item header → ATTEMPT
  cursor "item k of N", and the Success:/Error: summary); exit1+"Only
  installed" ⇒ partial (never flattened to failed); "ok" never claims
  activation (chained --activate failures don't touch exit code — list
  refresh is that truth); Cancel visible from start + "no output for Ns"
  ticker (cancel SAFE: fresh installs place via atomic rename — verified
  in WP's upgrader). Stage 3: CLI arms ride the same job (captured Tauri
  cmds + wrappers retired; CORE fns stay — blueprints use them). ✓ 383
  lib tests, examples + cli build, tsc; **live**
  `examples/wp_install_stream_check` ALL PASS (real WP+MySQL: multi-slug
  phases in order + cursor 2/2 + verbatim summary; delete+reinstall
  printed "Using cached file" — HOME survives env_clear+snapshot; cancel
  fired on the Downloading line → cancelled, nothing else ran);
  `examples/cli_wp_install_check` BEFORE/AFTER envelope capture —
  success `{"data":null,"ok":true}` identical, failure Warning/Error
  lines verbatim-identical (prefix only: drops debug "Some(1)"), exit
  codes + --json unchanged; WebKit 8/8 chips + 11/11 card states.
  **Awaiting human verify (packaged)** — chip wrap above input, live
  phases + Cancel-from-start on a real install, cancel-mid-download
  honest copy, `rex wp plugin install` unchanged. Known latent, flagged
  not fixed: wp_plugins_check passes the mysql BASE DIR as
  install_for_site's db_client (client binary expected).
- [x] **Phase-based install bar** (24 Jul, a7ebcec). The install card's
  indeterminate bar became step-wise HONEST progress: per-item milestone
  slices (fetched/unpacked/placing/installed/+activated only when
  requested — 4 or 5 slices, no unfillable slice), observed DISCRETE
  progress (every tick = a line wp-cli printed) — NOT the byte estimate
  B25 bans, distinction documented at every layer. Monotonic; forward
  implication (later milestone ⇒ earlier ones; new header ⇒ previous
  item complete — cached files, headerless already-installed slugs,
  missing activation lines JUMP the bar, never stall it); 100 ONLY on
  the batch-terminal literal — bare "Success:" rejected because chained
  THEME activation prints "Success: Switched to…" MID-batch (verified
  unguarded in the 2.12.0 phar); failure/cancel FREEZE in place (new
  Track "stopped" variant). ✓ 390 lib tests (7 new); **live** pct stream
  [0,12,25,37,50,50,62,75,87,99,100,100] exactly per table, cancel froze
  at 25; WebKit 19/19; packaged CLI installs streamed end-to-end.
  ✓ Human-verified 24 Jul (packaged, real multi-slug).
- [x] **Streamed site provisioning: honest New Site progress + v16 +
  retry** (24 Jul, 3 commits: dc5c7e9 → 8d2720d → 7dcfcfe). create_site's
  opaque await became a job (site_provision.rs, wp_install pattern):
  phases = OUR step boundaries (prepare inline → fetch → db →
  core_download → configure → core_install → [blueprint] → serve), zero
  output parsing, per-job logs (keep 4/domain). ProvisionProgress: FIXED
  coarse weights 3/27/5/35/5/10/5/10 renormalized over applicable phases
  (weights ≠ estimates: bar moves only on real completions + REAL Hub
  bytes folded into the fetch slice); monotonic, ≤99 until settle-ok,
  frozen on fail/cancel. Cancel never crosses jobs: prefetch DETACHED
  (aborting a shared Hub download would truncate-restart the next
  resolve — verified resume_from starts 0 per resolve), pgid kill only
  on own wp-cli children, shared services boundary-checked. v16
  sites.provisioned DEFAULT 1 (existing rows never badge); fail/cancel ⇒
  0 + "setup incomplete" pill + Retry (re-ENSURES docroot/index.php-if-
  missing/cert, re-enters idempotent steps) / Delete. B25 gap closed:
  core download/install were captured with NO cap → streamed under
  download_timeout(1) timers. Blueprint items stream per-item (captured
  plugin_install/theme_install retired). create_site = thin start+await
  wrapper (same blocking contract; failure reply names failing phase +
  log path); CLI rides it output-unchanged. adopt_dbs = DB-only adoption
  so fixtures never touch the edge. ✓ 396 lib tests (5 ProvisionProgress
  + v16 existing-rows); **live** `examples/site_provision_check` ALL
  PASS (staircase [0,31,36,73,78,89,99,100] per table; deterministic
  locale failure froze at 36 + provisioned=0 → RETRY ok@100 →
  provisioned=1, re-retry refused; cancel frozen 36); WebKit 27/27
  (adopt/running, fetch byte rows 12.4/34.0 MB · 2.1 MB/s + no-length
  item, ok stack-stopped summary, failed/cancelled frozen, badge+Retry→
  card); **packaged** CLI create verbatim-identical output both paths
  (success ✓ line + dup-domain "rex: domain already in use" exit 1),
  site genuinely SERVED (curl 200 — full serve phase), job log all
  phase markers, --json shape unchanged except additive "provisioned".
  **Awaiting human verify (packaged GUI)** — New Site card phases live,
  close-dialog→Sites re-adopt, cancel → badge → Retry.

## QA round — add-from-Git (18 Jul 2026)

## QA round — add-from-Git (18 Jul 2026)

- [x] **Running git job disappears on tab switch (dangerous — invited a
  double clone/install).** Cause: the backend job registry (RepoJobs)
  survives the WordPress-tab unmount and keeps streaming, but GitAddPanel
  held job+log in component state and had no way back to the live job id on
  remount — same class as the mount-frozen DNS tile. ✓ **Fixed:** new
  `repo_site_jobs(site, kind)` IPC (creation-ordered via a registry seq);
  panel adopts the newest unfinished job on mount, re-subscribes, and seeds
  its log pane from the job's log file via the existing tail IPC
  (`log_key` now in the snapshot; overlap-deduped merge with lines that
  stream during the tail fetch); live snapshots sync into the shared
  ["repo-jobs"] query cache so the "From Git" tab shows a spinner even from
  the wp.org tab (2.5s poll only while running); `repo_add` now refuses a
  second job for a dest with a running step under ONE registry lock (and
  truncates the log only after that check — never a running job's log);
  job card gained a `dir · url @ ref` header so a reconnected card names
  itself. ✓ 307 lib tests, tsc, vite build, WebKit harness: new
  `?rehydrate=1` scenario ALL PASS (zero-click adoption, seeded log,
  Cancel visible; blank mode still blank) + prior checks re-run green.
  ✓ **Human-verified 18 Jul** (§8: reconnect mid-clone with seeded log,
  wp.org-tab spinner, duplicate-Add refused, cancel-from-reconnected clean).

## QA round 1 — light issues (deferred, recorded 16 Jul 2026)

QA green-lit the round (no majors). These are the agreed-deferred light items so
they aren't lost; fix opportunistically or before the next deep test.

- [x] **Settings doesn't render `dns_status.mode`** — backend field + TS type
  exist (see the DNS-LaunchAgent item above); the Settings DNS card should say
  agent / in-process / down so a fallback-mode session is visible. ✓ **Done:**
  mode was already end-to-end (pure UI render). Settings DNS tile: mode in the
  mono status line + meaning line — agent green "Always on — resolves even when
  rexenv is closed", in-process AMBER dot + "DNS stops when you quit rexenv.
  Restart the app to retry the always-on agent", inactive unchanged. Services
  DNS row: same honest amber for in-process; stale "runs with the app" header
  copy fixed (agent survives quits). Follow-up fix `9338b32`: the Settings tile
  was FROZEN at mount-time state (Services was the only ["dns-status"] poller
  and it unmounts when Settings shows) — Settings now polls the same shared key
  at 5s, so both views read one cache entry. ✓ **Human-verified** (16 Jul):
  agent bootout while sitting on Settings → amber "in-process" within ~30s on
  BOTH views in sync; app relaunch → both green "agent / always on"; sites kept
  resolving throughout.
- [ ] **WP Manager cron list: arguments display** — QA to supply the exact
  complaint (recorded as a placeholder so it isn't lost; likely the event args
  column in the SiteDetail cron tab).
- [x] **Watchdog/Start-all race fix (`d07e4f2`) — observation window still open.**
  Code + regression tests are in; confirm `logs/health.log` shows no
  `restarted … port closed` events across the next few real Start-alls, then close.
  ✓ **Closed 16 Jul:** health.log stayed quiet through repeated rex stop/start/
  restart cycles (human-verified during the orphan-httpd fix round).
- [ ] **TLD v1 Done-when checklist not formally walked**
  (`docs/TLD-FEATURE-REPORT.md`) — de-facto mostly proven by later live work on
  `.rex` sites (auto-start reboot test, Herd suite), but the checklist itself
  was never ticked; walk it or fold into the next clean-Mac smoke test.

(Onboarding locked-visual eyeball + converted-multisite re-verify are already
tracked in the open Release 5.4 smoke-test item — not duplicated here.)

## Smoke-test fallout (10 Jul 2026 fresh-account run) — all fixed

- [x] **Multisite convert missing from the UI** (spec §2.1 "one-click enable/convert").
  Never surfaced, not a regression: §10.1 shipped backend+IPC only, §10.3's Network tab
  was gated multisite-only, and 12.3's toggle was create-time only — the convert-an-
  existing-site seam fell between the three. ✓ **Done** `f72ee5c`: Network sub-tab
  always shows; single sites get a convert panel (subdomain/subdirectory cards, wp-config
  + URL-structure warning, confirm) that flips to the network manager on success.
  Verified live both modes: subdirectory sub-site at `/site1`, subdomain sub-site over
  HTTPS with a valid lock (wildcard cert/route end-to-end), and Reset on a converted
  site returns a clean single-site install.
- [x] **Onboarding "Domains & SSL" was skippable** → app where no site loads (no
  resolver/CA). ✓ **Done** `fbe72be`: Continue locked until `dns_status` reports
  `resolverInstalled && caTrusted` (real state, refetched after every setup attempt);
  Welcome's "Skip setup" removed (same hole). No dead-end: setup stays retryable with
  the friendly cancelled-prompt error, window stays quittable. Logic verified; locked
  visual to be eyeballed on the next cold run (warm machines always show it unlocked).
- [x] **Onboarding window not draggable** (renders outside AppShell → no drag-region
  header). ✓ **Done** `d99d095`: 60px title-bar drag strip wired to the shared
  `onTitleBarMouseDown` (drag + double-click maximize). Verified via `#/onboarding` on
  tauri dev.
- [x] **Onboarding content clipped at small window heights** — flex-1 without min-h-0
  pushed the footer (Continue) out of the overflow-hidden root, no scroll. ✓ **Done**
  `6031a34`: content scrolls under a pinned footer; inner min-h-full wrapper keeps steps
  centered when there's room. Verified at the 640px minimum on the Install step.
- [x] **DB-export toast: "Show in Finder"** (nicety). ✓ **Done** `2d243f8`: new
  `ShellRunner::reveal` (macOS `open -R`, win/linux `todo!()`), thin `reveal_path`
  command, toasts gained an optional action button (10s TTL); both export call sites
  (Tools + Reset dialog) reveal the written `.sql`. Verified live from both.

## ✓ Shipped — 11 Jul 2026 session (Site Settings + WP Manager expansion)

Each line = one feature, live-verified before its commit.

- [x] Site Settings tab v1 — rename, site info (type / DB name / multisite), HTTPS
  cert card (issued/expires/SANs/folder via `ssl::site_cert_info`) `5f091c1`
- [x] Maintenance mode toggle (Tools) `edd3255`
- [x] Debug-constant toggles — WP_DEBUG_LOG / WP_DEBUG_DISPLAY / SCRIPT_DEBUG,
  core-whitelisted `baae862`
- [x] Permalink structure picker (stock presets, honest Custom display) `780c8c2`
- [x] Cache flush + delete-all-transients `8b69ffb`
- [x] Per-user role dropdown; primary-admin guard enforced in core `b4416ce`
- [x] Cron viewer + run-due + per-hook forceful run `3b95253`
- [x] Core checksum verify with benign/real triage (exit code never drives the
  verdict) `6b31503`
- [x] DB import — typed confirm + backup-first, bundled mysql over stdin `fe7654b`
- [x] WXR content export to Downloads `59c400c`
- [x] Tools regrouped: Backup & restore / Core / Maintenance cards `66d528e`
- [x] Cert regenerate — per-site Regenerate in the SiteDetail Settings-tab cert card,
  plus a fix for the global Settings button: a byte-identical Caddyfile makes plain
  `caddy reload` a no-op, so re-issued certs were never served until an edge restart —
  both paths now force-reload; re-issue is atomic (temp-write + rename, never
  delete-first), so a failure leaves the old cert intact and served `57b7281`
- [x] Site language switch — Tools Language card: picker (Installed/Available) from
  `wp language core list`, install-if-needed + `wp site switch-language` in one action;
  success gated on `language core is-installed`, NOT install's exit code (a failed/
  offline download still exits 0 — same trap class as checksum verify); install capped
  at 60s wall-clock (WP's download_url waits 300s/attempt — offline that froze the
  spinner; timed-out child is SIGKILLed, error surfaces, old language kept); core
  translations only, multisite note (main site only) shown honestly `ed7d74e`
- [x] Checksum "Clean up macOS system files" — deletes the panel's benign list with
  four backend guards per file (is_os_noise basename reused, relative/no-`..`, lstat
  regular-file — the guard that stops a `.DS_Store` symlink from deleting its TARGET —
  canonicalize+prefix inside docroot); skip-not-abort with reasons; auto re-verify
  refreshes the panel in the same round-trip `24d35bd`
- [x] Site options editor — curated 11-option scalar whitelist (default-deny: siteurl/
  home/active_plugins/serialized options unreachable by construction, `OPTION_FIELDS`
  enum in core enforces name + per-kind value on every write); non-scalar values
  refused at read AND write; typed inputs (timezone/role pickers fed live), confirm
  old → new, form re-reads from the site after save `fe80fb4`
- [x] Core version switch / downgrade — picker from the stable-check API (831
  releases, ≥6.0, insecure marked), `wp core update --version --force` under a 300s
  cap, success gated on `wp core version == target` (never the command's claim),
  post-switch db_version probe returns dbUpdateRequired so the panel states the
  "Database Update Required" screen explicitly; honest confirm + Export-DB-first in
  the flow `0d3a6e5`

## PHP-versions gap vs Herd/Valet (scoped 11 Jul 2026; #3 built, #1/#2 deferred)

- [x] **Per-version PHP settings (#3)** — memory_limit / upload_max_filesize /
  post_max_size / max_execution_time / max_input_time / max_input_vars, per minor
  (matches shared per-version pools). Whitelisted+typed validation (`core::php::SETTINGS`,
  default-deny like the options editor), `php-fpm -t` gate on a `.conf.candidate` before
  any restart, values as `php_value[…]` lines in the pool conf, SQLite `php_settings`
  table (migration v5). Both gotchas handled: per-site nginx `client_max_body_size`
  mirrors max(upload, post) of the site's version + upload>post rejected as a set;
  `request_terminate_timeout` rises to max_execution_time (300s floor, cap surfaced in
  the UI note). FrankenPHP sites unaffected (own embedded PHP — stated in the UI).
  Settings → PHP versions → per-installed-version "Settings" editor. ✓ **Done**
  `70266a7`: live-verified on a throwaway site — 512M via ini_get; 64M upload/post lets
  a >2M wp-admin Media upload through with `client_max_body_size 67108864` in that
  site's nginx block; upload-alone rejected (cross-field); "banana" rejected with no
  restart; cleared field reverts to default. 195 lib tests, clippy clean.
- [x] **More PHP versions (#1):** 8.0.30 / 8.4.23 / 8.5.8 pinned — offered set is now
  8.0–8.5. **7.4 stays absent:** static-php never shipped it — needs self-build +
  self-hosting, same blocked path as the Xdebug debug build. ✓ **Done** `ac19d31`: all
  12 artifacts downloaded, hashed, extracted, arch-checked, and RUN (arm64 native +
  x86_64 Rosetta; `php -v` + mysqli verified on each — no guessed hashes); registry/
  ports/pools/hub/UI flowed through untouched. Live-verified: 8.0 + 8.4 installed from
  the UI with real download progress, sites assigned show the right version, WordPress
  loads on 8.0 (bulk extension set OK on the oldest). Ongoing risk (accepted): upstream
  rebuilds in place → 24 pins to babysit.
- [x] **PHP patch updates (#2): Option A only** — pins ride app releases; NO in-app
  TOFU updater (static-php publishes no checksums — runtime update-discovery would move
  pin trust from the signed app binary to the user's machine). ✓ **Done** `ef0465d`:
  `seed_registry` reports stored-patch ≠ build-pin (one-shot, `installed` preserved);
  startup task prefetches the new patch via the hub BEFORE any lock (offline = old pool
  keeps serving, retried next launch), restarts bumped minors' live pools through the
  same `restart_php_pool` path as the settings editor, then GCs `php-<oldpatch>/`
  caches (narrow name rule — never debug builds/staging/other binaries/unpinned
  minors). Live-verified via DB simulation: pool restarted on the pin, site kept
  serving, fake stale cache GC'd + logged, second relaunch a no-op. Maintainer
  release procedure = bump `PHP_VERSIONS` + re-verify pins (binaries.rs:96) + update
  `docs/PORTS.md`; everything else is automatic.

## Parked (deliberate — needs explicit go; don't pick up silently)

- [ ] **Install WordPress into an empty LINKED folder** — deliberately left out of Stage 0
  (`docs/PLAN-linked-sites.md` §11.2). Linking today is adopt-only: we serve what's there
  and write nothing. Installing a full WP core into the user's OWN directory is a
  different operation and needs its own explicit flow with its own disclosure — not a
  checkbox on a feature whose promise is "we don't touch your folder". Shape if picked
  up: a deliberate action from the site page (not New Site), only offered when the linked
  folder is EMPTY (`detect_project` → `existing_install == false`), with a confirm naming
  exactly what gets written and where. The backend seam exists: `phase_defs`
  (`commands/site_provision.rs:130`) currently omits the WP phases whenever
  `docroot_managed == Some(false)`, so this would need an explicit opt-in flag rather than
  the blanket `linked` check. **Trap to fix first if built:** the `configure` phase is only
  half idempotent — it skips `wp config create` when wp-config.php exists but calls
  `create_database` unconditionally (`site_provision.rs:691-700`).
- [ ] **`teardown` never removes the Apache per-site config/log** — pre-existing gap found
  while mapping the delete path for Stage 0 (`docs/PLAN-linked-sites.md` §3).
  `core::sites::teardown` sweeps the FrankenPHP override config + log and the tunnel log
  (`core/sites.rs:625-634`) but never `apache::config_path` / `apache::log_path`
  (`core/apache.rs:168,177`), which exist and are written for every Apache override site.
  `change_site_domain`'s old-artifact cleanup (`commands/sites.rs:671-681`) has the same
  omission, so renaming an Apache site strands the old-domain conf too. Both are app-data
  files (never the user's), so the fix is additive and low-risk: add the two paths to
  teardown's best-effort sweep and to the domain-change cleanup, plus a test asserting an
  Apache site's conf/log are gone after teardown (the existing FrankenPHP assertion in
  `teardown_removes_row_and_per_site_artifacts` is the template).

- [x] **Change domain** — cross-cutting: cert re-issue + config regen + WP search-replace,
  and the DB name derives from the domain (L). ✓ **Done** in two commits:
  `688142a` (prerequisite) stores `db_name` on the site row (v6 migration backfills,
  every runtime reader switched — user-verified reset + export/import round-trip on a
  pre-migration site); `b0975b3` — `change_site_domain` orchestrates preflight (validate +
  uniqueness + multisite REFUSED honestly) → mandatory Downloads backup (abort on
  fail) → new-domain cert (additive) → `wp search-replace` dry-run gate then two real
  passes (`https://old→https://new`, bare `old→new`, `--all-tables`) → SQLite domain
  flip (docroot + db_name untouched by design) → config regen + forced edge reload →
  best-effort old-artifact cleanup (cert dir, tunnel, FrankenPHP config/logs). DNS
  needs nothing (wildcard `*.test`). UI: Settings → Domain card (disabled on
  multisite), destructive dialog with backup/email-rewrite/reversal notes; frontend
  `siteDbName()` derivation deleted in favor of stored `site.dbName`. Live-verified
  on a throwaway site (post + image): myapp.test → myshop.test served with valid
  SANs, no redirect to the old domain, serialized/attachment URLs rewritten, old
  domain no longer routes, reset works after the change (db_name consistent),
  multisite card disabled, old cert dir removed, backup in Downloads.
- [x] **Move site / custom docroot** — fixed `sites_dir` scheme + nginx root regen (M/L).
  ✓ **Done** `c098077`: `move_site_docroot` — preflight rejections before any file is
  touched → same-volume rename / cross-volume copy+VERIFY (partial cleaned up) → row
  flips only after files exist at target → regen+reload → old tree deleted LAST.
  `sites.path` confirmed the single runtime source (no db_name-style trap). Also fixed
  a real reconcile bug: running FrankenPHP overrides never restarted on a changed
  docroot/rewrite — now desired config is diffed against the on-disk file. UI:
  Settings → Site folder card (picker + confirm with caveats). Live-verified:
  same-volume + cross-volume (hdiutil image) moves, reset-after-move, rejection
  errors, FrankenPHP serving from the new root post-move.
- [x] **Per-site env vars** — conflicts with per-VERSION shared php-fpm pools; no clean
  seam (L). ✓ **Done** `7cc2751`: the seam existed after all — vars ride the REQUEST
  (per-site nginx `fastcgi_param` lines), pools untouched; FrankenPHP overrides get
  config `env` lines + real process env at spawn (own process per site;
  `spawn_logged_env`, trait default errors so stubs can't drop vars). `core::site_env`
  trust boundary: reserved-name list test-locked to `TEMPLATE_FCGI_PARAMS`, reject the
  unescapable (`$`, `{}`, control chars), escape `\`/`"`. v7 `site_env` table (CASCADE),
  manager map mirrors php_settings, Settings-tab editor with honest note. Live-verified
  probe on BOTH servers: getenv()/$_SERVER/$_ENV all set; injection strings + reserved
  names rejected; watchdog respawn keeps env. Footguns (variables_order EGPCS-no-ini,
  FrankenPHP spawn env) recorded in ARCHITECTURE.md.

## Known baselines (not bugs)

- WP builds shipping `wp-includes/php-ai-client/**` show those files as "foreign"
  in checksum verify until wordpress.org's manifest covers them. Honest tool
  output, not a bug — intentionally not filtered.

## Blocked on external work

- [ ] **Xdebug on PHP 8.0** (corner of §8.2) — the Nov 2024 static-php 8.0.30 build
  exports ZERO Zend symbols (`nm -gU` = 0; dlopen of any xdebug.so fails with
  `symbol not found: _OnUpdateBool`), and upstream still serves that exact build
  (re-verified 16 Jul 2026). 8.1+ solved via the bottle path (see Actionable). Fix
  needs an upstream rebuild of 8.0.30 with symbols, or the self-build recipe
  (`docs/xdebug-debug-build.md`, `php-debug` variant wiring in `core/binaries.rs`
  kept as the fallback path). PHP 8.0 is EOL — acceptable to leave excluded.
- [ ] **SMAppService privileged helper** (Phase 1 §10.4) — single-prompt system setup.
  Needs a signed + notarized bundle → packaging-era, after Developer ID signing.
- [ ] **Developer ID signing + notarization** (Release 1.6) — needs paid Apple account.
- [ ] **OpenLiteSpeed override server** (researched 15 Jul 2026 — moved here from
  "Deferred services": this is NOT a bundling problem, there is no macOS binary to
  bundle). Evidence: upstream releases ship Linux tarballs only
  (`openlitespeed-1.9.1-{aarch64,x86_64}-linux.tgz`); homebrew-core has NO formula
  (nothing content-addressed to pin); the only community path is a third-party
  source-build tap (`puleeno/homebrew-openlitespeed`) frozen at EOL 1.4.51 — fails
  the trust model (unpinned third party, on-machine compilation) AND version quality.
  Modern source (1.9.1) DOES carry Darwin branches in `build.sh` (brew/port dep
  detection, mod_security forced OFF, CMakeLists sed-patched at build time, clones
  `litespeedtech/third-party` for vendored deps) — so a MAINTAINER self-build +
  self-host is plausible but unproven; it needs the same missing infra as the Xdebug
  debug build (artifact hosting + checksum pin, ideally after Developer ID signing).
  Code is ready and honest today: `ensure_server_available` in CORE refuses OLS at
  create AND switch (no IPC path can make a phantom OLS site), UI never offers it,
  and the manager's `OverrideKind` seam means enabling it later = one new arm +
  config template, not a restructure. NOTE for Phase 4: on Linux this is CHEAP —
  official upstream tarballs exist.

## Deferred services (the dylib-tree-bundling step now EXISTS — Redis proved it)

- [x] **macOS dylib-tree bundling** (the family's shared blocker) — `4392a57`:
  `core/binaries.rs` `bundle_manifest`/`resolve_bundle` assemble Homebrew-bottle ghcr
  blobs (content-addressed: the URL embeds the pinned digest — pins can 404 but never
  drift, unlike static-php/FrankenPHP rebuilds; anonymous bearer) into ONE cached tree
  (include-filtered strip-2 extract; same staging→prepare→atomic-publish as
  `resolve_dir`), and the new `BinaryProvider::prepare_binary_tree` (macOS) rewrites
  every Mach-O's non-system load command (`@@HOMEBREW_*@@`) to `@loader_path`-relative
  paths into `lib/`, errors loudly on an unbundled dep, verifies post-relink, ad-hoc
  re-signs LAST. ✓ 248 lib tests (+7), clippy, tsc.
- [x] **Redis engine** — `cb1e9ac`: `core/redis.rs` (argv-only config, data under
  app-data — the `--dir` path doubles as the adoption marker), `DbEngine::Redis`
  available on macOS (Services row, watchdog, adoption, ports all via existing
  `available()` plumbing), Databases row shows a `redis-cli -p 16379` hint instead of a
  dead Browse (no Adminer driver). ✓ **Live-verified** (`examples/redis_bundle_check`,
  Jul 14): both bottles downloaded + published to the real cache; all 4 Mach-Os
  loads-clean (`otool -L` = system/@loader_path only) + `codesign --verify --strict`
  pass; served on :16379; PING→PONG + SET/GET round-trip through the bundled
  redis-cli; clean stop. x86_64 bottle digests are Homebrew-published — re-verify on
  the next Intel smoke run.
- [x] **MariaDB engine + site→engine selection at create** — `62c498a`/`b5861c4`.
  The feared dep closure collapsed on inspection: `mariadbd`/clients link ONLY
  openssl@3 + pcre2 (groonga/lz4/lzo/xz/zstd are mroonga/connect PLUGIN deps; plugins
  excluded ⇒ libs never bundled). Bundle = mariadb bottle (server + `mariadb` +
  `mariadb-dump` + bootstrap SQL + errmsg/charsets — the 221MB `bin/`, plugins,
  baked-brew-path scripts all excluded) + openssl@3 + pcre2. `core/mariadb.rs` init =
  `mariadbd --bootstrap` fed the bundled SQL over stdin (`@auth_root_socket=NULL` →
  passwordless root, the MySQL model; NO install-db script — it's a shell script full
  of baked brew paths); explicit `--lc-messages-dir`/`--character-sets-dir` (compiled
  defaults are placeholders). Site seam: v10 `sites.db_engine` (default mysql),
  `core/database.rs` fns now take the client/dump BINARY (MariaDB = same protocol),
  every site DB op dispatches on the site's engine (create/reset/export/import/
  change-domain backup/delete/sizes-per-engine), Start-all spawns MariaDB exactly when
  a site lives there, New-Site dialog's Database field is a real MySQL/MariaDB picker,
  Adminer browses 13307 via the MySQL driver. ✓ **Live-verified**
  (`examples/mariadb_bundle_check` + `examples/mariadb_site_check`, Jul 15): 3 bottles
  → merged tree, all 6 Mach-Os loads-clean + strictly signed; fresh bootstrap; served
  :13307, `SELECT VERSION()` = 12.3.2-MariaDB; then a full WP site: `wp core install`
  over 13307, siteurl round-trip through php mysqli, `mariadb-dump` export (91KB) +
  re-import, `reset_site` drop+reinstall — all green. 253 lib tests, clippy, examples,
  tsc. **Human verify next:** create a MariaDB site from the dialog, site over HTTPS,
  per-site Adminer Browse.
- [x] **Apache (httpd) override server** — the closure shrank on inspection again:
  `bin/httpd` links ONLY apr + apr-util + pcre2 (+ system expat/iconv); openssl/
  brotli/nghttp2 belong to mod_ssl/mod_brotli/mod_http2, which are excluded (TLS/H2
  are the edge's job) — bundle = httpd (server + ONLY the 10 conf-loaded modules +
  the real `mime.types` from the bottle's staged etc/) + apr + apr-util + pcre2.
  `core/apache.rs` mirrors frankenphp.rs: loopback backend on 8300–8399 (same FNV,
  own base — a server switch can't collide with itself), NEVER the edge; `.php` →
  the site's SHARED php-fpm pool via mod_proxy_fcgi (per-version PHP settings apply
  identically; env vars ride the request as `SetEnv`, same delivery class as nginx's
  `fastcgi_param`); `AllowOverride All` — `.htaccess` works, the point of Apache;
  subdirectory-multisite mirrors WP's canonical network rules in server context.
  Manager's override machinery generalized to KINDS (`OverrideKind`: reconcile with
  kind-change stop, config-diff, spawn, watchdog respawn, adoption, status rows,
  ports, serving probe — one seam, OLS drops in later). UI pickers offer
  "Apache (.htaccess)". ✓ **Live-verified** (`examples/apache_site_check`, Jul 15):
  4 bottles → merged tree, httpd + dylibs + modules loads-clean; `httpd -t` Syntax
  OK; served a probe site on :8329 against a throwaway fpm pool — PHP-via-fpm ✓,
  SetEnv env var per-request ✓, pretty-URL front-controller fallback ✓, css mime
  from the bundled map ✓, `.htaccess` RewriteRule 302 ✓. 258 lib tests, clippy,
  examples, tsc. **Human verify next:** create/switch a site to Apache in-app,
  site over HTTPS, plugin `.htaccess` rules.
- [x] **Per-engine DB version switch** — the family's last item. Offered sets:
  MySQL 8.4.6/8.0.44 · PostgreSQL 18.4.0/17.10.0/16.14.0 · MariaDB 12.3.2/11.4.12 LTS
  (Redis single — picker hides). **Per-SERIES datadirs** are the core design: never
  an in-place upgrade/downgrade (PG major datadirs are mutually incompatible;
  MySQL/MariaDB downgrades unsupported) — the default series keeps the legacy
  `<engine>/data` path (existing data never moves), other series live under
  `<engine>/<series>/data`; a selection orphaned by a pin bump falls back to the
  default, its datadir left intact. Selection = `db_version_<engine>` settings KV
  (validated in core, no migration); manager mirrors it (watchdog respawns on the
  SELECTED version); every site DB op resolves the effective version's client bins;
  Start-all prefetches the selected versions. Databases-row picker with an honest
  confirm (per-version data dirs named; running engine restarts). ✓ 261 lib tests
  (+ series/datadir/effective-version), clippy, examples, tsc. ✓ **Live-verified**
  (`examples/db_version_switch_check`, Jul 15): PG 17 → fresh `postgres/17/data`,
  marker DB created → switch to 16 → own fresh datadir, marker NOT visible
  (isolation) → back to 17 → marker still there (data survives the round-trip);
  MariaDB 11.4.12 + MySQL 8.0.44 resolved into the real cache and RUN (`--version`;
  their ports were serving the live stack). **Human verify:** switch a version from
  the Databases row in-app.

FrankenPHP + PostgreSQL prove the override/engine patterns; Redis proves the BUNDLE
pattern. **Adding a bundled DB engine:** mirror `core/redis.rs` (or `core/postgres.rs`
for init-style engines) — pin the bottles in `bundle_manifest` (`core/binaries.rs`),
fill the engine's arm in `core/db.rs`, wire `plan_for_engine`/`resolve_any`
(`core/downloads.rs`), update `docs/PORTS.md`. **Adding an override server:** mirror
`core/frankenphp.rs` (loopback backend, never the edge).

## Phase 4+ (next era)

- [ ] Windows platform impls — fill the `todo!()` stubs in `platform/windows/mod.rs`
  (trait-by-trait; architecture requires no restructuring)
- [ ] Linux platform impls — same, `platform/linux/mod.rs`
- [ ] Packaging polish: Tauri updater (Release 6.1, optional), public distribution
