# MCP server — let AI agents drive rexenv (scratch sites, DB access, tiered capability)

**Status: M1 SHIPPED 30 Jul 2026 (ledger #198–#203). M2a AND M2b SHIPPED and
code-complete 13 Aug 2026 — all eight executing tools registered in
`mcp_server/scratch.rs`, schema v27–v30. M3 (database access) SHIPPED 24–25 Aug 2026
(ledger #398–#404, v38/v39, `db_query` + the consent surface) and its §M3 gate was run.
EVERY milestone in this file is shipped; what is left, and the parity work that comes
next, lives in `docs/PLAN-mcp-parity.md` §1.**
Header corrected THREE times, and the third is the one to learn from (3 Sep 2026): it
read "M3 is the only milestone left, and nothing of it is in the tree" for nine days
after M3 shipped — the TODO row recording the correction was updated, this file was not.
A header that restates a milestone list is stale the moment the list moves.
21 Aug 2026: it had read "building M2a → M2b → M3" for eight days after
both were done, and listed D5 as open when it is settled (no SDK, hand-rolled — §9.5).
D2 (`wp_login_url`, scratch-only) is the one open decision and is non-blocking.
**Not shipped with the code: the human gates.** `docs/SMOKE-TEST.md` §M2a/§M2b carry 14
unticked steps and four HOLDs under a heading that says "ships only if this passes", and
MCP has now shipped in four releases without those being recorded as run.
Planned 28 Jul 2026 against `5dbaa8d`, from a full-codebase research pass + an
adversarial review that rewrote the security model (three blockers, §3.1
reckoning); scope ruled the next day. D1/D3/D4/D6/D7 settled (§9); D2/D5 open but
non-blocking; **the M3 database surface is ruled at M3, not now.** The one code
change already landed ahead of the feature is the mirrored-user GRANT
wildcard-escaping fix (its own commit, ledger #196 — §3.6). Line numbers are
anchors taken at `5dbaa8d` — trust the file, verify the line.

**Reconciled against shipped M1 on 1 Aug 2026** (§4/§5/§6/§7.3/§8). What M1
changed about this plan, so nothing below is read against the pre-M1 codebase:

- **`v26` is spent** — the agent activity feed took it. M2's sites migration is
  **v27**, and the feed's `actor` column is **v28** (§4.3).
- **M1 shipped three tools** — `list_sites`, `site_status`, `tail_log`. Mail is
  **not** in M1 (§7.3 said otherwise); it is M2b.
- **The M1 module split is now load-bearing** (`mcp_server/tools.rs` +
  `readctx.rs`, read-only guard #199) and dictates where M2's tools may live and
  where its TTL refresh may not (§2.6, §4.3).
- **S1 (1 Aug 2026): the dev-plugin link is a COPY-ON-WRITE CLONE, not a
  symlink** — `wp_run` makes the symlink's write-back guard unenforceable (§4.4).
- **S2: `wp_plugin`/`wp_theme` are dropped as tools** — `wp_run` subsumes them
  (§5).
- **S3: M2 splits into M2a (the scenario) and M2b (`set_php_version` + mail)**
  (§7.3).

This doc is the canonical home of the **agent capability model and its honest
limits** (§3), the **scratch-site lifecycle** (§4), and the **MCP attachment
architecture** (§2).

**Scope:** rexenv exposes a local MCP (Model Context Protocol) server so a
developer's AI agent — Claude Code, Cursor, VS Code Copilot — can drive rexenv
directly: spin up a disposable WordPress site, install the plugin under
development into it, run checks, query the site's database, read the logs, and
throw the site away. macOS first, same platform-trait discipline as everything
else. The driving scenario: *plugin dev asks their agent to test the
in-development plugin → agent creates a fresh WP site, links the plugin, runs the
check, site is disposable.* Database help during development is the co-headline.

---

## 0. The one-sentence shape

The MCP server is a **second mouth on the existing dispatch** — it lives in the
app process beside `cli_server`, speaks MCP over a new private `0600` unix socket
reached through a dumb `rex mcp` stdio pipe, and every tool resolves to the SAME
`commands::*` fn the UI and CLI call — but its **surface is a closed allowlist
with a capability tier per tool**, and its security is honest about one hard
fact: *running the user's site code is user-level power, and no tiering can undo
that* (§3.1).

## 1. What already exists — the mouth is mostly built

The one-fact rule at the API level is already proven by `rex`: one socket request
→ the same `commands::*` fn the UI invokes, never a parallel implementation
(`cli_server.rs:5-7,307-317`; ledger #57). Inventory the MCP server reuses as-is:

| Machinery | Where | What MCP gets for free |
|---|---|---|
| In-app socket server pattern | `cli_server.rs:86-121,1328-1351` — 0600 (`:95`), stale-unlink at bind (`:90-92`), connect-to-detect (`:82-85`), one tokio task per conn | The whole transport discipline; `handle_request` is generic over `tauri::Manager` and mock-testable (`cli_server.rs:143-147`) |
| Site provisioning as a pollable job | `commands/site_provision.rs:311-373`; settle-poll `state_of` at `:504` (the CLI blocks on it from `commands/sites.rs` `create_site`) | Scratch-site creation end to end, including the blueprint phase |
| Blueprints | `state/db.rs:48-62`, `core/blueprints.rs:38-86` — plugins/themes/WP_DEBUG presets, applied in the provision job | "Seed a site from a blueprint" is a create-time parameter, not new work |
| WP-CLI vetted ops + argument hygiene | `core/wordpress.rs` (plugins/themes/users/options/cron/search-replace…), whitelists in core (M7; `DEBUG_FLAGS`, slug guard `ensure_slugs`) | The safe WP operation set, injection guards already in core |
| Per-site PHP switch | `commands/sites.rs:374-411` — row + pool ensure + reload, no rebuild | The compatibility-matrix tool is a thin wrapper |
| Mailpit REST client | `core/mail.rs:26,35,243-324` — list/detail/raw/clear over loopback 18025 | Mail inspection is re-exposure, zero new plumbing (global-inbox caveat: §3.5) |
| Log tail IPC | `core/logs.rs:51-111` — per-site targets, `is_safe_key` (`:96-102`), 256 KB tail cap (`:18`) | Log reading with the traversal guard already structural (token-scrub caveat: §3.5) |
| Doctor composite | `cli_server.rs:1244+` — services + DNS + edge wire-identity + port scan + drifted takeovers | "What's serving and why not," minus a per-site HTTP probe (small add) |
| DB user/grant SQL shapes | `core/dbmirror.rs:81-131,211-222` — CREATE USER/GRANT, loopback-only `HOSTS` (`:36`), `RESERVED_USERS` refusal (`:88`), password-over-stdin (`:177-207`) | The SQL *shapes* for agent DB principals — but NOT its root auth (§3.6) |
| Secrets-can't-be-carried types | `dbimport.rs:64-128` (`DbConnection` not-Serialize, redacting Debug, `has_password`), `confedit.rs:43-56` (closed `RewriteKey`, no password variant) | The exact pattern for MCP output types (§3.5) |
| Ownership flags | `sites.docroot_managed` / `db_created` / `provisioned` — recorded, never derived | Scratch teardown inherits every existing deletion guard (§4.5) |

New work (does not exist): the MCP endpoint, the tool registry + tier
enforcement, the native-driver agent DB read path (§3.6), the consent dialog,
the `origin` column + reaper, agent-scoped DB users, the activity feed, a
per-site HTTP serving probe, link-target blast-radius validation (§3.3).

## 2. Attachment — how MCP reaches the dispatch

### 2.1 Where the server lives: in the app process

The MCP endpoint runs **inside the app**, a sibling of `cli_server::spawn`
(`lib.rs:399-404`). Not in the shim, not a standalone daemon:

- **One brain.** A headless MCP backend would be a second ServiceManager /
  second SQLite writer — the exact class ledger #54/#59 calls corruption, and
  why `rex` refuses to autostart anything (`cli_server.rs:8-10`).
- Consent dialogs (§3.4), the activity feed (§6), and job registries all need
  the app's UI and state.
- The app process is `mark_app_process`-blessed, so `core::stack_guard` friction
  never applies.

**No SDK — a hand-rolled minimal server (decided 29 Jul 2026, reversing the
draft's `rmcp` choice).** Every `rmcp` release is edition 2024 → rustc ≥ 1.85
(3.0 declares MSRV 1.88), and the repo pins `rust-version = 1.77.2`. Three reasons,
in the order that decided it:

1. **A proposed feature does not get to raise the project's declared toolchain
   floor.** If 1.77.2 moves, it moves on its own merits, decided for its own
   reason — not as a side effect of an SDK pick.
2. **Dependency-closure discipline.** This codebase pins and checksums every
   binary it touches, ships a 389-crate THIRD-PARTY-NOTICES, runs a root
   LaunchDaemon and holds a CA. Pulling `rmcp`'s whole tree in for a surface this
   small is out of grain with everything else here.
3. **Scope makes it safe, not brave.** M1 needs only MCP's stable, boring core —
   `initialize`, `tools/list`, `tools/call`, `ping`, over newline-delimited
   JSON-RPC 2.0 — the part every client handles identically. The evolving parts
   (resources, prompts, elicitation, output schemas, tool annotations, JSON-RPC
   batching) are explicitly out of v1 (§7.4), so we own only the part nobody
   disagrees about. `serde_json` is already a dep; the server is a few hundred
   lines under the closed registry.

**What we've taken on — owning a protocol means owning its compatibility.** We
implement the **2025-11-25** stable JSON-RPC core, and negotiate the handshake by
echoing the client's requested `protocolVersion` when it is one we recognise
(`2024-11-05` → `2025-11-25` share this core), else returning `2025-11-25`. We
deliberately do NOT implement the evolving features listed above. **Revisit
triggers, so this is re-openable with evidence rather than re-litigated from
scratch:** if the spec's *stable core* changes in a breaking way, or a v2 needs
the evolving features, `rmcp` comes back on the table — and the MSRV bump then has
a concrete reason of its own and a clearer cost. **Compatibility is proven against
a real client, not our own encoder** (§8): an L1 check speaks a spec-literal
handshake, and a manual `claude mcp add rexenv -- rex mcp` step confirms the #1
target client — because the hand-roll risk is precisely a framing or schema
detail our own tests accept and Claude Code rejects.

### 2.2 Transport: a second 0600 socket + a dumb `rex mcp` pipe. Never TCP.

- New socket `<config>/rexenv-mcp.sock`, same discipline as `rexenv-cli.sock`
  and the edge admin socket: born `0600`, stale file unlinked at bind, liveness
  = connect, **never TCP** (`cli_server.rs:12-14`, `proxy.rs:17-24`). A loopback
  TCP MCP port would be reachable by any local process and — the lesson WP
  Playground's bridge learned publicly — by DNS-rebinding web pages unless we
  add origin/token machinery. The unix socket makes that unnecessary: filesystem
  perms ARE the auth, the same trust boundary the CLI documents (ledger #55).
- Separate socket rather than reusing `rexenv-cli.sock`: lifecycles differ (CLI
  = one request per connection; MCP = a long-lived JSON-RPC session per client).
- **Client command is `rex mcp`** — a new subcommand on the existing sidecar
  binary, which stays a bidirectional pipe (stdin→socket, socket→stdout) that
  constructs no request and interprets no method — **amended 3 Sep 2026 (parity
  P6.2, #493): it does READ the ids of the requests it forwards**, for the one
  reason §2.3 deferred: to answer a request left pending when the app closes the
  socket with an in-band error the model can read — keeping the `cli/` crate's structural
  never-links-the-app-lib guarantee (`cli/Cargo.toml:9`). Zero-install: `rex` is
  already bundled and symlinked on PATH (`core/cli.rs:64-113`, Homebrew cask).
- Multiple clients = multiple concurrent connections, one MCP session each; the
  app records each session's `clientInfo` (name/version) from MCP `initialize`
  — **display and audit only, never policy** (a client-asserted string).

### 2.3 App not running: honest failure, no autostart

Same ruling as the CLI (`cli/src/main.rs:146-153`): the shim fails to connect,
prints a specific reason (`rexenv isn't running — open the rexenv app, then
reconnect…`) on stderr, exits 2. Deliberately no autostart / no headless mode
(§2.1). The reason travels on stderr + exit code, which an MCP client surfaces as
the server's startup error. Version skew: the tool registry lives app-side, so
the CLI's stale-binary unknown-command class (`cli_server.rs:1320-1322`) can't
happen for tools — a connected client always sees the running app's tools.
`serverInfo.version` = app version.

**Why the in-band variant stays deferred — and the exact condition that reopens
it.** In M1 there are no tools, so the only failure is "the client couldn't
connect at startup," which is *a human's* question ("why won't it connect?") and
a startup error on stderr reaches the human fine. The calculus changes at **M2**:
once an agent holds tools and the app quits **mid-session**, the model gets a bare
transport error mid-conversation and will *guess* — that is exactly when an
in-band JSON-RPC error (the shim reading the pending request's `id` and returning
a legible "rexenv stopped" error the model reads) earns its added complexity. So
this is not an open nicety: it is **deferred with a trigger** — revisit when the
first tools land (M2) and a mid-session app-quit becomes a model-facing failure,
not a human-facing one. Until then the shim stays a dumb pipe. **The trigger fired
at M2 and the item was built with MCP parity P6.2 (3 Sep 2026, #493)**: the bridge
tracks the ids of forwarded requests and, on socket EOF, replies to each pending one
with a JSON-RPC error whose message says rexenv stopped, that the outcome is unknown,
and to check before retrying anything that creates or changes something.

### 2.4 The registry IS the surface — an allowlist, not the 42

**One fact at the implementation level; a closed set at the surface level.**

```rust
struct AgentTool {
    name: &'static str,
    tier: Tier,              // T0 Unattended | T1 Consent | (T2 = absent from this table)
    // schema + handler → the SAME commands::* fn the UI calls
}
```

- Reusing `commands::*` means MCP inherits everything those fns can do. So the
  registry is **deny-by-default**: a tool absent from the table does not exist,
  and tier is declared where the tool is declared — you cannot register a tool
  without ranking it.
- Tier enforcement happens ONCE, in the registry dispatch loop, before any
  handler runs. T1 handlers additionally take a `ConsentGranted` witness
  argument constructible only by the consent-dialog resolution path (§3.4) — the
  `confverify::Verified` pattern (`core/confverify.rs:34`): a code path that
  skips the dialog does not compile.
- Tools carry honest MCP annotations (`readOnlyHint`/`destructiveHint`), but the
  spec itself says annotations are untrusted — UX hints for clients, never
  enforcement.
- MCP resources/prompts/elicitation: not in v1 (client support is uneven; tools
  cover the need). Tools only, `outputSchema` where clients benefit.

**The registry-walk test is real but proves a NARROW thing** — see §3.1 for why
it is not the containment guarantee it looks like.

### 2.5 Long operations

Scratch-site creation takes ~30–90 s warm-cache (WP core download + install),
worse cold. v1 does what the CLI does: **the tool call blocks** until the
provision job settles (`state_of` poll, the shape `create_site` already uses),
then returns the outcome + the job's flat log tail. Agents handle long tool
calls fine; progress streaming rides the existing "progress streaming over the
socket" design item (CLI-ROADMAP 🔴) if it lands, and MCP's Tasks extension is
too new to build on.

### 2.6 The M2 module boundary — M1 gated the VERB, M2 gates the OBJECT

M1's read-only guarantee is structural: a handler receives only a `ReadCtx`,
which exposes no mutating method and keeps its state field private, and a guard
scans BOTH `tools.rs` and the `readctx.rs` bridge (#199). **M2 breaks read-only
by design, so the question is what M2's equivalent discipline is — and the answer
is not the same shape.**

- **M1's guarantee survives M2's arrival untouched, and that is by construction,
  not by luck.** ✅ **Built 1–2 Aug (task 7, #209).** The guard is `include_str!` over exactly those two files and its
  claim is scoped to *M1 tools* — it does not say "the MCP surface is read-only",
  and neither does any ledger row (#198–#203) nor the Settings copy (whose "as
  rexenv adds more capable tools" clause was written for this moment). M2's tools
  therefore live in **`mcp_server/scratch/`** with their own ctx and their own
  capability; the guard keeps passing, and keeps firing loudly if anyone ever puts
  a mutating tool back in `tools.rs`.
- **M2's structural half is the OBJECT, not the verb.** ✅ **Built 1 Aug (task 6,
  #208):** `core::scratch::ScratchSite`, the `confverify::Verified` pattern —
  private field (module-private, not `pub(crate)`), no `From`, no other
  constructor, and the one door (`claim`) reads the row ITSELF so no caller can
  pass a `Site` it built or edited in memory. All three bypasses are compile
  errors, captured: E0423, E0451, E0277. The gate lives **in core, not the tool
  layer** (M7 style), so the CLI and the UI inherit the same refusal. Deletion is
  by record (`origin` ∧ `docroot_managed`) through the existing `delete_site`
  provenance rules.
  - **Refusals are policy statements, not type errors.** An agent reads them, so
    they name the site, the rule and the way forward — and "there is no such
    site" is kept distinct from "that one is yours", because those send an agent
    to different next steps. No path, no database name.
  - **The witness is not a lock, and the doc says so.** It proves the path was
    gated when claimed; `origin` can change a moment later (Keep). Destructive
    writes therefore re-assert `origin = 'agent'` in their own `WHERE` — the same
    pairing as the TTL touch. A one-time check on a mutable fact is a snapshot.
- **What is NOT structural, said plainly: nothing constrains the code that runs
  inside a scratch site.** Once a plugin activates, the only things left are the
  tier gate (the dangerous verbs are not tools), the disclosure, and the feed —
  no mechanism. That is D7/#197, and M2 is where that row is re-anchored from
  this plan doc to the toggle/registry code.

**What the SOCKET guarantees, once both registries are routed** (added with task
7, because this is the statement that changes even though #199 does not). M1's
claim is about what ITS tools can do. The endpoint's claim is a different
sentence, and it is:

- a call reaches exactly one registry, and its capability is decided by which one
  it came from — never by the tool's own say-so, never by its arguments, and
  never by lookup order, since the registries are **disjoint by test**;
- a READ tool cannot mutate anything (#199, untouched by M2's arrival);
- an EXECUTING tool can only reach a site the agent OWNS (its context's only door
  to a site is the origin-checked witness, #208) — and inside such a site it runs
  the user's code, which is user-level power over the machine (§3.1, #197).

**"The MCP socket is read-only" is therefore true of M1 alone and FALSE of the
endpoint** the moment a scratch tool lands. This is the §3.1(c) shape again — a
narrow truth positioned where the wide falsehood is the available reading — so
the Settings card's enable-moment copy ("Today those are read-only… not change or
run anything") is **held to the registry by a test**, not by anyone remembering:
it fails the day the first executing tool ships with that sentence still above the
toggle, and the failure message says what to write instead.

**Two things M1 leaves that M2 must actively fix, or a shipped claim narrows by
omission** (both found in the M2 pre-read, neither visible from M1 alone):

1. **The secret-leak sweep enumerates ONE registry** (`tools::sweep_plan`). A
   second registry with the sweep still walking only M1's would silently turn
   "every registered tool's output is swept" into "every M1 tool's". ✅ **Fixed
   1–2 Aug (task 7):** `sweep_tool_outputs` walks both, and `ScratchTool` carries
   the same required `sweep_args` field, so an executing tool cannot be
   registered unswept.
2. **`tool_target_site` reads `arguments.site_id`** — `scratch_create_site` has
   no `site_id` at call time, so the single most important feed row ("the agent
   created `foo.scratch.rex`") would record no target. ✅ **Fixed 1 Aug (task 4),
   and NOT from the tool's result** as this line first proposed: a result is
   exactly the channel a future tool could echo an argument through, and the feed
   is typed to keep agent content out. Instead `ActedTarget` — an out-parameter
   whose only setter takes a `&Site` (a row from our own sites table), so the
   recorded id is rexenv-derived by construction. Being an out-parameter rather
   than a return value is what makes it **survive `?`**: the create records the
   site the instant the row exists, so a provisioning failure AFTER the insert
   still names the half-built site. Rule: **a row names a site iff a row for it
   exists** — nothing created, nothing named (#206).

## 3. Security model — and its honest limits

### 3.1 The reckoning: running the user's code is user-level power

Assume every MCP client is **potentially hostile**: an agent does whatever the
last thing it read told it to, and this project has already watched
prompt-injected reviewer agents. The adversarial review of this doc's first
draft forced a correction that reshapes everything below, so it leads.

**This also corrects the feature's original framing, not just the draft's.** The
brief that started this work said "nothing root should be an agent's to trigger."
That is the wrong axis. Once an agent can activate a plugin it has arbitrary
user-level PHP, and user-level PHP can open the sibling CLI socket and reach
`start`/`stop`/`tunnel.start` — so drawing the line at "root" describes a fence
that isn't there. The honest line is the one below: a paved road and no silent
amplifier, with genuine containment only for the surface that executes no code.
Better to say that than to ship a tier model that reads as a sandbox and isn't.

**There are two client classes, and the tier model means different things to
each.**

1. **Shell-capable clients** (Claude Code, Cursor with a terminal) already hold
   user-level power *without rexenv*: they can read the CA key (it is `0600`
   owned by the user — `ssl.rs:17,134`), connect to passwordless loopback root
   MySQL (`client_base_args`, `database.rs:105-114`), run their own cloudflared,
   and drive the `rexenv-cli.sock` directly (which exposes `start`/`stop` →
   root prompt, and `tunnel.start` — `cli_server.rs:325,329,983-989`). For these
   clients, rexenv's tiers are **paved-road security**: injected instructions
   overwhelmingly use the tools in front of them, so a paved road that refuses
   `DROP` on real data and never offers a one-click tunnel defeats the realistic
   attack — but it cannot and does not *contain* a determined injected agent that
   already has a shell. The only hard gate that survives for this class is the
   **human auth prompt** on root ops, and rexenv's real duty is to never make
   that prompt routine.

2. **Shell-less clients** (sandboxed/browser agents, or a client configured with
   *only* the rexenv MCP server and no exec tool). For these the tier model is
   the real boundary — **until a tool grants code execution.** And here is the
   fact the first draft hid: **the scratch-site feature inherently grants code
   execution.** Activating a plugin runs its activation hook as the user; every
   subsequent `wp` call boots WordPress and loads active plugins; `wp_run`
   (D1) is arbitrary `wp eval`. You cannot offer "install the in-development
   plugin and run checks" without running the user's PHP as the user. And code
   running as the user can open `rexenv-cli.sock` and reach `start`/`stop`/
   `tunnel.start` — so the moment a shell-less client uses scratch sites for
   their intended purpose, it collapses into class 1.

**Therefore rexenv does NOT claim to contain a hostile agent below user level
once scratch code execution is in play.** Saying otherwise would be the kind of
overclaim this codebase treats as worse than no claim. What the tier model
actually delivers, stated precisely:

- **(a) A safe paved road.** The default, convenient path an agent reaches for
  contains nothing that destroys real work or publishes to the internet.
  Destructive and public actions are not sitting in the tool list to be fumbled
  into by a non-malicious agent following injected text.
- **(b) No new *silent* amplifier.** Every genuinely privileged escalation still
  requires a human (the root auth dialog) or is simply absent as a tool. rexenv
  never converts "agent ran a tool" into "root happened silently."
- **(c) A real boundary for the no-code-execution surface.** A client that uses
  only the diagnostic/read tools (M1) and never touches scratch execution stays
  genuinely contained — read-only and secret-scrubbed. **"No writes" is the
  imprecise version and this doc used to say it** (corrected 1 Aug 2026): a tool
  call always writes rexenv's OWN records — the activity feed, and from M2a the
  named scratch site's TTL. Neither is reachable by a handler (both happen in the
  session layer, on rexenv's account) and neither touches the user's sites, their
  files, or their databases, which is what the containment claim is about. Saying
  "no writes" invited someone to read the narrower true thing as a wider false
  one.

What the tier model does **not** deliver: containment of an agent that holds a
shell, or one that has scratch code execution and chooses to reach the CLI
socket or read the CA key. Those are user-level-power facts of a local dev tool
that runs your code. The residual — closing even the scratch-execution → CLI-
socket path — is a hard, separate hardening question (§9 D7), not a v1 claim.

The consequence for the rest of §3: the fences below are drawn to serve (a) and
(b) — keep the paved road safe, add no silent amplifier — and are **honest that
(c) ends where code execution begins.**

### 3.2 The tiers

| Tier | Meaning | Examples |
|---|---|---|
| **T0 — unattended** | Agent calls it, it runs, the activity feed records it | Everything scoped to **scratch sites** (create, delete, WP ops, raw wp-cli per D1, DB rw on own schema, PHP switch, link plugin); read-only diagnostics (list sites redacted, site status, log tail *scrubbed*); scratch mail; real-site DB **SELECT** only after a scoped, expiring T1 grant |
| **T1 — consent in the rexenv UI** | Native dialog in the app, per operation; deny on timeout, deny when no window. **Auto-allow (25 Aug 2026)** may answer T1 with yes for the DB read — session-scoped, in `AppState` not `settings`, still recording a flagged grant, and scoped to the PROMPT only: the tier rule is not a prompt (#408) | First read-only DB grant on a **real** site (§3.6); real-site mail/log exposure (§3.5); mutating a **real** site — M4, may ship never (§9 D3) |
| **T2 — never a tool** | Not in the registry; no MCP tool offers it | Everything in §3.3. NB: "not a tool" is (a)+(b), not containment — scratch code can still reach some of these via the CLI socket (§3.1); that is the D7 residual, stated not hidden |

The tier boundary is **ownership, recorded**: `sites.origin = 'agent'` (§4.1) is
what makes an operation unattended, never the site's name, never a path test —
the recorded-not-derived rule (`docroot_managed`, v17).

### 3.3 T2 — never offered as a tool, and why each

| Surface | Why never a tool |
|---|---|
| Service start/stop (whole stack) | `start_services`/`stop_services` reach `run_privileged` (edge daemon install/bootout — `proxy.rs:320,358`, via `services.rs:132`). Prompt-fatigue amplifier. Tools needing the stack return "stack is stopped — press Start in rexenv" |
| Anything TLD/resolver | `configure_resolver`/takeover/handback are root ops (`dns.rs:273,548`). Scratch creation is pinned under `.rex` with a never-prompt provision flag (§4.2) so it can never reach `ensure_resolver`'s `Absent` prompt (`dns.rs:346-353`) |
| CA operations, cert regeneration | The CA mints OS-trusted certs; its key is read-is-compromise (`ssl.rs:17,134`). No tool touches reissue; no output names the `ca/` dir (§3.5) |
| Tunnels (start/stop/list) | §3.7 |
| `system_setup` / `uninstall_system` / `cli_install` | Root ops (`commands/system.rs:227-384`) |
| Real-site create/delete/rename/move; linked-site creation | Deleting/mutating user work; linked-site creation takes an arbitrary path — a human flow (`validate_linked_docroot`). Agents get scratch sites only |
| `db.import` / `db.reset` / real-site DB drop | Destructive on user data (`wp_db_import` destructive-by-contract, `commands/wordpress.rs:704`) |
| Settings mutation (`sites_dir`, TLD, ports…), blueprint CRUD | `sites_dir` repoints what deletion may touch — the 24 Jul incident class. Read-only where needed, never write |
| Arbitrary file read/write tools | The generic leak/write primitive. `scratch_add_package` is NOT an exception to this — it is constrained to a header-bearing plugin/theme source that passes blast-radius validation, not a file-read primitive (§4.4); the first draft's version WAS such a primitive and is fixed |
| `wp_admin_login_url` on real sites | A one-time admin session handed to the agent. Scratch-only (D2) |

**The 7 `run_privileged` call sites** (exhaustive at `5dbaa8d`, inlined here so
this doc carries the fact rather than pointing at a research artifact):
`proxy.rs:275` (legacy edge start), `proxy.rs:320` (edge daemon install),
`proxy.rs:358` (edge daemon stop), `dns.rs:273` (resolver write), `dns.rs:548`
(resolver hand-back), `cli.rs:71` (CLI symlink), `setup.rs:95` (teardown).
Their command-layer entries are the T2 exclusions above.

**Enforcement is (a)+(b), and provable at the tool layer:** a lib test walks
every registered tool to its `commands::*` mapping and asserts none is one of the
enumerated privileged-reachable command entries; a second asserts no tunnel/
system/settings command is mapped at all. **These tests certify the tool surface,
NOT containment of executed code** (§3.1) — the ledger rows (§8) say so in those
words, so they can never be read as a guarantee they don't make.

### 3.4 Consent is a native rexenv dialog — never MCP elicitation

MCP elicitation is in-band (its context arrives through the channel the injected
agent controls) and is unavailable in Claude Code anyway — a gate built on it
silently degrades to no gate. So T1 consent = a **native dialog in the rexenv
app**, foreground (background prompts don't render), naming the client, the
operation, the target, and the blast radius, verbatim-drafted per tool. Default
**Deny**: timeout (120 s) denies; no window denies; the tool returns "user
declined" as an ordinary tool error. No session-wide "always allow." The one
durable grant (the RO DB grant, §3.6) is **scoped + expiring + visible +
revocable** — not blanket.

### 3.5 Output types that can't carry a secret — and the scrub covers *outputs*

Every MCP tool reply serializes an `Agent*` view type, never an internal struct;
conversions are explicit and lossy, the view types live in one module, adding a
field is a reviewable act. Proven pattern: `DbConnection` is not-Serialize with a
redacting Debug and a `has_password: bool` UI shape (`dbimport.rs:64-128`);
`RewriteKey` has no password variant so no diff can show one
(`confedit.rs:43-56`).

Leak audit — what each output could carry, and the rule:

| Output | Risk | Rule |
|---|---|---|
| Site views | docroot/db_name — fine. CA/cert paths, admin-socket path, `site_env` values — not fine | Views carry id/name/domain/type/php/server/origin/status/url/docroot/db_name. **No cert paths, no socket paths, no `site_env`** (users park real secrets there — `commands/sites.rs:573+` round-trips them to the UI; agents never see them) |
| `db_query` results | Whatever the DB holds. On a **real** site that includes `wp_users` password hashes and `wp_options` API keys/tokens | The T1 grant dialog says so in plain words (§3.6 copy). Row/byte caps bound volume, **not which rows** — the boundary is SELECT-only + the honest dialog, not redaction |
| Log tails | Access logs carry one-time tokens: `?rexenv_login=` (`wp_login.rs:169-197`), WP password-reset `?key=`, auth cookies | **Scrub known auth/token query params + `Set-Cookie` from returned lines** before any tool sees them (new, in `core/logs` or the view layer). Real-site log tail stays useful for debugging with tokens redacted; `is_safe_key` already blocks traversal |
| Mail | The Mailpit inbox is **global, no per-site tagging** (`core/mail.rs:65-75`) — it holds real sites' password-reset links and outbound mail | Mail is behind its **own opt-in sub-toggle, default off** (D4 flips): a global inbox carrying reset links is a credential-harvest pivot, not a safe T0 default. When on, only **scratch-tagged** messages are T0 — and the tag is one REXENV creates, not one the inbox already has (see below). The toggle's copy names the real-site exposure. Every read is feed-logged |

**The "scratch-addressed" filter, corrected (1 Aug 2026).** The first draft
assumed the recipient address identifies the site. It does not: WordPress mails
the *admin* address (`user@example.com`), and Laravel's default `MAIL_FROM` is
`hello@example.com` — neither carries the site's domain, so a To-address filter
would be a claim checked against a field that doesn't hold the fact (the
guard-covers-a-narrower-surface family). So **the tell is one rexenv creates**: a
scratch-only mu-plugin (the shape `mu-plugins/rexenv-tunnel.php` already uses)
forces `From` to the scratch site's own domain, and the filter matches that.

The property this buys is **fail-closed, and that is the point**: a message
without the tell is invisible to the agent, so the failure mode is *the agent
misses its own mail*, never *the agent reads the user's*. A site's own code can
overwrite `From` after our filter runs, in which case its mail simply stops being
visible — again the safe direction. This reasoning goes in **the toggle's own
copy**, not only here: the user turning it on is the person who needs to know
their real sites' mail is not in scope and why the scratch mail sometimes isn't
either.
| Login URLs | One-time admin session token (`wp_login.rs`) | Scratch sites only (D2); never real sites |
| `site_status` / doctor | The doctor composite strings can name the admin socket / Caddyfile (`cli_server.rs:1244+`) | The `Agent*` view drops those fields; see the sweep below |
| Errors | Errors embed paths/commands (port-conflict help embeds a copy-paste fix) | Error strings pass a scrub that never includes the CA key path or the admin socket path; otherwise verbatim — agents need real errors |

**The seal is a serialized-OUTPUT sweep, not a type-definition sweep** (the first
draft's gap): a test populates a fully-loaded state with planted secrets (CA key
path, a `site_env` value, a `?rexenv_login=` log line, a password string, an
admin-socket path), then calls **every registered tool** and asserts none of
those strings appears in the serialized output — doctor, log chunk, mail body,
site view, error alike. That test is the ledger row.

### 3.6 Database access — grants, a native driver, never the shell client

Two independent holes in the first draft's "the line is where MySQL enforces it":

1. **The bundled `mysql`/`mariadb` client interprets client-side commands before
   the server sees them.** `mysql -e "system id"`, `\! sh`, `source <file>`,
   `tee <path>` execute shells and write files as the user regardless of the SQL
   grant. Every existing exec helper feeds the client via `-e`
   (`database.rs:120-124`) or stdin (`dbmirror.rs:177-207`) — so "reuse dbmirror
   wholesale" would hand a `GRANT SELECT` principal shell-exec + file-write on a
   **real** site, from the one tool T1 authorizes.
2. **`GRANT … ON \`wp_<slug>\`.*` grants a pattern, not a schema** — MySQL treats
   `_` and `%` as wildcards in database-level grants even inside backticks, so a
   bare grant on `wp_shop` also covers `wpashop`, `wpXshop`, … "The site's own
   schema, nothing else" would actually be every db matching the pattern. **This
   was a live bug in the *existing* mirrored-user code (`dbmirror.rs`), not an MCP
   concern — it is fixed independently and ahead of this feature** (`grant_db_object`,
   ledger #196); MCP agent grants simply reuse it.

The corrected design:

- **Agent queries run through a native Rust MySQL driver** (e.g. `mysql_async`/
  `sqlx`), never the bundled interactive client. Single statement per call,
  multi-statements off, `local_infile` off, and the principal lacks `FILE` — so
  `system`/`\!`/`source`/`tee`, `LOAD DATA LOCAL`, and `INTO OUTFILE` are all
  unreachable. This is a **deliberate, reasoned departure from the bundled-client
  rule** (the same class as "composer is always our pinned phar" and "repo builds
  use the developer's toolchain"): the bundled-client rule exists so dump/restore
  match the server's version, but a native driver speaks the stable MySQL wire
  protocol to the same loopback port and, for the agent read path specifically,
  its refusal to interpret `\!`/`system` is the whole point. Pinned by an L1 test
  that connects as the RO principal and attempts `system`, a write, and
  `INTO OUTFILE`, each expected to fail.
- **Grant objects name exactly one database** via `grant_db_object` (escapes
  `_`→`\_`, `%`→`\%`) — already landed in the mirrored-user code (§3.6 hole 2,
  ledger #196), inherited here, not re-solved.
- **Principal, not root.** Agent connections use `rex_agent_<slug>` (scratch, rw
  on its own escaped schema) or `rex_ro_<slug>` (real, `GRANT SELECT` only),
  passwordless + loopback-only (strictly ≤ the root-on-loopback exposure any
  local process already has). The CREATE USER/GRANT statements that *provision*
  these run as root once (an admin op); the **agent's own queries never touch
  `client_base_args`** — a test asserts the agent query path never calls it.
  Reuses dbmirror's SQL *shapes* (`HOSTS`, `RESERVED_USERS`, `sql_str`), not its
  root auth.
- **`DROP DATABASE` is unavailable on real sites** by the SELECT-only fact.
  (Scratch `rex_agent_*` holds `ALL` on its own disposable schema, so it *can*
  drop that schema — harmless, it's disposable; the first draft wrongly said
  "everywhere.")
- **Real-site `db_query` requires ONE scoped, expiring T1 grant** — *as designed; RETIRED by D16, 4 Sep 2026 (`PLAN-mcp-parity.md` §7): the Agent access dial's Read answers it, no dialog, no expiry, the table kept only as the record of provisioned principals.* The dialog was
  drafted honestly: *"Allow "Claude Code" to read the database of `mysite.rex`?
  The agent will be able to read everything in it — including user password
  hashes and any API keys or tokens stored in wp_options. It cannot modify or
  delete anything. This access expires in 7 days."* Recorded in `agent_db_grants`
  (site_id, user, granted_at, expires_at — recorded, never re-derived), listed
  and revocable in the UI, dropped on site delete alongside the site's DB user
  (`commands/sites.rs:884-907`). **Expiry + client-change re-consent** answer the
  first draft's blanket-grant hole: a grant is bound to a scope and a clock, its
  first use per session surfaces in the feed, and a new `clientInfo` re-prompts.
- PostgreSQL: out of v1 (PG cannot host site databases — `core/db.rs:223-227`;
  zero exec/grant plumbing exists).

### 3.7 Tunnels — never a tool (v1), with the residual stated

Ruled **never a tool** (no tunnel in the registry; test-pinned). The case:
exposure is public-internet, unauthenticated, whole-docroot, no revocation lever
but killing the process (no account, no token); the fossil-tunnel incident (4
tunnels served publicly 9–15 days) is exactly "a share nobody remembers
starting," which agent-started shares are by nature; and the backend start path
has no server-side consent — the CLI's y/N is client-side only
(`cli/src/main.rs:1485-1497`).

**Honest residual (§3.1):** "not a tool" delivers (a)+(b) — the paved road never
offers it — but scratch code execution can reach `tunnel.start` through the
sibling CLI socket, so this is not containment against a hostile agent that
already has scratch exec. The mitigations that *do* hold regardless: every tunnel
still dies with the app and is swept at launch (including rowless orphans —
`core/tunnels.rs:485-525`), so an agent-started fossil cannot outlive the
session the way the 9–15-day ones did. Fully closing the path is D7.

The honest counter-case (**webhook testing** — Stripe → local Laravel/WP — is a
real loop where an agent-started tunnel would help) is recorded as the only
future reopening argument, gated on real demand + T1 + scratch-only + auto-stop
(§9 D6). Until then the agent may *see* a site is shared (status), never make one.

## 4. Scratch sites — the feature's centre

### 4.1 A recorded origin, a visible namespace

- **Truth = a column:** `sites.origin TEXT NOT NULL DEFAULT 'user'`
  (`'user' | 'agent'`), migration **v27** — v26 is spent (M1's `agent_actions`
  feed, `state/db.rs:313`) — plus `agent_client TEXT` (the clientInfo string,
  display-only) and `expires_at TEXT` (nullable). Recorded at create; **no IPC
  path sets `origin='agent'` on an existing row, and nothing flips agent→user
  except user action** (§4.3) — monotonic like `docroot_managed`.
- **Convention = a name:** agent sites live under `<name>.scratch.rex`.
  Multi-label domains are first-class — `validate_domain` accepts them (test pins
  `sub.mysite.test`, `core/sites.rs:25-57`), each gets its own cert with a
  one-level `*.<domain>` SAN (`ssl.rs:151,164`), exact Caddy host beats any
  wildcard (`proxy.rs:116-119,167-169`), and the DNS agent answers any depth. The
  suffix is UX so a human scanning the Sites list can't confuse scratch with real
  — **policy always reads `origin`, never the name**: a user who hand-creates
  `foo.scratch.rex` owns a normal site the reaper never touches.
- UI: Sites page groups scratch under an "Agent scratch" section with client
  badge + remaining TTL; SiteDetail works unchanged (they are real sites — the
  user can open, inspect, adopt them).

### 4.2 Creation: the ordinary provision path, fenced

`scratch_create_site(name, {php?, blueprint?, multisite?})` → the existing
`site_provision_job` with: domain forced to `<name>.scratch.rex`, type
WordPress, `origin='agent'` recorded at insert, docroot under the normal sites
dir (`docroot_managed=1` is the deletion authority, not the path), blueprint
honored. Fences:

- **A never-prompt provision flag for `origin='agent'` rows.** The `.rex`-resolver
  precheck alone is a snapshot (the one-fact/lifetime-guard lesson): the provision
  path calls `ensure_resolver` unconditionally (`site_provision.rs:353`), which
  prompts on `Absent`. So the agent path passes an explicit "never prompt — fail
  instead" flag that holds for the whole operation, and errors "finish rexenv
  setup first" rather than popping an auth dialog. Tested there, not at the tool
  layer. (`.rex` is the always-installed backbone, so this only bites a
  half-onboarded install.)
- **Cap:** max concurrent scratch sites, default 5 (setting). At cap the tool
  errors naming the count + the sites it may delete + TWO ways forward (delete
  one itself, or ask the user) plus the passive one (they expire) — ✅ built with
  task 8a. A refusal that states the rule and stops is where a model starts
  improvising: a different name, then another, then something else entirely. That is the answer to "an agent
  creates twenty" — it can't.
- Name is a single label (no dots), so an agent can't nest namespaces or squat
  `scratch.rex`. `unique_db_name` already disambiguates colliding slugs
  (`core/sites.rs:320`).

**The stack is stopped — it half-works, and it must SAY so.** Checked against the
job (`site_provision.rs:733` `spawn_db`; the job ends in a best-effort
`mgr.reload`): provisioning starts **the database engine** and nothing else. No
edge, no `run_privileged`. So an agent creating a scratch site with the stack
stopped gets a real, fully provisioned site that **does not serve**, and the tool
must return that as part of its success — `serving: false` plus the §3.3 shape
("rexenv's stack is stopped — press Start in rexenv"), never a bare "created".

Two consequences for what may be *claimed*, both load-bearing:

- **"No tool starts a service" would be FALSE** — `scratch_create_site` starts
  MySQL, exactly as creating a site in the UI does. Nothing in the guarantee, the
  card copy, or a ledger row may say otherwise. What is true and worth saying: it
  is a user-level start, it is never privileged, and it is never silent (feed row).
- **"Nothing an agent can call ever asks for an administrator password" is the
  strongest sentence in the guarantee and the most brittle** — it rests entirely
  on the never-prompt flag above holding for the WHOLE provision operation, not as
  an entry precheck (the one-fact/lifetime-guard lesson, twice bitten). Its test
  drives the operation end to end with the resolver absent and asserts the
  operation FAILS with the setup message rather than reaching
  `configure_resolver`; the sentence must be caught by that test the day it stops
  being true.

### 4.3 Lifecycle: TTL + cap + reaper + explicit teardown + Keep

"Disposable" = all of these, layered:

| Mechanism | Behavior |
|---|---|
| Explicit teardown | `scratch_delete_site(id)` — T0, refused in core unless `origin='agent'` (origin gate in core, not the tool layer — M7 style) |
| TTL | `expires_at` = create + 24 h (setting), refreshed by any agent op targeting the site. Idle scratch dies; active scratch lives. **The refresh lives in the SESSION layer, beside `log_action` — never in a tool handler**: `tools.rs` is scanned by the read-only guard and a refresh is a write, so putting it in a handler would either break M1's boundary or quietly weaken the guard. Recording the action and touching the TTL are the same write, one place. Free consequence: M1's read-only tools refresh a scratch TTL too, which is correct — the agent IS still using the site |
| Reaper | Launch + hourly in-app sweep: full `delete_site` path (DB drop honors provenance, teardown honors `docroot_managed`) for rows where `origin='agent'` ∧ expired ∧ `docroot_managed=Some(true)`. Deletes **by the record, never by name or path** (the 24 Jul example-cleanup lesson; ledger §8) |
| **A reap that FAILS** | `delete_site` already fails safe — a failed database drop leaves the site intact and retryable, never a silent orphan. What was missing is what happens next: **retry at most once per app launch** (never an hourly silent loop), record the failure as a feed row (below), and show the site in the Agent-scratch group as "expired — couldn't be removed: `<reason>`" with Retry / Delete. No new `sites` column: the feed carries the reason, the badge derives from expiry + presence |
| Keep, and **user-mutation implies Keep** | The scratch card's Keep action sets `origin='user'` + clears `expires_at`. **And any user-initiated mutation of a scratch row — rename, move, env edit, and starting a public share — auto-promotes it to `origin='user'` first**, closing the first draft's hole where a user renames a scratch site, the reaper still sees `origin='agent'` + `expires_at`, and deletes the site the user just adopted. Agents cannot call Keep or promote |
| Shared = **adopted** (promotion), skip-and-surface = the backstop | Sharing a scratch site IS adoption, so starting a tunnel on an `origin='agent'` row promotes it (`origin='user'`, `expires_at` cleared) and the UI says why — "kept, because you shared it". That collapses the first draft's ugly state (an expired site the reaper permanently refuses to touch) into a visible one-time adoption. The old skip-and-surface stays as the **backstop only**, for the share the promotion path never saw — a rowless orphan adopted at launch (`core/tunnels.rs:485-525`): the reaper skips it (`refuse_if_shared` guards deletion) and raises the persistent warning + notification, re-checked when the tunnel stops |

**Reaps in the activity feed — a typed `actor`, migration v28.** A reap is
rexenv-initiated, so recording it in `agent_actions` as shipped would make the
table's implicit claim ("an AI agent did this") false. Leaving it out makes the
other true claim false — that everything consequential which happens to
agent-owned sites is visible in one place — and *deleting a site is the most
consequential event in this lifecycle*. Both claims survive with one typed
column: `agent_actions.actor` (`'agent' | 'rexenv'`, DEFAULT `'agent'`), reaps
recorded as `actor='rexenv'`, `tool='scratch_reap'`, `target_site=<id>`, outcome
ok/error, `detail` = rexenv's OWN bounded reason. The typed-shape discipline
(#202) is untouched — a new typed column added deliberately, never a free-form
blob — the `client` on these rows is rexenv itself rather than an agent-asserted
string, and the card styles them distinctly. #202's wording is amended in the
same commit; the attribution half is its own row, #205.

**Built 1 Aug 2026 (task 3) — the rule that fell out of it: `actor` LABELS
everywhere and FILTERS in exactly one place.** Deciding what the existing
surfaces do was the substance, not the column:

- `recent` / `recent_for_site` (the card's list, SiteDetail's per-site section)
  return EVERY row whatever its actor. Hiding rexenv's rows would trade a false
  impression for a missing fact — and the reap is the fact most worth having.
- `recent_head` (the card's status line) excludes them, because that line is a
  claim about the AGENT's session: a sweep must never render as "Working —
  scratch_reap", and a failed reap must never render as "the last N calls
  errored". This is the only filter in the feed.
- In the UI a rexenv row reads **"rexenv · automatic"** rather than putting
  "rexenv" in the client-name slot, where it would simply read as an agent that
  calls itself rexenv.
- **A reap names a site it just deleted**, so `target_label` cannot resolve and
  the pre-v28 UI would have shown a bare UUID — the exact defect M1 fixed once
  already. The rule now: an unresolvable UUID target renders **"(deleted site)"**
  and the domain travels in rexenv's own `detail` text.

App closed at expiry → the launch sweep catches up (the tunnel `sweep_startup`
shape). Services keep serving expired scratch sites until then — harmless
loopback sites.

### 4.4 The dev-plugin loop — a copy-on-write CLONE, not a symlink (S1, ruled 1 Aug 2026)

The scenario's key step is *install the in-development plugin into the scratch
site*. The first draft made it an arbitrary-file-read primitive; the second made
it a **symlink** (the Valet-style edit-in-repo loop). **Both are wrong, and the
second one is wrong because of a decision made elsewhere in this plan.**

**Why the symlink fell.** D1 ships `wp_run` — raw wp-cli, scratch-only. A raw
runner can `wp plugin update <linked-slug>` and unpack over the user's real
checkout, unattended. The symlink design answered that with a linked-slug refusal
in the vetted `wp_plugin_update`/install commands — but a refusal in the vetted
path does not constrain a raw runner, and screening argv on a raw runner is
exactly the **guard-covers-a-narrower-surface-than-its-claim** family (`--force`,
aliases, `wp eval`, `wp package`, tomorrow's subcommand). That family has already
produced a cross-site exposure, a data-destruction hole, and a leak in this
codebase; a fourth instance whose failure mode is *the user's real checkout is
overwritten by an agent* is not one to ship knowingly.

**The ruling: clone, don't link.** `scratch_add_plugin(site_id, source_path)`
(and `_theme`) makes a **copy-on-write clone** (APFS `clonefile` — effectively
instant, near-zero disk) of the source into `wp-content/plugins/`. Writes inside
the scratch site touch the clone, never the source, so "your checkout is never
written to" stops being a hope about agent behaviour and becomes a property of
the filesystem. The edit-in-repo immediacy is bought back with an explicit verb:

- **`scratch_sync_plugin(site_id, slug)` — re-clone from the recorded source.**
  The texture cost of losing the symlink is real, so **the sync verb is part of
  the loop, not a footnote**: the tool description states the rhythm ("the scratch
  site runs the code as of the last sync — call this after you change the plugin")
  so an agent re-syncs without being told, and the scratch site's own UI card
  shows the source path and *when it was last synced*, so the first time a user
  hits "I changed my plugin and the site didn't see it" the answer is already on
  screen. The source path is **recorded** on the row at add-time, never
  re-derived, so a sync can only ever re-read where the clone came from.
- **The guarantee states the snapshot truth in the user's own words** (§6): what
  runs in the scratch site is the code as of the last sync, not what's in the
  editor. Cheaper to state than to let users infer it at the moment of confusion.
- **Blast-radius validation stays, unchanged and non-negotiable** — cloning is
  not a licence to read: `scratch_add_package(id, "$HOME")` must refuse exactly as
  hard as linking it would have. Both bullets below apply to the clone.
- ~~**The clone is a platform-trait op** whose macOS impl is `/bin/cp -c -R`~~ —
  **RETIRED 4 Aug 2026, on measurement. Do not re-open without re-measuring.**
  The premise was that the portable `std::fs::copy` recursion is a plain byte
  copy and that `cp -c` would buy APFS copy-on-write. **It is already
  copy-on-write**: Rust's `std::fs::copy` uses `fclonefileat` on macOS and falls
  back to `fcopyfile` when the filesystem can't clone — the exact behaviour this
  bullet wanted `cp -c` for, without a process spawn and without OS-specific
  plumbing. Measured on a realistic plugin tree (4802 files, 33 MB):

  | | wall clock (3 runs) | disk consumed by a 400 MB file |
  |---|---|---|
  | `std::fs::copy` recursion (**shipped**) | 0.75 / 0.61 / 0.65 s | **0 MB** |
  | `/bin/cp -c -R` | 0.81 / 1.33 / 0.83 s | 0 MB |
  | `/bin/cp -R` (plain) | 1.43 / 1.60 / 1.59 s | 400 MB |

  So the "optimisation" is **slower** than what ships, and would additionally
  cost a process spawn per tree, an OS-specific code path, and a `todo!()` on
  Windows/Linux where the portable version works today. It buys nothing and
  costs three things. (`cp -c`'s own degradation was verified too — `cp -c -R`
  onto a mounted HFS+ volume exits 0 and copies byte-identically — so the man
  page's claim is true; it simply isn't needed.)

The remaining constraints (unchanged from the symlink design):

- **Blast-radius validation.** The source passes `validate_linked_docroot`-grade
  refusals (`core/sites.rs:810-872`: `/`, `$HOME`, Desktop/Documents/Downloads,
  volume roots, app-data, overlap) — `repo::validate_link_target`
  (`core/repo.rs:1622-1654`) checks only self-nesting/cycles and is NOT enough.
  Without this, `scratch_add_plugin(id, "/Users/me/.ssh")` would put it inside the
  docroot and the server would happily serve `id_rsa` — not a dotfile, so the
  `/.`-segment dotfile guard (`frankenphp.rs:66`, `apache.rs:336`) does not cover
  it. **The clone changes the write direction, not the read direction: this guard
  is what stops the tool being a file-read primitive, and it applies verbatim.**
- **Must be a plugin/theme.** The source must contain a plugin header (or
  `style.css` theme header) before it is cloned — a source tree with no plugin
  header is refused. Turns "copy any directory in" into "add a plugin," which is
  the actual feature.
- **~~No write-back over a link~~ — retired by the clone (S1).** The refusal this
  bullet specified (extend the linked-slug guard to `wp_plugin_update`/install)
  was only ever a partial answer, and `wp_run` defeats it entirely. The clone
  removes the hazard at its root: there is no path from the scratch site back to
  the source, so nothing needs to remember to refuse. This also moots the vetted
  `wp_plugin`/`wp_theme` tools' one distinct value — see S2 in §5.

Read-and-serve of the (validated, header-bearing) clone is then T0: it is the
user's own machine, loopback, and the source is never written. `wp_run` (D1)
activates it — and per §3.1, activation runs the plugin's code as the user, which
is the feature, not a leak.

### 4.5 Interaction with existing flags — nothing new to invent

| Flag | Scratch value | Consequence |
|---|---|---|
| `docroot_managed` | `1` | Teardown removes the docroot — full disposal, by the recorded bit |
| `db_created` | `NULL` (our-provisioning semantics) | `may_drop_database` → dropped at delete (test `may_drop_database(site(None,None))==true`, `commands/sites.rs:978`) |
| `provisioned` | Normal job semantics | Failed scratch shows "setup incomplete" + Retry; the reaper also collects failed expired scratch |
| `content_dir`, `mu_dir_created`, `override_port` | Defaults | Unchanged |
| Linked sites | **Impossible for agents** | `scratch_create_site` never takes a docroot path; linking a whole external docroot is the human flow (`scratch_add_plugin` CLONES a plugin *into* a scratch docroot — different, and after S1 not a link at all, §4.4) |

## 5. Tool surface — ranked honestly

Every tool is a compatibility promise; v1 ships the smallest set that makes the
two headline scenarios real.

| Tool | Tier | Verdict |
|---|---|---|
| `list_sites` (redacted views) | T0 | **Essential context** — everything references site ids |
| `site_status(id)` — doctor + serving chain | T0 | **Genuinely useful**: "why isn't it serving" is the #1 agent question. Wraps the doctor composite + one new per-site HTTP probe (edge → vhost → status code), a gap the UI would benefit from too |
| `tail_log(id, source, lines)` (+ wp-debug), **scrubbed** | T0 | **Genuinely useful**: 502s live in fpm/nginx/wp-debug tails. Existing IPC + guards; token/cookie scrub added (§3.5) |
| `scratch_create_site` / `scratch_delete_site` (list rides `list_sites`) | T0 | **The centre.** Blueprint param included — seeding is a create-time flag, already built |
| `scratch_add_package` + `scratch_sync_package` | T0 | **The centre's second half**, clone-not-symlink per §4.4. The sync verb is part of the loop, not an extra. **Plugin and theme are ONE pair of tools, not two** (proposed with S1, veto-able at build time): §4.4 already requires reading the plugin/theme header before cloning, so the kind is a fact rexenv DERIVES, not a parameter the agent asserts — which is both two fewer permanent tools and one less thing an agent can get wrong |
| ~~`wp_plugin` / `wp_theme` (vetted list/install/activate/…)~~ | — | **DROPPED (S2, 1 Aug 2026).** `wp_run` subsumes them entirely, and their one distinct value — the linked-slug refusal on update/install — is moot under the clone (§4.4). Every tool is a permanent compatibility promise; two fewer |
| `wp_run(site, argv)` — raw wp-cli | T0, **scratch-only**, origin-gated in core | **Useful; D1.** Per §3.1 it does *not* change the containment story (plugin activation already grants user exec) — it is a usefulness call, not a new hole. Recommend ship scratch-only; the real-site raw-wp refusal stays permanent |
| `db_query(site_id, sql)` — native driver | T0 scratch (rw own schema); real SELECT-only after a scoped expiring T1 grant | **Co-headline** (§3.6) |
| `set_php_version(site_id, minor)` | T0 scratch; real M4/T1 | **Genuinely useful**: the compatibility matrix (scratch + plugin, run checks 8.1→8.5) is a real plugin-dev workflow rexenv is uniquely placed for |
| `mail_list` / `mail_get` | T0 scratch-addressed, behind an opt-in sub-toggle | **Useful for WP *and* Laravel** (password-reset, notifications, queued mail): trigger, read, assert. Global-inbox risk gates it behind its own toggle (§3.5, D4) |
| `wp_login_url(site)` | T0 scratch-only | **Marginal but cheap** (D2): hands the user a browser link into the scratch site, or drives a headless admin check. Never real sites |
| Run repo install/build steps | — | **Possible, not useful**: the agent has its own shell and repo view; our streamed job machinery adds ceremony, not capability |
| Real-site create/delete/rename/move; service start/stop; tunnels; blueprint/settings write; DB dump/export | — | Not offered (§3.3, §3.7); `site_status` reports "stack stopped" honestly. Real-site DB export is T1 material for M4 |

### 5.1 The Laravel / general-PHP developer — served, but the scratch *create* is WordPress-only in v1

The user base is "mostly WordPress, PHP and Laravel," so state the cut plainly
rather than let it hide: **`scratch_create_site` provisions WordPress only in
v1** — the provision path is WP-shaped (wp-cli core download/config/install), and
a Laravel scratch needs a different skeleton (`composer create-project`, `.env`,
`artisan key:generate`) plus a package-link shape that is a Composer path
repository, not a symlink into `wp-content`. That is real work, deferred, ranked
below.

What a Laravel / plain-PHP dev **does** get in v1, and it is not nothing: the
diagnostic and DB surface works against their **existing or linked** Laravel
site — `site_status` (is it serving, why not), `tail_log` (their `laravel.log`
rides the same log dir once targeted; the framework-log target is a small add),
`db_query` (real-site SELECT after the T1 grant — the co-headline serves Eloquent
schema exploration directly), `set_php_version` (the compat matrix), and
`mail_list`/`mail_get` (Laravel mailables/queued notifications land in the same
Mailpit inbox). Linking an in-development *Composer package* into a Laravel site,
and a `php_artisan(site, argv)` analogue to `wp_run`, are the Laravel headline —
**ranked into M-later** (§7.3), not silently dropped:

- **Scratch Laravel skeleton** — a `laravel` site type for `scratch_create_site`.
  Useful; needs the new provision shape. M-later.
- **`php_artisan` runner** (scratch-only, origin-gated) — the artisan analogue to
  `wp_run`, same tier reasoning. Useful; pairs with the skeleton. M-later.
- **Composer path-repo link** for an in-dev package — the Laravel equivalent of
  `scratch_add_package`. Useful; different mechanism (a path repository, where
  Composer itself symlinks — so S1's reasoning has to be re-run for it, not
  assumed). M-later.

**SHIPPED 3 Sep 2026 as parity P7** (`PLAN-mcp-parity.md` §5, D14): `site_artisan`
and `composer_link` on the USER's Laravel site under a `run` grant (#495, #496) — not
scratch-only, because scratch sites stayed WordPress (D13 deferred). S1 was re-run for
Composer and ruled the other way — a symlink, because the grant is the click.

**Tool count, recounted after S1/S2 (1 Aug 2026).** M1 shipped **3**
(`list_sites`, `site_status`, `tail_log`). M2a adds **5** (`scratch_create_site`,
`scratch_delete_site`, `scratch_add_package`, `scratch_sync_package`, `wp_run`).
M2b adds **3** (`set_php_version`, `mail_list`, `mail_get`). M3 adds **1**
(`db_query`). That is **12** through M3, plus `wp_login_url` if D2 lands — down
from the draft's 13-and-growing, because S2 dropped two and the derived
plugin/theme kind dropped two more. Fewer permanent promises for the same
scenario.

## 6. The developer-facing shape

### 6.0 The M2 guarantee — WRITTEN FIRST (1 Aug 2026), and it constrained the design

M1's honest-guarantee paragraph was written last and it was easy, because nothing
ran. M2's is hard, so it was drafted **before** any task started, deliberately, so
that a sentence we could not honestly write would expose a wrong scope while
changing it was still cheap. It did exactly that twice (the retreat in §4.4 that
became S1, and the "no tool starts a service" sentence §4.2 forbids). This is the
text; it lands in the Settings card above the M2 tools, near-verbatim:

> **What rexenv guarantees once an agent can create sites — and what it does not.**
>
> **An agent can SEE every site you have. It can only CHANGE the ones it made
> itself.** Those are two different sentences and collapsing them is the mistake
> this paragraph used to make: the read tools — list your sites, diagnose why one
> isn't serving, tail a WordPress debug log — work on ANY site, because a
> diagnostic that only saw the agent's own sites would answer no question worth
> asking. What that exposes is bounded rather than absent: only the site's own
> WordPress debug log, tail-only and capped, with rexenv-issued tokens, cookie
> headers and the paths rexenv knows stripped out — and it is raw log content, so
> whatever your code logged is in it.
>
> Everything that CHANGES anything is confined to sites rexenv created for the
> agent. (Creating one starts rexenv's database engine, exactly as creating a site
> in the app does — a user-level start, never a privileged one. And using a
> scratch site keeps it alive: an agent that looks at one pushes its expiry out,
> so the disposable sites that disappear are the ones nobody is using.) That is a
> recorded fact (`origin='agent'`, written at creation), never inferred from a
> name or a path — a site you named `foo.scratch.rex` yourself is yours, and the
> reaper will not touch it. **No tool can change or delete a site you made, and no
> tool can make a system change at all**: the resolver, the CA, the edge, tunnels
> and settings are absent from the tool surface entirely, and the ownership
> refusal lives in core, so the CLI and the UI enforce the same one. Scratch sites
> are capped, they expire, and they are deleted **by their record** — never by
> matching a name or a path. Anything you deliberately change yourself becomes
> yours permanently: rename it, move it, switch its PHP or web server, toggle
> Xdebug, edit its environment, or press Keep, and it stops being disposable.
> (Sharing a scratch site publicly does *not* adopt it — the reaper skips a shared
> site and tells you, rather than quietly claiming what you only meant to show
> someone.) Nothing an agent can call ever asks macOS for an administrator
> password.
>
> That is a fence around **which sites the tools name**. It is not a sandbox, and
> the difference is the thing to understand before you turn this on: installing
> and activating a plugin in a scratch site runs that plugin's PHP **as you**,
> with your files and your permissions. Code running as you can reach rexenv's own
> CLI socket and do things no MCP tool offers — start the stack (which does prompt
> you), start a public tunnel. rexenv does not contain that and will not claim to.
> What it does is refuse to make it easy or silent: those actions are not tools,
> every call is recorded in the activity feed, and the endpoint is off until you
> turn it on.
>
> **A plugin or theme you add to a scratch site is copied, not linked.** rexenv
> never joins the two, so nothing the SITE does — a plugin update, an uninstall, a
> file write by the code under test — can reach your checkout. The flip side is
> that the scratch site runs your code **as of the last sync**, not what is in
> your editor right now; sync again after you change it. What this does not mean
> is that your checkout is out of reach: an agent running `wp eval` is running PHP
> as you, and code running as you can write anywhere you can. The copy removes the
> accident, not the capability.

**RE-READ AGAINST SHIPPED M2a (3 Aug 2026) — three sentences had drifted, and
all three drifted the same way.** The paragraph was drafted before scratch sites,
`wp_run` or the reaper existed, which was the point; what it was NOT is
self-maintaining. Re-read line by line against the shipped tool list, three
claims were false, and every one is the **§3.1(c) shape** — a sentence true of
the narrow thing it was describing, positioned where the wider falsehood is the
available reading:

1. *"Your sites, their files, their databases … are not reachable from any
   tool."* **False.** Written from M2's vantage, where "reach" meant *mutate* —
   but M1's `list_sites`/`site_status`/`tail_log` reach every site the user has,
   and `tail_log` reads their site's own debug log. The available reading was
   "an agent sees only what it creates," which was never true of any shipped
   version. Split into two sentences: an agent SEES everything, CHANGES only its
   own, with the log surface's real bounds stated rather than implied away.
2. *"rename it, move it, **share it**, or press Keep."* **False since #214**,
   which deliberately removed sharing from the promotion rule ("sharing a scratch
   site is sharing a scratch site, not claiming it") — and the list had grown in
   the other direction: the six commands that actually promote are the PHP,
   web-server and Xdebug switches, the docroot move, the env edit and the domain
   change. Corrected, with the share ruling stated so the omission doesn't read
   as an oversight.
3. *"Nothing the agent runs can write back to your checkout."* **False since
   D1.** True of the CLONE — no link, so nothing the *site* does reaches the
   source — and false as a sentence about the agent, because `wp eval` is
   arbitrary PHP running as the user and can write anywhere the user can. #216's
   ledger row was already careful ("the guarantee is the direction rather than
   the mechanism"); the guarantee paragraph was not.

**What did NOT drift, and why that is the useful part:** none of this reached a
user. §6.0 says the paragraph lands in the card "near-verbatim" — it did not.
Task 8b rewrote it for the card (#211) *because the copy guard fired*, and the
rewritten copy is honest on all three points: it says an agent "can look at your
sites — their status and their logs", scopes the refusal to "cannot change or
delete", and makes no claim about the checkout at all. **The guarded text stayed
true; the unguarded draft rotted.** That is the argument for the must-say list
gaining the read-across claim (done in the same commit) rather than trusting a
re-read to happen again.

**The brittle sentence, flagged where it lands.** "Nothing an agent can call ever
asks macOS for an administrator password" is the strongest claim in the paragraph
and the one most likely to rot: it rests entirely on the never-prompt provision
flag holding for the whole operation (§4.2). Its ledger row pins the test that
would catch it going false — end-to-end with the resolver absent, not an entry
precheck.

- **Settings → "AI agents (MCP)" card, default OFF** — a new API surface into a
  tool that can read real databases and run scratch code should be opt-in, not
  ambient. Enabling binds the socket; disabling unlinks it and drops sessions.
  Mail is a second sub-toggle, also off (§3.5).
  - **The residual (§3.1) reads AT THE MOMENT OF ENABLING, not in a disclosure**
    — D7's binding condition, and worded so it is true in M1 (read-only, actually
    contained) AND still true when M2's executing tiers land, so a user who
    enabled it in M1 never needs a rewrite they won't see. It sits above the
    toggle, not behind an expander: *"Before you turn this on: this lets an AI
    agent connect to rexenv and use the tools you've enabled. Today those are
    read-only — it can look at your sites, their status and logs, but not change
    or run anything. As rexenv adds more capable tools, an agent will be able to
    create disposable sites and run code in them, and code in those sites has the
    same power over rexenv and this machine as code you run yourself. Turn this
    off when you're not using it."* The "as rexenv adds…" clause is the hinge: it
    is honest in M1 and stays honest at M2 without an edit.
- **Copy-paste connect:** the card shows `claude mcp add rexenv -- rex mcp` (this project) AND `claude mcp add --scope user rexenv -- rex mcp` (every project — Claude Code's default is per-project, which is the surprise this pair removes), plus
  the Cursor/VS Code JSON stanza, wired to the existing CLI-install card for the
  "not on PATH" case. Zero new install steps (§2.2).
- **Connected now — and "connected" vs "connected and working" are distinct.**
  The card lists live sessions (client name/version, since, last activity), but
  the status line must not stay green on the handshake alone: `initialize`
  succeeding then every call failing is the same connected-but-broken split
  rexenv cleans up elsewhere (a share showing Live while 530-ing; a service
  "running" on a bare port-listen). So the line reads from RECENT CALL OUTCOMES,
  not just the socket: **On — waiting** (no session) → **Connected** (a session,
  no calls yet) → **Working** (recent calls succeeding) → **Connected, but the
  last N calls errored** (handshake up, calls failing — named, not green). The
  activity feed's outcomes feed this line so header and feed can never disagree.
- **Activity feed — every agent action visible, none silent.** Append-only
  `agent_actions` (ts, client, tool, target site, argument summary — including
  full SQL text for `db_query` — outcome ok/error/denied). On the card (live
  feed) and per-site in SiteDetail's timeline for scratch sites. Audit trail
  *and* trust-building UI: the user watches the agent work. RO-grant first-use
  per session is a feed event (§3.6).
- Scratch sites carry client badge + TTL in the Sites list (§4.1); consent
  dialogs (§3.4) are the only interruptive surface.
- MCP-started provision/install jobs appear in the existing job registries like
  UI-started ones; the feed row is what marks them agent-initiated (no
  `initiator` field threaded through every job type in v1).

## 7. Costs, risks, staging

### 7.1 What this promises

- **A public API.** Tool names + schemas become compatibility surface the day
  someone scripts against them. Smallest v1; names chosen once; schema versioning
  rides `serverInfo.version`.
- **Dependencies**: **no MCP SDK** — the server is hand-rolled on `serde_json`
  (already a dep) to keep MSRV at 1.77.2 and the dependency closure tight (§2.1).
  The one new dependency the plan still adds is a **native MySQL driver** for the
  agent read path in M3 (§3.6, a reasoned bundled-client departure), pinned like
  everything else. The tradeoff of hand-rolling — owning protocol compatibility —
  is recorded in §2.1 with its revisit triggers.
- **A standing review burden**: every new tool is a security decision; the ledger
  grows rows (§8) that must stay green.
- Windows/Linux: socket path + shim go through `Paths`; Windows = named pipe
  behind the same seam, Phase 4.

### 7.2 Risks

| Risk | Mitigation |
|---|---|
| Prompt-injected agent calls destructive tools | Destructive = scratch-only (origin-gated in core); real-site mutation = T1 or absent; tunnels/system absent from the tool surface |
| Tool-output secret leak | `Agent*` view types + serialized-**output** planted-fixture sweep (§3.5); log token-scrub; mail behind opt-in *(D16: the inbox is a Read; the scrub gained the reset-key rule and error replies are scrubbed too, #499/#501)* |
| Auth-prompt fatigue via a tool | No registered tool reaches `run_privileged` — tool-layer test-pinned (§3.3). **NB (§3.1): scratch code can still reach root prompts via the CLI socket — a human still approves each; D7 tracks fully closing it** |
| `db_query` escapes SELECT via `system`/`tee` or grant wildcards | Native driver (no client-side commands), no `FILE`, `local_infile` off, escaped grant object — L1 adversarial test (§3.6) |
| `scratch_add_package` reads arbitrary files | Blast-radius validation + plugin/theme-header requirement (§4.4) — unchanged by S1: the clone changed the WRITE direction, not the read direction |
| An agent's `wp plugin update` destroys the user's checkout | **The clone (S1):** there is no path from the scratch copy back to the source, so no command — vetted or raw — can write to it. Replaces a refusal a raw runner could walk around (§4.4) |
| Real-site credential harvest via mail/logs | Mail opt-in + the rexenv-created scratch tag, fail-closed (§3.5); log token/cookie scrub *(D16: no mail opt-in — the whole inbox is a Read, said in the enable-moment paragraph; reset keys, login tokens and cookie headers scrubbed, encoded raw bodies omitted)* |
| Blanket/standing DB grant | Scoped + expiring + client-change re-consent + feed-surfaced (§3.6) *(D16: accepted as the dial's Read, by the owner's ruling — still SELECT-only on one database, every query in the feed)* |
| User renames or shares a scratch site and loses it | User mutation implies Keep; sharing promotes to `origin='user'` (§4.3) |
| A reap fails and the site is silently stuck expired | Fails safe (site intact), retried at most once per launch, recorded `actor='rexenv'`, surfaced with Retry/Delete (§4.3) |
| The user edits the plugin and the scratch site doesn't see it | The S1 texture cost, answered in the product not the doc: the sync verb's description states the rhythm, and the scratch card shows source + last-synced (§4.4) |
| Agent floods sites/disk | Cap 5 + TTL + reaper; provision already one-per-domain serialized |
| Reaper deletes the wrong thing | Deletes by recorded `origin` + `docroot_managed` only; share-guard skip-and-surface; lib-tested (§8) |
| Shell-less client gains user exec via scratch | **Stated, not hidden (§3.1):** running the user's plugin is user-level power; the master toggle is off by default and the reckoning is documented. D7 tracks the scratch-pool hardening that would narrow it |
| MCP spec churn (2026-07-28 RC) | Pin stable 2.2.x; stdio+tools is the stable core every client speaks; migrate deliberately (D5) |

### 7.3 Stages — each ships something real

**M1 — plumbing + read-only diagnosis. SHIPPED 30 Jul 2026** (#198–#203). Socket
+ `rex mcp` shim + registry + Settings card + activity feed + the
serialized-output secret sweep + **three** tools: `list_sites`, `site_status`
(stack-state verdicts, deliberately never requesting the site), `tail_log`
(scrubbed). **Mail did NOT ship in M1** — this line said it would; it is M2b,
which is where the global-inbox tell (§3.5) is built. *Ships: "ask your agent why
the site 502s."* Plumbing, audit
surface, and secrets discipline land here, small — and this is the surface that
is **genuinely contained** (§3.1c: no code execution). **M1 depends on zero
scratch machinery** — no `origin` column, no reaper, no DB principals; it ships
and stands on its own as the contained read-only surface even if M2 slips
indefinitely. That independence is deliberate: the piece with real containment is
also the piece that can ship first and alone.

**M2 — scratch sites. SPLIT (S3, 1 Aug 2026) into M2a and M2b**, because even
after S2 the single milestone carried ~10 tools plus a migration, a reaper and a
UI section. M2a is the scenario this plan is named for; M2b is two surfaces on
machinery that already exists and can wait for evidence anyone wants them.

**M2a — the scratch-site scenario.** v27 (`origin`, `agent_client`,
`expires_at`) + v28 (`agent_actions.actor`) + the `ScratchSite` witness and the
core origin gate (§2.6) + cap/TTL/reaper/reap-failure/share-promotion + Sites-UI
group + Keep + user-mutation-implies-Keep + the M2 tool module with the sweep
walking BOTH registries + tools: `scratch_create_site` (blueprint param),
`scratch_delete_site`, `scratch_add_package`, `scratch_sync_package`, `wp_run`
(scratch, D1 = yes). *Ships: the headline scenario end to end.*

**M2b — the two that can wait.** `set_php_version` (scratch — the compatibility
matrix) and `mail_list`/`mail_get` behind the opt-in sub-toggle with the
scratch-tag mu-plugin and the fail-closed filter (§3.5). *Ships: the compat
matrix and the mail-testing loop.*

**M3 — database access.** Native-driver query path + agent principals (escaped
grants, passwordless loopback, never `client_base_args`), `agent_db_grants`
(scoped + expiring), `db_query` (scratch rw; real SELECT behind the first scoped
T1 consent — the consent dialog lands here in its minimal one-shape form).
*Ships: the co-headline.*

**M-later — Laravel headline + real-site mutation.** The Laravel scratch skeleton
+ `php_artisan` runner + Composer path-repo link (§5.1); and, if real demand
shows, general T1 real-site vetted-WP ops + PHP switch + DB export (the old "M4").
Everything before is complete without these.

**Minimum useful version = M1 + M2a.** M1 alone is a diagnostics toy; M2a makes
it the feature the plan is named for. M2b and M3 complete what was asked. Laravel
parity is the first thing after.

Per-stage build order follows the house pattern (conventional commits, one task
per commit, `verify.sh` green, ledger rows + TODO ticks in the same commit as
their invariants; live-check example `mcp_socket_check` mirroring
`cli_socket_check` in M1, joined by `mcp_scratch_check` in M2a).

**M2a task order** (each one commit, `verify.sh` green; ledger numbering
continues at #204):

1. This reconcile (no new rows; #197's re-anchor noted).
2. **v27** — `origin`/`agent_client`/`expires_at`, store helpers, upgrade-path
   test from a v26 db (existing rows read `'user'`), recorded-not-inferred tests
   in the `docroot_managed` shape.
3. **v28** — `agent_actions.actor`, `feed::record_system`, card styling; amends
   #202.
4. **Feed: result-derived target** — `PendingLog.target_site` fillable from a
   tool's result, so a create records the site it made (§2.6).
5. **TTL touch in the session layer**, beside `log_action`; the M1 read-only
   guard must still pass untouched (§4.3).
6. **Core origin gate + `ScratchSite` witness** — a planted call passing a user
   site must fail to COMPILE (§2.6).
7. **M2 module + capability** — `mcp_server/scratch/`, registry union in
   dispatch, sweep walks BOTH registries, guard extended to assert the two
   registries are disjoint (§2.6).
8. **`scratch_create_site`** — forced domain, origin at insert, never-prompt flag
   tested END TO END at the provision path, cap, honest stack-stopped result,
   plus a test pinning the path never reaches `run_privileged` (§4.2).
9. **`scratch_delete_site` + reaper** — delete by record, once-per-launch retry,
   reap failures as `actor='rexenv'` feed rows, rowless-share backstop (§4.3).
10. **Keep + user-mutation promotion** — rename/move/env/**share** (§4.3).
11. **`scratch_add_package` / `scratch_sync_package`** — clone, blast-radius
    validation, header requirement, recorded source path (§4.4).
12. **`wp_run`** — scratch-only, origin-gated in core via the witness,
    feed-logged (D1).
13. **Sites UI** — Agent-scratch group, client badge, TTL, last-synced, Keep,
    expired-but-shared warning, reap rows; WebKit harness scenarios.
14. **L1 `mcp_scratch_check`** (fixture-owned per `examples/common/mod.rs`,
    sandbox tier) + the SMOKE-TEST M2 gate — including at least one step only a
    PACKAGED run can prove (the M1 enable-crash lesson).

### 7.4 Explicitly out of scope

Tunnel exposure as a tool (D6 is a future reopening, not a v1 option);
PostgreSQL/Redis query surfaces; MCP resources/prompts/elicitation; progress
streaming; headless/app-not-running operation; Windows/Linux; blueprint
authoring; multi-user/remote access; an `initiator` field threaded through
existing job types; a native mechanism to contain scratch code below user level
(D7 — the reckoning, not a v1 deliverable).

## 8. Verification + new claim-ledger rows

New invariants land with ledger rows in the same commit, house rule. The core set
(Tier 1 unless noted). **Each row states what it certifies AND what it does not**,
so a tool-surface test can never be read as a containment guarantee (§3.1):

| Claim | Proof shape |
|---|---|
| No registered MCP tool *maps to* a command that reaches one of the 7 `run_privileged` sites — **certifies the paved road (§3.1a), NOT that executed scratch code can't reach the CLI socket** | L0: registry walk vs the enumerated privileged command-layer entries |
| Tunnel/system/settings/file-write commands are absent from the registry | L0: closed-set assertion |
| MCP socket is 0600, never TCP | L0 bind test (mirror of #55) |
| No serialized tool **output** carries a credential, the CA key path, the admin-socket path, or a `?rexenv_login=`/reset token | L0: planted-fixture sweep over every tool's real output (§3.5) |
| Agent DB principals are never root/reserved, never `'%'`, and the grant object names one database | L0: extend dbmirror grant-shape tests. **The escaping half (`grant_db_object`) already landed as ledger #196, ahead of this feature** |
| **🚫 The MCP server is NOT a sandbox** (D7): once a client has scratch code execution it has user-level power over rexenv and the machine, reachable to the CLI socket; the tiers give a paved road + no silent amplifier, never containment | 🚫 inherently unprovable — a *containment* claim here would be FALSE, not merely unproven. Lands anchored to the MCP toggle/registry code with M2; forward-recorded now in CLAIM-LEDGER so nothing downstream describes MCP as sandboxed |
| The agent query path uses the native driver and never calls `client_base_args`; a real-site RO principal cannot write, `system`, or `INTO OUTFILE` | L0 (no-`client_base_args` assertion) + L1 live (attempt write/`system`/outfile, expect failure) |
| Reaper deletes only `origin='agent'` ∧ expired ∧ `docroot_managed=1`; a failed reap leaves the site intact, retries at most once per launch, and is RECORDED (`actor='rexenv'`), never silent | L0 |
| `origin` recorded at create; no IPC sets it; agent→user only via Keep or user-mutation (rename/move/env/**share**) | L0 (the `docroot_managed` test pattern) |
| `scratch_add_package` refuses blast-radius paths and header-less sources, and CLONES — the source is never written, so no later tool needs to remember to refuse (S1) | L0 + L1 (attempt `~/.ssh`, expect refusal; write inside the scratch copy, assert the source is byte-identical) |
| **Nothing an agent can call reaches a privileged prompt** — the never-prompt provision flag holds for the WHOLE operation, not as an entry precheck (§4.2). *The guarantee's most brittle sentence; this row is what catches it going false* | L0 end-to-end at the provision path with the resolver ABSENT: the operation fails with the setup message and `configure_resolver` is never reached |
| The M2 registry is disjoint from M1's, and the secret-leak sweep walks BOTH — a second registry may not silently narrow "every registered tool's output is swept" (§2.6) | L0 (disjointness + sweep-plan union) + `mcp_secret_sweep` live |
| A scratch mutator cannot be applied to a user site — the `ScratchSite` witness has ONE constructor, which reads `origin` from the row; the gate is in core, so CLI and UI inherit it | Structural (compile) + L0 constructor-scope test |
| The feed's actor is TYPED: a rexenv-initiated row (reap) can never read as an agent action, and an agent-initiated one can never claim to be rexenv | L0 (round-trip + the card's rendering split) |
| T1 handlers unreachable without a `ConsentGranted` witness; RO grant is scoped + expiring | Structural (compile) + L0 constructor-scope test |
| Scratch create refuses (never prompts) when the backbone resolver is missing, via the provision-path never-prompt flag | L0 at the provision path, not the tool layer |

L1: `mcp_socket_check` — real socket, real initialize, tool list, one T0 call,
the secrets sweep over live output, the RO-principal write/`system` refusal
(fixture-owned per `examples/common/mod.rs`).

**Real-client verification (condition of the hand-roll, §2.1).** Because the
risk of owning the protocol is a framing/schema detail our own encoder is happy
with but a real client rejects, T1's verification is two-layered: (a) the
`mcp_socket_check` example speaks a **spec-literal** handshake (bytes written to
match the MCP spec's own examples, not round-tripped through our types) and
asserts a valid `initialize` result + empty `tools/list`; (b) a **manual
real-client check** in the release manual list — `claude mcp add rexenv -- rex
mcp`, then confirm the server shows connected and `tools/list` returns cleanly in
Claude Code (the #1 target). (a) catches our own mistakes; only (b) proves the
client we actually care about — the L1-layer principle of testing against the
real thing.

## 9. Decisions — D1/D3/D4/D6/D7 SETTLED 29 Jul 2026; D2/D5 open

1. **D1 — raw `wp_run` on scratch sites. SETTLED: ship, scratch-only.** Plugin
   *activation* already grants user-level exec (§3.1), so `wp_run` does **not**
   widen the containment story — it is a usefulness call, and a large one ("run
   whatever check" is the scenario's verb; a vetted list can't name a plugin's
   own commands). Scratch-only, origin-gated in core, feed-logged; the real-site
   raw-wp refusal stays permanent. **Consequence found 1 Aug 2026 and worth
   keeping attached to this decision:** shipping a raw runner is what made the
   symlinked dev-plugin's write-back guard unenforceable, and therefore what
   forced S1 (clone, not symlink — §4.4). A raw-runner decision does not stay
   local to its own tool; it deletes every guarantee elsewhere that depended on
   knowing which commands run.
2. **D2 — `wp_login_url` scratch-only: include? SETTLED 5 Sep 2026 by the owner, WIDER
   than asked: a login link EVERYWHERE — real sites through `wp_user` → `login_url`
   (parity P3, `manage`), scratch sites through `scratch_login_url` (ledger #515) — always
   rexenv's magic token, never a password set or reset, usable from a browser or
   headlessly (cookie jar + redirect). The "never real sites" line in §3.5 and §5 is
   therefore superseded by the parity dial (D15). Original text:** Recommend include (cheap,
   human-in-the-loop useful); drop without argument if it reads as surface for
   surface's sake.
3. **D3 — real-site mutation (T1). SETTLED: left unpromised.** The plan is
   T0-scratch + RO-real-DB; real-site mutation is M-later and only if demand
   shows. The consent machinery still lands in M3 (the DB grant), so it is an
   extension not a rework.
4. **D4 — mail. SETTLED: opt-in sub-toggle, default off. REVERSED by D16, 4 Sep 2026**
   (`PLAN-mcp-parity.md` §7): the sub-toggle is gone — the stamp rides the endpoint and the
   whole inbox is a Read at the Agent access dial's floor, with the reset-key scrub §3.5
   promised finally built; the fail-closed scratch filter (#225/#227) is unchanged. The
   original ruling, for the record: The global inbox
   carrying reset links (§3.5) makes ambient T0 mail a credential-harvest pivot;
   the sub-toggle keeps the genuinely-useful WP *and* Laravel mail-testing loop
   available without making it ambient. **Amended 1 Aug 2026:** the scoping was
   specced as "scratch-addressed", which the inbox cannot support — no recipient
   field carries the site (§3.5). The tell is one rexenv creates (a scratch-only
   mu-plugin forcing `From`), the filter is fail-closed, and that reasoning goes
   in the toggle's copy, not only in this doc. Ships in **M2b**, not M1 as §7.3
   originally said.
5. **D5 — MCP SDK. SETTLED 29 Jul 2026: no SDK — hand-rolled minimal server**
   (was "pin rmcp 2.2.x"). Reversed on the discovery that every `rmcp` is edition
   2024 / rustc ≥ 1.85, which would force the repo's `rust-version = 1.77.2` floor
   up as a side effect of an SDK pick. Reasons + what we own + revisit triggers in
   §2.1. `rmcp` is the documented re-entry path if the stable core changes or v2
   needs the evolving features.
6. **D6 — tunnels. SETTLED: never a tool.** Confirmed as the standing ruling;
   webhook testing (§3.7) is recorded as the only future reopening argument (real
   demand + T1 + scratch-only + auto-stop).
7. **D7 — the scratch-execution → CLI-socket residual (§3.1, §3.7). SETTLED:
   (a).** Accept it as a documented user-level-power fact and ship, master toggle
   off by default, **with two binding conditions**: (1) the toggle's own copy
   states the residual where the enabling user reads it (§6), and (2) it is
   recorded in the claim ledger as an inherently-unprovable 🚫 with its scope (§8),
   so nothing downstream describes MCP as sandboxed. **(b) and (c) rejected on
   cost:** (c) gating the CLI socket's dangerous commands breaks `rex` for the
   user — a real, shipped feature paying for a theoretical boundary against a
   same-uid peer; (b) `disable_functions`-hardening scratch pools degrades test
   fidelity — a scratch site that can't run what the user's plugin runs isn't a
   test of that plugin. (b) is recorded as **future hardening** for deployments
   that must contain a shell-less client, not a v1 deliverable.
