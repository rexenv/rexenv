# TESTING — proofs at the right level, by design

The standing testing doctrine (written 28 Jul 2026 as a plan; its T1–T13 all
shipped the same day — evidence in `docs/archive/SHIPPED-2026-07.md` — so this now
reads as the reference for how this repo tests). Companion ledger:
`docs/CLAIM-LEDGER.md` (the full claim inventory §2 summarizes — and the project's
test metric; its mechanical tally is the current number, not any count quoted here).

## 0. Why this plan exists

The bugs that cost the most this month were not catchable by unit tests, and more unit
tests would not have caught them:

| Bug | What it needed |
|---|---|
| wp-config password truncated at `)` | real PHP parse semantics |
| mysqldump rejecting `--connect-timeout` in every group | the real binary |
| MariaDB clients unable to auth to MySQL 8 | a real server handshake |
| `h-full` collapsing against an auto-height wrapper | a real WebKit render |
| nginx's pid file emptied by an example | the real process on the real prefix |
| unlink-delete guard defeated on Bedrock | the real layout on disk |
| tunnel DNS negative-caching | a real network with a real resolver |

Every one was found by a human looking, or by an example running against something real.

So the goal is NOT "unit test everything." It is: **every claim we make has a
programmatic proof at the right level, and the levels are a deliberate design.** The
metric is claims proven (the ledger), never lines executed. And a test that asserts the
wrong thing gets deleted, not kept for the count — we already found one whose assertion
was itself wrong (§1.2 below).

## 1. The layer model

**No test counts are written down here.** Three of them drifted in a single day
(14 Aug 2026: L0 757 vs 784 actual, L1 116 vs 121, §4's 105) and each still read as
current. A number nobody can verify at a glance is worse than no number, so the counts
now come from the things that produce them: `cargo test --lib` prints L0's, and
`scripts/live-checks.sh list` prints L1's per tier. The fact that actually matters —
**every example is classified** — is enforced in both directions by that script (an
unlisted example and a tier entry with no example each fail the run), which is why it
can be stated here without a guard of its own.

Four layers exist de facto. Named, with what each CANNOT prove stated as loudly as what
it can:

### L0 — Logic (`cargo test --lib`)

- **Proves:** our pure functions, state machines, validators/refusals, config
  *generation* (regression-pinning bytes WE emit), SQLite migrations (rusqlite is real
  in-process), witness types ("this state is unrepresentable"), parser behavior against
  captured fixtures.
- **Cannot prove:** ANY fact about an external tool, server, network, or render. A flag
  list can't prove mysqldump accepts the flags. A plist string can't prove launchd loads
  it. A config substring can't prove nginx 404s a dotfile. An output fixture can't prove
  the tool still prints that line.
- **A special case worth naming: the source scan.** A handful of L0 tests read the
  TREE rather than a value — `every_wp_cli_argv_in_the_tree_is_pinned` (#228),
  `every_owned_mu_plugin_is_swept_by_the_cleanup_that_claims_them_all`,
  `only_one_place_in_this_module_builds_a_dist_archive_argv` (#230). They exist for
  claims about a WHOLE SURFACE ("every wp-cli spawn is pinned"), where the honest
  guard is not a list of the sites known today: #228's ledger row named four spawn
  sites and there were seven by the time it was worked, two added after the row was
  written. A scan turns "someone remembers to add it to the list" into a build
  failure. Two rules learned by breaking both: **strip comments before scanning** —
  a guard that reads prose reads its own explanation and passes (#235 shipped that
  way; #228's scan did it too and failed at 3 by counting its own doc comments) —
  and **carry a canary** that the detection still finds something, or the whole
  scan passes vacuously the day a path changes.
- **A citation is evidence too, and it rots the same way** (2 Sep 2026):
  `every_test_the_ledger_cites_exists` reads every test-shaped name in
  `CLAIM-LEDGER.md`'s verdict column and requires it to exist as a Rust test, an
  example, or a wk-check. It found TEN wrong citations on its first run,
  including one written the same day, and two rows whose ✅ rested on names that
  had never existed — a verdict pointing at a test nobody can find is worse than
  an empty one, because it stops the next reader looking. Identifiers that only
  look like tests (a database name from a live run, a WordPress function, a
  settings key, a cron hook) are listed in the guard with what they actually are.
- **The scan can also be a LINT — a rule about code shape, not about a value**
  (2 Sep 2026, ledger #61): `the_services_lock_is_never_held_across_an_await` walks
  every `.rs` under `src/` at runtime and tracks each bound `state.services.lock()`
  guard by brace depth, failing on any `.await` under it that is not on the guard
  itself. It exists because tokio's Mutex is BUILT to be held across awaits, so the
  rule ("spawn under the lock, `await_ready` after dropping it") had no enforcement
  anywhere — the row said so for weeks. **Both defects in the first version were
  found by plants that came back green**: guards retired on the line that bound them,
  and multi-line statements joined with spaces so `mgr\n.restart_pools_for(…)\n.await`
  no longer read as a call on the guard. A lint that inspects nothing fails exactly
  like the rule it watches — so plant a violation of the rule AND a break in the
  scan's own reach.
- **`common::sandbox` covers PATHS and says nothing about PORTS.** Every service- and
  network-tier example inherits that gap, and it fails in the direction that looks like
  success: with the user's stack up, an example that brings up its own services binds —
  or worse, TALKS TO — theirs. Found twice. `mcp_mail_check` would have planted test
  mail in a real inbox and proven the mail filter against the user's own messages;
  `tunnel_exposure_check` would have created its fixture database inside the user's
  RUNNING MySQL, because `install_for_site` just uses the port. Same accident, bigger
  blast radius, and both would have read as the example working. The refusal therefore
  lives in the harness (`common::require_ports_free`), not in each example — a correct
  implementation with a warning beside it has already failed to stop a repeat in this
  repo, so the next example inherits the guard rather than the lesson.
- **A probe needs a known-good case, for the reason an example needs a control leg.**
  Same principle, one level up: without a case whose answer you already know, a check
  cannot tell ITS OWN defect from the subject's, and it reports the app as broken.
  Twice in one session, both caught the same way. `wpverdict.js` matched the Update
  control by button TEXT, but the control is icon-only with its version in the
  `title` — it found nothing and read every row as "no update offered", failing the
  working rows; the known-good `wordpress-seo` row is what exposed it. The onboarding
  probe read `edge=` from the URL hash when the harness puts params in the search, so
  it failed both rendering cases; `onboarding-clear` passing while the others failed
  is what pointed at the probe. In both, the check that would have "found a bug" had
  the bug. So: every probe carries at least one case it should pass, and a probe that
  only ever asserts absence is the shape to distrust — absence is also what a broken
  selector returns.
- **A third party's error taxonomy is measured, never read.** `edge_wire_check`
  exists because `reqwest::Error::is_connect()` looked like the way to tell "nothing
  is listening" from "something answered wrongly", and is not: a TLS handshake against
  a plaintext server reports as a connect error, so the first mapping filed a real
  blocker as an empty port. Three real sockets settled it in one run. The rule this
  generalises: when a decision rests on how someone else's library classifies a
  failure, the classification is a FACT ABOUT THEIR CODE and belongs at L1 — reading
  the docs and believing them is how a posture ends up resting on a third party.
- **A pure function with no test runner is held where it RENDERS.** The repo has no
  JS test runner, and adding one for two assertions costs more than it buys — so
  #250's update-claim rule (`a row may claim an update only when the offer is newer`)
  is proven by `wk-checks/wpverdict.js` over fixture rows in the real component. The
  discipline that makes it a proof and not a screenshot: assert BOTH directions (the
  claim that must not render AND the real update a naive string compare would hide),
  prove the fixture rows are PRESENT first (absence looks identical to a working
  filter), and keep an ordinary row in the list so the two cannot pass on a panel
  where nothing ever offers anything.
- **A copy guard is an L0 that expires at the boundary of what it can see.** A
  must-say list proves the sentence is still in the source; it cannot prove what the
  source PRODUCES. #301 is the worked example: L0 asserts the card branches on whether
  the packages could be named, and only the L2 render (`wppackages-unnamed`) catches
  the branch producing `The 0 packages in ~/.wp-cli/packages — —`. Pair them whenever
  a copy variant exists that no developer's machine will ever render by accident.
- **Cost:** seconds. **Runs:** every `verify.sh`. **Bug class:** logic regressions,
  drift in our own generation/parsing, unrepresentable-state violations,
  whole-surface claims going stale as the surface grows.

### L1 — Tool (`src-tauri/examples/*.rs`, every one tiered in `scripts/live-checks.sh`)

- **Proves:** what real binaries accept and do — real mysqld/mariadbd handshakes, real
  nginx reloads, real php parsing our generated files, real wp-cli installs, real
  process lifecycle (adopt/orphan/reap), real filesystem layouts, **what a bundled
  binary was COMPILED with** (`wp_dns_check`, sandbox tier: the static-php.dev PHPs'
  libcurl uses c-ares, so it cannot see `/etc/resolver` — a fact no Rust test can hold,
  and the reason WP-Cron was silently dead on every site until 10 Aug 2026; the check
  asserts the bug still reproduces AND that the mu-plugin fixes it).
  **It goes red on DISAGREEMENT with `core::wp_dns::resolver_for`, not on the bug going
  away** (23 Aug 2026, ledger #381) — because rexenv's own 7.4 build is already threaded,
  so "fail when this build has no c-ares" would fail on good news. This is the honest
  shape for an L1 fact the code can only RECORD: L0 makes the record total over
  `PHP_VERSIONS`, L1 checks it against the artifact, and the gate is the mismatch. Before
  this the branch printed a note and exited 0, which `docs/TODO.md` called the signal that
  the bug class was eliminated — a line in a log nobody reads, on a run that passed. **This layer caught
  the most bugs this month.** Discipline: `common::sandbox` + `common::Reaped` +
  fixture ports (`examples/common/mod.rs` — read its invariant first).
  **`starter_seed_check` (sandbox tier, 27 Aug 2026, ledger #429) is the smallest case
  for why this layer exists at all.** The Blank-PHP starter's seed is idempotent because
  of one SQL clause — `WHERE NOT EXISTS` — and whether a server agrees with a piece of
  SQL is not a question a string comparison in Rust can answer. L0 proves the page and
  the DDL name the same columns; only its own mysqld can prove a re-seed leaves four
  rows and that the developer's own row survives a third one. The same run `php -l`s
  both generated files under the bundled PHP: a generated page with a syntax error
  cannot report itself, because PHP never reaches the code that renders the error card.
  **`macos_floor_check` (network tier, 30 Aug 2026, ledger #433) is what a DERIVED
  assertion buys over a recorded table, and it paid on the first run.** PORTS.md carried a
  `minos` table and a hand-maintained rule — the stated floor equals the max across the
  default stack — and the table was read from this machine's binary cache, which only ever
  holds the host arch. So the x86_64 half of a universal app's floor had never been measured
  at all. The check fetches both slices, verifies each against its pin before believing a
  number read off it, and compares. It failed immediately, on a real defect: nginx's x86_64
  slice declares `minos 26.0` against a stated floor of 15.0, so Intel users below macOS 26
  install an app whose web server may not load. **Two lessons this layer keeps re-teaching**:
  a number nobody compares to anything drifts silently (the same rule caught PostgreSQL
  sitting 11 majors above the floor for a fortnight), and a measurement taken only on the
  machine doing the measuring is an assumption wearing a number's clothes.
  **`tunnel_parent_death_check` (sandbox tier, 30 Aug 2026, ledger #432) is the layer
  aimed at a KERNEL mechanism, and it carries this month's sharpest lesson about how an
  example can be green and empty.** What it proves cannot be proved lower: that kqueue
  `EVFILT_PROC`/`NOTE_EXIT` fires for a pid we did not fork, that it fires on a SIGKILL —
  which runs no code in the dying process, the entire reason the guard exists — and that
  the guard then signals the right process and exits. Three real processes, a real kernel.
  **Both of its plants first came back ALL PASS**: `cargo run --example` builds the example
  and links the library, and leaves the `rexenv` BIN target — which is what the guard runs
  from — exactly as it was, so the example was agreeing with a change it had never
  executed. It now REFUSES when the binary predates the guard's source. Same family as
  `cli_socket_check`'s stale-server trap, and the general rule this layer keeps re-learning:
  **an example that re-executes the app must prove the app is the one you just edited.**
  Two fixture-hygiene defects found in the post-0.4.0 review (3 Sep 2026): its three
  stand-ins were reaped by explicit calls placed AFTER the assertions, so an `expect` that
  unwound mid-leg walked past them and left a `sleep 300`, a stand-in tunnel and a real
  guard process behind — they are now an `Owned` drop guard (kill + wait), because
  `common::Reaped` is for services on a fixture PORT and these listen on nothing. And
  `db_version_switch_check` (network tier) ran on `platform::current()` with no sandbox,
  so its two `initdb`s and its CREATE/DROP DATABASE landed in the REAL
  `<app_data>/postgres/{17,16}/data` on the production port — the exact shape
  `examples/common/mod.rs`'s invariant forbids; it runs in `common::sandbox` now.
  **`site_stop_start_check` (sandbox tier, 4 Sep 2026) is the layer aimed at what a
  BROWSER gets.** L0 proves the two halves of stopping one site separately — a stopped
  site gets a STOPPED nginx block in place of a serving one, and its edge route renders a
  503 — and neither can
  say what comes back over the wire. The failure the design exists to prevent is
  precisely a wire fact: with no block of its own, a Host can fall through to a
  NEIGHBOUR's block and publish someone else's site at the stopped site's address, which
  is exactly what `override_fallthrough_check` measured for override sites. So this one
  runs a real nginx and a real edge over the real generated configs and asserts on bytes:
  both sites serving their own marker first (without that control, "the stopped site did
  not serve its marker" cannot be told from "this fixture never served anything"), then
  the 503 carrying rexenv's own words and NOBODY's marker, the neighbour still answering
  while its neighbour is down, and the site serving again after the switch goes back —
  on the certificate it kept.
  **`frankenphp_mail_catch_check` (sandbox tier, 5 Sep 2026) is the same shape for the
  mail catch-all's third carrier.** L0 holds the FrankenPHP config TEXT (`php_ini
  sendmail_path`, the `MAIL_*` env lines) and the manager's merge; what no string can say
  is whether the embedded PHP READS them — and the quoting is the whole risk, because the
  Caddyfile lexer and PHP's ini parser each strip a layer. So it runs the real binary on a
  fixture port twice: the catch OFF first (no shim, empty `getenv` — the control without
  which "the shim was there" cannot be told from "this PHP had it anyway"), then ON, and
  asserts `ini_get` verbatim, `getenv`/`$_ENV`, and a real `mail()` that ran a fake
  sendmail at a path WITH A SPACE with sendmail's argv and the message on stdin.
  Leg 4 (3 Sep 2026) hands the guard a LIVE stand-in parent with a start time that is not
  its own — the recycled-pid shape, reproduced without recycling a pid — and requires the
  share to end while the stand-in survives. `valet_import_check` gained step 3b the same
  day: the one example that builds configs BOTH ways (an empty manager mirror vs the
  table's alias map), because every other example calls the DB path and could never see
  a recorded-but-unpushed alias.
  Also from that review: `scripts/wk-checks/contrast.js` read only an element's OWN
  `opacity`, so a badge inside a Services row dimmed with `opacity-[0.74]` reported 4.9:1
  and rendered at 3.1:1 — the gate that certified the periwinkle change was blind one
  compositing level up. It now multiplies every ancestor's opacity into the text alpha.
  That sharpening shipped BROKEN: the same commit put a raw backtick in a comment inside
  the probe's template literal, so `contrast.js` became a `SyntaxError` and the 44 pairs
  it had just started catching stayed unseen for four commits — the file's answer was
  "does not load", and only `run-all.js` was ever going to say so. **A probe that does not
  parse is a probe that passes.** L2 needs a browser and a dev server and so runs at
  release time, but its SYNTAX costs nothing, so `verify.sh` now runs `node --check` over
  every `scripts/wk-checks/*.js` on every commit. It proves the file is loadable, not that
  its assertions still bite — that stays the release sweep's job.
  The narrower half of "still bite" now has an L0 control. Probes assert on
  `window.__ipcCalls`, the tally `DevGitPanel`'s mock keeps of every command the app
  invokes; a key nobody writes reads back 0 forever, so a *"this did NOT happen"*
  assertion passes while watching nothing — which is precisely how `wpfocus.js`'s control
  half shipped as decoration (`wp_plugins:updates` against a flag actually called
  `checkUpdates`). `copy_scan::every_ipc_tally_key_a_probe_reads_is_a_key_the_app_can_produce`
  requires each key's command to be registered in `generate_handler!` and each `:suffix` to
  be one the tally synthesises — read out of `DevGitPanel.tsx`, not listed in the test.
  It proves the key is WRITABLE, never that the assertion around it is meaningful.

  **A control that reads TEXT cannot see an empty element.** `sharedstopped.js`
  (5 Sep 2026) checks that a share of a stopped site carries the backend's warning on
  the card. Its first control read a HEALTHY card and asserted the warning text was
  absent — which looked like a control and was not: making the strip unconditional
  renders an empty amber box on every card, because `{tunnel.warning}` of `undefined`
  prints nothing, so the plant it existed to catch came back green. Counting the
  elements (`[data-probe="share-warning"]`, exactly one) sees what reading them cannot.
  The check's load-bearing assertion is separate again: the rendered sentence must
  equal the backend's CHARACTER FOR CHARACTER, which is what distinguishes a
  pass-through from a second copy the UI composes and then drifts.
  And `wpfocus.js`'s ride-along control names the FLAG it proves (`refetchOnWindowFocus:
  false`), not the stale window it fires inside of.
  Its second lesson is the fixture one: the first stand-in cloudflared was
  `sh -c 'sleep 300' <argv…>`, which EXECS sleep, so `ps` reported `sleep 300` — no marker,
  no program name — and the guard correctly refused it. A positive leg written that way
  would have "passed" while testing nothing.
  **`relink_tree_check` (sandbox tier, 14 Aug 2026) is this layer aimed at the tool
  chain itself.** `prepare_binary_tree`'s bug was never arithmetic — it was WHAT the
  chain was asked to look at: `needs_tree_relink` treated any `@loader_path/` prefix
  as in-tree, so a bottle whose deps read `@loader_path/../../../../opt/<f>/lib/…`
  passed both the rewrite and the VERIFY loop, the provider reported success over 49
  Mach-Os, and dyld then refused the binary. No pure test can catch that, because the
  claim is about `otool`/`install_name_tool`/`codesign` on a real Mach-O. The fixture
  is the example's OWN binary with one `/usr/lib` dep rewritten, so it needs no
  download and no service; leg C is the control (an already-in-tree path must be left
  ALONE, or leg A would pass for the boring reason that the predicate rejects
  everything), and the plant — restoring the old prefix test — fails leg A by name.
  **`php_fpm_serve` takes a VERSION argument** (`cargo run --example php_fpm_serve
  7.4.33`) for the same reason: the pool config generator emits one file per
  minor, so "php-fpm accepts it" is a claim about each minor's own binary. Reading
  `PHP_VERSION` could only ever ask about the newest, which is the version least
  likely to break — 7.4's php-fpm is a different program (ledger #327).
  A live check does not have to be networked to earn its layer: `git_site_clone_check`
  (sandbox tier) builds its own fixture repositories with real `git init`/`commit` and
  clones them locally, so it needs no remote, no credentials and no service — and it
  still caught an ordering bug no unit test could have (provisioning overwrites
  `NewSite.path` with the resolved docroot before the clone/link rule ran, so every
  cloned site read as also-linked and was refused). **Building the fixture with the
  real tool is what makes a hermetic example an L1 rather than an L0 in disguise.**
  **`wp_packages_check` (sandbox tier) is the pattern taken one step further: it plants
  the thing the check is about.** The obvious version — ask for a package command,
  expect it missing — is green on every machine that never installed one, which is most
  of them. So it writes its own canary package, REQUIRES that WP-CLI resolves it
  unpinned, and reports **CONTROL LEG BROKEN, this run proves NOTHING** if it does not,
  rather than letting a command that was absent for its own reasons read as the pin
  working. A control that fails silently converts a check into "nothing happened either
  way" — the vacuous-pass shape, one layer up from where this project usually catches
  it. Its verdict is also scoped in the ledger to what the run showed (one hook style,
  one machine), not to the claim's ambition.
  **`wp_noise_check` (sandbox tier, 14 Aug 2026) is the same pattern aimed at somebody
  else's code.** The claim (#316) is about ORDER inside the real phar — that rexenv's
  `--require` shutdown function runs before a plugin's — which no lib test can reach: L0
  proves the cut and the argv, and would keep proving them the day wp-cli changed when
  required files load. So the fixture reproduces the MECHANISM rather than the plugin: a
  require file that registers a `before_wp_load` command printing JSON and a shutdown
  function that writes to stdout with `fwrite`, which is the exact call Elementor's real
  trace ends in. Leg A then requires that this still produces an UNPARSEABLE answer
  without the marker — if the disease stops reproducing, the check says CONTROL BROKEN
  and stops, because a fixture that no longer bites makes every later leg vacuous.
  Its leg D (#317, the other end of the same report) shows what a control is FOR when
  the environment can answer for you: the claim is that PHP's diagnostics go to stderr,
  and on a machine whose `php.ini` already redirects them every assertion would pass
  with the flag deleted. So D re-runs the production argv with the flag removed **as a
  pair** (`-d` and its value — dropping the value alone would make PHP read the phar
  path as an ini setting) and requires the diagnostic to appear on stdout before
  believing the flagged run. The canary is `trigger_error`, deliberately not the phar's
  real 8.5 deprecation: a check that depends on a bug in someone else's release goes
  green the day they fix it.
  **`wp_login_client_ip_check` (sandbox tier, 14 Aug 2026) adds the third variant: a
  matrix that defends ITSELF, so the check does not depend on anyone re-running the
  plants.** It extracts the shipped client-IP gate from `wp_login::MU_PLUGIN` between
  markers (the `adminer_login_gate_check` arrangement, so it cannot drift from what
  ships) and runs it through real PHP over 13 header shapes — the tunnel ones verbatim
  from the live measurement in CLAIM-LEDGER #307, not invented. The problem with a
  table of cases is that a WRONG implementation can satisfy it: reading the first hop,
  reading nothing, or hard-coding a verdict. Plants catch those on the day you run them
  and never again. So the properties are asserted about the MATRIX, before PHP starts —
  an allow-case and a deny-case must share a leftmost entry (a first-hop read then
  cannot produce the table) and must share a `REMOTE_ADDR` (an XFF-ignoring read
  cannot either), with a floor on both verdict counts. **The plants were still run
  — three of them — but the table no longer rests on that having happened.**
  **`mcp_mail_check` (service tier) adds the other half of that discipline: refusing to
  run when the environment could answer for you.** It brings its own Mailpit, because
  borrowing the user's would mean planting test mail in their real store to prove rexenv
  can tell their mail from a scratch site's. The hazard is that borrowing happens by
  ACCIDENT: with the stack up, `mail::start` cannot bind and exits, `mail::running()`
  then sees the user's catcher on the fixed port, and every leg proceeds against a real
  inbox. So it proves the ports are free, its child is alive, and the store is empty
  before it writes anything — and it never infers from absence, since an empty list is
  also what a correctly filtered empty inbox looks like.
  Its network-tier sibling `git_site_provision_check` is the other half, and the one
  that pays: it drives the REAL `site_provision_job` on a `tauri::test::mock_app`
  against real remotes, and on its first run found **two bugs no unit test could
  reach** — a composer download that died mid-stream reported as "you look offline"
  (24 of 25 packages had just arrived), and `wp core install` pinned to the docroot on
  a Bedrock site whose core Composer puts one level in. `ONLY=bedrock` runs one case;
  each is a real `composer install`.
  **`wporg_icons_check` (network tier, 18 Aug 2026) is the cheapest shape this layer
  has: no app data, no ports, no processes — just the real api.wordpress.org.** It
  exists because the plugin list's premium icons are DERIVED (`betterdocs-pro` →
  `betterdocs`), and a derivation is only worth anything if the slugs it invents
  resolve at the other end. A unit test can prove the string arithmetic and nothing
  else — it would happily bless a rule that produces URLs nobody serves. So the check
  fetches the derived icon and asserts what came back is an `image/*` of real size,
  and asserts the two rows that must STAY letter tiles (a private plugin, and a paid
  one no suffix rule reaches) still do — an over-eager rule that borrowed a
  stranger's logo would pass every assertion about the icons that do appear.
  **`wp_premium_update_check` (network tier, 18 Aug 2026, #370) plants the DISEASE and
  its control, because a one-sided plant would have passed either way.** The claim is
  that a paid plugin's update becomes visible once rexenv grants the capabilities the
  vendor's updater gates on — invisible to L0, since it is WordPress's own update
  pipeline deciding whether a filter registered. So it writes two fixture plugins and
  one mu-plugin that offers an update for each, the second behind the measured gate
  (`current_user_can( 'manage_options' )`), and asks twice: through `wp_run_raw` (which
  by structural guarantee carries no context) the gated update is ABSENT — the original
  bug on demand — and through `plugin_list(check_updates: true)` both are present with
  the offered version. The ungated fixture is what makes a green run mean something: it
  fails first if the plant itself broke. Each reading drops the update transient first,
  or WordPress answers from its 12h timer and the two legs differ by ORDER rather than
  by the grant. A third leg asks a later run whether it still has the capability (it
  must not). **Ran green 19 Aug 2026** and is plant-proven: dropping the flag from `checked_list` fails leg 2 by name while leg 1 stays green. **Cannot prove:** that a premium update INSTALLS — that needs a vendor's real package URL and a licence, so it is `docs/SMOKE-TEST.md`'s leg.
  **`wp_info_check` (network tier) gained a fifth leg on 5 Sep 2026: MySQL STOPPED, the
  real site must still be WordPress.** L0 holds the classification
  (`is_installed_stderr_separates_a_down_database_from_no_wordpress`) over wp-cli
  2.12.0's own three stderr texts, captured live against a real docroot with `DB_HOST`
  pointed at a closed port, `DB_NAME` at a missing schema, and a Blank-PHP path — all
  three exit 1, so a fixture that checked exit codes would pass on the pre-fix code.
  The example leg is the end-to-end half (`wp_info` itself, a real wp-cli, a real
  stopped mysqld). **Ran green 5 Sep 2026** with the stack stopped — the down leg
  reports `is_wordpress: true` with the same `version` the up leg read, and the
  Blank-PHP leg still says `false`, so the pass is not "everything is WordPress now".
  The user-visible repro (Stop all → open a WordPress site → Start all → tab + Magic
  Login appear in place) was hand-checked by the owner the same day.
  **`tldconsent.js` (5 Sep 2026) is L2 holding a WHERE, not a what.** The takeover
  consent existed and worked — on the Import page, for TLDs Valet's own sites used —
  and a user whose Valet had no `.test` site left met the refusal in Change domain with
  no button anywhere. The check types `x.test` with `?foreign=test` (the mock's "Valet
  owns it" fixture) and asserts the card AND a disabled Change button under that input;
  `x.rex` clears both; consenting clears the card on the re-read (the mock flips the TLD
  to `borrowed` — a card that hid on the click would pass a frozen fixture); a fresh page
  with no fixture shows nothing for `.test` (control against always-on); and the same
  card renders under the default-TLD setting. **Plant-proven**: removing the ownership
  gate from `valid` fails by name while every other leg stays green. **Cannot prove:**
  the privileged write (`take_over_resolver`'s backup-then-row-then-write order) — core's
  L0 and the live leg own that; and the dev machine has no foreign resolver file, so the
  real `/etc/resolver/test` read behind `resolver_tld_status` rides `docs/SMOKE-TEST.md`.
  The scan's side of the same report — a leftover file with no Valet site behind it
  must become a row — is L0 (`dns::tlds_not_matching_signature`, the complement asserted
  over the same fixture dir as ours-by-signature, bad-label names excluded from BOTH);
  the Import page rendering it in the empty state has tsc behind it only, since the
  harness has no scan mock.
  **The Tunnels filter (`uireview.js`, `tunnels-*`, 19 Aug 2026, #371) is L2 asserting a
  SENTENCE, not a layout.** The feature is a search box; the risk is that filtering out a
  live public URL reads as "nothing is shared". So the fixture carries two live tunnels —
  one would let the probe pass on a count of 1 where the copy must say "2 shared sites
  are" — and each scenario asserts what the page SAYS while rows are hidden: the count,
  the consequence clause, and the empty state that must still carry the warning. Typed
  through the real input rather than assigned, because a controlled input whose `.value`
  is set behind React's back leaves the state empty and the probe then reads an
  unfiltered page. **Plant-proven:** deleting the warning's two call sites reddens three
  of four scenarios by name while `tunnels-plain` stays green — a plant that fails
  everything only proves the view mounted.
  **The AA scan grew two routes on 19 Aug 2026 (#373), and both convicted real text the
  day they landed** — which is the honest answer to "does this guard cover its claimed
  surface". It read `text-rex-*` classes and raw CSS `color:`. It now also reads
  `text-status-*` (a second Tailwind name for the same tokens — that half found an
  "Installed" label and four destructive buttons at 4.14–4.45:1 in light) and inline
  `style={{ color: var(--rex-*) }}`, which is how every chip in the app is coloured —
  that half found the site-type and Mail-avatar accents at 4.27–4.41:1. Icons are
  excluded from the inline route by the same structural reader the Tailwind route uses,
  so a colour drops out on what it SITS on rather than on its name. Beside it,
  `no_raw_tailwind_hue_reaches_the_ui` closes the route none of this could ever compute:
  a palette class has no second theme to check.
  **`wpgitchip.js` (19 Aug 2026) is L2 covering what L0 structurally cannot see: the
  Tailwind CONFIG.** `every_rex_colour_class_names_a_token_that_exists` reads
  `tokens.css`, so a class whose token exists but whose key was never added to
  `tailwind.config.js` passes it — and Tailwind emits no rule, leaving the element with
  no colour at all. Proven by plant: deleting `accent-blue-bg` from the config fails the
  L2 check in both themes and leaves the L0 guard green. It also closed a plain coverage
  hole found while verifying dark mode — the git chips on a plugin row had no fixture in
  any harness, so their colours were changed with nothing rendering them.
  **Run the sandbox tier with the stack STOPPED at least once per release, and it is not
  a formality** (20 Aug 2026). The tier's definition is "safe with the stack RUNNING",
  which is a statement about what it may HARM — not a licence to depend on it. Running it
  with the stack down found `linked_site_check` asserting a `404` that came from the
  user's own php-fpm pool through the vhost's `try_files → index.php` fallback: with the
  stack up it read 404, with it down 502, and the check had been passing on the machine's
  state rather than on rexenv's behaviour. Nothing in a stack-up run can see that.
  **`common::await_listening` is where "the service is up" is decided, and a flat sleep
  is not an answer** (20 Aug 2026). Every spawn helper in this tree bottoms out in
  `Command::spawn()`, which returns at fork — so an example that spawns and then requests
  is racing the bind. A swept audit found 33 such sites across 28 examples. What makes it
  worth naming as a LAYER rule rather than a bug list is the failure MODE: it never
  reports "not ready", it reports the front-end's symptom (`php-via-fpm=false`, `502`,
  `503`, `ConnectionRefused`), so the reader investigates the product. One instance cost
  17 days as an unexplained transient. The helper waits and then fails AT THE
  PRECONDITION with the port named and the service's own log spilled — the log every one
  of these examples was discarding.
  **A connect probe cannot tell your service from the corpse of your last one — and
  `SO_REUSEPORT` means you will not even collide with it** (21 Aug 2026). Four leaked
  FrankenPHP backends from earlier `frankenphp_edge_serve` runs were found LISTENING on
  the same override port at once. Caddy binds with `SO_REUSEPORT`, so each new run joined
  them rather than failing, and the kernel then split requests across five processes, four
  of which served deleted docroots; `ports::is_listening` connects, so every readiness
  gate on that port passed instantly against a corpse. The example had been recorded for a
  day as "its own edge never answers", which was the wrong subject entirely. **The rule:
  a readiness gate proves something is THERE, never that it is YOURS** — the only defence
  is refusing a busy port before you create anything (`common::require_ports_free`), and
  the only reason that works is that it runs before the leak can be joined.
  **The corpse does not have to be yours** (23 Aug 2026, found by running the service
  tier against a LIVE stack). `delete_site_serve`'s nginx failed to take `:18088` — the
  user's had it — and `await_listening(18088)` then passed against the USER's nginx. The
  run continued and printed `del.test -> HTTP 200 / keep.test -> HTTP 200`, a fixture
  reporting that its precondition holds while reading a server it does not own. It failed
  only later, on an `unwrap` of a reload whose pid file was empty; had that reload
  happened to succeed, the example could have reported green having proven nothing about
  its own services. **19 of the 23 service-tier examples still have no
  `require_ports_free`** — most are saved by production's `ensure_free` firing when they
  try to BIND, which is luck rather than design: it does nothing for an example that
  reaches a reload or a read path first. **The runner enforces it now** (23 Aug 2026): the
  service and network tiers REFUSE with the stack up, naming every port that
  answered, and the stack tier refuses with it down. Those were two `echo NOTE:`
  lines before — a precondition written down and not checked, which is the same
  shape as the two notes-that-were-called-guards closed earlier the same day.
  Per-example `require_ports_free` is still worth adding: the runner protects the
  tier, not an example someone runs by hand.
  **ACCEPTING is not ANSWERING, and the edge is where the difference bites** (21 Aug
  2026). `await_listening` proves the socket accepts; Caddy binds its listener before it
  has loaded certificates and routes, so a request inside that window comes back `000` and
  the example records it as the SITE being broken. `common::await_answering` polls for any
  HTTP status through our own CA — any status, because the assertions afterwards still
  demand 200, which is what keeps it a readiness gate rather than a retry loop that hides
  a failure. Worth stating as a layer rule because the first fix for it was a longer
  sleep, and a sleep is not a check: it is a long one.
  **A check that only PRINTS cannot fail, and that is not a small thing** (20 Aug 2026).
  Several `*_serve` examples ended with `println!("READY https://…:8443")` and a sleep for
  a human to curl. Caddy had been dying at startup for weeks — its admin socket exceeded
  macOS's 103-byte limit under the sandbox root — and every one of those runs still exited
  0. The defect was found by ADDING ASSERTIONS (readiness gates), not by reading: the very
  first thing the new gate did was fail on a service nothing had been checking. When an
  example's output is an invitation rather than a verdict, its exit code is measuring
  nothing.
- **Cannot prove:** a CSS chain resolves, a WKWebView quirk, real-internet DNS
  propagation, anything needing root or a second device (some examples DO take prompts
  — those are L3-adjacent and marked).
- **Cost:** seconds to minutes each; some need network/downloads; some need the stack
  stopped, a few need it running. **Runs:** on demand per feature; `verify.sh` only
  BUILDS them today. §6 adds a tiered runner. **Bug class:** assumed-tool-behavior,
  real-process lifecycle, real-layout path bugs.

### L2 — Render (`scripts/wk-checks/`, Playwright WebKit + mockIPC dev routes)

- **Probes (2 Sep 2026): `importbar.js`** — the import batch bar shows the CHILD
  job's label and freezes rather than rolling back when a site fails, driven
  through the real card by a scripted batch on a `?panel=import-bar` harness. Its
  control is that the bar must actually advance: "it never rolled back" is
  satisfied by a bar that never moves.
- **Probes (2 Sep 2026): `frameancestors.js`** — proves the ENGINE enforces the
  `frame-ancestors` we emit for the Adminer console, using a fixture served
  through Playwright's routing rather than the app. Its first version read
  `iframe.contentDocument`, which is null for any cross-origin frame, so it
  passed with the header deleted: a probe measuring the same-origin policy and
  reporting it as CSP. The lesson generalises — when a check's subject is a
  browser RULE, the plant that removes the rule must go red, or the check is
  watching something else.
- **Probes (2 Sep 2026): `wpfocus.js`** — the WordPress panel re-reads its list
  on focus while the costly update pass does NOT ride along. Its control needed
  a harness change: list and update check are the same command with a flag, so
  the tally had to key them apart (`wp_plugins:updates`) — counting the command
  alone let the ride-along assertion pass on a number that never moved, and
  guessing the flag's name kept it decorative for one more run.
- **Probes (2 Sep 2026): `statusagree.js`** — the sidebar footer and the Services
  page must agree on running/total, and the summary word must match its own
  arithmetic. Its control is the FIXTURE being mixed: with everything running,
  two views that count separately would still agree, and the check would pass on
  nothing.
- **Probes (2 Sep 2026): `tunnelhealth.js`** — "Live" is earned by a reachable
  probe, never painted on anything running. The mock ships one tunnel per health
  so the two non-reachable cards are the CONTROL; without them the check is
  satisfied by a screen that says Live everywhere. Its backend half (dead
  children settled before the snapshot) is an L0 source-ORDER guard, because
  both orders compile and return the same type.
- **Probes (2 Sep 2026): `domains.js`** — the Domains card renders the list the
  BACKEND returned rather than local state, and the primary has no Remove. It is
  L2 because the difference is only visible in a browser: a card that kept its
  own state would satisfy every unit test of the wrappers under it. Its mock is
  deliberately MUTABLE, since a fixture answering the same list forever would let
  exactly that card pass, and its control leg (an untouched extra surviving a
  remove) is what stops "the row is gone" from proving nothing.

- **Proves:** layout and copy in the engine family that ships (WKWebView class bugs
  Chrome hides): overflow, collapse, control chrome, dialog flows — via dev-only
  harness routes with mocked IPC, zero backend. Also data-FRESHNESS wiring, by counting
  mocked IPC calls rather than reading pixels (`focusrefresh.js`: a focus event must
  re-read git status and branches, and must NOT fire the lazy network read).
  A rendered IMAGE is proved by decoding, not by presence: `openin.js` reads each app
  icon's `naturalWidth`, because a broken `data:` URI still leaves an `<img>` in the DOM
  that a count-the-elements assertion would happily pass.
  **And it catches what `tsc` structurally cannot: the hooks rule is a RUNTIME contract.**
  `phppicker.js` (21 Aug 2026) was written for a copy-and-disabled claim and its first run
  found `SiteDetail` throwing "Rendered more hooks than during the previous render" — a
  `useQuery` below an `if (!site)` early return, so a COLD render of `/sites/:id` ran fewer
  hooks than the render after `sites` resolved, and the page went blank. A typecheck cannot
  see it, no lib test reaches it, and navigating from the Sites list HIDES it because the
  query is already cached. What exposed it was loading the route cold, which is what a
  browser check does by construction.
  **Since 30 Aug 2026 this layer can also drive STREAMS**, which changes what "needs a real
  site" means. The harness used to answer `plugin:event|listen` with a bare `1`: every
  `listen()` in the app resolved and could never fire, so no streamed UI — update progress,
  repo jobs, installs, provision — had ever rendered here. `mockIPC(…, { shouldMockEvents:
  true })` turns events on, and `wpupdate.js` emits `wp-update://plugins/dev` exactly as the
  backend does, then reads the rendered bar (0 → 25 → 60, phase, `n of m`, and the bar
  CLEARING at the end). Two claims (#249 wiring, #250 timing) had been queued for a live run
  for three weeks; what they needed was a listener that could fire and a query that could
  resolve late. **When a row says "needs a real site", check first whether it needs an
  EVENT** — that is the cheaper half, and it was unavailable for a year without anyone
  writing it down.
  **The same check also shows how an L2 case fails to prove things**: its first version passed
  the plant it was written for. `settleAfterUpdate`'s cancel and its invalidate turn out to be
  redundant — either alone keeps a stale badge off — so a plant that removed only the cancel
  changed nothing observable. The check now holds the composite claim and says in its own
  comments that it cannot attribute further, which is the honest shape when a guard is
  redundant: measure the truth table, keep the user-visible assertion, and refuse the
  attribution the layer cannot make.
- **Cannot prove:** backend truth (IPC is mocked BY DESIGN) — **it proves a card RENDERS,
  never that its command WORKS.** The MCP toggle rendered correctly across 10 harness
  scenarios while `mcp_set_enabled` *aborted the packaged app* on click (the mock returned
  success; the real command was never invoked). A backend crash is invisible here by
  construction — the same shape as "a lib test can't prove mysqldump accepts the flag"
  (§1's table): read ten green harness scenarios as render coverage, never as backend
  coverage. Also can't prove: real cookies/schemes (`rexdb://` doesn't exist in Playwright),
  aesthetics. **And the mock is a fixture that can be WRONG**: `repo_job_state` answered one
  canned job for every id, so the panel's (correct) post-attach snapshot re-read swapped the
  job the click had started for a different one, and `repopanel.js` sat red for months on a
  UI bug that did not exist. A harness that ignores its arguments produces false RED as
  readily as false green — mock by id (fixed 10 Aug 2026).
- **Cost:** ~a minute + a vite dev server. **Runs:** after UI changes; part of the full
  gate (§6). **Bug class:** percentage-height collapse, WebKit metrics overflow,
  states that render wrong or not at all.

### L3 — World (SMOKE-TEST.md, PUBLISH-TESTING.md — scripted manual)

- **Proves:** Gatekeeper/quarantine, privileged prompts, keychain trust, launchd under
  real root, second-device tunnel reach, router DNS caches, sleep/wake, how it looks.
- **Cannot prove:** anything repeatably or cheaply. Which is why everything that can
  move down a layer must (§4), and what stays is SHORT and scripted (§5).
- **Cost:** human hours, sometimes a clean Mac/VM. **Runs:** release gate + per-feature
  items. **Bug class:** the real world disagreeing with all three layers above.

### 1.1 Audit: over-tested at the wrong level (L0 asserting L1 facts)

From the full test sweep (distribution: core/ 469, platform/ 36, state/ 16,
commands/ 9 — commands/ is 1.7% of tests for ~20 files of orchestration):

- **launchd/security/dyld string-pinning** (`platform/macos/mod.rs` — ~15 tests):
  plist XML substrings, `launchctl bootstrap` shell strings, `security add-trusted-cert`
  flag arrays, `@loader_path` semantics. All launchd's/`security`'s/dyld's call. Zero
  live coverage of the daemon path. These stay as template-drift guards but must stop
  COUNTING as proof (ledger #67, #155–156, #162 note the unproven halves).
- **Manifest tests named as if they fetch** (`core/binaries.rs` — 18 tests):
  `every_offered_db_version_is_pinned_and_resolves` — "resolves" means a HashMap lookup.
  No fetch proof at all for cloudflared, composer, non-default DB versions.
  **Closed 15 Aug 2026** (`manifest_sweep_check`, network tier, #335): every pin
  HEAD-probed on both arches and everything under the size cap re-hashed against
  its digest — including the Intel pins that were hashed once and never re-run.
- **The design system's colour contrast was asserted in one comment and checked
  nowhere** (#337, scan landed 16 Aug 2026). `every_text_on_surface_pairing_meets_wcag_aa`
  and `every_rex_colour_class_names_a_token_that_exists` in `core::copy_scan` derive
  both the text set and the surface set from the frontend's own `text-rex-*` /
  `bg-rex-*` usage, so a token joins the check by being used. **They ship as a
  RATCHET first, not as a red bar** — `verify.sh` red would block every commit through
  the pre-commit receipt — and **both ratchets are now gone, deleted with the debt they
  held**. That lifecycle is the point: record, refuse to let it grow, fail when it
  shrinks so the repayment gets written down, delete. Both assertions are unconditional
  today. Two things the scan taught that generalise: it could not originally see a text
  colour set in RAW CSS, and `globals.css` was styling every `::placeholder` at
  2.58-3.35:1 there — the app's most widespread text; and its exemptions were keyed on
  TOKEN NAMES, so a plant that moved an "icon-only" class onto a `<span>` sailed through.
  Icons are excluded structurally now, by reading the element the class sits on. **An
  exemption keyed on a name cannot notice when the thing changes underneath it.**
- **A licence obligation asserted only in prose** (#336, closed 16 Aug 2026). The
  claim "an artifact rexenv BUILT ships its licence texts beside its bytes" is
  discharged by FILES on disk, so a manifest arm returning `Some` proves nothing
  about it. L0 covers the derivation (a self-hosted version with no licence pin
  fails by name) and the staleness rule; **only `php_versions_check` (network
- `php_update_check` (network) — the in-app PHP update chain end to end against the LIVE signed manifest: verify against the compiled-in key, resolve a patch this build was never made with, download it through the existing digest gate (the first time that gate compares against a network-supplied number), run the interpreter, serve FastCGI from it, and check the selection/floor/revert. Does NOT cover the live pool swap on the production port — that is SMOKE-TEST. Ledger #351.
  tier) sees the texts land** — beside `php` AND `php-fpm`, which are separate
  artifacts published by separate resolves. The field-repair path (a cache
  predating the fix) is planted rather than assumed.
- **SQL text under `sql_mode`** (`dbmirror.rs:282`): backslash-escape assertions that
  change meaning under `NO_BACKSLASH_ESCAPES`; the live restore example is the real
  proof.
- **PHP substrings superseded by live checks** (`wp_tunnel.rs:208`, `wp_login.rs:202`):
  the examples run the real plugin under real PHP; the needle lists add nothing and one
  needle (`".test"`) matches anywhere in the file.
- **The incident surface itself was untested at ANY level** (closed by T4,
  `db_dump_flags_check`): `dbdump.rs`'s real dump argv and `database.rs`'s exclusion of
  `--connect-timeout` were documented in prose only — prose the new proof promptly
  FALSIFIED (mariadb-dump warns-and-ignores rather than hard-erroring; ledger #195).

### 1.2 Audit: assertions that are wrong or vacuous (delete/fix, not keep)

- The **>80-char verdict check still exists and is still wrong**: `dbcompat.rs:456`
  `text.len() > 80` — bytes not chars (em-dashes count 3×), and length is not a proxy
  for "teaches anything." Same pattern: `dbcompat.rs:580`, `confverify.rs:211`,
  `phpconf.rs:842`.
- Unfailable: `terminal.rs:201` (`contains("")` always true).
- Split-assert blindness: `macos/mod.rs:2007` (`RunAtLoad` and `<true/>` asserted
  independently — a `<false/>` plist passes).
- Constant tautologies: `mail.rs:358/363/384`, `wordpress.rs:2696`, `binaries.rs:2409`.
- Names promising external facts fixtures can't prove: `…_resolves`,
  `the_live_sample_imports_cleanly…`, `…parse_live_wp_cli_output`.

### 1.3 Audit: untested because no layer fit (until now)

- `commands/` orchestration honesty (ledger #175, #181–182, #186, #188–190).
  **Closed 2 Sep 2026, and what closed it is worth naming**: every one of these is an
  ORDER or a SURFACE, and both are source-guardable at L0 once you stop trying to
  observe them at runtime. #188 (share guards over every command that invalidates a
  live share), #181 (the 3×2 exclusion matrix between provision, import and rewrite),
  #182 (apply/revert crash ordering — which side of the truth each window lands on),
  #186 (a cancel lands between sites, never inside one), #189 (the row is deleted
  before anything on disk). #190 was already live-proven and #175 already three-layered.
  The layer that "did not fit" was runtime: you cannot watch a crash between two
  statements, or a command that has not been written yet — but you can read both.
- `webview_dialogs.rs` — zero tests, three stacked wry/WebKit claims (#166).
  **Closed 15 Aug 2026, and the layer moved during the measuring** — the queued L2
  check would have tested Playwright's own dialog machinery, not wry's (the subject
  is absent from that harness), so the proofs landed where the subject lives: an L0
  against the real ObjC runtime, `webview_dialogs_check` (L1, real `WKWebView`,
  plant-proven), and SMOKE's drop-table step for the render/eye half.
  `docs/archive/PLAN-webview-dialog-proofs.md` records the measurement.
- Frontend honest-UI surface: StatusPill non-running states, StartStopToggle,
  StatusFooter, Tunnels tri-state — the WKWebView pill-metrics fix was verified once by
  hand and never committed as a check. **Both halves of this bullet have since been
  closed and the entry is kept as the record of what the gap cost:** the pill metrics
  are now the `pills` probe, and `SiteProvisionCard` gained the `provisionRow` probe on
  9 Aug 2026 — after a long backend phase label pushed the site's domain out of the
  card in the shipped app (ledger #248). The probe failed the first attempted fix,
  which is the argument for having it. The `deleteGate` probe (9 Aug 2026) joined them
  when site delete gained the type-the-domain gate: it types into every delete variant
  and asserts EVERY destructive button — the connected variant has two — is dead on an
  empty box and on a near-miss, and live on the exact domain. Proven by removing the
  `disabled` binding: both widths failed.
- `uireview.js` cannot fail (assertion-free screenshots, overflow non-fatal, not in
  `run-all.js`). **Fixed:** it asserts now — horizontal overflow is a failure, pageerror
  and console.error are failures, per-scenario probes run — and it is in `run-all.js`.
- **The `php-versions` probe checks the row's TRUTH as well as its layout** (18 Aug
  2026). Every defect in the in-app-update row was found by a person looking at a
  screenshot: chips overlapping, the EOL badge wrapping mid-pill, "8.2.32 exists"
  printed beside "Update to 8.2.32", and the button still offered after the update was
  applied. So the probe now asserts a button appears **iff** a verified manifest offers
  something for an INSTALLED minor, that the "exists" chip disappears once the button
  names the same version, that a row with nothing pending renders quiet (no amber
  `serving`), and that the why-no-button note only appears on a screen where some
  upstream version genuinely has no button. Plus a fixture-coverage assert over five
  states, because before it every mock row was installed and none was post-update —
  each rule would have been a branch nothing exercised.

  **What it cannot ask, stated rather than implied:** whether the patch displayed is
  the post-update one. Nothing in the DOM carries the compiled-in pin, deliberately —
  the row's job is to show what the minor WILL RUN. That comparison is L0
  (`after_an_update_the_row_shows_the_new_patch_and_offers_nothing`); this layer adds
  the half L0 cannot see.

  **Two of these rules were wrong when first written and planting is what said so.**
  Un-suppressing the chip failed by name (good). But asserting "offers a patch ⇒ has a
  button" failed a LEGITIMATE state — a manifest carrying 8.0.31 for a minor the user
  never installed, where the absence of a button is correct — so the rule is now scoped
  to installed rows. A probe that convicts correct behaviour trains people to ignore it.
- **The harness itself stopped answering questions it does not know** (18 Aug 2026).
  `DevUiReview`'s `mockIPC` default arm returned `1` for any unmocked command. The PHP
  section fires `php_update_check` on mount, so `1` was published into the versions
  query cache and `versions.some(…)` threw — the whole view rendered nothing, at the
  exact moment a second reader of that array appeared. The default now accepts Tauri's
  own plumbing quietly and THROWS for an app command, naming it. A fixture that answers
  everything with a friendly value cannot fail; an unmocked command is the harness
  saying it does not know.

- **Release builds logged nothing at all** (found 18 Aug 2026 by a user asking where to
  read one line). `tauri_plugin_log` was installed inside `if cfg!(debug_assertions)`, so
  every `log::` call in the codebase was dev-only on an installed app — including both
  "skipped the sweep" warnings that are the cache GC's safety valves. **Fixed:** the sink
  set is a value (`log_sinks`), the file is unconditional, and `rexenv.log` is a source in
  the app's own Logs tab. Worth recording as a TESTING fact and not just a bug: it is the
  purest form of the class this document exists for — **the dev machine is a fixture, and
  it was friendlier than production**. Nothing failed; a whole diagnostic surface was
  simply absent, and only on the builds users run.

## 2. The claim inventory

Full inventory: **`docs/CLAIM-LEDGER.md`** — one row per invariant-language comment in
`src-tauri/src`, each with file:line, the claim, and a verdict. **The tally is not
copied here.** It was, once: "194 claims … 117/32/36/9", written 28 Jul 2026 and still
sitting in this file on 21 Aug when the real numbers were 375 rows at ✅292 · ◐53 ·
🔨25 · 🚫5 — a second copy of a number that is generated in one place, drifting exactly
as the ledger's own summary had before `scripts/ledger-tally.sh` was written to stop it.
Run that script, or read the line it enforces at the bottom of the ledger. The 🔨 rows
plus the unproven halves of ◐ rows ARE the testing backlog, worked highest-risk first:

1. CF-header set as tunnel discriminator (wp_tunnel + wp_login) — L1
2. "a tunnel can only expose its one site" — asserted in three modules, negative never
   probed — L1
3. dotfile guard proven only as a config substring — L1 (live 404) *(closed: nginx at T11, Apache+FrankenPHP 15 Aug 2026 — #103 ✅)*
4. login-autostart "never download / never prompt" — untested at any level — L0+L1
5. wp_login PHP-injection safety inherited, not tested at the injection point — L0
6. DNS answer-anything justified by an unasserted loopback bind — L0
7. `webview_dialogs.rs` wry/WebKit claims — L2 *(measured 15 Aug 2026: L2 cannot
   see the subject; closed at L0+L1+SMOKE instead — see #166)*

Ledger discipline: a new "must never / always / is safe because" comment adds a row in
the same commit. A proof-commit flips the verdict and names the proof. That keeps the
metric honest and the ledger from rotting into the archive problem.

**And for any claim whose subject is a third party's behaviour, the FIRST step is a
measurement of which layer can observe the subject at all — before any test is
designed.** Promoted from a note to standing rule 15 Aug 2026, after it saved two
sessions in one week: #2/#33 (the CF-header set was simulated in every guard until a
live run measured it) and #40/#166 (both carried `🔨 L2` for months; the measurement
showed Playwright contains neither subject, and a wk-check would have tested
Playwright's own machinery forever). The dotfile guards went eight months as a config
substring for want of the same question.

## 3. Coverage of the classes we actually hit

Per recurring class: the honest mechanism — lint, test helper, or documented audit.
"Only a periodic audit catches this" is stated where true.

### 3.1 One fact computed in two places / recorded-vs-derived (same family)

- **Test pattern (exists, now named): the POISON TEST.** Plant a derivable state that
  contradicts the record; assert the record wins
  (`content_dir_rel_reads_layout_markers_and_resists_poison` is the template). Every
  recorded fact (`db_name`, `override_port`, `docroot_managed`, `content_dir`,
  `db_created`) must have one; ledger tracks which do.
- **Lint (partial, honest about limits):** a source-shape lib test (the
  `include_str!` pattern already used for identity-drift guards) greps for known
  re-derivation shapes of recorded columns outside their minting sites. Catches known
  shapes only.
- **Residual: design review.** The class is born at write time; the checklist question
  is "is this fact recorded anywhere already?" No lint can see a novel derivation.

### 3.2 One-time check on a mutable fact (lifetime guards)

- **Not lint-able in general.** The two July instances were both cross-site exposure
  under a live share; each got an instance test.
- **Mechanism: documented audit question** at review time: *"does this guard hold for
  the dependent thing's LIFETIME, or is it a snapshot?"* Plus: any new `ensure_*`/
  preflight that reads mutable state gets a test for the state-changed-after case
  (`commands/sites.rs`'s share-guard family, ledger #188).
- **One family IS lint-able, and now is** (2 Sep 2026, ledger #188): the share guards.
  Every function in `commands/sites.rs` that calls a tunnel-invalidating core mutator
  (`set_web_server`, `check_docroot_move`, `check_docroot_relink`, `set_domain`,
  `teardown`) must carry `refuse_if_shared` or `stop_for_domain`
  (`every_command_that_invalidates_a_live_share_carries_a_guard`). What made it
  lint-able is that the mutations are NAMED — a surface defined by what a function
  does, not by what it is called — so the sixth command that grows one is caught
  without anyone remembering this rule. The general case stays an audit question.
- **The class also arrives disguised as an optimisation** (23 Aug 2026, ledger #379).
  `docs/TODO.md` carried "`validate_linked_docroot` does a per-call `list(conn)` —
  hoist if imports grow" as a straightforward perf note. Taking it would have broken
  the Valet import: the apply loop creates sites one at a time through that
  validation, so a nested pair is refused only because the second call reads the row
  the first one wrote. **A snapshot is what a hoist IS**, which makes "cache this
  read" and "convert this lifetime guard into a one-time check" the same edit
  described in two vocabularies — and only one of them sounds dangerous.
  So the audit question has a second form: *"is this repeated read the guard?"*
  Freshness needs its OWN test; a rule test passes against a pre-seeded fixture
  whether the read is fresh or cached, because it only ever calls the function once.

### 3.3 Path assumption a layout invalidates (Bedrock class)

- **Test helper: a layout fixture matrix.** ✓ BUILT 21 Aug 2026 —
  `core::layouts` (`#[cfg(test)]`, the same shape as `core::copy_scan`):
  `layouts(tag)` returns stock, Bedrock (`web/wp` + `web/app` + `config/`) and
  subdir-docroot (`public/`) as REAL on-disk skeletons, because a function that asks the
  filesystem cannot be tested with a string. Each row carries what the layout MEANS
  (`core_root`, `content_rel`, `config_readable`), so a test asserts against the row
  rather than restating the expectation. Parameterized today: `wordpress::core_root` +
  `core_path_arg`, and `logs::wp_debug_log_status`'s `indeterminate` verdict. **The L1
  half already exists** — `git_site_provision_check` case 4 provisions a real
  Bedrock-shaped site and lands 12 tables (ledger #294).
  **Plant-proven, and its own canary fired first.** Reverting `core_root` to its
  pre-#294 body fails the matrix naming `bedrock`. And the landmark canary
  (`every_layout_row_is_a_real_tree_that_matches_what_it_claims`) caught a defect in the
  FIXTURE on the first run: every row held a cleanup guard over the same root, and `for l
  in layouts(..)` consumed the vec, so dropping row 1 deleted the tree row 2 was about to
  read. Without the canary that would have read as a bug in `core_root` — which is the
  argument for a canary in one incident.
- The class is fully testable now that the matrix exists — no audit needed, but adding a
  NEW supported layout must add a matrix row (review checklist).

### 3.4 Assumed tool behavior (the mysqldump class)

- **Rule: every argv/flag/config handed to an external tool has an L1 example that
  feeds it to the REAL tool.** The ledger is the registry — each such claim is a row;
  a new tool-fact needs its example named in the verdict column.
- **Not lint-able.** A grep can't know which strings reach argv. The mechanism is the
  ledger + the §1.1 rule that an L0 string test never counts as proof of acceptance.
- Immediate backlog: dump-flag acceptance per bundled vendor (§4), launchctl daemon
  strings (L3 clean-VM procedure — root), git version-dependent flags (already
  L1-covered by `repo_*` examples), `frankenphp validate` (its example currently
  asserts nothing).

### 3.5 Recorded-vs-derived disagreement

Covered by 3.1 (same family — poison tests + the recorded-fact ledger rows).

**The class reaches the CHECKS too, and there it hides longer** (24 Aug 2026,
ledger #390). `site_resources_check` re-derived a site's database name with
`wordpress::db_name_for(site_type, domain)` and asserted the result exists —
while `models::Site::db_name`'s own doc says the name is "derived ONCE at
creation and stored, never re-derived". That is true only for sites rexenv
CREATED: an imported site keeps the database it came with, so the check demanded
`wp_photocontest_test` from a site recording `photocontest` and failed against a
healthy 41-table database. **It fires on any install with an imported site** —
the whole Valet/Herd cohort — and had never been seen, because the stack tier it
belongs to had a precondition nobody could satisfy and so was never run.
Two rules follow, and the second is the one that costs:
- when a fact is RECORDED, a test that re-derives it is testing a different
  fact, and will disagree exactly where the two legitimately differ;
- **a tier nobody can run to green is a tier whose checks rot silently.** Three
  of that tier's checks were wrong simultaneously (#388–#390), each for its own
  reason, and nothing had reported any of them.

### 3.6 Percentage-height chain (the h-full class)

- **Test helper: an L2 heights probe.** wk-checks gains a shared measurement helper:
  key containers assert height ≥ floor, the Adminer iframe ≠ intrinsic-default
  (~150px), bottom within viewport after scroll. The harness already mounts the real
  `DatabaseTab` in a replica region chain — the probe just needs attaching. Other
  full-height surfaces (Terminal, Mail, Databases, SiteLogs) get harness scenarios or
  are named manual.
- **Class-level honesty:** the probe catches collapse on surfaces IN the harness.
  A new full-height surface must be added to the sweep (review checklist).

## 4. The real-dependency layer (L1) — making the best layer cheaper

Audit that MOTIVATED this section, taken before 28 Jul 2026 — a dated snapshot, not
the current state. Since then the runner exists (`scripts/live-checks.sh`, with the
tier table and its two-way completeness check). The rest is kept because it is the
inventory the work was aimed at: 105 examples; only 15 use
`common::sandbox`, 9 use `Reaped`; three still have the exact incident-3 shape
(`nginx_php_serve`, `php_fpm_serve`, `php_pools_serve` — real prefix/config, production
ports); 8 sandboxed examples still bind production ports (18088/9783); the WP-install
block is copy-pasted ~12×; temp-DB filenames collide across two example pairs; verdict
reporting is inconsistent (assert vs exit(1) vs print-only); **there is no runner**.

Workstreams, in order:

1. **Uniform verdict contract.** Every example exits 0 = proven / non-zero = not.
   Print-only "examples" (`frankenphp_subdir_validate`, `dns_serve`, demos) either gain
   assertions or are renamed `*_demo` and excluded from the runner.
   **The contract had a hole in 13 examples until 21 Aug 2026, and it failed in the
   direction that looks like success.** The idiom
   `if let Err(e) = mgr.start_all(..).await { eprintln!("start_all failed: {e}"); return; }`
   returns from `async fn main() -> ()`, which exits **0** — so the tier printed
   `all green` for a run in which the stack never started, nothing was asserted, and every
   readiness gate below was jumped over. The `start_all failed:` line was visible only to
   someone reading the log of a run that had passed. All 13 now return
   `std::process::ExitCode`; the reasoning, including why `FAILURE` and not
   `process::exit(1)` (exit skips the guards' destructors and leaks the service), lives in
   `examples/common/mod.rs` under "The verdict contract". Worth remembering as a shape:
   the runner's verdict came from a signal the example was free to not send.
2. **A tiered runner** (`scripts/live-checks.sh`), same discipline as verify.sh (own
   cwd, no pipes between check and verdict, one green line):
   - `sandbox` tier — sandboxed, fixture ports, no network, no prompts: safe anytime.
   - `service` tier — spawns real services on fixture ports; stack may be running.
   - `network` tier — needs internet (fetch checks, tunnel, wp.org).
   - `stack` tier — needs the user's running stack (adopt/wire-probe/resource checks).
   - `system` tier — prompts/root/system mutation: run deliberately, never in bulk.
   Tier membership is a table in the script; unlisted examples are a build-time error
   so new examples must declare a tier.
3. **Fixture library** in `examples/common/`: `fixture_db(tag)` (collision-free temp
   SQLite), fixture-port allocator (kills the sandboxed-but-production-port class),
   `wp_fixture()` (the 12× install block), a shared `Check` reporter with the uniform
   exit contract.
4. **Sandbox adoption**, worst first: the three incident-3-shaped examples, then the
   8 production-port sandboxed ones, then the ServiceManager cohort (real Sites-folder
   docroots today). Deliberately-real examples (`seed_and_list`, `system_setup`…) get
   an explicit REAL-SYSTEM banner + `stack_guard` opt-in instead of a sandbox.
5. **New examples the gaps demand** (from the ledger and §3.4): dump-flag acceptance
   matrix per bundled vendor+version; MariaDB-client-vs-MySQL8 negative auth; confedit/
   confrewrite output through real `php -l`; Bedrock live provision; dotfile-guard live
   404 (all three templates — landed 15 Aug 2026); fpm candidate-vs-live `-t`
   isolation (landed 15 Aug 2026); tunnel second-Host negative probe; manifest
   HEAD+digest sweep (network tier — landed 15 Aug 2026, #335).

**What moves down from manual:** the frankenphp validate step (was: human pipes
output), fpm reload isolation, dotfile 404s, Bedrock login flow, dump-tool flags —
all currently "found by a human looking."

## 5. What stays manual — short, and scripted

Everything here needs root, a second device, a clean machine, or an eye. Each maps to a
written procedure; "manual" means scripted-for-a-human, never remembered.

| Item | Procedure |
|---|---|
| Gatekeeper/quarantine first launch from the dmg | PUBLISH-TESTING §A |
| Privileged prompts (foreground, cancel/retry) + root daemon on-disk ownership (ledger #67/#155) | SMOKE-TEST robustness + PUBLISH-TESTING §B |
| Keychain CA trust dialog | SMOKE-TEST first-run |
| Second-device tunnel reach; router DNS negative-cache | SMOKE-TEST tunnels (explicitly not-a-bug note) |
| Resolver takeover/restore on a clean VM | PUBLISH-TESTING §F |
| Sleep/wake + reboot edge recovery; DNS agent handoff (#46) | SMOKE-TEST robustness (added, T13) |
| Datadir corruption recovery (B22/B23) | PUBLISH-TESTING §E |
| Firefox out-of-the-box trust (#154) | SMOKE-TEST robustness (added, T13) |
| Visual design / theme correctness | SMOKE-TEST settings + uireview screenshot sweep (human eyeballs the shots) |
| Radicle layout reality (#95) | flagged in code; first real project confirms |
| Phase-A-never-resolves wire spot-check (#19) | PUBLISH-TESTING §L (added, T13) |
| **The tray: every click, and closing the window** (#436–#439) | SMOKE-TEST "The menu bar" |

**Why the tray's L1 example was planned and then NOT written** (measured before building
it, the standing first step of §2). `PLAN-menubar-tray.md` D5 proposed
`tray_lifetime_check`: hide the window, then assert the CLI socket still answers. Nothing
in the app can hide the window except a human clicking the red button — an example talks
to the app over the CLI socket, and there is no hide command there. Writing one would
mean adding a `rex` verb that exists only so a test can call it: new public surface, in
the CLI's own vocabulary, for a claim the SMOKE list already covers in one line. The
example would also have to be pointed at the developer's REAL running app, since a second
instance is the second-writer bug this whole feature avoids.

So the tray's proof splits honestly: the menu's RULES are L0 and thorough (`core/tray.rs`,
11 tests, plant-proven three ways; `the_tray_acts_only_through_the_commands_the_ui_uses`
for the no-second-path claim; `the_cli_names_the_start_command_and_never_runs_it` for the
never-autostart one), and everything that needs a click, a login, or an eye is L3. The two
launch-ordering bugs found the day it shipped (#439 — a window shown before the Accessory
switch is a window the switch takes away; a show at the top of `setup` whose warning has
no logger yet) are exactly that class: a call that silently does nothing, visible only by
launching the app and looking.

## 6. The gate

`verify.sh` stays THE pre-commit bar, unchanged in discipline (own cwd, exit codes
load-bearing, green only from its own line). It grows tiers by composition, not by
bloating the fast path:

- **`scripts/verify.sh`** (fast, pre-commit, minutes): lib tests + **`cli` crate tests**
  + examples build + clippy -D warnings + tsc + **eslint (two react-hooks rules)**.
  **The lint half landed 24 Aug 2026 and is deliberately not a style linter** (ledger
  #396). `tsc` proves types and says nothing about the rule that decides whether a
  component renders at all: `rules-of-hooks` catches the crash fixed in `e9fc144` — a
  `useQuery` after an early return, which reached a user-visible failure on `/sites/:id`
  and was found by a WebKit render check written for something else entirely.
  Scope is two rules plus `no-control-regex`, and widening it is a separate decision with
  a separate cost: a general ruleset over a codebase that has never run one produces
  hundreds of findings, and a gate nobody can get to zero becomes `--max-warnings 999`.
  `reportUnusedDisableDirectives` is the half that keeps the suppressions honest — three
  of the seventeen that existed suppressed problems that no longer did, and nothing could
  say which three.
  **Grew the `cli` step 12 Aug 2026, and the gap is worth remembering:** the bar ran
  `cargo test --lib` in `src-tauri` only, so the `cli` crate — a SHIPPED binary, linked
  onto every user's PATH by the cask — was never entered. It also had no tests to run.
  §D then found `rex --version` hanging forever on an app that accepts and never
  answers (ledger #300). A crate that ships and a crate the gate visits are now the
  same set; keep them that way when a third crate appears.
- **The two generated-doc gates, inside `verify.sh`:** `scripts/ledger-tally.sh`
  computes the CLAIM-LEDGER tally, and `scripts/doc-counts.sh` computes the counts
  the docs state about the code (schema version, per-file command counts, IPC
  exports) **and, since 23 Aug 2026, checks every `docs/*.md` path the tree cites
  actually exists.** The pointer half came out of a sweep that repaired four code
  comments citing `docs/TODO.md` for rows that had moved to `docs/archive/`; it
  found a fifth the sweep was not looking for.
  **What it does NOT prove, because this is the commoner failure:** a pointer rots
  most often when a ROW moves between files, not when a file disappears — and the
  path stays valid through that, so nothing fires. Closing THAT needs anchors in
  the target, which is a convention change, not a check. `docs/archive/` is
  excluded as a source (history is not edited to satisfy a linter) and `dist/` as
  a build output; both are still valid targets.
  **And it cannot tell a mention from a reference:** prose *about* a missing doc
  reads as a citation *of* it, so anything describing this gate must avoid
  spelling the path out. That caught the gate's own comment and then the TODO row
  announcing it — twice in one afternoon.
- **`scripts/verify-full.sh`** (before release / after touching a layer's subject,
  ~10–20 min): runs verify.sh, then the L1 `sandbox` tier, then wk-checks `run-all`
  (spawns its own vite on 5199, kills it after). Own `verify-full: all green` line.
- **`network` / `stack` / `system` L1 tiers:** run deliberately when the change
  touches their subject (the runner prints per-tier one-liners for the commit note).
- **L3:** SMOKE-TEST per release; PUBLISH-TESTING per its per-item triggers.

A gate nobody can afford to run stops being one: the fast bar stays fast, and nothing
above it is required per-commit. The metric the gate serves is the ledger tally, which
only proof-commits move.

## Task list (ALL SHIPPED 28 Jul 2026 — kept for the record; evidence in
`docs/archive/SHIPPED-2026-07.md`)

Small, one commit each, ✓-ticked with evidence. Ordered so every
commit lands standalone value.

- **T1** ✍ this plan + the ledger + TODO entry (docs).
- **T2** Fix/delete wrong assertions (§1.2): the four `len() >` checks → structural
  assertions; `terminal.rs:201`; `macos/mod.rs:2007`; delete the constant tautologies.
- **T3** Rename lying test names (§1.2) — `…_resolves` family, `…_live_…` fixtures;
  reduce the two PHP-needle tests superseded by live examples to their non-redundant
  core.
- **T4** NEW `examples/db_dump_flags_check.rs` — every bundled dump tool × the exact
  argv from `dbdump.rs`/`database.rs`, real binaries, incl. the negative
  (`--connect-timeout` rejected). Closes the incident class. Ledger update.
- **T5** Make `frankenphp_subdir_validate` actually validate (run the real binary,
  assert, exit contract). Ledger row for the placeholder-typo class.
- **T6** `examples/common`: `fixture_db`, fixture-port allocator, `Check` reporter,
  `wp_fixture()`; migrate two examples as proof.
- **T7** De-fang the three incident-3-shaped examples (sandbox + fixture ports).
- **T8** `scripts/live-checks.sh` tiered runner with declared-tier enforcement.
- **T9** wk-checks: heights probe on the dbtab scenarios (§3.6), pageerror listeners +
  fatal overflow in uireview.js, add it to run-all; StatusPill all-states scenario.
- **T10** wp_login injection-point test (#36) + DNS loopback-bind assertion (#44) +
  autostart guard tests (#175, L0 half).
- **T11** Dotfile-guard live 404 example (#103, nginx first).
- **T12** `scripts/verify-full.sh`.
- **T13** SMOKE-TEST/PUBLISH-TESTING additions from §5 + ledger re-tally.

Backlog after T13 (ledger-driven, next sessions): tunnel second-Host negative (#10),
CF-header discriminator probes (#2/#33), ~~manifest HEAD+digest sweep~~ (closed
15 Aug 2026, #335), Bedrock live
provision, ~~fpm candidate isolation (#104)~~ (closed 15 Aug 2026), sandbox adoption cohorts (§4.4),
~~`webview_dialogs` L2 coverage (#166)~~ (closed 15 Aug 2026 at L0+L1+SMOKE — the L2
shape was measured impossible), ~~import-graph lint (#163)~~ (closed 15 Aug 2026), rusqlite-outside-state
guard (#167 — closed 15 Aug 2026; see the ledger row).
