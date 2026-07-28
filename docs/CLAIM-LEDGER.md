# CLAIM LEDGER — every asserted-but-unproven invariant, and what proves it

The complete inventory behind `docs/PLAN-testing-strategy.md` §2. A **claim** is a
statement the code asserts (doc comment, safety posture, honest-UI promise) without a
programmatic proof. This file is the project's test metric: **claims proven / claims
provable** — never line coverage. Update the verdict column when a proof lands, with the
proof's name; add a row when a new invariant is written into a comment.

Compiled 28 Jul 2026 from a full sweep of `src-tauri/src` (~1,037 invariant-language
comment lines read against 535 lib tests and 105 examples). Line numbers are anchors,
not contracts — trust the file, verify the line.

Verdicts:
- ✅ **proven** — a named lib test or example exercises exactly this claim.
- ◐ **half-proven** — the stated half is proven; the noted half is not (layer in note).
- 🔨 **provable-unproven** — no proof exists; the layer that could settle it is named.
  These rows ARE the backlog.
- 🚫 **inherently unprovable** — needs a real second device / third-party behavior /
  human eye. Each maps to a scripted-manual item (PLAN §5) or is an accepted posture.

Layers (PLAN §1): L0 = lib test · L1 = example vs real binary · L2 = WebKit harness ·
L3 = scripted manual.

⚠ = security posture resting on an assumption about a third party's behavior — the
`wp_tunnel` "a dead tunnel receives no requests" shape that burned us.

## core/wp_tunnel.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 1 | wp_tunnel.rs:8 | Quick-tunnel URL invisible to PHP, can't be read from the request | 🔨 L1 (assert cloudflared forwards no public-host header) |
| 2 | wp_tunnel.rs:8 | ⚠ CF-header set is a sound AND complete tunnel discriminator | 🔨 L1 (headers-present/absent both directions through a real tunnel) |
| 3 | wp_tunnel.rs:19 | Local requests untouched while shared | ✅ `tunnel_muplugin_check` mode 1 |
| 4 | wp_tunnel.rs:22 | mu-plugin lifetime ≤ tunnel + one relaunch | ◐ sweep fates proven (`tunnel_sweep`); the bound across a real crash+relaunch 🔨 L1 |
| 5 | wp_tunnel.rs:87 | Host regex never rewrites lookalike domains | ✅ `tunnel_muplugin_check` buffer mode |
| 6 | wp_tunnel.rs:134 | Content dir read from record, never derived at write time | ✅ `bedrock_layout_writes_where_wp_loads…` |
| 7 | wp_tunnel.rs:139 | Origin validation: nothing escapes the PHP string | ✅ `validate_origin_rejects_php_string_escapes` |
| 8 | wp_tunnel.rs:189 | Removing from a layout that never had them changes nothing | ✅ same test as #6 |
| 9 | wp_tunnel.rs:32 | Subdomain multisite shares main site only; subdirectory whole-network | 🔨 L1 (combine multisite + tunnel examples) |

## core/tunnels.rs + commands/tunnels.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 10 | tunnels.rs:8 | ⚠ Sharing one site can't expose another site or a tool | 🔨 L1 (probe a SECOND Host through the live tunnel) |
| 11 | tunnels.rs:66 | api.trycloudflare.com never recorded as public URL | ✅ `extract_url_never_takes_the_registration_endpoint` |
| 12 | tunnels.rs:103 | PID_PENDING sentinel structurally inert | 🔨 L0 (assert the sentinel is never signalled) |
| 13 | tunnels.rs:110 | Override-site refusal; premise: default-server fallthrough publishes another site | ◐ refusal proven; premise 🔨 L1 |
| 14 | tunnels.rs:137 | Transport errors are non-evidence, never downgrade | ✅ `fold_probe_reads_one_fact_honestly` |
| 15 | tunnels.rs:150 | Any non-530 response proves the tunnel path | ◐ fold proven; semantic premise 🚫 |
| 16 | tunnels.rs:279 | Edge IP from live lookup, never a constant | 🔨 L0 (no-IP-literal source guard) |
| 17 | tunnels.rs:282 | Passing edge check ≠ every POP routes; second device is the only e2e test | 🚫 → PLAN §5 |
| 18 | tunnels.rs:334 | 300s gate cap outlives any early negative-cache | 🚫 third-party resolver behavior |
| 19 | tunnels.rs:352 | Phase A never produces a system-DNS query | ◐ decision layer ✅ `phase_a_never_plans_a_system_dns_query`; reqwest `.resolve()` never falls back 🔨 L3 (dtrace procedure) |
| 20 | tunnels.rs:365 | Gate flag only ever flips open; never re-enters Phase A | 🔨 L0 |
| 21 | tunnels.rs:380 | Positive edge status never upgrades toward Reachable | ✅ `fold_diagnosis_reads_the_vector_honestly` +1 |
| 22 | tunnels.rs:395 | Token-match identity; recycled pid gets cleanup, never a signal | ✅ 2 lib tests + `tunnel_sweep` live |
| 23 | tunnels.rs:457 | We never spawn cloudflared without the identity pair | 🔨 L0 (drift guard on `start()`'s argv) |
| 24 | commands/tunnels.rs:3 | Tooling vhosts aren't sites so can never be shared | 🔨 L0 (negative test on the entry path) |
| 25 | commands/tunnels.rs:56 | Deleted/renamed site never stays publicly reachable | 🔨 L1 (re-fetch public URL after delete) |
| 26 | commands/tunnels.rs:102 | Start's claim never outlives stop/delete/rename | 🔨 L0 (claim/revoke race test) |
| 27 | commands/tunnels.rs:117 | `Err` from try_wait reads alive; death needs positive evidence | ✅ `take_dead_removes_only_exited_children` |
| 28 | commands/tunnels.rs:140 | Dead tunnel never sits in registry showing Live | ◐ registry ✅; UI half 🔨 L2 |
| 29 | commands/tunnels.rs:156 | rexenv never stops a share on the user's behalf | 🔨 L0 (call-site guard) |
| 30 | commands/tunnels.rs:420 | Reaped pids never re-signalled in the exit hook | 🔨 L0 (exit-hook reaped-set test; `tunnel_sweep` covers launch only) |
| 31 | commands/tunnels.rs:498 | A dead child never reads "already sharing" | 🔨 L0 |
| 32 | commands/tunnels.rs:723 | A just-discovered tunnel never renders Live on its discovery poll | 🔨 L2 |

## core/wp_login.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 33 | wp_login.rs:9 | ⚠ Token can't replay through a public tunnel (CF-header + XFF + Host) | ◐ CF-header leg ✅ `wp_login_check` (C); XFF-spoof leg + always-carries-CF premise 🔨 L1 |
| 34 | wp_login.rs:7 | Single-use: deleted on first attempt, success or not | ✅ `wp_login_check` (B) + lib test |
| 35 | wp_login.rs:98 | Hardcoded wp-content silently breaks the feature on Bedrock | ◐ path ✅; "never loads" premise 🔨 L1 (Bedrock live check) |
| 36 | wp_login.rs:104 | Domain can't escape the single-quoted PHP string | ✅ `injection_point_refuses_what_could_escape_the_php_string` — ensure_muplugin now re-checks at the injection point (T10) |

## core/adminer.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 37 | adminer.rs:5 | ⚠ Not a Site row ⇒ can never be a tunnel origin | 🔨 L0 (assert tunnel start can't reach ADMINER_HOST) |
| 38 | adminer.rs:88 | Loopback gate is exact host match, never prefix | ✅ `adminer_login_gate_check` (real PHP matrix) + lib test |
| 39 | adminer.rs:186 | Not frameable by a local `:1420` squatter; no wildcard | ◐ string ✅; browser enforcement 🔨 L2 |
| 40 | adminer.rs:288 | ⚠ WKWebView never follows a custom-scheme redirect | 🔨 L2 |
| 41 | adminer.rs:130 | Autologin guard keyed per target server | ✅ `autologin_guard_is_keyed_per_target_server` |
| 42 | adminer.rs:342 | Never follow off the vhost | ✅ `relative_locations_resolve_on_the_vhost` |
| 43 | adminer.rs:44 | Partitioned-cookie rewrite defeats ITP in the iframe | ◐ behavior ✅ `adminer_proxy_check`; ITP causal attribution 🚫 |

## core/dns.rs + core/tld.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 44 | dns.rs:8 | ⚠ Answer-anything is safe ONLY because the bind is loopback | ✅ structural since T10 — `serve_udp(port)` picks loopback itself (non-loopback unrepresentable); `loopback_bind_is_structural` pins it |
| 45 | dns.rs:6 | No in-process TLD state; adding a TLD never restarts DNS | ✅ `any_tld_answers_loopback` +1 |
| 46 | dns.rs:189 | Agent never exits on busy port; seamless takeover | 🔨 L3 (two-process handoff procedure) |
| 47 | dns.rs:283 | Foreign resolver file never overwritten without backup-first takeover | ✅ 2 lib tests |
| 48 | dns.rs:356 | At most one backup per TLD by construction | ✅ 2 lib tests |
| 49 | dns.rs:366 | Backup + record land BEFORE the privileged write | 🔨 L0 (cancelled-prompt rollback test) |
| 50 | dns.rs:600 | Invalid TLD label never reaches the privileged rm | ✅ 2 lib tests |
| 51 | tld.rs:3 | Blocked TLD refused in core, even via direct IPC | ✅ 2 lib tests |
| 52 | tld.rs:17 | RFC 2606/6761 TLDs never delegated; `.rex` unblockable | ◐ policy ✅; ICANN premise 🚫 |
| 53 | tld.rs:157 | Scanned names never trusted by the privileged sweep | ✅ 2 lib tests |

## cli_server.rs / main.rs / lib.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 54 | cli_server.rs:3 | `cli/` never links the lib — second-brain class impossible | 🔨 L0 (Cargo.toml dependency-graph drift check) |
| 55 | cli_server.rs:12 | 0600 socket, never TCP; same trust boundary as admin sock | ◐ bind ✅; threat-model equivalence L3 posture review |
| 56 | cli_server.rs:34 | Byte cap + deadline never cut a legitimate request | ✅ `read_request_line_bounds_size_and_timeout` |
| 57 | cli_server.rs:307 | Every command routes to the SAME commands::* fn as the UI | 🔨 L0 (general drift guard; one migration compared live) |
| 58 | cli_server.rs:1422 | Reply is an error envelope, never a panic | ✅ lib test |
| 59 | main.rs:6 | `--dns-agent` never opens a window / touches SQLite / starts services | 🔨 L0 (what run_agent can reach) |
| 60 | lib.rs:121 | In-process resolver fallback means DNS never regresses | 🔨 L1 (agent-death fallback) |
| 61 | lib.rs:410 | Locks never held across .await; polls never block the UI | 🔨 L0/lint (today a reading discipline) |

## core/proxy.rs (edge)

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 62 | proxy.rs:44 | admin_alive proves our process, not the wire | ✅ `wire_probe_check` step 4 + lib test |
| 63 | proxy.rs:57 | Marker-only identity; never `Server: Caddy` fallback | ✅ lib test + `wire_probe_check` |
| 64 | proxy.rs:32 | Health probe answered at the edge, never reaches nginx/PHP | ✅ `wire_probe_check` steps 2–3 |
| 65 | proxy.rs:148 | Explicit `tls` ⇒ Caddy never invokes internal issuer | ◐ config ✅; runtime 🔨 L1 |
| 66 | proxy.rs:168 | Non-wildcard site never shadowed by `*.site` | ✅ lib test |
| 67 | proxy.rs:287 | Daemon binary root-owned copy, never the user-writable cache (LPE) | ◐ command shape ✅; on-disk reality 🔨 L3 (root) |
| 68 | proxy.rs:418 | A developer's own Caddy on :2019 never touched | ◐ bind side ✅; live never-touched 🔨 L3 |
| 69 | proxy.rs:515 | A wedged admin socket can't hang reload/stop forever | ✅ `wait_ok_within_times_out_and_kills…` |

## core/stack_guard.rs / core/proc.rs / core/service_manager.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 70 | stack_guard.rs:9 | Non-app process may stop only what it spawned | ✅ lib test + `stack_guard_check` live |
| 71 | stack_guard.rs:43 | cfg(test) escape hatch: no unit test can trip the guard | 🔨 L3 (review posture — the deny path has exactly one test) |
| 72 | proc.rs:3 | Services outlive the app; next launch adopts | ✅ `adopt_check` two-phase |
| 73 | proc.rs:18 | Adopted processes never killed implicitly | ✅ lib test |
| 74 | proc.rs:49 | Adopted processes never "starting" | ✅ lib test |
| 75 | service_manager.rs:141 | Adopted pid never trusted bare; identity via owned_master | ✅ 2 lib tests |
| 76 | service_manager.rs:1551 | Privileged edge never auto-restarted; watchdog never starts what the user didn't | ✅ 2 lib tests + `edge_adopt_reload_check` |
| 77 | service_manager.rs:2047 | Foreign :443 must not read as our edge up | ✅ `wire_probe_check` step 4 |
| 78 | service_manager.rs:950 | Never reap a port a different site's backend holds | ✅ lib test |
| 79 | service_manager.rs:1314 | Unclean master kill leaks workers (residual) | 🔨 L1 (`health_watchdog_check` doesn't assert on leaked workers) |
| 80 | service_manager.rs:1461 | Startup adoption strictly offline, no download | 🔨 L0 (zero-network assertion) |
| 81 | service_manager.rs:2110 | A hung service never parks the manager; logging can't kill the watchdog | ✅ lib test + `ready_split_check` |
| 82 | service_manager.rs:2271 | Panicking mock: tests can't touch untestable surface | ✅ by construction |

## core/binaries.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 83 | binaries.rs:149 | Corrupted resume fails closed, partial removed, never silent | ✅ 4 lib tests |
| 84 | binaries.rs:1804 | Archive traversal/symlink escapes rejected | ✅ 2 lib tests |
| 85 | binaries.rs:950 | Stage→rename publish atomic; truncated artifact never cached | ✅ 5 lib tests |
| 86 | binaries.rs:1123 | Failed relink/codesign never leaves a poisoned cache | 🔨 L1 (macOS) |
| 87 | binaries.rs:1362 | Unknown-length ceiling; honest transfers never approach slack | ✅ lib test |
| 88 | binaries.rs:1504 | Handshake-then-silence servers bounded | ✅ lib test |
| 89 | binaries.rs:2208 | Unhosted php-debug artifact must not resolve | ✅ 2 lib tests |
| 90 | binaries.rs:340 | Upstream can only 404, never swap bytes silently | 🚫 CDN behavior (local mitigation = #83) |

## core/sites.rs + ownership model

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 91 | sites.rs:1046 | Docroot ownership read from record, never inferred from path | ✅ 4 lib tests |
| 92 | sites.rs:1206 | Linked docroot never created/written/deleted by us | ✅ 2 lib tests + `linked_site_check` |
| 93 | sites.rs:270 | mu-plugins dir removed only when recorded ours | ✅ 2 lib tests |
| 94 | sites.rs:684 | Content-dir detection never runs at write time (poison-resistant) | ✅ `content_dir_rel_reads_layout_markers_and_resists_poison` |
| 95 | sites.rs:704 | Radicle `public/content` layout UNVERIFIED (self-declared) | 🚫 needs a real Radicle project → PLAN §5 |
| 96 | sites.rs:368 | Recorded-port conflict belt on the allocator | ✅ 2 lib tests |
| 97 | sites.rs:377 | db_name stored not re-derived; injective + bounded | ✅ 2 lib tests |
| 98 | sites.rs:800 | Blast-radius link refusals (docroot is one click from public) | ◐ refusal ✅; dotfile-guard premise = #103 |
| 99 | sites.rs:266 | Progress freezes on failure, never rolls back; bar never reverses | ✅ 3 lib tests |
| 100 | sites.rs:631 | Ownership flags monotonic toward safety (1→0 only) | ✅ 3 lib tests |
| 101 | sites.rs:2547 | Move dialog's "kept — not deleted" stays structurally true | ✅ 2 lib tests |

## core/logs.rs / services.rs / php.rs / site_env.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 102 | logs.rs:7 | Log keys validated; tail never handed a UI path | ◐ key gate ✅; call-site discipline 🔨 L0 |
| 103 | services.rs:414 | ⚠ Dotfile paths 404, never reach fastcgi (all three templates) | 🔨 L1 — proven only as a config SUBSTRING; nothing HTTP-requests a dotfile through a live server |
| 104 | services.rs:122 | `-t` gate runs against a candidate the live pool never reads | ◐ shape ✅; candidate-vs-live isolation 🔨 L1 |
| 105 | services.rs:315 | nginx never 413s an upload PHP would accept | ✅ 2 lib tests |
| 106 | services.rs:769 | Long requests not killed at 300s | ✅ lib test |
| 107 | services.rs:621 | Foreign nginx on our port never signalled | ◐ decision ✅; live half 🔨 L3 |
| 108 | php.rs:197 | Default-deny whitelist: stored values can't smuggle directives | ✅ 3 lib tests |
| 109 | php.rs:75 | Unsupported minor can never grow a debug pool | ✅ lib test |
| 110 | php.rs:905 | Alive-in-grace never reaped; dead master always | ✅ lib test |
| 111 | site_env.rs:4 | Per-site env never touches shared pools; unescapables rejected | ✅ 4 lib tests |

## Witness-type modules (confverify / dbdump / dbrestore / dbmirror / dbsource / confedit / confrewrite / dbcompat)

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 112 | confverify.rs:5 | connected ⇐ witness ⇐ proof ⇐ real sign-in; probe can never gate | ✅ 4 lib tests + `config_rewrite_check` |
| 113 | confverify.rs:20 | Password never touches argv or logs | ◐ file path ✅; argv-absence 🔨 L0 |
| 114 | dbdump.rs:4 | Refusable pairing can't reach a connection attempt | ✅ witness-type test |
| 115 | dbdump.rs:14 | Partial dump unrepresentable as an artifact | ✅ lib test + `db_dump_check` |
| 116 | dbdump.rs:23 | ⚠ Cancelled dump changes nothing server-side | 🔨 L1 (metadata locks after mid-dump kill) |
| 117 | dbdump.rs:56 | Ours = positive identification, never a string match | ✅ lib test |
| 118 | dbdump.rs:388 | Manifest carries no credential (type-level) | ✅ `the_manifest_type_cannot_carry_a_credential` |
| 119 | dbrestore.rs:3 | Strand-then-refuse-cleanup order has no code path | ✅ lib test |
| 120 | dbrestore.rs:18 | Retry is drop-and-refeed; pre-existing DB never dropped | ✅ 2 lib tests + 2 examples |
| 121 | dbrestore.rs:27 | "Some tables exist" can never read as success | ✅ lib test |
| 122 | dbmirror.rs:7 | Never our root; loopback-scoped; idempotent | ✅ 4 lib tests |
| 123 | dbmirror.rs:21 | Password over stdin, never argv/env/logs | ◐ SQL shape ✅ + live restore; never-logged 🔨 L0 |
| 124 | dbsource.rs:6 | Plists label, listeners decide | ✅ 2 lib tests |
| 125 | dbsource.rs:13 | Identification never authenticates; declarations can't override the wire | ✅ 3 lib tests + `db_source_check` |
| 126 | dbsource.rs:22 | MariaDB clients cannot auth to MySQL 8 at all | ◐ positive matrix ✅; the NEGATIVE ("cannot at all") 🔨 L1 |
| 127 | dbsource.rs:269 | Probe never writes; server sees a clean disconnect | ✅ lib tests |
| 128 | confedit.rs:5 | No password key exists; diff IS the write; writes refuse more than reads | ✅ 9+ lib tests |
| 129 | confedit.rs:35 | Byte-preserving means byte-preserving | ✅ 2 lib tests |
| 130 | confrewrite.rs:5 | Backup-first; half-written unrepresentable; unknown digest reads conservative | ✅ 5 lib tests |
| 131 | dbcompat.rs:12 | Cautions never change whether it runs; a block is never a dead end | ✅ 3 lib tests |
| 195 | dbdump.rs (dump_tool_flags) + database.rs:155 | Every dump-tool flag is accepted by the real binaries; --connect-timeout is not honored (mysqldump hard-errors, mariadb-dump warns+ignores — vendor split DISCOVERED by the proof, old "every tool hard-errors" comment corrected) | ✅ `db_dump_flags_check` (28 Jul 2026, all 4 cached tools) |

## core/valet.rs / repo.rs / wordpress.rs / devtools.rs / mail.rs / ports.rs / ssl.rs

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 132 | valet.rs:3 | Scan writes nothing, anywhere | ◐ surfaced-not-dropped ✅; absolute write-nothing ✅ live (`valet_scan_check` fingerprints) |
| 133 | valet.rs:14 | No site silently dropped | ✅ lib test + `valet_scan_check` |
| 134 | repo.rs:11 | No frozen spinner; prompts fail fast; argv validated at parse | ✅ 4 lib tests + `repo_clone_check` |
| 135 | repo.rs:756 | Clone refuses existing dir; submodules never processed | ✅ lib test |
| 136 | repo.rs:1157 | Never merge/rebase/force for the user; `--prune-tags` never | ✅ 2 lib tests + `repo_git_ops_check` |
| 137 | repo.rs:1349 | Unknown fingerprint reads unverified, never stale | ✅ 2 lib tests |
| 138 | repo.rs:1657 | Linked assets unlinked, never handed to wp-cli | ✅ lib test + `repo_link_check` |
| 139 | repo.rs:2796 | Only total silence trips the idle watchdog | ✅ 2 lib tests |
| 140 | repo.rs:475 | Group kill: a grandchild can't stall the join | ✅ lib test |
| 141 | wordpress.rs:157 | ⚠ Timeout cap safe because wp-cli is the tighter bound | ◐ local half ✅; wp-cli-is-tighter premise 🔨 L1 |
| 142 | wordpress.rs:266 | Install progress monotonic, never 100 early | ✅ 4 lib tests |
| 143 | wordpress.rs:202 | Partial never flattened to failed; exit 0 ≠ activated | ✅ lib test + `wp_install_stream_check` |
| 144 | wordpress.rs:1025 | Modified core file never benign; `ok` alone never drives a pass | ✅ 2 lib tests |
| 145 | wordpress.rs:1120 | UI paths never trusted; slugs/versions can't smuggle argv | ✅ 5 lib tests |
| 146 | wordpress.rs:1473 | Serialized options never round-tripped / editable | ✅ 3 lib tests |
| 147 | wordpress.rs:1731 | Success judged after install, never on exit codes | ✅ `wp_install_serve` + command gating |
| 148 | devtools.rs:3 | Missing tool = honest $-fix error, never silent fallback | ✅ 3 lib tests + `devtools_check` |
| 149 | mail.rs:5 | Port offset avoids standalone Mailpit/Herd Pro | ◐ distinctness ✅; third-party defaults 🚫 |
| 150 | mail.rs:395 | Wedged Mailpit bounded | ✅ 2 lib tests |
| 151 | ports.rs:96 | Our leftover never gets a `sudo kill` suggestion | ✅ lib test |
| 152 | ssl.rs:75 | CA loaded never regenerated; trust stays byte-identical | ✅ 2 lib tests |
| 153 | ssl.rs:243 | Cert reissue failure leaves previous material intact | ✅ 2 lib tests |
| 154 | ssl.rs:368 | Firefox failure never fails the trust; modern Firefox works OOTB | ◐ fallback half 🔨 L0; Firefox behavior 🚫 → PLAN §5 |

## platform/

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 155 | macos/mod.rs:867 | ⚠ Root daemon never executes a user-writable binary (LPE) | ◐ command shape ✅ (3 tests); on-disk reality after real install 🔨 L3 (root) |
| 156 | macos/mod.rs:1917 | chown `-h` never follows a symlink onto another daemon's socket | ◐ arg shape ✅; live 🔨 L3 |
| 157 | macos/mod.rs:591 | Suggested copy-paste commands are allowlisted (attacker-influenceable name) | ✅ 2 lib tests |
| 158 | macos/mod.rs:278 | kill never fails for permission on own children; SIGKILL wait() returns | ✅ 2 lib tests |
| 159 | macos/mod.rs:440 | Unrelated port-squatter never terminated | ◐ marker path ✅; live negative 🔨 L3 |
| 160 | macos/mod.rs:1279 | Missing CLT never pops a GUI dialog from background | 🔨 L3 (macOS) |
| 161 | macos/mod.rs:1375 | Never ship/publish a tree that can't load | ✅ 3 lib tests |
| 162 | macos/mod.rs:2059 | Login keychain only, never System | ✅ 2 lib tests |
| 163 | traits.rs:3 | core/ depends only on traits, never OS-specific imports | 🔨 L0 (import-graph lint; today a review rule) |
| 164 | traits.rs:153 | Recycled pid fails identification, never signalled | ✅ 4 lib tests |
| 165 | traits.rs:346 | `env -0` parse survives rc noise | ✅ lib test |
| 166 | webview_dialogs.rs:8 | ⚠ class_addMethod additive-only; main-thread guaranteed; JS suspension = confirm contract | 🔨 L2 — module has ZERO tests |

## state/

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 167 | store.rs:1 | Only state/ knows the sites table shape; core never writes SQL | 🔨 L0 (rusqlite-outside-state grep guard) |
| 168 | store.rs:478 | ConnectedVerified mint demands the witness; probe only upgrades | ✅ 2 lib tests |
| 169 | store.rs:715 | INSERT never upsert — first backup wins | ✅ lib test |
| 170 | db.rs:143 | NULL = present-unverified, never stale; upgrades never spray alarms | ✅ 2 lib tests |
| 171 | db.rs:205 | Pre-existing DB never dropped by any path | ✅ 4 lib tests |
| 172 | db.rs:301 | Every writer reads the content-dir record, never re-derives | ✅ 2 lib tests |
| 173 | db.rs:919 | Failing migration rolls back atomically, reruns clean | ✅ 2 lib tests |
| 174 | app.rs:92 | Snapshot is the single liveness truth; views can never disagree | ◐ shared source ✅; footer-vs-tab agreement 🔨 L2 |

## commands/ (honesty layer)

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 175 | commands/services.rs:206 | ⚠ Login autostart: never download, never prompt | ◐ both DECISIONS proven at L0 since T10 (`uncached_names_lists_exactly…`, `login_edge_action_never_runs_a_privileged_plan` — pure fns the command now calls); the end-to-end login run stays 🔨 L3 |
| 176 | commands/services.rs:287 | Single monitor source of truth; DNS deliberately not a row | ✅ lib test + 2 examples |
| 177 | commands/system.rs:94 | Footer and Services tab can never disagree | ✅ 2 lib tests (counting halves) |
| 178 | commands/system.rs:61 | Reading it can never panic | 🔨 L0 |
| 179 | commands/site_provision.rs:6 | Genuine bytes never estimates; cancel touches nothing shared | ✅ 5 lib tests + `site_provision_check` |
| 180 | commands/wp_install.rs:6 | Milestones can't lie forward; no idle watchdog (wp-cli's own 300s) | ◐ progress ✅; wp-cli-300s premise 🔨 L1 |
| 181 | commands/db_import.rs:6 | One import at a time, never alongside a provision (both directions) | ◐ progress ✅; cross-guard flags 🔨 L0 |
| 182 | commands/rewrite.rs:10 | Second apply never skips account creation; clear-before-restore crash ordering | ◐ converge ✅; crash ordering 🔨 L0 |
| 183 | commands/repo.rs:9 | Repo scripts never run implicitly | ✅ 2 lib tests + `repo_run_all_check` |
| 184 | commands/repo.rs:575 | Never-ran steps read skipped, never failed | ✅ lib test |
| 185 | commands/repo.rs:674 | Check never runs repo code; delete partitions on fs truth | ✅ 2 lib tests + `repo_link_check` |
| 186 | commands/valet_import.rs:181 | Cancel stops after the current site, never mid-site | ◐ link half ✅; cancel boundary 🔨 L0 |
| 187 | commands/sites.rs:99 | A site is not a process; no fabricated per-site CPU/RAM | ✅ 2 lib tests + `site_resources_check` |
| 188 | commands/sites.rs:311 | Share guards hold for the tunnel's lifetime | 🔨 L0 |
| 189 | commands/sites.rs:465 | Delete ordered so the row never points at a missing path | 🔨 L0 |
| 190 | commands/sites.rs:853 | Deleted site's tunnel killed first; mirrored user dropped by record | ◐ record ✅; kill ordering 🔨 L1 |
| 191 | commands/php.rs:102 | Never brick a pool (validation + candidate `-t` gate) | ◐ validation ✅; candidate isolation 🔨 L1 (= #104) |
| 192 | commands/wordpress.rs:150 | Symlinked plugin dir unlinked, never wp-cli-deleted | ✅ lib test |
| 193 | commands/wordpress.rs:350 | Commands scoped: site row must exist in OUR db | 🔨 L0 |
| 194 | commands/downloads.rs:64 | Leaving onboarding never cancels downloads | ✅ 2 lib tests + example |

## Tally (28 Jul 2026, after T4)

- ✅ proven: **120** (of 195; #195 added and proven the same day — its proof falsified
  a prose claim, which is the ledger working; #36/#44 proven and #175 half-proven in T10)
- ◐ half-proven (unproven half in the backlog): **33**
- 🔨 provable-unproven: **33**
- 🚫 inherently unprovable: **9** (each mapped in PLAN §5 or an accepted posture)

**Provable total = 185; proven (incl. proven halves) ≈ 117 full + 32 half. The working
backlog = every 🔨 row + the noted half of every ◐ row.**

## The highest-risk cluster (work these first)

Security postures resting on unproven third-party assumptions — the shape that burned us:

1. **#2/#33** — CF-header set as a sound-and-complete tunnel discriminator (wp_tunnel +
   wp_login both stand on it; wp_login adds leftmost-XFF trust).
2. **#10/#13/#37** — "a tunnel can only expose its one site": three modules assert it,
   none tests the negative.
3. **#103** — the dotfile guard (`~/.ssh` one bug from the internet, per #98) is proven
   only as a substring in generated config text. Never a live 404.
4. **#175** — login-autostart "never download / never prompt": untested at any level.
5. **#36** — wp_login's PHP-injection safety inherited, not re-checked at the injection
   point (its sibling has a dedicated test).
6. **#44** — DNS answer-anything justified by a loopback bind nothing asserts.
7. **#40/#166** — WebKit/wry internals claims; `webview_dialogs.rs` has zero tests.
