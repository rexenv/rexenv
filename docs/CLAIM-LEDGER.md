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
| 199 | mcp_server/tools.rs + readctx.rs + view.rs | **Scope sharpened 1 Aug 2026 (M2a task 5): this is about the HANDLER, not the call.** A tool call always writes rexenv's OWN records — the activity feed, and the named scratch site's TTL (#207) — both in the session layer (`log_action`), unreachable from a handler, and neither touching the user's sites, files or databases. "An M1 call writes nothing" is the false wider reading of the true narrower claim, and is now said nowhere (the plan's §3.1c "no writes" and `mcp_socket_check`'s module doc were corrected in the same commit). The claim itself is unchanged: M1 tools cannot MUTATE rexenv's state (the honest guarantee, assembly-review re-scoped from "physically cannot write/delete"): a handler reaches state ONLY through `ReadCtx`, which exposes no mutator (private state field); the read-only guard scans BOTH `tools.rs` (no manager/command/AppState/syscall) AND the `readctx.rs` bridge (no mutator/executor). NOT a claim a handler can't do arbitrary in-process work. `AgentSiteView` DROPS docroot+db_name — but "we dropped a field" is only true if NOTHING re-emits it (the same-information-different-door class: `tail_log` re-emitted the docroot via log content until #201 scrubbed it) | ✅ `m1_read_only_boundary_holds_across_tools_and_the_read_bridge` (both surfaces; proven to fire loudly on a planted violation) + `the_view_carries_only_the_agent_fields` + `mcp_socket_check` live |
| 200 | mcp_server/view.rs (classify) + tools.rs (sweep_plan) | `site_status` keeps failures DISTINCT (edge-down / edge-blocked / backend-down / setup-incomplete / serving — never collapsed to "not serving"), each non-serving verdict resolves to `user-action-in-rexenv` (an agent can't fix infra). It reads the serving path from the STACK'S OWN STATE and NEVER requests the site (Option A — M1 runs nothing; a GET would boot WP + fire wp-cron), so the site's own render errors are `tail_log`'s territory, stated in the verdict. The secret-leak sweep enumerates the WHOLE registry (every content tool swept by construction); `AgentSiteStatus` carries no path/internal by TYPE (so its sweep presence is a type-guarantee, not a non-vacuity proof — that's `list_sites`/`tail_log`) | ✅ `classify_keeps_the_failures_distinct…` + `every_non_serving_names_the_user_and_serving_states_it_never_ran_the_site` + `every_tool_declares_valid_sweep_args…` + `mcp_secret_sweep` live (Ok+error paths) + `mcp_socket_check` live |
| 201 | mcp_server/view.rs (scrub_log_line) + tools.rs (tail_log) + readctx.rs | `tail_log` is CONSTRAINED, not trusted-to-a-filter: only the WordPress debug log (per-site; shared server/edge/db/access logs NOT exposed), tail-only, line-capped (≤200). The scrubber removes KNOWN rexenv login tokens, cookie headers, AND the site's own docroot prefix (an absolute stack-trace path would otherwise hand the agent the docroot + OS username that `AgentSiteView` drops — the assembly-review leak), but the tool's note/copy explicitly does NOT claim the content is safe/sanitised. A non-WordPress site returns a normal empty result, never a "concerning" error row | ✅ `the_scrubber_removes_tokens_cookies_and_the_docroot_but_keeps_benign_content` + `the_log_tail_note_never_claims_the_content_is_safe` + `mcp_secret_sweep` live (planted token + Set-Cookie + a REALISTIC absolute-path stack trace in a fixture log → all scrubbed, benign line kept; Ok AND error paths swept) + `mcp_socket_check` live |
| 202 | mcp_server/feed.rs + mcp_server.rs (session) | **Amended 1 Aug 2026 — see #205 (attribution, v28 `actor`) and #206 (`target_site`'s second, rexenv-derived provenance).** The agent activity feed is COMPLETE for executed/attempted tool calls (the session records EVERY tools/call outcome AND the non-happy-paths — unknown tool, malformed request, handler error — at one place; protocol handshakes are deliberately NOT logged), a TYPED shape (only the stable `target_site` id is stored, no free-form arg column — its human `target_label`/domain is resolved at READ time by a `commands::mcp` view join over the sites table, never stored, never agent content), BOUNDED on every write (row cap AND every field length-capped — the AGENT-controlled `client`/`tool`/`target_site` hardest, the assembly-review write-amplification fix), user-CLEARABLE, and it survives app restart (a SQLite table) | ✅ 8 `feed` lib tests (round-trip, typed-shape key-set, detail+agent-field bounds, row cap, clear, reopen-persistence) + `the_non_happy_paths_are_loggable_by_construction` + `mcp_socket_check` live (recorded 3 ok tools + unknown-tool + bad-request, ping NOT logged, client attributed, rows scoped-cleaned) |
| 203 | mcp_server.rs (McpControl/spawn_if_enabled/start) + commands/mcp.rs | The MCP endpoint is OPT-IN, not ambient: the socket is bound ONLY while `AppState.mcp` holds a running server's handle, which happens only when the user enabled it (`mcp_enabled`, default off) AND `start` bound the socket — so the toggle can never read on while nothing listens (bind first, persist "true" after). Disabling drops every live session (each `select!`s on the shutdown watch) and unlinks the socket; app exit drops the sender, which `serve` reads as stop. The status line is derived from recent call OUTCOMES in a 15-min window (self-recovering), never the handshake alone | ◐ control state-machine ✅ `mcp_control_is_off_by_default_and_stop_signals_shutdown_idempotently` + status derivation ✅ `recent_head_*` (window head, trailing-error count, ages-out, empty); the LIVE bind→unbind + over-the-wire session-drop 🔨 L1 (an enable/disable example asserting the socket unbinds and a live session ends) + card renders the derived status honestly at L2 (`uireview` `agents-working/erroring/idle/off` WebKit scenarios: working=green, erroring=amber-named "last N errored" not green, concerning rows muted-amber, empty state, toggle matches enabled) + `binding_the_socket_needs_no_ambient_runtime` (the sync `mcp_set_enabled` binds OFF the tokio runtime, where `tokio::net::UnixListener::bind` aborted on `Handle::current` — the packaged enable-crash found at the L3 gate; fixed by binding with `std` + adopting via `from_std` inside serve, proven to fail on the pre-fix path) |
| 204 | state/db.rs (v27) + state/models.rs (`SiteOrigin`, `Site::reap_due`) + state/store.rs | **Site ownership is a RECORDED fact with a conservative read** (MCP M2a): `origin` is written at insert and never derived from the domain or the path — a user's hand-made `foo.scratch.rex` is a normal site — and an unrecognised stored value reads as the USER's, never as reapable (the asymmetry is deliberate: erroring would fail the whole sites list over one cell, and reading wrong toward "scratch" feeds a real site to the reaper). The v27 upgrade is a default that is CORRECT, not convenient — nothing could have written `'agent'` before the migration existed, so every pre-v27 row IS the user's (unlike v17/v19/v24, where the fact was genuinely unknown and nullability was the honest answer). **`expires_at` NULL means NEVER**, the shape a user site and a KEPT scratch site share, so `reap_due` — the SINGLE expression of the reap predicate — cannot treat them differently. `agent_client` is the one agent-controlled value on the row: capped at the WRITE (whatever built the `Site`, not only the MCP path) and display-only, pinned where branching on it would actually hurt — the deletion predicate ignores it | ✅ `v27_every_existing_site_migrates_to_the_users_own_and_never_expires` (upgrade path from a v26 db, production-shaped row; proven to FAIL on a `DEFAULT 'agent'`) + `null_expiry_means_never_for_a_user_site_and_a_kept_scratch_site_alike` + `reap_is_due_only_for_an_expired_agent_site_whose_docroot_we_recorded_making` (every clause says no on its own, incl. `docroot_managed = None`) + `a_stored_origin_that_isnt_exactly_agent_reads_as_the_users_site` + `nothing_about_a_deletion_reads_the_agent_asserted_client_name` + `v27_fields_round_trip_through_the_row_mapping` + `the_agent_asserted_client_name_is_capped_at_the_write`. **Scope: this certifies the RECORD and the predicate, NOT the reaper** — no reaper exists yet (M2a task 9); when it lands it must consult `reap_due` rather than re-express it |
| 205 | state/db.rs (v28) + mcp_server/feed.rs (`FeedActor`, `record_system`, `recent_head`) + components/mcp/AgentActivityFeed.tsx | **Feed attribution is TYPED, and the two true claims coexist** (MCP M2a): "an AI agent did this" was true while the session loop was the only writer, and M2a's reaper breaks it — a deleted scratch site must not be invisible, but it is rexenv's own doing, so an unlabelled row would make every neighbour's attribution a lie by juxtaposition. `actor` **LABELS everywhere and FILTERS in exactly one place**: `recent`/`recent_for_site` return every row (hiding one would trade a false impression for a missing fact), while `recent_head` — the card's "an agent is working" line, a claim about the AGENT's session — excludes rexenv rows, so a reaper sweep can never render as agent activity nor make a failed reap read as "the last N calls errored". The v28 `DEFAULT 'agent'` records a KNOWN fact (`feed::record` had exactly one caller), but the READ deliberately fails the OTHER way — an unrecognised actor reads as rexenv, never as the agent, because a false accusation in an accountability record is the damaging error and a future actor value is by definition not the agent. In the UI a rexenv row is marked "rexenv · automatic" rather than wearing the client-name slot (where "rexenv" would read as an agent that calls itself rexenv), and a reap's now-deleted target reads "(deleted site)" instead of the bare UUID it can no longer resolve | ✅ `v28_every_existing_feed_row_migrates_to_the_agent_that_wrote_it` (upgrade path from a v27 db; proven to FAIL on `DEFAULT 'rexenv'`) + `a_rexenv_row_is_listed_and_labelled_but_never_says_an_agent_is_working` + `a_failed_reap_is_recorded_as_concerning_and_still_ours` + `an_unrecognised_actor_reads_as_rexenv_never_as_the_agent` (both halves: not listed as the agent's AND cannot drive the status line) + `only_the_typed_fields_are_stored…` (the guard fired on `actor` — a field joins the record by editing that list, never by a struct growing) + L2 `uireview` `agents-*` (rexenv row LISTED, labelled, no bare UUID — all three probes proven to fail on the pre-v28 rendering). **Scope: certifies the record and its surfaces, NOT the reaper** — no writer of `record_system` exists yet (M2a task 9) |
| 206 | mcp_server/feed.rs (`ActedTarget`) + tools.rs (`ToolHandler`) + mcp_server.rs (`target_for_record`) | **A feed row names a site if and only if a row for it exists, and neither half is a guess** (MCP M2a). `target_site` gains a second provenance — the site rexenv ACTED on, needed because a create has no `site_id` argument to record — WITHOUT opening a channel for agent content: `ActedTarget::set` takes a `&Site` (a row from our own sites table) and there is no constructor from a string, so an argument or a tool RESULT cannot reach the feed through it (reading the id out of the result would have been the easy version and the wrong one — results are what a future tool could echo an argument through). It is an **out-parameter, not a return value, precisely so it survives `?`**: a handler records the site the instant the row exists, so provisioning failing AFTER a successful insert still names the half-built site the user can see, retry or delete — the case where "record what rexenv did" is most useful and most easily dropped. Nothing created ⇒ nothing named. Precedence: what rexenv DID beats what the agent ASKED for; M1's read tools still record the ask, including when they fail ("asked about a site that isn't there" is the diagnostic). Nothing branches on the recorded value | ✅ `a_create_that_fails_after_the_row_exists_still_names_the_site` + `a_create_that_fails_before_any_row_names_nothing_rather_than_guessing` + `what_rexenv_did_beats_what_the_agent_asked_for` + `a_recorded_target_can_only_be_a_site_row_never_agent_content` (the row's id lands, and it is taken ONCE so a stale target can't re-attribute to the next call) — the two load-bearing ones **proven to fail on a no-op `set`**; the no-string-constructor half is structural (compile). **Scope: certifies the mechanism, not a user of it** — no handler calls `set` yet (M2a task 8) |
| 207 | state/store.rs (`touch_site_expiry`) + mcp_server.rs (`refresh_scratch_ttl`) + core/sites.rs (`SCRATCH_TTL_HOURS`) | **Using a scratch site keeps it alive, and a touch can only MOVE an expiry — never establish one, never reach a site the agent doesn't own** (MCP M2a). "Idle scratch dies, active scratch lives" is implemented as a no-op-unless-live-scratch UPDATE whose every `WHERE` clause carries one refusal: `origin = 'agent'` (a user's own site — including one they hand-named `*.scratch.rex`, and a KEPT site whose origin Keep flipped — is never given lifecycle state by an agent naming it), `expires_at IS NOT NULL` (writing an expiry onto a row that had none would CREATE deletion state, the one direction this must never move in), and `id = ?1` on an UPDATE (a deleted site matches nothing; an UPDATE cannot resurrect or insert). The write lives in the SESSION layer beside the feed record — never in a handler — so M1's read-only boundary (#199) is untouched and read tools refresh a TTL for free, which is correct: naming a site to diagnose it IS using it. An already-past expiry is deliberately refreshed (the reaper hasn't collected it and the agent is demonstrably still using it) | ✅ `using_a_scratch_site_pushes_its_expiry_out` (moves to now + `SCRATCH_TTL_HOURS`, one definition shared with creation) + `a_read_tool_naming_the_users_own_site_touches_nothing` (fixture's real site carries a STALE expiry, so the refusal rests on `origin`, not on a convenient NULL) + `a_touch_extends_a_deadline_and_never_starts_a_clock` + `naming_a_site_that_no_longer_exists_creates_nothing` + `an_expired_but_uncollected_scratch_site_is_revived_by_use`. **Each `WHERE` clause mutation-proven load-bearing**: dropping `origin` fails the user's-site test, dropping `IS NOT NULL` fails the never-starts-a-clock test |
| 208 | core/scratch.rs (`ScratchSite`, `claim`) | **An agent tool cannot be applied to the user's site, and that is a COMPILE error, not a check someone remembered** (MCP M2a). M1's boundary gated the VERB (a read-only handler had no mutating method); M2 mutates by design, so its structural half gates the OBJECT: every scratch mutator takes a `ScratchSite`, and `claim` — which reads the row and tests the RECORDED `origin`, never the name or the path — is the only door. The field is private to the module (not `pub(crate)`), there is no `From<Site>` and no other constructor, and `claim` reads the row ITSELF, so a caller cannot pass a `Site` it built or edited in memory. The gate is in `core`, so the CLI and the UI inherit it — the tool layer is never the only thing between an agent and the user's work. Refusals are **policy statements an agent can act on**, not type errors leaked into a tool result: they name the site, the rule and the way forward, and are distinct for "no such site" vs "that one is yours" (different next steps), carrying no path or database name. **NOT a lock**: the witness proves the path was gated when claimed, it does not freeze `origin` (the user may press Keep a moment later), so destructive writes re-assert `origin = 'agent'` in their own `WHERE` — belt on the same fact from the other side, per the one-fact-lifetime lesson | ✅ plant-and-capture, all three bypasses (`ScratchSite(site)` → **E0423** "constructor is not visible here due to private fields"; `ScratchSite { 0: site }` → **E0451** "field `0` is private"; `site.into()` → **E0277** no `From` impl) — each names the file, line and field, and the field carries a comment against rustc's own "consider making the field publicly accessible" suggestion, which would delete the guarantee + `claim_proves_an_agent_owned_site` + `claiming_the_users_own_site_refuses_with_a_policy_statement` (names site/rule/way-forward, leaks no path or db name) + `claiming_a_site_that_does_not_exist_says_so_distinctly` + `a_site_the_user_hand_named_scratch_is_still_the_users` (the NAME never grants status) + `due_for_reap_yields_witnesses_only_for_rows_the_predicate_admits`. **Scope: certifies the door, not its users** — the mutators that take the witness land at tasks 9/11/12 |
| 209 | mcp_server/scratch.rs + mcp_server.rs (`find_tool`/`tools_list_result`/`sweep_tool_outputs`) | **The socket routes two registries, and the capability is decided by WHICH ONE a tool came from** (MCP M2a) — never by the tool's say-so, its arguments, or lookup order. Executing tools live in their own module with their own context (`ScratchCtx`, whose ONLY door to a site is the origin-checked witness #208), so #199's read-only guarantee is untouched by their arrival instead of being widened to fit them. The registries are **disjoint by test**: a duplicate name would mean the tool an agent CALLED is not the tool that RAN. `tools/list` advertises the UNION (a registered tool that isn't listed is one no agent can call), and the secret-leak sweep walks BOTH — `ScratchTool` carries the same required `sweep_args`, so an executing tool cannot be registered unswept. **The SOCKET-level claim is a different sentence from M1's and this row is where it lives: "the MCP socket is read-only" is true of M1 alone and FALSE of the endpoint once a scratch tool lands** — the §3.1(c) shape (a narrow truth where the wide falsehood is the available reading), so the card's enable-moment copy is held to the registry by a test rather than by memory | ✅ `the_two_registries_are_disjoint_and_say_which_side_a_tool_belongs_on` — **plant-proven**: a duplicate `tail_log` fires a message that names the offender (and only it), both module paths, the rule that decides the side, and what goes wrong (shadowing); it `panic!`s the message rather than `assert_eq!`-ing it, because the Debug form escapes the guidance onto one line exactly when someone needs to read it + `tools_list_offers_the_union_of_both_registries` (count = both registries, so neither an unlisted nor a doubly-listed tool passes) + `a_scratch_handler_can_only_reach_a_site_through_the_origin_checked_witness` + `the_enable_moment_copy_cannot_keep_claiming_read_only_once_a_tool_executes` (cross-layer `include_str!` over `AgentsMcpCard.tsx`; **plant-proven** — a non-colliding executing tool makes it fail with the rewrite instructions). **Scope: the executing registry is EMPTY until task 8** — this certifies the structure the first tool arrives into, and the two plant-proofs are what make that non-vacuous |
| 210 | core/sites.rs (`Ownership`, `provision_with`) + core/dns.rs (`ResolverPrompt`) + commands/site_provision.rs (`start`) + commands/sites.rs (`create_site_owned`) | **An agent-created site is recorded as the agent's AND can never raise a privileged password prompt — from ONE value, so the two cannot disagree** (MCP M2a). `Ownership` decides both what lands on the row (`origin`/`agent_client`/`expires_at`, written at the INSERT so a create that fails later still expires and still gets reaped) and, via `resolver_prompt()`, whether the create may prompt. `ResolverPrompt` is a REQUIRED parameter of `ensure_resolver`, not a default-plus-opt-out: a future call site cannot be silent about its policy, which is what makes "never prompts" hold for the operation rather than at an entry check. **Ownership is not reachable from IPC** — `create_site` has no such parameter, so no caller through the UI, the CLI or the socket can mint a site the reaper may later delete unattended; the agent path calls `create_site_owned` inside the app. The cap and the domain shape refuse with a WAY FORWARD, because a refusal that states a rule and stops is where a model improvises | ✅ `an_agent_create_never_reaches_a_privileged_prompt` — drives the REAL `start()` end to end (resolver absent, recording `PrivilegeManager`) and asserts ZERO escalations + a refusal naming the user's action, **with `the_same_path_does_prompt_for_a_user_create_so_the_flag_is_what_stops_it` as its permanent control**: same fixture, same missing resolver, ownership the only difference, and it MUST escalate — so the agent test can never pass because the fixture couldn't prompt at all + `an_agent_create_records_origin_client_and_a_ttl_at_the_insert` (round-trips through the row; the user variant gets no client, no clock, never reapable) + `a_scratch_name_is_a_single_label_and_the_refusal_shows_the_shape` + `the_cap_refusal_names_the_sites_and_two_ways_forward` (lists only the AGENT's sites, never the user's). **Scope: the create PATH, not the tool** — `scratch_create_site` lands next (task 8b) |
| 211 | mcp_server/scratch.rs (`scratch_create_site`, `translate_create_failure`) + commands/sites.rs (`CreateFailure`) + components/mcp/AgentsMcpCard.tsx | **The first executing tool — and every way it can stop hands back an action, with the site named whenever one exists** (MCP M2a). Stack-stopped is a SUCCESS carrying M1's own vocabulary (`serving:false`, `verdict:"edge-down"`, `resolution:"user-action-in-rexenv"`), so "not serving, and whose move it is" reads identically whether the agent asked `site_status` or just created the site — an agent is never left to hunt for a start tool that deliberately does not exist. Every refusal names a way forward (resolver-missing → the user's setup; cap → the sites it may delete + ask the user + they expire; bad name → the SHAPE, not just the rule; domain taken; no default PHP; provisioning service absent). **A create that fails AFTER the row exists names the site**: `CreateFailure` carries the site id, so the reply says it was created, gives the id, splits the actions by owner (retry is theirs, delete is the agent's) and drops the local log path the app/CLI text carries — reporting a bare failure would be false AND would send the agent to create another, filling the pool with half-built sites. The feed names it from the same fact (#206). The enable-moment copy describes CAPABILITY rather than counting tools, so it stays true through tasks 9–12 without a rewrite only new users would see | ✅ `the_created_view_speaks_m1s_verdict_and_resolution_vocabulary` (+ no docroot/db-name in the payload) + `a_create_that_half_builds_names_the_site_rather_than_reading_as_nothing_happened` (names site + id, owner-split, no log path, feed target set) + `a_create_that_built_nothing_says_so_without_naming_a_site` (refusal stands alone, feed names nothing) + `the_cap_refusal_names_the_sites_and_two_ways_forward` + `a_scratch_name_is_a_single_label_and_the_refusal_shows_the_shape` (#210) + L2 `uireview` `agents-*` at both widths. **The copy guard (#209) is now scoped to DRIFT, not to its first trip** — that job is spent and cannot recur; the surviving halves are regression (the ban list) and erosion (the must-say list), **both re-proven to fire** after the rewrite, and the code says which |
| 212 | mcp_server/scratch.rs (`scratch_delete_site`, `ScratchCtx::delete`) + core/scratch.rs (`still_the_agents`) + mcp_server/feed.rs (`record_reap`) | **An agent deleting a site is gated by the recorded fact, re-read immediately before anything destructive** (MCP M2a). The gate is `claim` (#208) — not a name test, not the `.scratch.` suffix — so a site the user hand-named `*.scratch.rex` is refused with the OWNERSHIP policy statement, never a "not found" that would send the agent looking again. The witness is a snapshot, so `ScratchCtx::delete` re-reads `origin` under the lock BEFORE the destructive path and reports "no longer a scratch site — they kept it" rather than passing as a silent no-op. **The re-read is deliberately at the FIRST destructive step, not the last write**: discovering an adoption after the database was already dropped would be worse than useless, so the guard is placed where it can still refuse. The residual window between the re-read and resource teardown is milliseconds and cannot be closed without a transaction spanning MySQL — stated in the code, not pretended away. Deletion runs the app's OWN full path (`delete_site_owned`: tunnel stop, DB drop by provenance, teardown by `docroot_managed`, reload), never a second implementation. **A reap that keeps failing says so ONCE**: retry-once-per-launch would otherwise append an identical row every launch forever — a slow flood that buries the feed and makes one persistent problem look like many events — so `record_reap` writes the first occurrence, stays quiet while the outcome and reason are unchanged, speaks again when the reason changes, and NEVER dedupes a success (a site being gone is the most consequential event in the lifecycle) | ✅ `deleting_is_gated_by_the_witness_not_by_the_name` + `a_site_kept_between_the_claim_and_the_delete_is_no_longer_the_agents` (Keep mid-flight, and a deleted row, both read as not-the-agent's) + `a_reap_that_keeps_failing_says_so_once_not_once_per_launch` (1 row across 6 identical failures, a new row on a new reason, successes never deduped) + **`Site::reap_due`'s three clauses each mutation-proven load-bearing**: dropping `origin` makes a user's site reapable, dropping `docroot_managed` puts an adopted docroot at risk, dropping the expiry clause collects a site with no clock (that one fails two tests). **Scope: the delete TOOL and the record; the reaper's sweep + scheduling land next (task 9b)** |
| 213 | state/store.rs (`keep_site`) + core/scratch.rs (`ensure_capacity`) | **Keep is ONE write expressing ONE decision, and it frees a slot immediately** (MCP M2a). `origin` and `expires_at` together say "not disposable any more", so they move in a single atomic `UPDATE` rather than two writes that must agree — a crash between them (a user's site with a live clock, or an agent's with none) is not representable rather than merely unlikely. The clear is still worth doing even though the degenerate case is INERT — `Site::reap_due` tests `origin` first, so an expiry stranded on a user's row can never collect it — because a kept site must not read "expires in 4h" in the UI. It is also the ONE promotion path for every user-initiated mutation of a scratch site (rename, move, env edit, sharing), so four more call sites cannot each invent their own. Guarded by `AND origin = 'agent'`, so it is idempotent and never touches a site that is already the user's. **Keeping a site at the ceiling frees a slot straight away** (the cap counts `is_scratch()` rows), which makes Keep a pressure valve rather than a trap — and the cap refusal now says so, turning a wait into an action the human can take | ✅ `keep_is_one_write_and_frees_a_slot_for_the_agent_immediately` (origin flipped, clock cleared, never reapable, capacity passes at the ceiling, idempotent, unknown id is a no-op) + `the_cap_refusal_names_the_sites_and_two_ways_forward` (now asserts the Keep clause too). **Scope: the operation and the cap; the confirm copy and the four promotion CALL SITES are 10b** |
| 214 | commands/sites.rs (`promote_if_scratch` + the six mutation commands) | **A scratch site the user deliberately changed becomes theirs, through ONE choke point** (MCP M2a). The rule is "the user deliberately changed THIS site", not a list — applying it to §4.3's inherited four ADDED the PHP switch, the web-server switch and the Xdebug toggle, and REMOVED share (sharing a scratch site is sharing a scratch site, not claiming it; the reaper's skip-and-surface already tells that story). It excludes the agent's own tools — an agent promoting its own sites would be a cap bypass with a plausible face — and rexenv's housekeeping, which is not user intent. One function, because Keep is one atomic write (#213): six call sites each setting `origin` and clearing `expires_at` would be six places for two facts to disagree. Best-effort, so failing to promote never fails the change the user asked for | ✅ `every_user_facing_site_mutation_promotes_through_the_one_choke_point` — a SOURCE guard over the six commands, so a command that stops calling it (or a NEW mutation command that never did) fails the build with the rule, not a diff + `promotion_turns_a_scratch_row_into_the_users_in_one_write` (the effect half: origin flipped, clock cleared, never reapable). **Honest split, stated in the code: the effect test does not drive `change_site_domain` end to end (that needs a platform + Tunnels fixture), so the dropped-call coverage is the source guard's alone** — the test is named for what it proves, not for what would sound stronger |
| 215 | commands/scratch.rs (`reap_expired`, `summary`, `spawn`) | **The unattended sweep skips a shared site rather than stopping it, and can delete at most 5 in one pass** (MCP M2a). Skip-never-stop is not a preference: `delete_site_owned`'s first act is stopping the site's tunnel, and #29 says rexenv never stops a share on the user's behalf — a reaper calling the normal delete path would break that invariant UNATTENDED, which is what the tunnel work exists to prevent. A failed stop is a skip too, by construction: nothing is deleted while anything still serves it publicly. The per-sweep ceiling (= the scratch cap, since a correct sweep can never need more) turns a wrong predicate from an incident into a bug — N sites and a loud log, not everything it selected. **Blast radius, stated honestly in the module doc**: the witness `due_for_reap` returns is minted from the same predicate, so it is a type and not a second opinion; what IS independent is the teardown, which removes a docroot only where `docroot_managed == Some(true)` and drops a database only under recorded provenance. So *nothing outside what rexenv created is deleted, and that holds independently of the predicate* — not "nothing is deleted". A site rexenv made and the user later adopted is the real loss case, which the ceiling and the promotion choke point (#214) exist for | ◐ shape ✅ `the_sweep_ceiling_is_the_scratch_cap_so_a_correct_sweep_never_needs_more` + `a_sweep_that_did_nothing_says_nothing` (no summary on a quiet launch; a SKIP is worth surfacing); the skip-don't-stop and delete legs 🔨 L1 — they need a live tunnel + a real provisioned site (`mcp_scratch_check`, task 14). **Now wired** (launch + hourly) — and only with the summary, because a sweep at launch without one is the silent bulk delete a week-away user cannot distinguish from data loss. The summary NAMES the domains rather than counting them (a user recognises one they cared about and can act), says what was not touched (a claim the teardown backs independently of the predicate), and is silent on a quiet launch so a user with no scratch sites never learns the reaper exists — `the_summary_names_the_sites_and_a_quiet_launch_says_nothing`. Feed rows are the durable record either way, so a dismissed banner loses nothing |
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
| 196 | dbmirror.rs (grant_db_object) | GRANT `ON db.*` names exactly ONE database — `_`/`%` are pattern wildcards even inside backticks, so a bare `wp_shop` grant also covers `wpashop`; escaping them stops a mirrored user reaching a sibling site's schema (Tier-1 cross-site; was a live bug, `wp_<slug>` names carry `_`) | ✅ `grant_names_exactly_one_database_escaping_wildcard_metachars` |
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

## MCP server (PROPOSED — pre-implementation posture, `docs/PLAN-mcp-server.md`)

Forward-recorded so nothing downstream describes MCP as sandboxed before the code
exists. Re-anchors to the toggle/registry code when it lands (M2).

| # | Anchor | Claim | Verdict |
|---|---|---|---|
| 197 | PLAN-mcp-server.md §3.1 (D7) | ⚠ The MCP server is NOT a sandbox: once a client has scratch code execution it holds user-level power over rexenv and the machine (plugin activation is arbitrary user PHP; user PHP reaches the CLI socket), so the tiers deliver a paved road + no silent amplifier, NEVER containment | 🚫 inherently unprovable — a *containment* claim would be FALSE, not merely unproven; the posture is the claim (accepted, D7). Re-anchor to the toggle/registry code at M2 |

## Tally (mechanical — count rows by their LEADING verdict emoji)

The tally is recomputed, never hand-maintained (the hand-kept version drifted within
one day of being written):

```sh
for v in ✅ ◐ 🔨 🚫; do printf "%s " "$v"; grep -c "| $v" docs/CLAIM-LEDGER.md; done
```

As of 1 Aug 2026 (master; the dbmirror fix #196, the MCP plan #197, MCP M1
#198–#203, and M2a's ownership record, feed attribution, target provenance and
TTL touch, the scratch witness, the two-registry split, the agent create path and
the first executing tool, the guarded delete, Keep, the promotion choke point and
the reaper's sweep #204–#215): **✅ 134 · ◐ 39 · 🔨 37 · 🚫 5** of 215 rows, plus 5 🚫 premises living inside ◐/✅ rows (#15, #43, #52, #149, #154).
Recomputed mechanically with the one-liner above. The working backlog = every 🔨
row + the noted half of every ◐ row, ranked below.

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

These shapes have each shipped a false or overstated guard this month. They are one
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
- **Data / fixture — a mock or fixture friendlier than production.** The check exercises
  the right surface at the right time, but on UNREPRESENTATIVE inputs, so the gap is in the
  DATA/mock, not the code. THREE misses this session, and the escalation is the point — the
  same shape hid a leak, a cosmetic bug, and a crash:
  - **A leak, unseen:** the secret-leak sweep passed on a debug.log with a *relative* path
    when production logs carry *absolute* stack-trace paths that leak the docroot + OS
    username (the fixture misled the THING verified — it never triggered the leak).
  - **A cosmetic bug, certified fine:** the DevUiReview mock used friendly ids (`s-ea`), so
    the WebKit screenshot showed `→ myblog.test` while production stores a `uuid::new_v4()`
    and rendered `→ 550e8400-e29b-4…` (the fixture misled the VERIFIER).
  - **A crash, masked:** DevUiReview's `mockIPC` returns a canned `McpStatus` for
    `mcp_set_enabled`, so 10 green harness scenarios certified the toggle while the REAL
    command *aborted the packaged app* (`Handle::current` off the runtime, #203). A mock of
    the command under test can hide any backend behaviour — up to a hard crash.
  Each fixed by making the input production-shaped AND baking a probe that fails on the fake
  shape (real UUIDs + a no-bare-UUID probe; a plain-thread bind guard). Memory
  `fixtures-must-look-like-production`; L2's structural blind spot is stated in TESTING.md §1.

**The audit question this adds** — belongs in whatever the audit procedure becomes:
for every guard, *does the check cover the whole surface the claim names, for the
whole lifetime the claim spans, on data shaped like production?* If the claim says
"all X" and the check reads "one X" — or reads X on friendly-fake data — the guard is
narrower than its claim, and that gap (in code OR in the fixture) is exactly where the
next false-safety comment hides.

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
