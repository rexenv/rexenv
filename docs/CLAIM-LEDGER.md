# CLAIM LEDGER — every asserted-but-unproven invariant, and what proves it

The complete inventory behind `docs/TESTING.md` §2. A **claim** is a
statement the code asserts (doc comment, safety posture, honest-UI promise) without a
programmatic proof. This file is the project's test metric: **claims proven / claims
provable** — never line coverage. Update the verdict column when a proof lands, with the
proof's name; add a row when a new invariant is written into a comment.

Compiled 28 Jul 2026 from a full sweep of `src-tauri/src` (~1,037 invariant-language
comment lines read against 535 lib tests and 105 examples). Line numbers are anchors,
not contracts — trust the file, verify the line.

**Maintenance rule (also in CLAUDE.md): an invariant comment isn't finished until its
ledger row exists with a verdict — same commit, like the TODO tick.** A ledger that
drifts is worse than none; this project has already shipped one false safety comment
and two doc-drift audits.

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
| 198 | mcp_server.rs (SOCKET_FILE) | MCP socket is 0600, never TCP — the CLI socket's convention, not a second one (reuses `cli_server::bind`) | ✅ `mcp_socket_check` (0600 assert + spec-literal handshake) + `mcp_server` unit tests; never-TCP structural (`UnixListener`); shares #55's binder |
| 199 | mcp_server/tools.rs + readctx.rs + view.rs | M1 tools cannot MUTATE rexenv's state (the honest guarantee, assembly-review re-scoped from "physically cannot write/delete"): a handler reaches state ONLY through `ReadCtx`, which exposes no mutator (private state field); the read-only guard scans BOTH `tools.rs` (no manager/command/AppState/syscall) AND the `readctx.rs` bridge (no mutator/executor). NOT a claim a handler can't do arbitrary in-process work. `AgentSiteView` DROPS docroot+db_name — but "we dropped a field" is only true if NOTHING re-emits it (the same-information-different-door class: `tail_log` re-emitted the docroot via log content until #201 scrubbed it) | ✅ `m1_read_only_boundary_holds_across_tools_and_the_read_bridge` (both surfaces; proven to fire loudly on a planted violation) + `the_view_carries_only_the_agent_fields` + `mcp_socket_check` live |
| 200 | mcp_server/view.rs (classify) + tools.rs (sweep_plan) | `site_status` keeps failures DISTINCT (edge-down / edge-blocked / backend-down / setup-incomplete / serving — never collapsed to "not serving"), each non-serving verdict resolves to `user-action-in-rexenv` (an agent can't fix infra). It reads the serving path from the STACK'S OWN STATE and NEVER requests the site (Option A — M1 runs nothing; a GET would boot WP + fire wp-cron), so the site's own render errors are `tail_log`'s territory, stated in the verdict. The secret-leak sweep enumerates the WHOLE registry (every content tool swept by construction); `AgentSiteStatus` carries no path/internal by TYPE (so its sweep presence is a type-guarantee, not a non-vacuity proof — that's `list_sites`/`tail_log`) | ✅ `classify_keeps_the_failures_distinct…` + `every_non_serving_names_the_user_and_serving_states_it_never_ran_the_site` + `every_tool_declares_valid_sweep_args…` + `mcp_secret_sweep` live (Ok+error paths) + `mcp_socket_check` live |
| 201 | mcp_server/view.rs (scrub_log_line) + tools.rs (tail_log) + readctx.rs | `tail_log` is CONSTRAINED, not trusted-to-a-filter: only the WordPress debug log (per-site; shared server/edge/db/access logs NOT exposed), tail-only, line-capped (≤200). The scrubber removes KNOWN rexenv login tokens, cookie headers, AND the site's own docroot prefix (an absolute stack-trace path would otherwise hand the agent the docroot + OS username that `AgentSiteView` drops — the assembly-review leak), but the tool's note/copy explicitly does NOT claim the content is safe/sanitised. A non-WordPress site returns a normal empty result, never a "concerning" error row | ✅ `the_scrubber_removes_tokens_cookies_and_the_docroot_but_keeps_benign_content` + `the_log_tail_note_never_claims_the_content_is_safe` + `mcp_secret_sweep` live (planted token + Set-Cookie + a REALISTIC absolute-path stack trace in a fixture log → all scrubbed, benign line kept; Ok AND error paths swept) + `mcp_socket_check` live |
| 202 | mcp_server/feed.rs + mcp_server.rs (session) | The agent activity feed is COMPLETE for executed/attempted tool calls (the session records EVERY tools/call outcome AND the non-happy-paths — unknown tool, malformed request, handler error — at one place; protocol handshakes are deliberately NOT logged), a TYPED shape (only `target_site` is stored, no free-form arg column), BOUNDED on every write (row cap AND every field length-capped — the AGENT-controlled `client`/`tool`/`target_site` hardest, the assembly-review write-amplification fix), user-CLEARABLE, and it survives app restart (a SQLite table) | ✅ 8 `feed` lib tests (round-trip, typed-shape key-set, detail+agent-field bounds, row cap, clear, reopen-persistence) + `the_non_happy_paths_are_loggable_by_construction` + `mcp_socket_check` live (recorded 3 ok tools + unknown-tool + bad-request, ping NOT logged, client attributed, rows scoped-cleaned) |
| 203 | mcp_server.rs (McpControl/spawn_if_enabled/start) + commands/mcp.rs | The MCP endpoint is OPT-IN, not ambient: the socket is bound ONLY while `AppState.mcp` holds a running server's handle, which happens only when the user enabled it (`mcp_enabled`, default off) AND `start` bound the socket — so the toggle can never read on while nothing listens (bind first, persist "true" after). Disabling drops every live session (each `select!`s on the shutdown watch) and unlinks the socket; app exit drops the sender, which `serve` reads as stop. The status line is derived from recent call OUTCOMES in a 15-min window (self-recovering), never the handshake alone | ◐ control state-machine ✅ `mcp_control_is_off_by_default_and_stop_signals_shutdown_idempotently` + status derivation ✅ `recent_head_*` (window head, trailing-error count, ages-out, empty); the LIVE bind→unbind + over-the-wire session-drop 🔨 L1 (an enable/disable example asserting the socket unbinds and a live session ends) + card renders the derived status honestly at L2 (`uireview` `agents-working/erroring/idle/off` WebKit scenarios: working=green, erroring=amber-named "last N errored" not green, concerning rows muted-amber, empty state, toggle matches enabled) |
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
| 103 | services.rs:414 | ⚠ Dotfile paths 404, never reach fastcgi (all three templates) | ◐ nginx leg ✅ `dotfile_guard_check` (T11: .env/.git/.hidden-php 404 over the wire, secret never crosses, .well-known exempt); Apache + FrankenPHP legs 🔨 L1 |
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
| 167 | store.rs:1 | Only state/ hand-writes SQL against the app's SQLite schema (core reaches it only via store.rs fns). NOT "core writes no SQL" — withdrawn 29 Jul: core runs `information_schema` reads + `CREATE DATABASE`/`GRANT` on the developer's MySQL/Postgres in dbmirror | 🔨 L0 (planned rusqlite-outside-state grep guard — note: it would scan IMPORTS, not SQL-string content, so it is itself the surface-coverage shape — see Defect families below) |
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

## Tally (mechanical — count rows by their LEADING verdict emoji)

The tally is recomputed, never hand-maintained (the hand-kept version drifted within
one day of being written):

```sh
for v in ✅ ◐ 🔨 🚫; do printf "%s " "$v"; grep -c "| $v" docs/CLAIM-LEDGER.md; done
```

As of 29 Jul 2026 (this branch, after #198–#202: the MCP M1 socket, read-only
boundary, site_status/sweep, tail_log/scrubber, and activity feed): **✅ 122 · ◐
37 · 🔨 37 · 🚫 4** of 200 rows, plus 5 🚫 premises living inside ◐/✅ rows (#15,
#43, #52, #149, #154). Rows #196 (grant-escaping ✅) and #197 (MCP posture 🚫)
land from the sibling `fix/dbmirror-grant-wildcard-escaping` and
`docs/mcp-server-plan` branches, so post-merge = ✅ 123 · 🚫 5 of 202 (recompute
mechanically after merge — the tally is never hand-maintained). The working
backlog = every 🔨 row + the noted half of every ◐ row, ranked below.

## 🚫 wording audit (28 Jul 2026)

Directive: an inherently-unprovable claim must not read as a guarantee — that is
exactly how wp_tunnel's comment became a false safety claim. All nine read against
their code comments:

- **Re-scoped in code** (they overclaimed to a reader): #15 ("any non-530 proves the
  path" — now names the no-forged-HTTP assumption and that a forge can only
  over-claim Live), #18 ("negative-cache no longer possible" — now "compliant
  resolver, RFC 2308; a noncompliant router can still lie, hence the second-device
  check"), #90 ("can never swap bytes silently" — now states it's ghcr's contract,
  untestable, and that the local checksum is what's actually load-bearing), #149
  ("never clash" — now "their stock ports, their defaults not a guarantee;
  `ensure_free` is the actual guard").
- **Already carried their scope**: #17 (self-labeled HONEST LIMIT), #43 (behavior
  proven live; only the ITP attribution is unprovable), #52 (the RFC citation IS the
  scope), #95 (self-flagged UNVERIFIED), #154 (dated, versioned, with its
  falsification case).

## Defect families — the claim and the check aren't looking at the same thing

Three shapes have each shipped a false or overstated guard this month. They are one
family: a claim asserts a property of THING X, but the check that "proves" it looks
at THING Y ≠ X. They diverge along different axes:

- **Redundant computation** — one fact computed in two places; the claim is that the
  two agree, and nothing checks that they *can't* diverge (the single-source-of-truth
  rows #174/#177, footer-vs-tab).
- **Time** — a one-time check on a mutable fact; the claim holds for the dependent
  thing's LIFETIME, the check holds ONCE at mint (the share-guard #188 shape; memory
  `one-fact-lifetime-guards`).
- **Coverage / surface** — a guard asserts a property of a whole SURFACE but checks
  one PLACE inside it. Instances this month:
  - `tail_log` docroot leak (#199/#201): "docroot dropped from the view" asserted of
    ALL output; the drop was checked on `list_sites` and re-emitted via log content.
  - the sandbox invariant (`examples/common/mod.rs`): "no example writes real app
    data" asserted of ALL 109 examples; structure covers the ~20 that call
    `sandbox()`, and the bin cache is a real, mutable hole even there.
  - the M1 read-only guard (#199): asserted of the whole read surface; originally
    scanned only `tools.rs` when the boundary is also `ReadCtx` in `readctx.rs`
    (fixed — now scans both, proven to fire on a planted violation).
  - #167's planned rusqlite grep: asserts "core writes no app SQL", but a grep for
    the `rusqlite` import checks IMPORTS, not SQL-string content.

**The audit question this adds** — belongs in whatever the audit procedure becomes:
for every guard, *does the check cover the whole surface the claim names, for the
whole lifetime the claim spans?* If the claim says "all X" and the check reads "one
X", the guard is narrower than its claim, and that gap is exactly where the next
false-safety comment hides.

## The 🔨 backlog, ranked by blast radius

Directive: worked top-down when there's slack — never a session grinding the tail.
Rank = what a FALSE claim costs, not how easy the proof is.

**Tier 1 — cross-site exposure, auth bypass, or data loss if false:**

| Rows | If false |
|---|---|
| #10, #13-half | a tunnel publishes ANOTHER site's content (second-Host probe through a live tunnel; default-vhost fallthrough premise) |
| #2, #33-half | CF-header discriminator fails ⇒ login-token replay through a public tunnel |
| #37 | Adminer (passwordless DB) reachable as a tunnel origin |
| #25, #26, #29, #30, #31 | a share outlives its site / claim races ⇒ stale PUBLIC exposure; #30 additionally signals a recycled (foreign) pid |
| #54, #59 | a second SQLite writer/brain (cli crate linking the lib; dns-agent touching state) ⇒ corruption class |
| #49 | cancelled takeover loses the user's own resolver config (Valet's file, no backup) |
| #116 | a cancelled dump mutates THEIR server (locks/sessions) |
| #190-half | site delete leaves its tunnel publishing a dead docroot |

**Tier 2 — silent wrong answer (the debugging-days class):**

| Rows | If false |
|---|---|
| #57 | CLI silently diverges from the UI code path |
| #79, #80, #86 | leaked workers defeat probes; adoption downloads on a poll; a poisoned cache ships a binary that can't load |
| #104/#191-half | a rejected PHP value reaches the live pool anyway |
| #141, #180-half | wp-cli's internal bounds looser than assumed ⇒ false timeouts/hangs |
| #46, #60 | DNS agent handoff/fallback fails ⇒ sites dark with green health |
| #12, #20, #23 | tunnel sentinel/gate/argv drift ⇒ wrong lifecycle decisions |
| #28-half, #32, #174-half | UI renders Live/agreeing status that the registry already knows is false |
| #40, #166 | WebKit internals assumptions (redirect replay, dialog wiring) |
| #1, #4-half, #9 | tunnel semantics (URL invisibility, relaunch bound, multisite scope) |
| #19-half | reqwest `.resolve()` fallback (procedure now exists: PUBLISH-TESTING §L) |
| #71, #178, #181-half, #182-half, #186-half | guard escapes: cfg(test) hatch, panic on read, import cross-guard, crash ordering, cancel boundary |

**Tier 3 — untidy if false (structural lints and scoping tests; fine forever on the
shelf):** #16, #24, #61, #102-half, #160, #163, #167, #188, #189, #193, #55-half.

L3-by-nature rows (#46 two-process handoff, #67/#155/#156 root-install reality,
#107/#159 live negatives) route to SMOKE-TEST/PUBLISH-TESTING, not this backlog.

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
