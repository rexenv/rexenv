# MCP server — let AI agents drive rexenv (scratch sites, DB access, tiered capability)

**Status: SCOPE RULED 29 Jul 2026 — build M1 → M2 → M3, one task at a time.**
Planned 28 Jul 2026 against `5dbaa8d`, from a full-codebase research pass + an
adversarial review that rewrote the security model (three blockers, §3.1
reckoning); scope ruled the next day. D1/D3/D4/D6/D7 settled (§9); D2/D5 open but
non-blocking; **the M3 database surface is ruled at M3, not now.** The one code
change already landed ahead of the feature is the mirrored-user GRANT
wildcard-escaping fix (its own commit, ledger #196 — §3.6). Line numbers are
anchors taken at `5dbaa8d` — trust the file, verify the line.

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

SDK: **`rmcp`**, the official Rust MCP SDK — tokio-based, and its `IntoTransport`
accepts any AsyncRead+AsyncWrite pair, so a `tokio::net::UnixStream` is a
transport out of the box. Pin the stable line (2.2.x, spec 2025-11-25); the 3.0
betas track a spec RC finalized 28 Jul 2026 — migrate deliberately later (§9 D5).

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
  binary, which stays a dumb bidirectional byte pipe (stdin→socket,
  socket→stdout), no MCP parsing, keeping the `cli/` crate's structural
  never-links-the-app-lib guarantee (`cli/Cargo.toml:9`). Zero-install: `rex` is
  already bundled and symlinked on PATH (`core/cli.rs:64-113`, Homebrew cask).
- Multiple clients = multiple concurrent connections, one MCP session each; the
  app records each session's `clientInfo` (name/version) from MCP `initialize`
  — **display and audit only, never policy** (a client-asserted string).

### 2.3 App not running: honest failure, no autostart

Same ruling as the CLI (`cli/src/main.rs:146-153`): the shim fails to connect,
prints `rexenv isn't running — open the app first` on stderr, exits non-zero.
Deliberately no autostart / no headless mode (§2.1). Version skew: the tool
registry lives app-side, so the CLI's stale-binary unknown-command class
(`cli_server.rs:1320-1322`) can't happen for tools — a connected client always
sees the running app's tools. `serverInfo.version` = app version.

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
  genuinely contained — read-only, secret-scrubbed, no writes.

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
| **T1 — consent in the rexenv UI** | Native dialog in the app, per operation; deny on timeout, deny when no window | First read-only DB grant on a **real** site (§3.6); real-site mail/log exposure (§3.5); mutating a **real** site — M4, may ship never (§9 D3) |
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
| Arbitrary file read/write tools | The generic leak/write primitive. `scratch_link_plugin` is NOT an exception to this — it is constrained to be a plugin/theme link, not a file-read primitive (§4.4); the first draft's version WAS such a primitive and is fixed |
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
| Mail | The Mailpit inbox is **global, no per-site tagging** (`core/mail.rs:65-75`) — it holds real sites' password-reset links and outbound mail | Mail is behind its **own opt-in sub-toggle, default off** (D4 flips): a global inbox carrying reset links is a credential-harvest pivot, not a safe T0 default. When on, scratch-addressed messages are T0; the toggle's copy names the real-site exposure. Every read is feed-logged |
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
- **Real-site `db_query` requires ONE scoped, expiring T1 grant.** The dialog is
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
  (`'user' | 'agent'`), migration **v26** (latest is v25 `mu_dir_created`,
  `state/db.rs:307`), plus `agent_client TEXT` (the clientInfo string,
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
  errors naming the count + reclaim options. That is the answer to "an agent
  creates twenty" — it can't.
- Name is a single label (no dots), so an agent can't nest namespaces or squat
  `scratch.rex`. `unique_db_name` already disambiguates colliding slugs
  (`core/sites.rs:320`).

### 4.3 Lifecycle: TTL + cap + reaper + explicit teardown + Keep

"Disposable" = all of these, layered:

| Mechanism | Behavior |
|---|---|
| Explicit teardown | `scratch_delete_site(id)` — T0, refused in core unless `origin='agent'` (origin gate in core, not the tool layer — M7 style) |
| TTL | `expires_at` = create + 24 h (setting), refreshed by any agent op targeting the site. Idle scratch dies; active scratch lives |
| Reaper | Launch + hourly in-app sweep: full `delete_site` path (DB drop honors provenance, teardown honors `docroot_managed`) for rows where `origin='agent'` ∧ expired ∧ `docroot_managed=Some(true)`. Deletes **by the record, never by name or path** (the 24 Jul example-cleanup lesson; ledger §8) |
| Keep, and **user-mutation implies Keep** | The scratch card's Keep action sets `origin='user'` + clears `expires_at`. **And any user-initiated mutation of a scratch row — rename, move, env edit — auto-promotes it to `origin='user'` first**, closing the first draft's hole where a user renames a scratch site, the reaper still sees `origin='agent'` + `expires_at`, and deletes the site the user just adopted. Agents cannot call Keep or promote |
| Shared-expired = skip **and surface** | A scratch site the user manually shared (live tunnel) is skipped by the reaper (`refuse_if_shared` guards deletion) — but a silent skip is the fossil-tunnel mode reborn. So it becomes a **persistent UI warning + notification** ("an expired agent site is still shared publicly"), re-checked when the tunnel stops |

App closed at expiry → the launch sweep catches up (the tunnel `sweep_startup`
shape). Services keep serving expired scratch sites until then — harmless
loopback sites.

### 4.4 The dev-plugin loop — a *constrained* link, not a file primitive

The scenario's key step — *install the in-development plugin into the scratch
site* — is `scratch_link_plugin(site_id, source_path)` (and `_theme`): a
**symlink** into `wp-content/plugins/`, the Valet-style edit-in-repo loop. The
first draft made this an arbitrary-file-read primitive; the fix has three parts:

- **Blast-radius validation.** The target passes `validate_linked_docroot`-grade
  refusals (`core/sites.rs:810-872`: `/`, `$HOME`, Desktop/Documents/Downloads,
  volume roots, app-data, overlap) — `repo::validate_link_target`
  (`core/repo.rs:1622-1654`) checks only self-nesting/cycles and is NOT enough.
  Without this, `scratch_link_plugin(id, "/Users/me/.ssh")` would symlink it into
  the docroot and nginx (which follows symlinks) would serve `id_rsa` — not a
  dotfile, so the `/.`-segment dotfile guard (`frankenphp.rs:66`, `apache.rs:336`)
  does not cover it.
- **Must be a plugin/theme.** The target must contain a plugin header (or
  `style.css` theme header) before linking — a source tree with no plugin header
  is refused. Turns "link any directory" into "link a plugin," which is the
  actual feature.
- **No write-back over a link.** `wp_plugin_delete`/`_theme_delete` already
  partition on symlink truth and only unlink (`commands/wordpress.rs:148-178`),
  but `wp_plugin_update` and install-from-wp.org do **not** — `wp plugin update
  <linked-slug>` unpacks over the link and destroys the user's real checkout,
  unattended. So the linked-slug refusal is extended to **update and install**
  (refuse a slug whose path is a symlink) before those tools are exposed. Ledger
  row in §8.

Read-and-serve of the (validated, header-bearing) plugin is then T0: it is the
user's own machine, loopback, no writes to the source. `wp_run` (D1) or vetted
`wp_plugin activate` activates it — and per §3.1, activation runs the plugin's
code as the user, which is the feature, not a leak.

### 4.5 Interaction with existing flags — nothing new to invent

| Flag | Scratch value | Consequence |
|---|---|---|
| `docroot_managed` | `1` | Teardown removes the docroot — full disposal, by the recorded bit |
| `db_created` | `NULL` (our-provisioning semantics) | `may_drop_database` → dropped at delete (test `may_drop_database(site(None,None))==true`, `commands/sites.rs:978`) |
| `provisioned` | Normal job semantics | Failed scratch shows "setup incomplete" + Retry; the reaper also collects failed expired scratch |
| `content_dir`, `mu_dir_created`, `override_port` | Defaults | Unchanged |
| Linked sites | **Impossible for agents** | `scratch_create_site` never takes a docroot path; linking a whole external docroot is the human flow (`scratch_link_plugin` links a plugin *into* a scratch docroot — different, §4.4) |

## 5. Tool surface — ranked honestly

Every tool is a compatibility promise; v1 ships the smallest set that makes the
two headline scenarios real.

| Tool | Tier | Verdict |
|---|---|---|
| `list_sites` (redacted views) | T0 | **Essential context** — everything references site ids |
| `site_status(id)` — doctor + serving chain | T0 | **Genuinely useful**: "why isn't it serving" is the #1 agent question. Wraps the doctor composite + one new per-site HTTP probe (edge → vhost → status code), a gap the UI would benefit from too |
| `tail_log(id, source, lines)` (+ wp-debug), **scrubbed** | T0 | **Genuinely useful**: 502s live in fpm/nginx/wp-debug tails. Existing IPC + guards; token/cookie scrub added (§3.5) |
| `scratch_create_site` / `scratch_delete_site` (list rides `list_sites`) | T0 | **The centre.** Blueprint param included — seeding is a create-time flag, already built |
| `scratch_link_plugin` / `scratch_link_theme` | T0 | **The centre's second half**, constrained per §4.4 |
| `wp_plugin` / `wp_theme` (list/install-from-wp.org/activate/deactivate/update/delete) | T0 scratch; real M4/T1 | **Useful** — vetted ops with core hygiene exist; update/install refuse linked slugs (§4.4) |
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
  `scratch_link_plugin`. Useful; different mechanism. M-later.

Roughly 13 tools in the full WordPress build-out; M1+M2 ship 9 (§7.3).

## 6. The developer-facing shape

- **Settings → "AI agents (MCP)" card, default OFF** — a new API surface into a
  tool that can read real databases and run scratch code should be opt-in, not
  ambient. Enabling binds the socket; disabling unlinks it and drops sessions.
  Mail is a second sub-toggle, also off (§3.5).
  - **The card's own copy states the residual (§3.1), verbatim** — this is D7's
    binding condition, so the reckoning lives where the person enabling it will
    read it, not only in this plan: *"While this is on, an AI agent can create
    disposable sites and run their code. Code running in those sites has the same
    power over rexenv and this machine that your own code does — including
    reaching rexenv's command socket. Turn this off when you're not using it."*
- **Copy-paste connect:** the card shows `claude mcp add rexenv -- rex mcp` plus
  the Cursor/VS Code JSON stanza, wired to the existing CLI-install card for the
  "not on PATH" case. Zero new install steps (§2.2).
- **Connected now:** the card lists live sessions — client name/version, since,
  last activity (socket connect + MCP initialize = "connected").
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
- **Dependencies**: `rmcp` (official, mature, Apache-2.0) + a **native MySQL
  driver** for the agent read path (§3.6, a reasoned bundled-client departure) —
  both pinned; the rmcp 2.x→3.x spec transition forces one deliberate migration
  later (D5).
- **A standing review burden**: every new tool is a security decision; the ledger
  grows rows (§8) that must stay green.
- Windows/Linux: socket path + shim go through `Paths`; Windows = named pipe
  behind the same seam, Phase 4.

### 7.2 Risks

| Risk | Mitigation |
|---|---|
| Prompt-injected agent calls destructive tools | Destructive = scratch-only (origin-gated in core); real-site mutation = T1 or absent; tunnels/system absent from the tool surface |
| Tool-output secret leak | `Agent*` view types + serialized-**output** planted-fixture sweep (§3.5); log token-scrub; mail behind opt-in |
| Auth-prompt fatigue via a tool | No registered tool reaches `run_privileged` — tool-layer test-pinned (§3.3). **NB (§3.1): scratch code can still reach root prompts via the CLI socket — a human still approves each; D7 tracks fully closing it** |
| `db_query` escapes SELECT via `system`/`tee` or grant wildcards | Native driver (no client-side commands), no `FILE`, `local_infile` off, escaped grant object — L1 adversarial test (§3.6) |
| `scratch_link_plugin` reads arbitrary files | Blast-radius validation + plugin-header requirement (§4.4) |
| `wp plugin update` destroys a linked checkout | Linked-slug refusal extended to update/install (§4.4) |
| Real-site credential harvest via mail/logs | Mail opt-in + scratch-addressed; log token/cookie scrub (§3.5) |
| Blanket/standing DB grant | Scoped + expiring + client-change re-consent + feed-surfaced (§3.6) |
| User renames a scratch site and loses it | User mutation implies Keep (§4.3) |
| Agent floods sites/disk | Cap 5 + TTL + reaper; provision already one-per-domain serialized |
| Reaper deletes the wrong thing | Deletes by recorded `origin` + `docroot_managed` only; share-guard skip-and-surface; lib-tested (§8) |
| Shell-less client gains user exec via scratch | **Stated, not hidden (§3.1):** running the user's plugin is user-level power; the master toggle is off by default and the reckoning is documented. D7 tracks the scratch-pool hardening that would narrow it |
| MCP spec churn (2026-07-28 RC) | Pin stable 2.2.x; stdio+tools is the stable core every client speaks; migrate deliberately (D5) |

### 7.3 Stages — each ships something real

**M1 — plumbing + read-only diagnosis.** Socket + `rex mcp` shim + registry
(tiers + tool-layer tests day one) + Settings card + activity feed + the
serialized-output secret sweep + tools: `list_sites`, `site_status` (incl. the
new HTTP probe), `tail_log` (scrubbed), and `mail_list`/`mail_get` behind the
opt-in sub-toggle. *Ships: "ask your agent why the site 502s."* Plumbing, audit
surface, and secrets discipline land here, small — and this is the surface that
is **genuinely contained** (§3.1c: no code execution). **M1 depends on zero
scratch machinery** — no `origin` column, no reaper, no DB principals; it ships
and stands on its own as the contained read-only surface even if M2 slips
indefinitely. That independence is deliberate: the piece with real containment is
also the piece that can ship first and alone.

**M2 — scratch sites.** v26 migration (`origin`, `agent_client`, `expires_at`) +
cap/TTL/reaper/shared-skip-surface + Sites-UI group + Keep + user-mutation-implies-
Keep + tools: `scratch_create_site` (blueprint param), `scratch_delete_site`,
`scratch_link_plugin/_theme` (validated), `wp_plugin`/`wp_theme` (scratch,
linked-slug refusal on update/install), `set_php_version` (scratch), `wp_run`
(scratch, if D1 = yes). *Ships: the headline scenario end to end.*

**M3 — database access.** Native-driver query path + agent principals (escaped
grants, passwordless loopback, never `client_base_args`), `agent_db_grants`
(scoped + expiring), `db_query` (scratch rw; real SELECT behind the first scoped
T1 consent — the consent dialog lands here in its minimal one-shape form).
*Ships: the co-headline.*

**M-later — Laravel headline + real-site mutation.** The Laravel scratch skeleton
+ `php_artisan` runner + Composer path-repo link (§5.1); and, if real demand
shows, general T1 real-site vetted-WP ops + PHP switch + DB export (the old "M4").
Everything before is complete without these.

**Minimum useful version = M1 + M2.** M1 alone is a diagnostics toy; M2 makes it
the feature the plan is named for. M3 completes what was asked. Laravel parity is
the first thing after.

Per-stage build order follows the house pattern (conventional commits, one task
per commit, `verify.sh` green, ledger rows + TODO ticks in the same commit as
their invariants; live-check example `mcp_socket_check` mirroring
`cli_socket_check` in M1).

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
| Reaper deletes only `origin='agent'` ∧ expired ∧ `docroot_managed=1`, skips-and-surfaces shared | L0 |
| `origin` recorded at create; no IPC sets it; agent→user only via Keep or user-mutation | L0 (the `docroot_managed` test pattern) |
| `scratch_link_plugin` refuses blast-radius paths and non-plugin targets; update/install refuse linked slugs | L0 + L1 (attempt `~/.ssh`, expect refusal) |
| T1 handlers unreachable without a `ConsentGranted` witness; RO grant is scoped + expiring | Structural (compile) + L0 constructor-scope test |
| Scratch create refuses (never prompts) when the backbone resolver is missing, via the provision-path never-prompt flag | L0 at the provision path, not the tool layer |

L1: `mcp_socket_check` — real socket, real initialize, tool list, one T0 call,
the secrets sweep over live output, the RO-principal write/`system` refusal
(sandbox tier, fixture-owned per `examples/common/mod.rs`).

## 9. Decisions — D1/D3/D4/D6/D7 SETTLED 29 Jul 2026; D2/D5 open

1. **D1 — raw `wp_run` on scratch sites. SETTLED: ship, scratch-only.** Plugin
   *activation* already grants user-level exec (§3.1), so `wp_run` does **not**
   widen the containment story — it is a usefulness call, and a large one ("run
   whatever check" is the scenario's verb; a vetted list can't name a plugin's
   own commands). Scratch-only, origin-gated in core, feed-logged; the real-site
   raw-wp refusal stays permanent.
2. **D2 — `wp_login_url` scratch-only: include?** Recommend include (cheap,
   human-in-the-loop useful); drop without argument if it reads as surface for
   surface's sake.
3. **D3 — real-site mutation (T1). SETTLED: left unpromised.** The plan is
   T0-scratch + RO-real-DB; real-site mutation is M-later and only if demand
   shows. The consent machinery still lands in M3 (the DB grant), so it is an
   extension not a rework.
4. **D4 — mail. SETTLED: opt-in sub-toggle, default off.** The global inbox
   carrying reset links (§3.5) makes ambient T0 mail a credential-harvest pivot;
   the sub-toggle keeps the genuinely-useful WP *and* Laravel mail-testing loop
   available without making it ambient.
5. **D5 — rmcp pin: stable 2.2.x now (spec 2025-11-25), migrate to 3.x after the
   2026-07-28 spec finalizes.** OPEN (technical, not blocking) — recommend
   exactly that; revisit at M3.
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
