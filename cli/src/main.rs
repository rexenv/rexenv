//! `rex` — remote control for the RUNNING rexenv app.
//!
//! Design (see src-tauri/src/cli_server.rs for the server half): this binary
//! is a pure client. It never opens the app database and never touches a
//! process — every command is one JSON line over the app's private `0600`
//! unix socket, executed by the app itself through the same code path the UI
//! uses. If the app isn't running, `rex` says so and exits; it never starts a
//! second backend.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long `soft_request` waits for an app that has accepted the connection.
/// Best-effort by contract, so a wedged app costs a pause, never the command.
const SOFT_DEADLINE: Duration = Duration::from_secs(2);

/// How long `request` stays silent before telling the user it is still waiting.
const STALL_NOTICE_AFTER: Duration = Duration::from_secs(10);

/// **The CLI never starts the app.** It NAMES the command and stops there.
///
/// `rex` is a remote control for a RUNNING app, so auto-spawning would launch a
/// second process behind the user's back — one that adopts services, opens the
/// database as a second writer and takes over both sockets — as a side effect
/// of something as innocent as `rex status` inside a shell script. Naming the
/// command costs one paste and leaves the decision where it belongs.
///
/// macOS resolves `-a rexenv` through LaunchServices, so it finds the app
/// wherever it is installed — but only if it IS installed. A dev build run out
/// of `target/` is not registered, which is why the line says "the installed
/// app" rather than promising the command works everywhere.
#[cfg(target_os = "macos")]
const NOT_RUNNING: &str = "rexenv isn't running — open the app first (the CLI controls the \
running app).\n  Start it with:  open -a rexenv    (the installed app)";
#[cfg(not(target_os = "macos"))]
const NOT_RUNNING: &str =
    "rexenv isn't running — open the app first (the CLI controls the running app).";

// A SPECIFIC reason for `rex mcp`, not a generic transport error: an MCP client
// surfaces this on stderr when the bridge can't reach the app, so the agent
// learns WHY rather than guessing at an opaque failure.
#[cfg(target_os = "macos")]
const MCP_NOT_RUNNING: &str =
    "rexenv isn't running — open the rexenv app (open -a rexenv), then reconnect. No MCP server is available until rexenv is running.";
#[cfg(not(target_os = "macos"))]
const MCP_NOT_RUNNING: &str =
    "rexenv isn't running — open the rexenv app, then reconnect. No MCP server is available until rexenv is running.";

const USAGE: &str = "\
rex — control the running rexenv app

USAGE:
  rex [--json] <command>

COMMANDS:
  status        Services + DNS state (the app's ownership-and-liveness truth)
  open          Bring the rexenv window to the front (it has no dock icon)
  start         Start the shared stack (same as the app's Start all)
  stop          Stop the shared stack (same as Stop all)
  restart       stop, then start
  site list     All sites + whether each is actually serving
  site info <domain>    Full detail: config, serving state, cert, resources, WP
  site open <domain>    Open https://<domain> in the browser
  site login <domain>   Open a logged-in wp-admin (magic link; --print to not open)
  site create <domain> [--name N] [--type wordpress|php|laravel] [--php 8.3]
              [--server nginx|frankenphp|apache] [--db mysql|mariadb|postgres]
              [--blueprint <name>] [--multisite subdomain|subdirectory]
              [--path <folder>]
                Create a site (defaults mirror the app's New Site dialog;
                WordPress sites get the one-click install). --path serves an
                EXISTING folder in place: it is adopted as-is, never written
                into, and never deleted with the site
  site delete <domain> [--yes]
                Delete a site — drops its database and docroot (asks first)
  site logs <domain> [--source K] [--lines N] [--follow]
                Tail a site's log sources (no --source lists them)
  logs [key] [--lines N] [--follow]
                Tail any service log (no key lists all log files)
  doctor        Diagnose: DNS + resolver takeovers, edge wire identity, ports, CLI link
  db export <domain>
                Dump the site's database to ~/Downloads (prints the path)
  db import <domain> <file.sql> [--yes]
                Import a dump — OVERWRITES the site's tables (asks first)
  db reset <domain>       Drop + reinstall WordPress (type the domain to confirm)
  db versions [--set <engine> <version>]   Per-engine server versions
  db browse               Open Adminer in the browser
  php list      Pinned PHP versions: installed, default, pool port
  php default <minor>      Default version for new sites
  php install <minor> / php uninstall <minor>
  php settings <minor> [set K=V]     Whitelisted ini settings (set restarts the pool)
  site php <domain> <minor>          Switch a site's PHP version
  site xdebug <domain> on|off        Toggle the site's Xdebug debug pool
  site server <domain> nginx|frankenphp|apache   Switch the web server
  site restart <domain> [--pool]     Restart the site's own backend (--pool also bounces its shared PHP pool)
  site start <domain> | --all        Serve this site (or every site, --all) again
  site stop <domain> | --all         Stop serving THIS site, or every site — rexenv's services keep running
  site domains <domain> [--add N | --remove N]   Extra hostnames the site answers on
  service restart <nginx|edge|php-8.3>           Bounce one web-tier service on a fresh config
  site rename <domain> <name>        Display name only (domain unchanged)
  site domain <domain> <new-domain>  Change the domain (URL rewrite; asks first)
  site move <domain> <dest-parent>   Move the docroot under a new parent folder
  site relink <domain> <path>        Re-point a LINKED site at a folder you moved (records only)
  site retry <domain>                Finish a site whose setup stopped part-way
  site env <domain> [set K=V | unset K]          Per-site env vars
  site cert <domain> [--regenerate]  Certificate info / fresh leaf
  blueprints                         Saved blueprints (for site create --blueprint)
  wp <domain> plugin list|install|activate|deactivate|update|delete [slug…] [--activate]
  wp <domain> theme  list|install|activate|update|delete [slug…] [--activate]
  wp <domain> user   list|create|set-password|set-role|delete …
                WordPress manager (vetted WP-CLI ops; passwords are
                auto-generated and printed once — never passed on argv)
  wp <domain> search-replace <from> <to> [--dry-run] [--yes]
  wp <domain> cache-flush | cron run | maintenance [on|off] | core update
  wp <domain> core versions | core switch <version>
  repo <domain> list [--status]        Git-backed plugins/themes (--status adds live state)
  repo <domain> status <dir> [--theme] Branch, changes, ahead/behind, remote, link target
  repo <domain> branches <dir> [--theme]   Local + remote branches + tags
  repo <domain> check <dir> [--theme] [--install]
                Zero-exec dependency check (composer/npm missing or stale?) —
                reports + offers steps, runs nothing itself; --install runs the
                offered steps in order, stopping at the first failure
  repo <domain> prs <dir> [--theme]    PR/MR head refs from the remote (checkout
                a listed ref lands detached — refs carry number + sha only)
  repo <domain> adopt <dir> [--theme]  Manage an existing checkout (metadata only)
  repo <domain> link <path> [--name N] [--theme]
                Symlink an external folder in (deleting later only unlinks)
  repo <domain> watch list | watch start <dir> <script> [--tail] | watch tail <dir>
                            | watch stop <dir>
                Dev watchers — run inside the app, stop when it quits.
                --tail (or `watch tail`) follows the output here; Ctrl-C stops
                following, not the watcher.
  repo <domain> add <url> [--branch B] [--name N] [--theme] [--install]
                Clone a repo in (public https/owner-repo, private via YOUR ssh
                keys); --install also runs detected composer/npm/build steps.
                Output appears at completion — live view is in the app panel
  repo <domain> pull|fetch|push <dir> [--install]
  repo <domain> checkout <dir> <ref> [--install]
                Git ops on an asset (pull is --ff-only; push never forces;
                --install re-installs when the op changed lockfiles)
  repo <domain> run <dir> <script>     Run one package.json script to completion
  repo <domain> delete <dir> [--theme] [--yes]
                Delete with the loss-warning preview; symlinked assets are
                UNLINKED only (your real folder is never touched)
  repo tools [--refresh]               Detected git/node (login-shell resolution)
  service start|stop <mysql|mariadb|postgres|redis|mailpit>
                Start/stop one optional service (web tier stays via rex start/stop)
  config get|set <key> [value]       Settings the CLI may touch (refusals say why)
  mail          List caught messages (Mailpit) · mail list --unread [query] filters
  mail open     Open the Mailpit web UI · mail mark-read marks every message read
  mail clear    Delete ALL caught messages (--yes to skip the prompt)
  tunnel list | tunnel start|stop <domain>
                Public cloudflared tunnels (start prints the public URL)
  tld [--set <tld>] [--repair <tld>] [--remove <tld>]
                Default TLD for new sites; --repair puts back the OS resolver
                file for a TLD your sites answer on (what `doctor` names)
  version       App + CLI versions (needs the app; -v/--version works without)
  mcp           MCP stdio bridge for an AI agent's client — used in the client's
                config, not run by hand (e.g. `claude mcp add rexenv -- rex mcp`)
  completions zsh|bash    Print a shell completion script (eval or install it)
  help          Show this help

OPTIONS:
  --json        Machine-readable output (raw response data)

EXIT CODES:
  0 ok · 1 command failed · 2 rexenv isn't running";

fn socket_path() -> PathBuf {
    // Test/dev override only — there is no discovery protocol, the path is fixed.
    if let Ok(p) = std::env::var("REXENV_CLI_SOCKET") {
        return PathBuf::from(p);
    }
    // Mirrors the app's platform paths (macOS `directories::ProjectDirs`
    // with qualifier "dev", org "rexenv", name "rexenv").
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/dev.rexenv.rexenv/config/rexenv-cli.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("rex: this platform is not supported yet");
        exit(1)
    }
}

/// The MCP socket path — the app's `mcp_server` endpoint, a sibling of the CLI
/// socket in the same config dir. Same fixed-path convention (`REXENV_MCP_SOCKET`
/// overrides for tests only), never a second discovery scheme.
fn mcp_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("REXENV_MCP_SOCKET") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/dev.rexenv.rexenv/config/rexenv-mcp.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("rex: this platform is not supported yet");
        exit(1)
    }
}

/// The one sentence the bridge says IN-BAND when the app goes away with a
/// request still unanswered (PLAN-mcp-server §2.3's deferred-with-a-trigger
/// item, built with MCP parity P6.2). It reaches the MODEL, as the error of the
/// call it was waiting on — a bare transport EOF mid-conversation is exactly
/// where a model starts guessing ("the site must have been created").
const MCP_STOPPED: &str =
    "rexenv stopped while this call was in progress — the app quit or was closed. Nothing more \
     will arrive for it. Whether the operation finished is unknown from here: ask the person \
     you're working with to open rexenv again, then reconnect and check (list_sites, \
     site_status) before retrying anything that creates or changes something.";

/// The JSON-RPC `id` of a REQUEST line (has both `method` and `id`); `None`
/// for a notification, a reply, or anything that is not JSON. The bridge reads
/// ids and nothing else — it still constructs no request and interprets no
/// method; the app stays the brain.
fn request_id(line: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    v.get("method")?;
    v.get("id").cloned().filter(|id| !id.is_null())
}

/// The `id` of a REPLY line (has `id` and `result` or `error`).
fn reply_id(line: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("result").is_none() && v.get("error").is_none() {
        return None;
    }
    v.get("id").cloned().filter(|id| !id.is_null())
}

/// The in-band error for one unanswered request — a JSON-RPC error reply the
/// client routes to the pending call, not a protocol-level failure.
fn stopped_error(id: &serde_json::Value) -> String {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": MCP_STOPPED } }).to_string()
}

/// The requests sent and not yet answered, in order. Shared by the two pump
/// threads; drained by the socket side when the app goes away.
#[derive(Default)]
struct PendingIds(std::sync::Mutex<Vec<serde_json::Value>>);

impl PendingIds {
    fn sent(&self, id: serde_json::Value) {
        if let Ok(mut v) = self.0.lock() {
            v.push(id);
        }
    }
    fn answered(&self, id: &serde_json::Value) {
        if let Ok(mut v) = self.0.lock() {
            v.retain(|p| p != id);
        }
    }
    fn drain(&self) -> Vec<serde_json::Value> {
        self.0.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }
}

/// `rex mcp` — the MCP stdio bridge. A bidirectional pipe that copies newline-
/// delimited JSON-RPC between the client's stdio and the app's MCP socket. It
/// constructs no request and interprets no method — the app is the brain — but
/// since MCP parity P6.2 it does READ the ids of the requests it forwards, for
/// one reason: when the app closes the socket with a request still unanswered,
/// the bridge answers that request itself with an in-band error saying rexenv
/// stopped (`MCP_STOPPED`), so the model reads a sentence instead of an EOF.
/// On a dead socket at startup it fails with a specific reason and exits 2 —
/// never a generic transport error the agent papers over with a guess. Either
/// side closing ends the whole bridge, so the client sees the server go away.
fn run_mcp_bridge() -> ! {
    use std::io::BufRead;
    let socket = match UnixStream::connect(mcp_socket_path()) {
        Ok(s) => s,
        // ENOENT (never bound) and ECONNREFUSED (stale after a crash) both mean
        // the app isn't there to serve — the CLI socket's exact treatment.
        Err(_) => {
            eprintln!("{MCP_NOT_RUNNING}");
            exit(2);
        }
    };
    let mut sock_write = match socket.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rex: could not set up the MCP bridge: {e}");
            exit(1);
        }
    };
    let sock_read = socket;
    let pending = std::sync::Arc::new(PendingIds::default());
    // socket → stdout, line by line. When the app closes the socket the server
    // is gone: every request still pending gets the in-band error, then the
    // whole process ends so the client observes the server exit (even if our
    // stdin is still open).
    let pump = {
        let pending = std::sync::Arc::clone(&pending);
        std::thread::spawn(move || {
            let mut out = std::io::stdout().lock();
            let reader = std::io::BufReader::new(sock_read);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(id) = reply_id(&line) {
                    pending.answered(&id);
                }
                if out.write_all(line.as_bytes()).is_err() || out.write_all(b"\n").is_err() {
                    break;
                }
                let _ = out.flush();
            }
            for id in pending.drain() {
                let _ = out.write_all(stopped_error(&id).as_bytes());
                let _ = out.write_all(b"\n");
            }
            let _ = out.flush();
            exit(0);
        })
    };
    // stdin → socket, line by line, remembering each request's id. Client EOF
    // means the session is done: half-close so the app sees end-of-input, then
    // let the socket→stdout side finish.
    let stdin = std::io::stdin().lock();
    for line in stdin.lines() {
        let Ok(line) = line else { break };
        if let Some(id) = request_id(&line) {
            pending.sent(id);
        }
        if sock_write.write_all(line.as_bytes()).is_err() || sock_write.write_all(b"\n").is_err() {
            break;
        }
        let _ = sock_write.flush();
    }
    let _ = sock_write.shutdown(std::net::Shutdown::Write);
    let _ = pump.join();
    exit(0);
}

/// One request line out, one reply line back. Exits the process on transport
/// or command errors — callers only ever see successful data.
/// `request`, but asking the app to STREAM progress while it works.
///
/// The protocol is one request line in, zero or more `{"progress": …}` lines,
/// then exactly one `{"ok": …}` envelope. Progress goes to stderr so `--json`
/// and pipes keep getting only the result on stdout; the envelope is handled
/// exactly as `request` handles it.
///
/// Streaming is REQUESTED, never assumed: an app that streamed unasked would
/// hand an older `rex` — which reads one line and stops — a progress record as
/// the reply.
/// Print one streamed provisioning record as a terminal line.
///
/// stderr, always: the result of the command belongs on stdout, and a `--json`
/// caller or a pipe must not have progress mixed into the value it is parsing.
fn print_provision_progress(p: &Value) {
    let pct = p["pct"].as_u64().unwrap_or(0);
    match p["phase"]["label"].as_str() {
        Some(label) => eprintln!("  [{pct:>3}%] {label}"),
        // A record with no phase is a status change (the job finished, failed
        // or was cancelled between polls) — still worth a line, because the
        // alternative is a terminal that goes quiet with no explanation.
        None => eprintln!("  [{pct:>3}%] {}", p["status"].as_str().unwrap_or("working")),
    }
}

fn request_streaming(cmd: &str, args: Value, mut on_progress: impl FnMut(&Value)) -> Value {
    request_inner(cmd, args, true, &mut on_progress)
}

fn request(cmd: &str, args: Value) -> Value {
    request_inner(cmd, args, false, &mut |_| {})
}

fn request_inner(
    cmd: &str,
    args: Value,
    stream: bool,
    on_progress: &mut dyn FnMut(&Value),
) -> Value {
    let path = socket_path();
    // ENOENT (app never bound) and ECONNREFUSED (stale file after a crash)
    // mean the same thing to the user: the app isn't there to take commands.
    let mut sock = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("{NOT_RUNNING}");
            exit(2);
        }
    };
    let line = json!({ "cmd": cmd, "args": args, "stream": stream }).to_string();
    if sock
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|_| sock.flush())
        .is_err()
    {
        eprintln!("{NOT_RUNNING}");
        exit(2);
    }
    let mut reply = String::new();
    // No read timeout on purpose: mutating commands (site create) legitimately
    // run for minutes; the app closes the connection when it's done. But an app
    // that has gone deaf looks exactly like one that is working, so say so
    // rather than leaving a terminal with no output at all.
    let waiting = stall_notice(STALL_NOTICE_AFTER, || {
        eprintln!(
            "rex: no reply yet after {}s — the app is either still working or wedged. \
             Ctrl-C is safe; nothing is sent twice.",
            STALL_NOTICE_AFTER.as_secs()
        );
    });
    // Read until the ENVELOPE. Progress records are identified by their key,
    // not by position, so a client can join a stream it does not understand and
    // still know which line ends it.
    let mut reader = BufReader::new(sock);
    let read = loop {
        reply.clear();
        match reader.read_line(&mut reply) {
            Err(e) => break Err(e),
            Ok(0) => break Ok(0),
            Ok(n) => {
                let Ok(v) = serde_json::from_str::<Value>(reply.trim()) else { break Ok(n) };
                match v.get("progress") {
                    Some(p) => on_progress(p),
                    None => break Ok(n),
                }
            }
        }
    };
    waiting.store(true, Ordering::Relaxed);
    if read.is_err() || reply.trim().is_empty() {
        eprintln!("rex: the app closed the connection without replying");
        exit(1);
    }
    let envelope: Value = match serde_json::from_str(reply.trim()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rex: unreadable reply from the app: {e}");
            exit(1);
        }
    };
    if envelope["ok"] == json!(true) {
        envelope["data"].clone()
    } else {
        let msg = envelope["error"].as_str().unwrap_or("unknown error");
        eprintln!("rex: {msg}");
        // Only for the one error that is ALWAYS a build mismatch. A typo is
        // caught client-side by `rex`'s own match, so an unknown command
        // reaching the app means it sent something this app does not answer.
        // Costs a round trip on a path that has already failed, and nothing at
        // all on every other path.
        if msg.contains("unknown command") {
            if let Some(skew) = version_skew() {
                eprintln!("rex: {skew}");
            }
        }
        exit(1);
    }
}

/// What to add to an error when `rex` and the running app are different builds.
///
/// **This replaces the roadmap's 🟡 "protocol version handshake", and the
/// substitution is deliberate.** A protocol integer answers "is the wire
/// contract compatible", which is not the question anyone has here: `rex` and
/// the app ship in the SAME cask, at the same version, so a difference is never
/// a compatibility negotiation — it is a stale build, and naming which one is
/// stale is the whole fix. A protocol number would also stay silent for exactly
/// the case that keeps happening, since adding commands is backward-compatible
/// and would never bump it.
///
/// The case it is for: `cli_server` answers "unknown command: X (this rex may be
/// newer than the running app)" — a hedge, because the app cannot know. This
/// turns it into a fact. It has bitten in development more than once (a rebuilt
/// `cli_server.rs` against an app still running the old binary) and would bite a
/// user who updated the cask without restarting rexenv.
///
/// `None` when the versions match or the app cannot be asked — silence is
/// correct there, and guessing would put a version warning on an unrelated bug.
fn version_skew() -> Option<String> {
    let app = soft_request("version")?;
    version_skew_between(
        env!("CARGO_PKG_VERSION"),
        app["version"].as_str()?,
        env!("REX_GIT_COMMIT"),
        app["commit"].as_str().unwrap_or(""),
    )
}

/// The comparison, with both sides as parameters so a test can drive it.
///
/// Split out for the reason the Xdebug and debug-PHP gates were: the live
/// function reads a compile-time constant and a running app, so through it the
/// interesting states — skewed, matched, unknown — are not reachable at all.
///
/// **The COMMIT is the load-bearing half, and the first version of this did not
/// have it.** Both builds carry the same `CARGO_PKG_VERSION` for a whole release
/// cycle, so a version comparison is silent for exactly the case that keeps
/// happening: a rebuilt CLI against an app still running an older binary.
/// Measured 23 Aug 2026 by running the very command this check was written for —
/// `rex site relink` against an app built from `460981c` returned "unknown
/// command" and this said nothing, because both reported 0.3.0.
fn version_skew_between(
    mine: &str,
    theirs: &str,
    my_commit: &str,
    their_commit: &str,
) -> Option<String> {
    // An app that could not be asked, or one too old to report a commit, is not
    // evidence of anything — guessing would put a build warning on every failure
    // from an app that simply is not running.
    if theirs.is_empty() {
        return None;
    }
    if theirs != mine {
        return Some(format!(
            "this rex is {mine} and the running rexenv is {theirs} — they ship together, so \
             one of them is stale. Quit and reopen rexenv to pick up the newer app; if that \
             does not change it, the app on disk is the older one."
        ));
    }
    // Same version, different build. `unknown` on either side means the stamp is
    // missing rather than different, which is not a mismatch.
    let unknown = |c: &str| c.is_empty() || c == "unknown";
    if unknown(my_commit) || unknown(their_commit) || my_commit == their_commit {
        return None;
    }
    // Deliberately does NOT say which side is behind. Observed 24 Aug 2026: after
    // the app was rebuilt first, the APP was the newer one and this told the user
    // to quit and reopen it — advice for the opposite situation. A commit sha
    // gives no ordering, and guessing produces confidently wrong instructions in
    // half the cases. Both fixes are named instead; whichever applies is cheap.
    Some(format!(
        "both are {mine}, but this rex was built from {my_commit} and the running rexenv \
         from {their_commit} — same version, different build. Rebuild whichever is behind: \
         quit and reopen rexenv to pick up a newer app, or rebuild `rex` to pick up a newer \
         CLI."
    ))
}

/// Best-effort request: `None` on any transport/command failure — for output
/// that must not require a running app (`--version`).
fn soft_request(cmd: &str) -> Option<Value> {
    soft_request_at(&socket_path(), cmd, SOFT_DEADLINE)
}

/// The half of `soft_request` that a test can point somewhere else.
///
/// The deadline is the whole point. "The app isn't running" is not only ENOENT
/// and ECONNREFUSED: an app can hold the socket, ACCEPT the connection, and
/// then never answer — which is exactly what a stale App-Translocated instance
/// did on 2026-08-12, hanging `rex --version` in `recvfrom` with no output
/// until that pid was killed (`docs/PUBLISH-TESTING.md` §D). A connect that
/// succeeds proves a listener exists, never that anything is behind it, so
/// "best effort" has to be bounded in time and not just in error kind.
fn soft_request_at(path: &Path, cmd: &str, deadline: Duration) -> Option<Value> {
    let mut stream = UnixStream::connect(path).ok()?;
    // Both directions: a peer that never reads can block the write just as a
    // peer that never writes blocks the read.
    stream.set_read_timeout(Some(deadline)).ok()?;
    stream.set_write_timeout(Some(deadline)).ok()?;
    let line = json!({ "cmd": cmd, "args": Value::Null }).to_string();
    stream.write_all(format!("{line}\n").as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).ok()?;
    let envelope: Value = serde_json::from_str(reply.trim()).ok()?;
    (envelope["ok"] == json!(true)).then(|| envelope["data"].clone())
}

/// Say, once, that we are still waiting — then keep waiting.
///
/// `request` deliberately has no read timeout, and that cannot change: the app
/// writes a finished command's reply in ONE write at the end, so "no bytes yet"
/// looks identical for a `site create` three minutes into real work and for an
/// app that will never answer. Killing the wait would break the first to fix
/// the second. What is fixable is the terminal looking dead: this prints a
/// single line to stderr after `after`, unless the caller has already flipped
/// the returned flag by then.
fn stall_notice(after: Duration, notify: impl Fn() + Send + 'static) -> Arc<AtomicBool> {
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    // Detached on purpose: the process exits when the command does, and a
    // thread parked in `sleep` must never hold that up.
    std::thread::spawn(move || {
        std::thread::sleep(after);
        if !flag.load(Ordering::Relaxed) {
            notify();
        }
    });
    done
}

fn main() {
    let mut json_output = false;
    let mut words: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => json_output = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return;
            }
            // Native version: must work WITHOUT the app (unlike `rex version`,
            // the app round-trip) — CLI version always, app version best-effort.
            //
            // **The CLI's own commit prints unconditionally, and that is the
            // point of this arm.** A DOWNLOADED rexenv could not previously be
            // asked what built it: nothing here carried a commit, and
            // `rex version` asks the running APP over the socket — so pointing a
            // dmg's own `rex` at it reported whatever was running locally. That
            // made every release row's source commit an inference from the tag
            // and the build timeline rather than a fact read off the bytes
            // (`docs/PUBLISH-TESTING.md` §A, 0.3.0).
            //
            // Both halves are LABELLED, because the failure this replaces was
            // ambiguity, not absence: two version numbers side by side with one
            // commit under them invites reading the commit as belonging to
            // either.
            "-v" | "-V" | "--version" => {
                print!("rex {} ({})", env!("CARGO_PKG_VERSION"), env!("REX_GIT_COMMIT"));
                if let Some(app) = soft_request("version") {
                    print!(
                        " · app rexenv {} ({})",
                        app["version"].as_str().unwrap_or("?"),
                        app["commit"].as_str().unwrap_or("?"),
                    );
                }
                println!();
                return;
            }
            _ => words.push(arg),
        }
    }
    // `rex mcp` is the MCP bridge, not a request/reply command: it uses its own
    // socket and holds a long-lived session, so it runs BEFORE the CLI-socket
    // preflight below and never returns.
    if words.first().map(String::as_str) == Some("mcp") {
        run_mcp_bridge();
    }
    // Preflight: every subcommand below talks to the app. Probe the socket
    // ONCE up front so a not-running app prints only the honest message and
    // exits 2 — never after a misleading in-progress line ("starting
    // services…", "creating <domain>…"). The app can still die between this
    // probe and a request; `request` then prints the same message.
    match words.first().map(String::as_str) {
        None | Some("completions") => {} // native output, no app needed
        _ => {
            if UnixStream::connect(socket_path()).is_err() {
                eprintln!("{NOT_RUNNING}");
                exit(2);
            }
        }
    }
    match words.first().map(String::as_str) {
        None => println!("{USAGE}"),
        Some("status") => cmd_status(json_output),
        Some("open") => cmd_open_app(json_output),
        Some("start") => cmd_lifecycle(&["start"], json_output),
        Some("stop") => cmd_lifecycle(&["stop"], json_output),
        Some("restart") => cmd_lifecycle(&["stop", "start"], json_output),
        Some("logs") => cmd_logs(&words[1..], json_output),
        Some("doctor") => cmd_doctor(json_output),
        Some("php") => cmd_php(&words[1..], json_output),
        Some("wp") => cmd_wp(&words[1..], json_output),
        Some("repo") => cmd_repo(&words[1..], json_output),
        Some("service") => cmd_service(&words[1..], json_output),
        Some("config") => cmd_config(&words[1..], json_output),
        Some("mail") => cmd_mail(&words[1..], json_output),
        Some("tunnel") => cmd_tunnel(&words[1..], json_output),
        Some("tld") => cmd_tld(&words[1..], json_output),
        Some("version") => cmd_version(json_output),
        Some("completions") => cmd_completions(words.get(1).map(String::as_str)),
        Some("blueprints") => {
            let data = request("blueprint.list", Value::Null);
            if json_output {
                print_json(&data);
            } else {
                match data["blueprints"].as_array().filter(|b| !b.is_empty()) {
                    None => println!("no saved blueprints (create them in the app: Settings → Blueprints)"),
                    Some(rows) => {
                        for b in rows {
                            println!("{}", b["name"].as_str().unwrap_or("?"));
                        }
                    }
                }
            }
        }
        Some("db") => match words.get(1).map(String::as_str) {
            Some("export") => cmd_db_export(&words[2..], json_output),
            Some("import") => cmd_db_import(&words[2..], json_output),
            Some("reset") => cmd_db_reset(&words[2..], json_output),
            Some("versions") => cmd_db_versions(&words[2..], json_output),
            Some("browse") => open_url("https://adminer.rexenv.rex"),
            _ => {
                eprintln!("rex: usage: rex db <export|import|reset|versions|browse>\n\n{USAGE}");
                exit(1);
            }
        },
        Some("site") => match words.get(1).map(String::as_str) {
            Some("list") => cmd_site_list(json_output),
            Some("create") => cmd_site_create(&words[2..], json_output),
            Some("delete") => cmd_site_delete(&words[2..], json_output),
            Some("info") => cmd_site_info(&words[2..], json_output),
            Some("logs") => cmd_site_logs(&words[2..], json_output),
            Some("php") => cmd_site_php(&words[2..], json_output),
            Some("xdebug") => cmd_site_xdebug(&words[2..], json_output),
            Some("server") => cmd_site_server(&words[2..], json_output),
            Some("restart") => cmd_site_restart(&words[2..], json_output),
            Some("start") => cmd_site_enabled(&words[2..], true, json_output),
            Some("stop") => cmd_site_enabled(&words[2..], false, json_output),
            Some("domains") => cmd_site_domains(&words[2..], json_output),
            Some("rename") => cmd_site_rename(&words[2..], json_output),
            Some("domain") => cmd_site_domain(&words[2..], json_output),
            Some("move") => cmd_site_move(&words[2..], json_output),
            Some("relink") => cmd_site_relink(&words[2..], json_output),
            Some("retry") => cmd_site_retry(&words[2..], json_output),
            Some("env") => cmd_site_env(&words[2..], json_output),
            Some("cert") => cmd_site_cert(&words[2..], json_output),
            Some("open") => cmd_site_open(&words[2..]),
            Some("login") => cmd_site_login(&words[2..], json_output),
            _ => {
                eprintln!("rex: usage: rex site <list|create|delete|info|open|login>\n\n{USAGE}");
                exit(1);
            }
        },
        Some(other) => {
            eprintln!("rex: unknown command `{other}`\n\n{USAGE}");
            exit(1);
        }
    }
}

fn print_json(data: &Value) {
    println!("{}", serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()));
}

// ── start / stop / restart ───────────────────────────────────────────────────

/// Runs each lifecycle step as its own request; `request` exits on the first
/// Bring the app's window up.
///
/// Worth a verb of its own because rexenv has no dock icon: the window is
/// reached from the menu bar or from here, and a developer already in a
/// terminal should not have to go looking for a status item. It is the same
/// command a SECOND rexenv launch sends before exiting
/// (`cli_server::hand_off_to_running_instance`), so "another instance brought
/// the window forward" and `rex open` are one code path, not two.
fn cmd_open_app(json_output: bool) {
    request("app.open", Value::Null);
    if json_output {
        println!("{}", json!({ "opened": true }));
    } else {
        println!("✓ rexenv window opened");
    }
}

/// failure, so a failed stop never chains into a start. A start can run for a
/// while on a cold cache (the app downloads binaries) — say so up front.
fn cmd_lifecycle(steps: &[&str], json_output: bool) {
    for step in steps {
        if !json_output {
            match *step {
                "start" => println!("starting services… (first run may download binaries)"),
                _ => println!("stopping services…"),
            }
        }
        request(step, Value::Null);
        if !json_output {
            println!("✓ {step} done");
        }
    }
    if json_output {
        print_json(&json!({ "ok": true }));
    }
}

// ── site list ────────────────────────────────────────────────────────────────

fn cmd_site_list(json_output: bool) {
    let data = request("site.list", Value::Null);
    if json_output {
        return print_json(&data);
    }
    let Some(sites) = data["sites"].as_array() else {
        return println!("(no sites)");
    };
    if sites.is_empty() {
        return println!("no sites yet — create one with the app or `rex site create <domain>`");
    }
    // `serving` is the live wire truth (edge up AND the site's upstream up);
    // sites the stack isn't serving right now show "down".
    let serving: Vec<(&str, bool)> = data["serving"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| Some((r["domain"].as_str()?, r["serving"] == json!(true))))
                .collect()
        })
        .unwrap_or_default();
    let col = |key: &str, min: usize| -> usize {
        sites
            .iter()
            .filter_map(|s| s[key].as_str())
            .map(str::len)
            .max()
            .unwrap_or(min)
            .max(min)
    };
    let (dw, nw) = (col("domain", 6), col("name", 4));
    // The extra names a site answers on (v42), after the columns every site
    // has — `find_site` accepts them and the error it prints sends people
    // here, so a list that hid them sent people to a dead end.
    let also = |s: &Value| -> String {
        s["id"]
            .as_str()
            .and_then(|id| data["aliases"][id].as_array())
            .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    };
    println!("{:<dw$}  {:<nw$}  {:<9}  {:<5}  {:<10}  {:<7}  {:<7}  ALSO", "DOMAIN", "NAME", "TYPE", "PHP", "SERVER", "DB", "STATE");
    for s in sites {
        let domain = s["domain"].as_str().unwrap_or("?");
        let up = serving.iter().any(|(d, up)| *d == domain && *up);
        println!(
            "{:<dw$}  {:<nw$}  {:<9}  {:<5}  {:<10}  {:<7}  {:<7}  {}",
            domain,
            s["name"].as_str().unwrap_or("?"),
            s["type"].as_str().unwrap_or("?"),
            s["phpVersion"].as_str().unwrap_or("?"),
            s["webServer"].as_str().unwrap_or("?"),
            s["dbEngine"].as_str().unwrap_or("?"),
            if up { "serving" } else { "down" },
            also(s),
        );
    }
}

// ── site create / delete ─────────────────────────────────────────────────────

/// The value after `flag`, or `None` — including when the next word is itself a
/// flag. That guard arrived with the first SWITCH on `site create`
/// (`--starter-db`): before it, `--name --starter-db` bound the name to the
/// literal string `"--starter-db"` and created a site called that, silently.
/// A forgotten value reads as an absent flag now, which the callers already
/// handle, instead of as a value nobody typed.
fn flag_value(words: &[String], flag: &str) -> Option<String> {
    words
        .iter()
        .position(|w| w == flag)
        .and_then(|i| words.get(i + 1))
        .filter(|v| !v.starts_with("--"))
        .cloned()
}

/// Refuse a flag this command does not know, naming it.
///
/// An unrecognised flag is otherwise DROPPED, and the command runs with the
/// default the flag existed to override. On `site create` that built the wrong
/// site; on `wp search-replace` it is worse — `--dry-runn --yes` turns a
/// rehearsal into a real replace across the database, with no prompt, because
/// the typo removes the dry-run and the `--yes` removes the question.
///
/// `known` is the command's own list, passed by the caller that reads those
/// flags, so the two cannot drift apart in different files.
fn reject_unknown_flags(words: &[String], command: &str, known: &[&str], usage: &str) {
    if let Some(bad) = unknown_flag(words, known) {
        eprintln!("rex: unknown flag `{bad}` for `{command}`\n{usage}");
        exit(2);
    }
}

/// The decision behind [`reject_unknown_flags`], split out so a test can hold
/// it: the refusal itself ends the process, which no unit test can survive.
fn unknown_flag<'a>(words: &'a [String], known: &[&str]) -> Option<&'a String> {
    words.iter().find(|w| w.starts_with("--") && !known.contains(&w.as_str()))
}

/// `site create`'s value-taking flags, and the socket key each one fills. ONE
/// list: the loop below sends them and the unknown-flag check measures against
/// it, so a flag can never be accepted-but-unsent or refused-but-supported.
const CREATE_FLAGS: [(&str, &str); 8] = [
    ("--name", "name"),
    ("--type", "type"),
    ("--php", "php"),
    ("--server", "server"),
    ("--db", "db"),
    ("--blueprint", "blueprint"),
    ("--multisite", "multisite"),
    ("--path", "path"),
];

/// Every accepted flag appears here — enforced, because two of them
/// (`--blueprint`, `--multisite`) had worked since they shipped and were in no
/// usage line, so the only way to find them was to read the source.
const CREATE_USAGE: &str = "rex: usage: rex site create <domain> [--name N] [--type T] [--php V] \
                            [--server S] [--db D] [--path FOLDER] [--starter-db] \
                            [--blueprint NAME] [--multisite subdomain|subdirectory]";

/// The flags `site create` accepts. A function, not a literal inside the
/// command, so a test can ask the REAL list rather than re-reading the text the
/// list was written in: the first version of that test scanned the same lines
/// the list is built from, so deleting an entry deleted the evidence too and
/// the plant came back green.
fn create_known_flags() -> Vec<&'static str> {
    let mut v: Vec<&str> = CREATE_FLAGS.iter().map(|(f, _)| *f).collect();
    v.push("--starter-db");
    v
}

fn cmd_site_create(words: &[String], json_output: bool) {
    let Some(domain) = words.first().filter(|w| !w.starts_with("--")) else {
        eprintln!("{CREATE_USAGE}");
        exit(1);
    };
    // A misspelt flag here does not fail — it is IGNORED, and the site is
    // created with the default the flag was there to override. `--phpp 8.4`
    // gives you a site on the default minor with no word said, and a site is a
    // durable artifact: docroot, database, certificate, config. Refusing costs
    // one retype; the silence costs a delete and a re-create.
    reject_unknown_flags(words, "site create", &create_known_flags(), CREATE_USAGE);
    let mut args = serde_json::Map::new();
    args.insert("domain".into(), json!(domain));
    for (flag, key) in CREATE_FLAGS {
        if let Some(v) = flag_value(words, flag) {
            args.insert(key.into(), json!(v));
        }
    }
    // A switch, not a value: the dialog's Database field for a Blank-PHP site,
    // which creates the database, seeds `starter_items` and writes `db.php`.
    // Sent only when asked — the server treats it as absent otherwise, and an
    // older app that has never heard of the key is unchanged by its absence.
    if words.iter().any(|w| w == "--starter-db") {
        args.insert("starterDb".into(), json!(true));
    }
    if !json_output {
        println!("creating {domain}… (WordPress sites install on first create — this can take a minute)");
    }
    let created = if json_output {
        request("site.create", Value::Object(args))
    } else {
        request_streaming("site.create", Value::Object(args), print_provision_progress)
    };
    if json_output {
        return print_json(&created);
    }
    println!(
        "✓ created {} ({}, PHP {}, {}, {}) → https://{}",
        created["domain"].as_str().unwrap_or(domain),
        created["type"].as_str().unwrap_or("?"),
        created["phpVersion"].as_str().unwrap_or("?"),
        created["webServer"].as_str().unwrap_or("?"),
        created["dbEngine"].as_str().unwrap_or("?"),
        created["domain"].as_str().unwrap_or(domain),
    );
    // Reported from the ROW the app wrote back, not from the flag we sent —
    // core records the starter database only where the question was asked, so
    // echoing our own argument would claim a seed the site may not have.
    if created["starterDb"] == json!(true) {
        println!("  starter database seeded — `starter_items` and a db.php your index.php can require");
    }
}

/// One spelling for a hostname typed at the CLI: trimmed, no trailing dot,
/// lower-case — the same normalisation the app applies before it stores one.
fn normalize_hostname(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// The site that answers on `domain` as an EXTRA name, if any — `(id, primary)`.
fn alias_owner(data: &Value, domain: &str) -> Option<(String, String)> {
    let owner = data["aliases"].as_object()?.iter().find_map(|(id, list)| {
        list.as_array()?
            .iter()
            .any(|d| d == &json!(domain))
            .then(|| id.clone())
    })?;
    let primary = data["sites"]
        .as_array()?
        .iter()
        .find(|s| s["id"] == json!(owner))?["domain"]
        .as_str()?
        .to_string();
    Some((owner, primary))
}

/// Resolve a `<domain>` argument to the site object via the app's own list —
/// exits with a helpful error otherwise. Any name the site answers on works.
fn find_site(words: &[String], usage: &str) -> Value {
    let Some(domain) = words.first().filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: {usage}");
        exit(1);
    };
    // The spelling the app stores: `dig` prints a trailing dot and people
    // type capitals, and the app accepted both on the way in.
    let domain = normalize_hostname(domain);
    let data = request("site.list", Value::Null);
    let sites = data["sites"].as_array().cloned().unwrap_or_default();
    // The site's OWN domain first, then its EXTRA domains (v42): a site that
    // answers on `shop.rex` should be findable by typing `shop.rex`. Without
    // this, extra domains were a thing the app served and the CLI could not
    // name — `rex site info shop.rex` said "no site with domain shop.rex" about
    // a site that answers on it.
    let by_primary = sites.iter().find(|s| s["domain"] == json!(domain)).cloned();
    let site = by_primary.or_else(|| {
        let (owner, _) = alias_owner(&data, &domain)?;
        sites.iter().find(|s| s["id"] == json!(owner)).cloned()
    });
    match site {
        Some(site) => site,
        None => {
            eprintln!("rex: no site answers on `{domain}` (see `rex site list`)");
            exit(1);
        }
    }
}

/// macOS default-browser open; prints the URL either way so the command is
/// still useful over SSH or when `open` is unavailable.
fn open_url(url: &str) {
    println!("{url}");
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).status();
    }
}

fn cmd_site_info(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site info <domain>");
    let data = request("site.info", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    let s = &data["site"];
    let field = |label: &str, v: String| println!("{label:<12} {v}");
    let str_of = |v: &Value| v.as_str().unwrap_or("?").to_string();
    field("domain", format!("https://{}", str_of(&s["domain"])));
    // Extra domains (v42) on their own line, and only when there are any: this
    // verb answers "tell me everything about this site", and a site answering
    // on three names while `info` shows one is the half-answer the Sites list
    // carried until this morning.
    if let Some(extra) = data["domains"].as_array().map(|d| &d[1.min(d.len())..]) {
        if !extra.is_empty() {
            field(
                "also",
                extra.iter().map(|d| format!("https://{}", str_of(d))).collect::<Vec<_>>().join("  "),
            );
        }
    }
    field("name", str_of(&s["name"]));
    field("state", if data["serving"] == json!(true) { "serving".into() } else { "down".into() });
    field(
        "type",
        format!(
            "{}{}",
            str_of(&s["type"]),
            data["wp"]["version"].as_str().map(|v| format!(" {v}")).unwrap_or_default()
        ),
    );
    if data["wp"]["multisite"] == json!(true) || s["multisite"].as_str().is_some_and(|m| m != "none") {
        field("multisite", str_of(&s["multisite"]));
    }
    field(
        "php",
        format!(
            "{}{}",
            str_of(&s["phpVersion"]),
            if s["xdebug"] == json!(true) { " (Xdebug)" } else { "" }
        ),
    );
    field("server", str_of(&s["webServer"]));
    field("database", format!("{} ({})", str_of(&s["dbEngine"]), str_of(&s["dbName"])));
    field("path", str_of(&s["path"]));
    if let Some(days) = data["cert"]["daysLeft"].as_i64() {
        field("cert", format!("{days} days left (expires {})", str_of(&data["cert"]["notAfter"])));
    }
    let res = &data["resources"];
    if res.is_object() {
        let mut parts = Vec::new();
        if let Some(c) = res["cpuPercent"].as_f64() {
            parts.push(format!("cpu {c:.1}%"));
        }
        if let Some(r) = res["ramMb"].as_u64() {
            parts.push(format!("ram {r} MB"));
        }
        if let Some(r) = res["requestsPerMin"].as_u64() {
            parts.push(format!("{r} req/min"));
        }
        if !parts.is_empty() {
            field("resources", parts.join(" · "));
        }
    }
    field("created", str_of(&s["createdAt"]));
}

fn cmd_site_open(words: &[String]) {
    let site = find_site(words, "rex site open <domain>");
    open_url(&format!("https://{}", site["domain"].as_str().unwrap_or_default()));
}

fn cmd_site_login(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site login <domain> [--print]");
    if site["type"] != json!("wordpress") {
        eprintln!("rex: `{}` is not a WordPress site", site["domain"].as_str().unwrap_or("?"));
        exit(1);
    }
    let data = request("site.login", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    let url = data["url"].as_str().unwrap_or_default();
    if words.iter().any(|w| w == "--print") {
        println!("{url}");
    } else {
        open_url(url);
    }
}

// ── php versions ─────────────────────────────────────────────────────────────

fn cmd_php(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        Some("list") | None => {
            let data = request("php.list", Value::Null);
            if json_output {
                return print_json(&data);
            }
            let Some(versions) = data["versions"].as_array() else { return println!("(none)") };
            println!(
                "{:<7} {:<9} {:<6} {:<10} {:<8} NOTE",
                "MINOR", "PATCH", "PORT", "INSTALLED", "DEFAULT"
            );
            for v in versions {
                // PATCH is what this build PINS. Two facts can differ from it and
                // both must be said here, not only in the GUI: what the pool is
                // actually executing (a bump that has not taken yet), and what
                // php.net says exists. A CLI that prints only the pin is the same
                // silent lie the desktop row exists to prevent.
                let mut note = String::new();
                if let Some(serving) = v["serving"].as_str() {
                    note.push_str(&format!("serving {serving}"));
                }
                if let Some(upstream) = v["upstream"].as_str() {
                    if !note.is_empty() {
                        note.push_str(" · ");
                    }
                    note.push_str(&format!("{upstream} exists"));
                }
                println!(
                    "{:<7} {:<9} {:<6} {:<10} {:<8} {}",
                    v["minor"].as_str().unwrap_or("?"),
                    v["patch"].as_str().unwrap_or("?"),
                    v["fpmPort"].as_u64().unwrap_or(0),
                    if v["installed"] == json!(true) { "yes" } else { "-" },
                    if v["isDefault"] == json!(true) { "✓" } else { "" },
                    note,
                );
            }
        }
        Some("default") => {
            let Some(minor) = words.get(1) else {
                eprintln!("rex: usage: rex php default <minor>");
                exit(1);
            };
            request("php.default", json!({ "minor": minor }));
            println!("✓ PHP {minor} is the default for new sites");
        }
        Some("settings") => {
            let Some(minor) = words.get(1).filter(|w| !w.starts_with("--")) else {
                eprintln!("rex: usage: rex php settings <minor> [set K=V]");
                exit(1);
            };
            match words.get(2).map(String::as_str) {
                None => {
                    let data = request("php.settings", json!({ "minor": minor }));
                    if json_output {
                        return print_json(&data);
                    }
                    for s in data["settings"].as_array().map(Vec::as_slice).unwrap_or_default() {
                        println!(
                            "{:<24} {}",
                            s["key"].as_str().unwrap_or("?"),
                            s["value"]
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("(default: {})", s["default"].as_str().unwrap_or("?"))),
                        );
                    }
                }
                Some("set") => {
                    let Some((k, v)) = words.get(3).and_then(|kv| kv.split_once('=')) else {
                        eprintln!("rex: usage: rex php settings <minor> set KEY=value");
                        exit(1);
                    };
                    // The backend applies the FULL submitted set — resend every
                    // stored value plus the change (unset keys stay default).
                    let current = request("php.settings", json!({ "minor": minor }));
                    let mut pairs: Vec<(String, String)> = current["settings"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|s| {
                                    Some((s["key"].as_str()?.to_string(), s["value"].as_str()?.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    pairs.retain(|(key, _)| key != k);
                    pairs.push((k.to_string(), v.to_string()));
                    let payload: Vec<Value> =
                        pairs.iter().map(|(key, val)| json!({ "key": key, "value": val })).collect();
                    let r = request("php.settings.set", json!({ "minor": minor, "settings": payload }));
                    if json_output {
                        return print_json(&r);
                    }
                    println!("✓ {k}={v} (PHP {minor} pool restarted if live)");
                }
                _ => {
                    eprintln!("rex: usage: rex php settings <minor> [set K=V]");
                    exit(1);
                }
            }
        }
        Some(action @ ("install" | "uninstall")) => {
            let Some(minor) = words.get(1) else {
                eprintln!("rex: usage: rex php {action} <minor>");
                exit(1);
            };
            if action == "install" {
                println!("installing PHP {minor}… (binaries download on first start)");
            }
            request("php.installed", json!({ "minor": minor, "installed": action == "install" }));
            println!("✓ PHP {minor} {}", if action == "install" { "installed" } else { "uninstalled" });
        }
        _ => {
            eprintln!("rex: usage: rex php <list|default|install|uninstall>\n\n{USAGE}");
            exit(1);
        }
    }
}

/// Print the post-switch site line the backend returns (the updated row).
fn print_site_update(site: &Value) {
    println!(
        "✓ {} — PHP {}{} on {}",
        site["domain"].as_str().unwrap_or("?"),
        site["phpVersion"].as_str().unwrap_or("?"),
        if site["xdebug"] == json!(true) { " (Xdebug)" } else { "" },
        site["webServer"].as_str().unwrap_or("?"),
    );
}

fn cmd_site_php(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site php <domain> <minor>");
    let Some(minor) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site php <domain> <minor>");
        exit(1);
    };
    if !json_output {
        println!("switching {} to PHP {minor}…", site["domain"].as_str().unwrap_or("?"));
    }
    let updated = request("site.php", json!({ "id": site["id"], "version": minor }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

fn cmd_site_xdebug(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site xdebug <domain> on|off");
    let enabled = match words.get(1).map(String::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => {
            eprintln!("rex: usage: rex site xdebug <domain> on|off");
            exit(1);
        }
    };
    let updated = request("site.xdebug", json!({ "id": site["id"], "enabled": enabled }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

/// Long repo jobs hold the connection while the app runs them; the socket
/// can't stream, so output arrives AT COMPLETION — say so up front and tick
/// dots on stderr while waiting (live output is in the app panel / job log).
fn request_long(cmd: &str, args: Value, doing: &str) -> Value {
    eprintln!("{doing} — output appears when it finishes (watch live in the app panel)…");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = stop.clone();
    let ticker = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !s2.load(std::sync::atomic::Ordering::Relaxed) {
                eprint!(".");
            }
        }
    });
    let data = request(cmd, args);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = ticker.join();
    eprintln!();
    data
}

/// Print a settled repo job: step glyphs, the job log, and exit non-zero on
/// any failed/cancelled step (after printing everything).
fn print_repo_job(data: &Value, json_output: bool) {
    if json_output {
        print_json(data);
    } else {
        for st in data["job"]["steps"].as_array().unwrap_or(&vec![]) {
            let glyph = match st["status"].as_str().unwrap_or("") {
                "ok" => "✓",
                "failed" => "✕",
                "cancelled" => "–",
                // Never ran — an earlier step in a run-all failed/cancelled.
                "skipped" => "»",
                "running" => "…",
                _ => "·",
            };
            println!("{glyph} {}", st["label"].as_str().unwrap_or("?"));
            if let Some(e) = st["error"].as_str() {
                for line in e.lines() {
                    println!("    {line}");
                }
            }
        }
        let pending_offers: Vec<&str> = data["job"]["steps"]
            .as_array()
            .map(|steps| {
                steps
                    .iter()
                    .filter(|st| st["status"] == json!("pending"))
                    .filter_map(|st| st["label"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if !pending_offers.is_empty() {
            println!(
                "! dependency steps offered, not run: {} — re-run with --install, or use the app panel",
                pending_offers.join(", ")
            );
        }
        if let Some(w) = data["job"]["nodeWarning"].as_str() {
            println!("! {w}");
        }
        if let Some(log) = data["log"].as_array().filter(|l| !l.is_empty()) {
            println!("── job output ──");
            for l in log {
                println!("{}", l.as_str().unwrap_or(""));
            }
        }
    }
    let failed = data["job"]["steps"]
        .as_array()
        .map(|steps| {
            steps.iter().any(|st| {
                matches!(st["status"].as_str().unwrap_or(""), "failed" | "cancelled")
            })
        })
        .unwrap_or(false);
    if failed {
        exit(1);
    }
}

const REPO_USAGE: &str =
    "rex repo <domain> list|status|branches|prs|adopt|link|watch … (or: rex repo tools)";

/// Git/asset assets — wave 1: pure request/response commands. Every call
/// rides the same commands::repo fns the app UI uses (one code path).
/// Follow a watcher's log until Ctrl-C.
///
/// The key comes from the SNAPSHOT (`logKey`), never rebuilt here. The name is
/// a server-side rule and a caller that re-derives it is one rename away from
/// tailing a file nobody writes — which is how `site_resources_check` came to
/// demand a database name the model forbids re-deriving (ledger #390). An older
/// app that does not send the field is told so, rather than guessing.
fn follow_watch_log(w: &Value, dir: &str) {
    let Some(key) = w["logKey"].as_str().filter(|k| !k.is_empty()) else {
        eprintln!(
            "rex: this app build does not report the watcher's log file, so `--tail` has \
             nothing to follow — `rex version` will say if the app is older than this rex"
        );
        exit(1);
    };
    eprintln!("— following {dir} ({key}); Ctrl-C to stop watching the LOG (the watcher keeps running) —");
    tail_loop(json!({ "key": key }), 200, true);
}

fn cmd_repo(words: &[String], json_output: bool) {
    // `rex repo tools` is app-wide, not site-scoped.
    if words.first().map(String::as_str) == Some("tools") {
        let refresh = words.iter().any(|w| w == "--refresh");
        let data = request("repo.tools", json!({ "refresh": refresh }));
        if json_output {
            return print_json(&data);
        }
        if let Some(rows) = data["tools"].as_array() {
            for t in rows {
                if t["ok"] == json!(true) {
                    println!(
                        "{:<9} {:<28} {}",
                        t["name"].as_str().unwrap_or("?"),
                        t["version"].as_str().unwrap_or("?"),
                        t["path"].as_str().unwrap_or(""),
                    );
                } else {
                    println!("{:<9} MISSING", t["name"].as_str().unwrap_or("?"));
                    for line in t["error"].as_str().unwrap_or("").lines() {
                        println!("          {line}");
                    }
                }
            }
        }
        println!("{:<9} bundled composer.phar (runs on each site's PHP)", "composer");
        return;
    }

    let site = find_site(words, REPO_USAGE);
    let id = site["id"].clone();
    let theme = words.iter().any(|w| w == "--theme");
    let sub = words.get(1).map(String::as_str);
    let rest: Vec<&String> =
        words.iter().skip(2).filter(|w| !w.starts_with("--")).collect();
    let dir_arg = |usage: &str| -> String {
        match rest.first() {
            Some(d) => (*d).clone(),
            None => {
                eprintln!("rex: usage: {usage}");
                exit(1);
            }
        }
    };
    match sub {
        Some("list") | None => {
            let with_status = words.iter().any(|w| w == "--status");
            let data = request("repo.list", json!({ "id": id, "status": with_status }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["assets"].as_array().filter(|a| !a.is_empty()) else {
                return println!(
                    "no git-backed assets (add one in the app, or: rex repo <domain> adopt <dir>)"
                );
            };
            for a in rows {
                let key =
                    format!("{}/{}", a["kind"].as_str().unwrap_or("?"), a["dirName"].as_str().unwrap_or("?"));
                let st = &data["statuses"][&key];
                let live = if st["error"].as_str().is_some() {
                    " (status unavailable)".to_string()
                } else if st.is_object() {
                    let dirty = st["changed"].as_u64().unwrap_or(0) + st["untracked"].as_u64().unwrap_or(0);
                    format!(
                        " {} {}↑{}↓{}",
                        st["branch"].as_str().unwrap_or("detached"),
                        if dirty > 0 { format!("{dirty} dirty ") } else { "clean ".into() },
                        st["ahead"].as_u64().unwrap_or(0),
                        st["behind"].as_u64().unwrap_or(0),
                    )
                } else {
                    String::new()
                };
                println!(
                    "{:<7} {:<28} {:<8} {}{}",
                    a["kind"].as_str().unwrap_or("?"),
                    a["dirName"].as_str().unwrap_or("?"),
                    a["source"].as_str().unwrap_or("?"),
                    a["url"].as_str().filter(|u| !u.is_empty()).unwrap_or("(no remote)"),
                    live,
                );
            }
        }
        Some("status") => {
            let dir = dir_arg("rex repo <domain> status <dir> [--theme]");
            let data = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let head = if data["unborn"] == json!(true) {
                "no commits yet".to_string()
            } else if data["detached"] == json!(true) {
                "detached HEAD".to_string()
            } else {
                data["branch"].as_str().unwrap_or("?").to_string()
            };
            println!("branch     {head}");
            let (ch, un) =
                (data["changed"].as_u64().unwrap_or(0), data["untracked"].as_u64().unwrap_or(0));
            println!(
                "tree       {}",
                if ch + un == 0 {
                    "clean".to_string()
                } else {
                    format!("{ch} changed, {un} untracked")
                }
            );
            match data["upstream"].as_str() {
                Some(up) => println!(
                    "upstream   {up} (↑{} ↓{})",
                    data["ahead"].as_u64().unwrap_or(0),
                    data["behind"].as_u64().unwrap_or(0)
                ),
                None => println!("upstream   (none)"),
            }
            if let Some(r) = data["remote"].as_str() {
                println!("remote     {r}");
            }
            if let Some(t) = data["linkTarget"].as_str() {
                println!("linked →   {t}");
            }
            if let Some(w) = data["lossWarning"].as_str() {
                println!("at risk    {w}");
            }
        }
        Some("branches") => {
            let dir = dir_arg("rex repo <domain> branches <dir> [--theme]");
            let data = request("repo.branches", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let current = data["current"].as_str().unwrap_or("");
            for b in data["local"].as_array().unwrap_or(&vec![]) {
                let name = b.as_str().unwrap_or("?");
                println!("{} {name}", if name == current { "*" } else { " " });
            }
            for b in data["remote"].as_array().unwrap_or(&vec![]) {
                println!("  {}", b.as_str().unwrap_or("?"));
            }
            let tags = data["tags"].as_array().cloned().unwrap_or_default();
            if !tags.is_empty() {
                println!("tags:");
                for t in &tags {
                    println!("  {}", t.as_str().unwrap_or("?"));
                }
            }
        }
        Some("check") => {
            let dir = dir_arg("rex repo <domain> check <dir> [--theme] [--install]");
            let install = words.iter().any(|w| w == "--install");
            let payload = json!({ "id": id, "dir": dir, "theme": theme, "install": install });
            let data = if install {
                request_long("repo.check", payload, "checking + installing what's needed")
            } else {
                request("repo.check", payload)
            };
            print_repo_job(&data, json_output);
        }
        Some("prs") => {
            let dir = dir_arg("rex repo <domain> prs <dir> [--theme]");
            let data = request("repo.prs", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let prs = data.as_array().cloned().unwrap_or_default();
            if prs.is_empty() {
                println!("(no PR/MR refs advertised by the remote)");
            }
            for p in &prs {
                let sha = p["sha"].as_str().unwrap_or("?");
                println!(
                    "#{:<6} {:.7}  {}",
                    p["number"].as_u64().unwrap_or(0),
                    sha,
                    p["ref"].as_str().unwrap_or("?"),
                );
            }
        }
        Some("adopt") => {
            let dir = dir_arg("rex repo <domain> adopt <dir> [--theme]");
            request("repo.adopt", json!({ "id": id, "dir": dir, "theme": theme }));
            let st = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&st);
            }
            println!(
                "adopted {dir} — branch {}, remote {} (metadata only; nothing on disk changed)",
                st["branch"].as_str().unwrap_or("?"),
                st["remote"].as_str().unwrap_or("(none)"),
            );
        }
        Some("link") => {
            let raw = dir_arg("rex repo <domain> link <path> [--name N] [--theme]");
            let target = match std::fs::canonicalize(&raw) {
                Ok(t) => t.to_string_lossy().into_owned(),
                Err(e) => {
                    eprintln!("rex: {raw}: {e}");
                    exit(1);
                }
            };
            let name = words
                .windows(2)
                .find(|w| w[0] == "--name")
                .map(|w| w[1].clone());
            let data = request(
                "repo.link",
                json!({ "id": id, "theme": theme, "target": target, "name": name }),
            );
            if json_output {
                return print_json(&data);
            }
            println!(
                "linked as {} ({})",
                data["dirName"].as_str().unwrap_or("?"),
                if data["isGit"] == json!(true) { "git checkout" } else { "not a git repo" },
            );
            if data["wp"]["kind"] == json!("none") {
                println!("note: no plugin/theme header at the folder root — WordPress won't list it until one exists");
            }
            println!("deleting this asset later removes ONLY the link — the folder stays.");
        }
        Some("watch") => match words.get(2).map(String::as_str) {
            Some("list") | None => {
                let data = request("repo.watch.list", json!({ "id": id }));
                if json_output {
                    return print_json(&data);
                }
                match data["watchers"].as_array().filter(|w| !w.is_empty()) {
                    None => println!("no watchers running"),
                    Some(rows) => {
                        for w in rows {
                            println!(
                                "{:<28} {:<12} {}{}",
                                w["dirName"].as_str().unwrap_or("?"),
                                w["script"].as_str().unwrap_or("?"),
                                w["status"].as_str().unwrap_or("?"),
                                w["exit"].as_i64().map(|c| format!(" (code {c})")).unwrap_or_default(),
                            );
                        }
                    }
                }
            }
            Some("start") => {
                let (Some(dir), Some(script)) = (words.get(3), words.get(4)) else {
                    eprintln!("rex: usage: rex repo <domain> watch start <dir> <script> [--theme] [--tail]");
                    exit(1);
                };
                let w = request(
                    "repo.watch.start",
                    json!({ "id": id, "dir": dir, "script": script, "theme": theme }),
                );
                if json_output {
                    return print_json(&w);
                }
                println!(
                    "watching {dir} — {script} (runs inside the app; output in the app panel \
                     and logs/repo-*-watch.log; stops when the app quits, never auto-restarts)"
                );
                if words.iter().any(|x| x == "--tail") {
                    follow_watch_log(&w, dir);
                }
            }
            // Follow a watcher that is ALREADY running — the same view
            // `start --tail` gives, for the common case where the watcher was
            // started from the app and the terminal wants to see it.
            Some("tail") => {
                let Some(dir) = words.get(3) else {
                    eprintln!("rex: usage: rex repo <domain> watch tail <dir>");
                    exit(1);
                };
                let data = request("repo.watch.list", json!({ "id": id }));
                let found = data["watchers"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|w| w["dirName"] == json!(dir.as_str())))
                    .cloned();
                let Some(w) = found else {
                    eprintln!(
                        "rex: no watcher running for `{dir}` (see `rex repo <domain> watch list`)"
                    );
                    exit(1);
                };
                follow_watch_log(&w, dir);
            }
            Some("stop") => {
                let Some(dir) = words.get(3) else {
                    eprintln!("rex: usage: rex repo <domain> watch stop <dir> [--theme]");
                    exit(1);
                };
                request("repo.watch.stop", json!({ "id": id, "dir": dir, "theme": theme }));
                if !json_output {
                    println!("stopped watching {dir}");
                }
            }
            _ => {
                eprintln!(
                    "rex: usage: rex repo <domain> watch list | start <dir> <script> [--tail] | \
                     tail <dir> | stop <dir>"
                );
                exit(1);
            }
        },
        Some("add") => {
            let url = dir_arg("rex repo <domain> add <url> [--branch B] [--name N] [--theme] [--install]");
            let flag_val = |flag: &str| {
                words.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
            };
            let install = words.iter().any(|w| w == "--install");
            let data = request_long(
                "repo.add",
                json!({
                    "id": id, "theme": theme, "url": url,
                    "branch": flag_val("--branch"), "name": flag_val("--name"),
                    "install": install,
                }),
                &format!("cloning {url}{}", if install { " + installing" } else { "" }),
            );
            print_repo_job(&data, json_output);
        }
        Some(op @ ("pull" | "fetch" | "checkout" | "push")) => {
            let usage = format!("rex repo <domain> {op} <dir> {}[--theme]",
                if op == "checkout" { "<ref> " } else { "" });
            let dir = dir_arg(&usage);
            let target_ref = if op == "checkout" {
                match rest.get(1) {
                    Some(r) => Some((*r).clone()),
                    None => {
                        eprintln!("rex: usage: {usage}");
                        exit(1);
                    }
                }
            } else {
                None
            };
            let install = words.iter().any(|w| w == "--install");
            let data = request_long(
                "repo.op",
                json!({
                    "id": id, "theme": theme, "dir": dir, "op": op,
                    "ref": target_ref, "install": install,
                }),
                &format!("git {op} in {dir}"),
            );
            print_repo_job(&data, json_output);
        }
        Some("run") => {
            let usage = "rex repo <domain> run <dir> <script> [--theme]";
            let dir = dir_arg(usage);
            let Some(script) = rest.get(1) else {
                eprintln!("rex: usage: {usage}");
                exit(1);
            };
            let data = request_long(
                "repo.run",
                json!({ "id": id, "theme": theme, "dir": dir, "script": script }),
                &format!("running {script} in {dir}"),
            );
            print_repo_job(&data, json_output);
        }
        Some("delete") => {
            // Alias over the ALREADY-GUARDED wp delete (the same
            // wp_plugin_delete/wp_theme_delete fns carrying the unlink-only
            // symlink interception) — plus the UI's loss-warning preview.
            let dir = dir_arg("rex repo <domain> delete <dir> [--theme] [--yes]");
            let st = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            let preview = if st["linkTarget"].as_str().is_some() {
                format!(
                    "LINKED folder — removes only the link; {} stays untouched.",
                    st["linkTarget"].as_str().unwrap_or("your folder")
                )
            } else if let Some(w) = st["lossWarning"].as_str() {
                w.to_string()
            } else {
                "clean and pushed — nothing at risk.".to_string()
            };
            eprintln!("{dir}: {preview}");
            if !words.iter().any(|w| w == "--yes") {
                eprint!("delete this {}? [y/N] ", if theme { "theme" } else { "plugin" });
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err()
                    || !matches!(a.trim(), "y" | "Y" | "yes")
                {
                    eprintln!("aborted");
                    exit(1);
                }
            }
            let key = if theme { "wp.theme.delete" } else { "wp.plugin.delete" };
            request(key, json!({ "id": id, "names": [dir] }));
            if !json_output {
                println!("deleted {dir}");
            }
        }
        _ => {
            eprintln!("rex: usage: {REPO_USAGE}");
            exit(1);
        }
    }
}

// ── shell completions ────────────────────────────────────────────────────────

/// Static word completion (subcommand tree only — domains change too often to
/// bake in; a dynamic version can call `rex site list --json` later).
/// zsh:  rex completions zsh  > ~/.zfunc/_rex   (with ~/.zfunc in $fpath)
/// bash: rex completions bash > /usr/local/etc/bash_completion.d/rex
fn cmd_completions(shell: Option<&str>) {
    const TOP: &str = "status start stop restart site wp repo php db service logs doctor mail tunnel tld blueprints config version completions help";
    const SITE: &str = "list create delete info open login logs php xdebug server restart start stop domains rename domain move relink retry env cert";
    const DB: &str = "export import reset versions browse";
    const PHP: &str = "list default install uninstall settings";
    const WPA: &str = "plugin theme user search-replace cache-flush cron maintenance core";
    const REPO: &str =
        "list status branches prs check adopt link watch add pull fetch checkout push run delete";
    match shell {
        Some("zsh") => println!(
            "#compdef rex\n\
             local -a words2\n\
             case $CURRENT in\n\
             2) compadd {TOP} ;;\n\
             3) case $words[2] in\n\
                site) compadd {SITE} ;;\n\
                db) compadd {DB} ;;\n\
                php) compadd {PHP} ;;\n\
                service) compadd start stop restart ;;\n\
                mail) compadd list open mark-read clear ;;\n\
                tunnel) compadd list start stop ;;\n\
                completions) compadd zsh bash ;;\n\
                repo) compadd tools ;;\n\
                esac ;;\n\
             4) case $words[2] in wp) compadd {WPA} ;; repo) compadd {REPO} ;; esac ;;\n\
             esac"
        ),
        Some("bash") => println!(
            "_rex() {{\n\
             local cur=${{COMP_WORDS[COMP_CWORD]}}\n\
             case $COMP_CWORD in\n\
             1) COMPREPLY=($(compgen -W \"{TOP}\" -- \"$cur\")) ;;\n\
             2) case ${{COMP_WORDS[1]}} in\n\
                site) COMPREPLY=($(compgen -W \"{SITE}\" -- \"$cur\")) ;;\n\
                db) COMPREPLY=($(compgen -W \"{DB}\" -- \"$cur\")) ;;\n\
                php) COMPREPLY=($(compgen -W \"{PHP}\" -- \"$cur\")) ;;\n\
                service) COMPREPLY=($(compgen -W \"start stop restart\" -- \"$cur\")) ;;\n\
                mail) COMPREPLY=($(compgen -W \"list open mark-read clear\" -- \"$cur\")) ;;\n\
                tunnel) COMPREPLY=($(compgen -W \"list start stop\" -- \"$cur\")) ;;\n\
                completions) COMPREPLY=($(compgen -W \"zsh bash\" -- \"$cur\")) ;;\n\
                repo) COMPREPLY=($(compgen -W \"tools\" -- \"$cur\")) ;;\n\
                esac ;;\n\
             3) case ${{COMP_WORDS[1]}} in wp) COMPREPLY=($(compgen -W \"{WPA}\" -- \"$cur\")) ;; repo) COMPREPLY=($(compgen -W \"{REPO}\" -- \"$cur\")) ;; esac ;;\n\
             esac\n\
             }}\n\
             complete -F _rex rex"
        ),
        _ => {
            eprintln!("rex: usage: rex completions zsh|bash");
            exit(1);
        }
    }
}

// ── site settings: server / rename / domain / move / env / cert ─────────────

fn cmd_site_server(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site server <domain> nginx|frankenphp|apache");
    let Some(server) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site server <domain> nginx|frankenphp|apache");
        exit(1);
    };
    let updated = request("site.server", json!({ "id": site["id"], "server": server }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

/// `rex site restart <domain> [--pool]`.
///
/// The output is deliberately not a cheerful "restarted": most sites have no
/// process of their own (shared nginx + a pool shared with every site on that
/// PHP minor), so the honest report is what was actually done and what a pool
/// restart would cost — which is why `--pool` exists and is not the default.
/// `rex site domains <domain> [--add <name> | --remove <name>]`.
///
/// One verb for read and write because it is one question from the user's side
/// — which names does this site answer on — and the reply is always the whole
/// list, so an add or a remove shows its own result rather than a bare "ok".
fn cmd_site_domains(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site domains <domain> [--add <name> | --remove <name>]");
    let flag = |name: &str| {
        words.iter().position(|w| w == name).and_then(|i| words.get(i + 1)).cloned()
    };
    let (add, remove) = (flag("--add"), flag("--remove"));
    if add.is_some() && remove.is_some() {
        eprintln!("rex: pass --add or --remove, not both");
        exit(1);
    }
    let r = request(
        "site.domains",
        json!({ "id": site["id"], "add": add, "remove": remove }),
    );
    if json_output {
        return print_json(&r);
    }
    let Some(domains) = r["domains"].as_array() else { return };
    // The primary is first, and saying so is the point: the extras are names
    // this site ALSO answers on, not equals of the one its files, database and
    // certificate directory are named for.
    for (i, d) in domains.iter().enumerate() {
        let name = d.as_str().unwrap_or("?");
        match i {
            0 => println!("https://{name}   (primary)"),
            _ => println!("https://{name}"),
        }
    }
    // WordPress decides its own canonical address from `siteurl`, so an extra
    // domain REACHES the site and is then redirected to the primary. Printed
    // only when there is an extra name to be redirected, and only for
    // WordPress: a note about a thing that cannot happen is noise, and noise is
    // how the useful notes stop being read.
    if domains.len() > 1 && site["type"] == json!("wordpress") {
        let primary = domains.first().and_then(Value::as_str).unwrap_or("?");
        println!(
            "\nnote: WordPress sends visitors to {primary} — the extra names reach this site \
             and then redirect there."
        );
    }
}

/// `rex site start|stop <domain>` (v44).
///
/// Named `stop` rather than `disable` because it is the same verb the app's own
/// button uses, and a CLI that renames the app's actions makes the two feel like
/// different features. What it does not share with `rex stop` — which stops the
/// whole stack — is said in the output every time, because the two words are one
/// argument apart and the mistake is silent otherwise.
fn cmd_site_enabled(words: &[String], enabled: bool, json_output: bool) {
    let verb = if enabled { "start" } else { "stop" };
    // `--all` is the Sites page's bulk switch, NOT `rex stop`: every site's
    // serving surface changes and rexenv's services stay up. Handled before
    // `find_site`, which would otherwise demand a domain.
    if words.iter().any(|w| w == "--all") {
        let r = request("sites.enabled", json!({ "enabled": enabled }));
        if json_output {
            return print_json(&r);
        }
        let total = r["total"].as_u64().unwrap_or(0);
        let changed = r["changed"].as_u64().unwrap_or(0);
        let skipped = r["skippedUnprovisioned"].as_u64().unwrap_or(0);
        println!(
            "{total} site(s) {} now ({changed} changed) — rexenv's services were not touched",
            if enabled { "served" } else { "stopped" },
        );
        if skipped > 0 {
            println!(
                "{skipped} site(s) skipped: their setup never finished (rex site retry <domain>)"
            );
        }
        if let Some(note) = r["note"].as_str() {
            println!("{note}");
        }
        return;
    }
    let site = find_site(words, &format!("rex site {verb} <domain>"));
    let r = request("site.enabled", json!({ "id": site["id"], "enabled": enabled }));
    if json_output {
        return print_json(&r);
    }
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    // The backend's own words when it has something to say (a started site that
    // still is not serving); otherwise the fact plus its blast radius.
    if let Some(note) = r["note"].as_str() {
        println!("{note}");
        return;
    }
    if enabled {
        println!("started {domain} — https://{domain} is serving again");
    } else {
        println!(
            "stopped {domain} — https://{domain} now answers \"site stopped\"\n\
             your other sites keep running (this is not `rex stop`, which stops the whole stack)"
        );
    }
}

fn cmd_site_restart(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site restart <domain> [--pool]");
    let pool = words.iter().any(|w| w == "--pool");
    let r = request("site.restart", json!({ "id": site["id"], "pool": pool }));
    if json_output {
        return print_json(&r);
    }
    let str_of = |v: &Value| v.as_str().unwrap_or("?").to_string();
    let domain = str_of(&site["domain"]);
    let minor = str_of(&r["phpMinor"]);
    let on_pool = r["sitesOnPool"].as_u64().unwrap_or(0);
    match r["kind"].as_str().unwrap_or("") {
        "backend" => println!(
            "restarted {} for {domain} on 127.0.0.1:{}",
            str_of(&r["server"]),
            r["port"].as_u64().unwrap_or(0)
        ),
        "refused" => {
            eprintln!(
                "rex: {domain}'s {} backend was adopted from another session and this process \
                 may not stop it — restart it from the app",
                str_of(&r["server"])
            );
            exit(1);
        }
        _ => {
            println!("reloaded {domain}: config rebuilt, nginx + edge reloaded");
            println!(
                "  {domain} has no backend of its own — it is served by the shared nginx and \
                 the php-{minor} pool, which {on_pool} site(s) share"
            );
        }
    }
    if r["poolRestarted"].as_bool().unwrap_or(false) {
        println!("  restarted the php-{minor} pool ({on_pool} site(s) affected)");
    } else if on_pool > 0 {
        println!("  add --pool to bounce the php-{minor} pool as well ({on_pool} site(s) affected)");
    }
}

fn cmd_site_rename(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site rename <domain> <name>");
    let name = words[1..].iter().filter(|w| !w.starts_with("--")).cloned().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        eprintln!("rex: usage: rex site rename <domain> <name>");
        exit(1);
    }
    let r = request("site.rename", json!({ "id": site["id"], "name": name }));
    if json_output {
        return print_json(&r);
    }
    println!("✓ {} is now named “{name}”", site["domain"].as_str().unwrap_or("?"));
}

fn cmd_site_domain(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site domain <domain> <new-domain> [--yes]");
    let old = site["domain"].as_str().unwrap_or("?").to_string();
    let Some(new_domain) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site domain <domain> <new-domain> [--yes]");
        exit(1);
    };
    if !words.iter().any(|w| w == "--yes") {
        eprint!(
            "change {old} → {new_domain}? WordPress URLs are rewritten across the \
             database (a backup is taken first). [y/N] "
        );
        let mut a = String::new();
        if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
            eprintln!("aborted (domain unchanged)");
            exit(1);
        }
    }
    let r = request("site.domain", json!({ "id": site["id"], "domain": new_domain }));
    if json_output {
        return print_json(&r);
    }
    println!(
        "✓ {old} → https://{} ({} URL replacement{}{})",
        r["site"]["domain"].as_str().unwrap_or(new_domain),
        r["replacements"].as_u64().unwrap_or(0),
        if r["replacements"] == json!(1) { "" } else { "s" },
        r["backup_path"]
            .as_str()
            .or(r["backupPath"].as_str())
            .map(|p| format!("; backup: {p}"))
            .unwrap_or_default(),
    );
}

fn cmd_site_move(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site move <domain> <dest-parent>");
    let Some(dest) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site move <domain> <dest-parent>");
        exit(1);
    };
    let dest = match std::fs::canonicalize(dest) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rex: cannot use {dest}: {e}");
            exit(1);
        }
    };
    let r = request(
        "site.move",
        json!({ "id": site["id"], "destParent": dest.to_string_lossy() }),
    );
    if json_output {
        return print_json(&r);
    }
    println!("✓ moved → {}", r["path"].as_str().unwrap_or("?"));
}

/// `rex site relink <domain> <path>` — re-point a LINKED or imported site at a
/// folder the user moved themselves.
///
/// Not the same command as `site move`, and the difference is the whole reason
/// this exists: `move` relocates a docroot rexenv owns (copy, then delete), and
/// it REFUSES a linked folder because that folder is the user's and never ours
/// to delete. `relink` records the new location and reloads the config; it
/// touches no file, so a wrong path costs nothing but a second run.
fn cmd_site_relink(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site relink <domain> <path>");
    let Some(path) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site relink <domain> <path>");
        exit(1);
    };
    // Canonicalised HERE as well as backend-side: the backend canonicalises the
    // path it stores, and resolving it first means a relative path typed at a
    // shell prompt (`rex site relink x.rex ./moved`) means what the user's cwd
    // says it means, not what the app's does.
    let path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rex: cannot use {path}: {e}");
            exit(1);
        }
    };
    let r = request("site.relink", json!({ "id": site["id"], "path": path.to_string_lossy() }));
    if json_output {
        return print_json(&r);
    }
    println!("✓ now serving from {}", r["path"].as_str().unwrap_or("?"));
}

/// `rex site retry <domain>` — finish a site whose provisioning stopped part-way.
///
/// The CLI equivalent of the app's "setup incomplete" Retry: a site whose row
/// exists with `provisioned = 0` because provisioning stopped part-way. The app
/// has offered that button since 24 Jul; the CLI had nothing.
///
/// NOT claimed: that `site create`'s failure text points here. `docs/CLI-ROADMAP.md`
/// says so and nothing in the tree matches — it is the roadmap's sentence, left
/// as the roadmap's.
///
/// Blocks for the whole run, like `site create` does: the app streams phases to
/// its own card, and the socket has no read timeout precisely so a long
/// provision can finish on it.
fn cmd_site_retry(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site retry <domain>");
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    eprintln!("retrying {domain}… (downloads and installs may take a minute)");
    let r = if json_output {
        request("site.retry", json!({ "id": site["id"] }))
    } else {
        request_streaming("site.retry", json!({ "id": site["id"] }), print_provision_progress)
    };
    if json_output {
        return print_json(&r);
    }
    let status = r["status"].as_str().unwrap_or("?");
    // Report what the job SAYS, never a cheerful default: a retry that failed
    // again is the case this command exists for, and it has to be readable.
    match status {
        "ok" => println!("✓ {domain} finished provisioning"),
        "running" => println!(
            "… {domain} is still running after the wait — open rexenv to watch it, or check {}",
            r["logKey"].as_str().unwrap_or("the provision log")
        ),
        other => {
            let phase = r["phases"]
                .as_array()
                .and_then(|p| p.iter().find(|x| x["status"] == json!("failed")))
                .and_then(|x| x["label"].as_str())
                .unwrap_or("?");
            eprintln!(
                "rex: {domain} {other} at phase `{phase}`{}",
                r["error"].as_str().map(|e| format!(" — {e}")).unwrap_or_default()
            );
            exit(1);
        }
    }
}

fn cmd_site_env(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site env <domain> [set K=V | unset K]");
    let id = site["id"].clone();
    let fetch = || -> Vec<(String, String)> {
        request("site.env", json!({ "id": id }))["vars"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|e| {
                        Some((e["name"].as_str()?.to_string(), e["value"].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    match words.get(1).map(String::as_str) {
        None => {
            if json_output {
                return print_json(&request("site.env", json!({ "id": id })));
            }
            let vars = fetch();
            if vars.is_empty() {
                return println!("(no env vars)");
            }
            for (k, v) in vars {
                println!("{k}={v}");
            }
        }
        // The backend replaces the whole set — merge client-side.
        Some("set") => {
            let Some((k, v)) = words.get(2).and_then(|kv| kv.split_once('=')) else {
                eprintln!("rex: usage: rex site env <domain> set KEY=value");
                exit(1);
            };
            let mut vars = fetch();
            vars.retain(|(name, _)| name != k);
            vars.push((k.to_string(), v.to_string()));
            let payload: Vec<Value> =
                vars.iter().map(|(n, val)| json!({ "name": n, "value": val })).collect();
            let r = request("site.env.set", json!({ "id": id, "vars": payload }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {k}={v} (site backend reloaded)");
        }
        Some("unset") => {
            let Some(k) = words.get(2) else {
                eprintln!("rex: usage: rex site env <domain> unset KEY");
                exit(1);
            };
            let mut vars = fetch();
            let before = vars.len();
            vars.retain(|(name, _)| name != k);
            if vars.len() == before {
                eprintln!("rex: no env var `{k}` on this site");
                exit(1);
            }
            let payload: Vec<Value> =
                vars.iter().map(|(n, val)| json!({ "name": n, "value": val })).collect();
            let r = request("site.env.set", json!({ "id": id, "vars": payload }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ removed {k}");
        }
        _ => {
            eprintln!("rex: usage: rex site env <domain> [set K=V | unset K]");
            exit(1);
        }
    }
}

fn cmd_site_cert(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site cert <domain> [--regenerate]");
    if words.iter().any(|w| w == "--regenerate") {
        let r = request("site.cert.regenerate", json!({ "id": site["id"] }));
        if json_output {
            return print_json(&r);
        }
        println!("✓ fresh certificate issued (edge reloaded)");
        return;
    }
    let data = request("site.cert", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    if data.is_null() {
        return println!("no certificate yet (issued on first serve)");
    }
    println!(
        "expires {} ({} days left)\nSANs: {}",
        data["notAfter"].as_str().unwrap_or("?"),
        data["daysLeft"].as_i64().unwrap_or(0),
        data["sans"]
            .as_array()
            .map(|s| s.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
    );
}

// ── service / mail / tunnel / tld / version ──────────────────────────────────

fn cmd_service(words: &[String], json_output: bool) {
    let (action, name) = (words.first().map(String::as_str), words.get(1).map(String::as_str));
    // The web tier takes RESTART only, and start/stop refuse it by name rather
    // than by silence: a stopped nginx is every default site 502-ing with
    // nothing on screen to explain it, so stopping the stack is `rex stop`.
    if let (Some(action @ ("start" | "stop")), Some(name @ ("nginx" | "edge" | "caddy"))) =
        (action, name)
    {
        eprintln!(
            "rex: the web tier has no single-service {action} — {name} serves every site on it. \
             Use `rex service restart {name}` to bounce it on a fresh config, or `rex {action}` \
             for the whole stack."
        );
        exit(1);
    }
    if action == Some("restart") {
        let Some(target) = name else {
            eprintln!("rex: usage: rex service restart <nginx|edge|php-8.3>");
            exit(1);
        };
        let r = request("service.restart", json!({ "target": target }));
        if json_output {
            return print_json(&r);
        }
        let service = r["service"].as_str().unwrap_or(target);
        match r["outcome"].as_str().unwrap_or("") {
            "restarted" => println!("✓ {service} restarted on a freshly generated config"),
            "reloaded" => println!(
                "✓ {service} reloaded (new config live). The edge is a supervised root daemon — \
                 a true restart is Stop all → Start in the app."
            ),
            "notRunning" => {
                eprintln!("rex: {service} is not running — `rex start` brings the stack up in order");
                exit(1);
            }
            other => {
                eprintln!("rex: {service} was not restarted ({other})");
                exit(1);
            }
        }
        return;
    }
    let (Some(action @ ("start" | "stop")), Some(name)) = (action, name) else {
        eprintln!(
            "rex: usage: rex service start|stop <mysql|mariadb|postgres|redis|mailpit>\n                    rex service restart <nginx|edge|php-8.3>"
        );
        exit(1);
    };
    let running = action == "start";
    let r = if name == "mailpit" {
        request("service.mail", json!({ "running": running }))
    } else {
        request("service.db", json!({ "key": name, "running": running }))
    };
    if json_output {
        return print_json(&r);
    }
    println!("✓ {name} {}", if running { "started" } else { "stopped" });
}

/// `rex config get|set` — the settings the CLI is allowed to touch.
///
/// The allow-list is NOT here. `core::settings_access` rules on each key and the
/// server enforces it, because the same list is read by the guard that checks a
/// writable key is either validated or explicitly justified. A copy in the CLI
/// would be a second opinion about a security boundary.
///
/// So this command deliberately does no filtering of its own: it sends the key
/// and prints what comes back, refusal included. The refusals say WHY — the
/// signed update chain, a version pin, the agent socket's consent toggle — and
/// that sentence is the useful half.
fn cmd_config(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        Some("get") => {
            let Some(key) = words.get(1) else {
                eprintln!("rex: usage: rex config get <key>");
                exit(1);
            };
            let r = request("config.get", json!({ "key": key }));
            if json_output {
                return print_json(&r);
            }
            match r["value"].as_str() {
                // An unset key is not an error: `sites_dir` empty means "the
                // default", and printing nothing says that better than a fake.
                Some(v) => println!("{v}"),
                None => eprintln!("rex: {key} is not set"),
            }
        }
        Some("set") => {
            let (Some(key), Some(value)) = (words.get(1), words.get(2)) else {
                eprintln!("rex: usage: rex config set <key> <value>");
                exit(1);
            };
            let r = request("config.set", json!({ "key": key, "value": value }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {key} = {value}");
        }
        _ => {
            eprintln!("rex: usage: rex config get <key> | rex config set <key> <value>");
            exit(1);
        }
    }
}

fn cmd_mail(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        None | Some("list") => {
            // `mail.list` has always taken a search term and an unread filter —
            // the UI's own inbox search uses them — and this sent `Null`, so
            // both were unreachable from the CLI. Same shape as `mail.mark_read`
            // below: a dispatch arm answering something nothing asked.
            let rest: Vec<&String> = words.iter().skip(1).filter(|w| !w.starts_with("--")).collect();
            let unread = words.iter().any(|w| w == "--unread");
            let query = rest.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            let mut args = json!({});
            if !query.is_empty() {
                args["query"] = json!(query);
            }
            // Absent, not `false`: the server treats a missing `unread` as "the
            // whole inbox", and sending `false` explicitly would mean the same
            // thing today while making the CLI depend on it.
            if unread {
                args["unread"] = json!(true);
            }
            let data = request("mail.list", args);
            if json_output {
                return print_json(&data);
            }
            let (total, unread) = (data["total"].as_i64().unwrap_or(0), data["unread"].as_i64().unwrap_or(0));
            let shown = data["messages"].as_array().map(Vec::len).unwrap_or(0) as i64;
            // The counts describe the MAILBOX; the rows below are what the
            // filter left. Saying only "3 messages (2 unread)" above two rows
            // reads as a bug in the listing — observed on the first live run of
            // `--unread`, which is what a filter with no label always looks like.
            if shown < total {
                println!(
                    "{shown} of {total} message{} shown ({unread} unread in the mailbox)",
                    if total == 1 { "" } else { "s" }
                );
            } else {
                println!("{total} message{} ({unread} unread)", if total == 1 { "" } else { "s" });
            }
            for m in data["messages"].as_array().map(Vec::as_slice).unwrap_or_default() {
                println!(
                    "{} {:<28} {}",
                    if m["read"] == json!(true) { " " } else { "•" },
                    m["from"]["address"].as_str().unwrap_or("?"),
                    m["subject"].as_str().unwrap_or(""),
                );
            }
        }
        Some("open") => {
            let status = request("mail.status", Value::Null);
            match status["uiUrl"].as_str().filter(|u| !u.is_empty()) {
                Some(url) => open_url(url),
                None => {
                    let port = status["httpPort"].as_u64().unwrap_or(18025);
                    open_url(&format!("http://127.0.0.1:{port}"));
                }
            }
        }
        Some("clear") => {
            if !words.iter().any(|w| w == "--yes") {
                eprint!("delete ALL caught messages? [y/N] ");
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted");
                    exit(1);
                }
            }
            let r = request("mail.clear", Value::Null);
            if json_output {
                return print_json(&r);
            }
            println!("✓ mailbox cleared");
        }
        Some("mark-read") => {
            // The last unreachable arm of the whole dispatch table: the server
            // has answered `mail.mark_read` since the mail feature shipped and
            // no `rex` verb sent it (docs/TODO.md, 21 Aug 2026 reconcile).
            //
            // Named `mark-read` rather than `read` because it marks EVERY
            // message — `rex mail read` would read as "show me one".
            let r = request("mail.mark_read", Value::Null);
            if json_output {
                return print_json(&r);
            }
            println!("✓ all messages marked read");
        }
        _ => {
            eprintln!("rex: usage: rex mail [list [--unread] [query] | open | mark-read | clear]");
            exit(1);
        }
    }
}

fn cmd_tunnel(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        None | Some("list") => {
            let data = request("tunnel.list", Value::Null);
            if json_output {
                return print_json(&data);
            }
            let tunnels = data["tunnels"].as_array().map(Vec::as_slice).unwrap_or_default();
            if tunnels.is_empty() {
                return println!("no public tunnels running");
            }
            for t in tunnels {
                println!("{:<24} {}", t["domain"].as_str().unwrap_or("?"), t["url"].as_str().unwrap_or(""));
                if let Some(w) = t["warning"].as_str() {
                    println!("{:<24} warning: {w}", "");
                }
            }
        }
        Some(act @ ("start" | "stop")) => {
            let site = find_site(&words[1..], "rex tunnel start|stop <domain>");
            if act == "start" && !words.iter().any(|w| w == "--yes") {
                eprint!(
                    "expose {} PUBLICLY via a cloudflared tunnel? [y/N] ",
                    site["domain"].as_str().unwrap_or("?")
                );
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted (nothing exposed)");
                    exit(1);
                }
            }
            let r = request(&format!("tunnel.{act}"), json!({ "id": site["id"] }));
            if json_output {
                return print_json(&r);
            }
            if act == "start" {
                println!("✓ public URL: {}", r["url"].as_str().unwrap_or("?"));
                // A share of a STOPPED site is allowed and never silent: the
                // link works, and what visitors get is rexenv's stop page. The
                // sentence is the app's own (one source, `core::tunnels`).
                if let Some(w) = r["warning"].as_str() {
                    println!("warning: {w}");
                }
            } else {
                println!("✓ tunnel stopped");
            }
        }
        _ => {
            eprintln!("rex: usage: rex tunnel [list|start <domain>|stop <domain>]");
            exit(1);
        }
    }
}

fn cmd_tld(words: &[String], json_output: bool) {
    // `--repair` puts back an OS resolver file for a TLD your sites answer on:
    // the fix `rex doctor` names when it finds one missing. Separate from
    // `--set`, which only decides what NEW sites are called — conflating them
    // would make "change my default" quietly install a system file.
    if let Some(tld) = flag_value(words, "--repair") {
        let r = request("tld.repair", json!({ "tld": tld }));
        if json_output {
            return print_json(&r);
        }
        return println!(
            "✓ .{} resolves here again — sites on it should load now",
            r["tld"].as_str().unwrap_or(&tld)
        );
    }
    // `--remove` takes a resolver file back OUT — ours only, and only for a
    // TLD no site answers on. Nothing does this automatically (a site delete
    // must not raise a password prompt for housekeeping), so it is a verb.
    if let Some(tld) = flag_value(words, "--remove") {
        let r = request("tld.remove", json!({ "tld": tld }));
        if json_output {
            return print_json(&r);
        }
        let tld = tld.trim_start_matches('.');
        return if r["removed"] == json!(true) {
            println!("✓ removed the resolver file for .{tld} — nothing here answered on it")
        } else {
            println!("nothing to remove — there was no resolver file for .{tld}")
        };
    }
    if let Some(tld) = flag_value(words, "--set") {
        let r = request("tld.set", json!({ "tld": tld }));
        if json_output {
            return print_json(&r);
        }
        return println!("✓ new sites default to .{tld}");
    }
    let data = request("tld.get", Value::Null);
    if json_output {
        return print_json(&data);
    }
    println!(".{}", data["tld"].as_str().unwrap_or("?"));
}

fn cmd_version(json_output: bool) {
    let data = request("version", Value::Null);
    if json_output {
        return print_json(&json!({ "app": data, "cli": env!("CARGO_PKG_VERSION") }));
    }
    // The commit is the point: "is the running app the code I just changed?"
    // should be one command, not a forensic exercise.
    println!(
        "rexenv {} ({}) · rex {}\n  app built {} from {}\n  cli built {} from {}",
        data["version"].as_str().unwrap_or("?"),
        data["platform"].as_str().unwrap_or("?"),
        env!("CARGO_PKG_VERSION"),
        data["builtAt"].as_str().unwrap_or("?"),
        data["commit"].as_str().unwrap_or("?"),
        env!("REX_BUILT_AT"),
        env!("REX_GIT_COMMIT"),
    );
    // Printed HERE rather than left for the reader to spot: this command
    // already shows both numbers, and two builds side by side are only
    // obviously different to someone who was looking for a difference.
    //
    // Routed through the SAME comparison as the unknown-command path rather than
    // re-testing `version` here. The first cut of this did compare versions
    // inline and stayed silent on a commit mismatch — in the command whose whole
    // stated purpose is "is the running app the code I just changed?".
    if let Some(skew) = version_skew_between(
        env!("CARGO_PKG_VERSION"),
        data["version"].as_str().unwrap_or(""),
        env!("REX_GIT_COMMIT"),
        data["commit"].as_str().unwrap_or(""),
    ) {
        eprintln!("\nrex: {skew}");
    }
}

// ── wp: plugins / themes / users ─────────────────────────────────────────────

const WP_USAGE: &str = "rex wp <domain> <plugin|theme|user> <action> …";

/// 16-char password from /dev/urandom — generated and PRINTED ONCE instead of
/// ever accepting one on argv (argv is world-readable via ps).
fn generate_password() -> String {
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 16];
    if std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").expect("urandom"),
        &mut bytes,
    )
    .is_err()
    {
        eprintln!("rex: could not read /dev/urandom");
        exit(1);
    }
    bytes.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect()
}

fn cmd_wp(words: &[String], json_output: bool) {
    let site = find_site(words, WP_USAGE);
    if site["type"] != json!("wordpress") {
        eprintln!("rex: `{}` is not a WordPress site", site["domain"].as_str().unwrap_or("?"));
        exit(1);
    }
    let id = site["id"].clone();
    let (area, action) = (words.get(1).map(String::as_str), words.get(2).map(String::as_str));
    let rest: Vec<String> =
        words.iter().skip(3).filter(|w| !w.starts_with("--")).cloned().collect();
    let activate = words.iter().any(|w| w == "--activate");
    match (area, action) {
        (Some("plugin"), Some("list")) | (Some("plugin"), None) => {
            let data = request("wp.plugins", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["plugins"].as_array() else { return println!("(none)") };
            for p in rows {
                println!(
                    "{:<32} {:<9} {:<10} {}",
                    p["name"].as_str().unwrap_or("?"),
                    p["status"].as_str().unwrap_or(""),
                    p["version"].as_str().unwrap_or(""),
                    if p["update"] == json!("available") { "update available" } else { "" },
                );
            }
        }
        (Some("plugin"), Some("install")) => {
            let Some(slug) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> plugin install <slug> [--activate]");
                exit(1);
            };
            let r = request("wp.plugin.install", json!({ "id": id, "slug": slug, "activate": activate }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ installed {slug}{}", if activate { " (activated)" } else { "" });
        }
        (Some("plugin"), Some(act @ ("activate" | "deactivate" | "update" | "delete"))) => {
            if rest.is_empty() {
                eprintln!("rex: usage: rex wp <domain> plugin {act} <name…>");
                exit(1);
            }
            let r = request(&format!("wp.plugin.{act}"), json!({ "id": id, "names": rest }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {act}d: {}", rest.join(", "));
        }
        (Some("theme"), Some("list")) | (Some("theme"), None) => {
            let data = request("wp.themes", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["themes"].as_array() else { return println!("(none)") };
            for t in rows {
                println!(
                    "{:<32} {:<9} {:<10} {}",
                    t["name"].as_str().unwrap_or("?"),
                    t["status"].as_str().unwrap_or(""),
                    t["version"].as_str().unwrap_or(""),
                    if t["update"] == json!("available") { "update available" } else { "" },
                );
            }
        }
        (Some("theme"), Some("install")) => {
            let Some(slug) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> theme install <slug> [--activate]");
                exit(1);
            };
            let r = request("wp.theme.install", json!({ "id": id, "slug": slug, "activate": activate }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ installed {slug}{}", if activate { " (activated)" } else { "" });
        }
        (Some("theme"), Some("activate")) => {
            let Some(name) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> theme activate <name>");
                exit(1);
            };
            let r = request("wp.theme.activate", json!({ "id": id, "name": name }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ activated {name}");
        }
        (Some("theme"), Some(act @ ("update" | "delete"))) => {
            if rest.is_empty() {
                eprintln!("rex: usage: rex wp <domain> theme {act} <name…>");
                exit(1);
            }
            let r = request(&format!("wp.theme.{act}"), json!({ "id": id, "names": rest }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {act}d: {}", rest.join(", "));
        }
        (Some("user"), Some("list")) | (Some("user"), None) => {
            let data = request("wp.users", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["users"].as_array() else { return println!("(none)") };
            println!("{:<5} {:<20} {:<30} ROLES", "ID", "LOGIN", "EMAIL");
            for u in rows {
                println!(
                    "{:<5} {:<20} {:<30} {}",
                    u["id"].as_u64().unwrap_or(0),
                    u["login"].as_str().unwrap_or("?"),
                    u["email"].as_str().unwrap_or(""),
                    u["roles"].as_str().unwrap_or(""),
                );
            }
        }
        (Some("user"), Some("create")) => {
            let (Some(login), Some(email)) = (rest.first(), rest.get(1)) else {
                eprintln!("rex: usage: rex wp <domain> user create <login> <email> [--role R]");
                exit(1);
            };
            let role = flag_value(words, "--role").unwrap_or_else(|| "subscriber".into());
            let password = generate_password();
            let r = request(
                "wp.user.create",
                json!({ "id": id, "login": login, "email": email, "role": role, "password": password }),
            );
            if json_output {
                return print_json(&r);
            }
            println!("✓ created {login} ({role})\n  password: {password}   (shown once — store it now)");
        }
        (Some("search-replace"), from_word) => {
            // Grammar: rex wp <domain> search-replace <from> <to> [--dry-run] [--yes]
            const SR_USAGE: &str =
                "rex: usage: rex wp <domain> search-replace <from> <to> [--dry-run] [--yes]";
            // The most dangerous flag set in the CLI: a typo'd `--dry-runn`
            // leaves `dry` false, and a `--yes` beside it removes the question,
            // so a rehearsal becomes a real replace across the database.
            reject_unknown_flags(words, "wp search-replace", &["--dry-run", "--yes"], SR_USAGE);
            let (Some(from), Some(to)) = (from_word, words.get(3).map(String::as_str)) else {
                eprintln!("{SR_USAGE}");
                exit(1);
            };
            // …and the positionals must be VALUES. `search-replace old --dry-run new`
            // otherwise reads `to` as the flag and writes the literal string
            // `--dry-run` across every row it matches.
            if from.starts_with("--") || to.starts_with("--") {
                eprintln!(
                    "rex: `{from}` → `{to}`: a flag cannot be the text to search for or write. \
                     Put <from> and <to> before the flags.\n{SR_USAGE}"
                );
                exit(2);
            }
            let dry = words.iter().any(|w| w == "--dry-run");
            if !dry && !words.iter().any(|w| w == "--yes") {
                eprint!(
                    "replace `{from}` → `{to}` across the database? (tip: --dry-run first, \
                     `rex db export` for a backup) [y/N] "
                );
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted (nothing replaced)");
                    exit(1);
                }
            }
            let r = request(
                "wp.search-replace",
                json!({ "id": id, "from": from, "to": to, "dryRun": dry }),
            );
            if json_output {
                return print_json(&r);
            }
            println!(
                "✓ {} replacement{}{}",
                r["replacements"].as_u64().unwrap_or(0),
                if r["replacements"] == json!(1) { "" } else { "s" },
                if dry { " (dry run — nothing written)" } else { "" },
            );
        }
        (Some("cache-flush"), _) => {
            let r = request("wp.cache-flush", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("cache flushed"));
        }
        (Some("cron"), Some("run")) => {
            let r = request("wp.cron-run", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("due events run"));
        }
        (Some("maintenance"), mode) => {
            let on = match mode {
                Some("on") => Some(true),
                Some("off") => Some(false),
                None => None,
                _ => {
                    eprintln!("rex: usage: rex wp <domain> maintenance [on|off]");
                    exit(1);
                }
            };
            let payload = match on {
                Some(on) => json!({ "id": id, "on": on }),
                None => json!({ "id": id }),
            };
            let r = request("wp.maintenance", payload);
            if json_output {
                return print_json(&r);
            }
            println!("maintenance {}", if r["on"] == json!(true) { "ON" } else { "off" });
        }
        (Some("core"), Some("versions")) => {
            let data = request("wp.core-versions", Value::Null);
            if json_output {
                return print_json(&data);
            }
            for v in data["versions"].as_array().map(Vec::as_slice).unwrap_or_default() {
                println!("{:<10} {}", v["version"].as_str().unwrap_or("?"), v["status"].as_str().unwrap_or(""));
            }
        }
        (Some("core"), Some("switch")) => {
            let Some(version) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> core switch <version>");
                exit(1);
            };
            println!("switching core to {version}… (download + install)");
            let r = request("wp.core-switch", json!({ "id": id, "version": version }));
            if json_output {
                return print_json(&r);
            }
            println!(
                "✓ core is now {}{}",
                r["version"].as_str().unwrap_or(version),
                if r["dbUpdateRequired"] == json!(true) {
                    " — DB update required (open wp-admin once)"
                } else {
                    ""
                },
            );
        }
        (Some("core"), Some("update")) => {
            println!("updating WordPress core… (this can take a minute)");
            let r = request("wp.core-update", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("core updated"));
        }
        (Some("user"), Some("delete")) => {
            let Some(who) = rest.first() else {
                eprintln!(
                    "rex: usage: rex wp <domain> user delete <login|id> --reassign <login|id> \
                     | --delete-posts"
                );
                exit(1);
            };
            // Resolving BOTH ids through the app's own list, so a typo in the
            // reassign target fails here rather than after the account is gone.
            let users = request("wp.users", json!({ "id": id }));
            let resolve = |who: &str| -> u64 {
                who.parse::<u64>().ok().unwrap_or_else(|| {
                    users["users"]
                        .as_array()
                        .and_then(|us| {
                            us.iter().find(|u| u["login"] == json!(who)).and_then(|u| u["id"].as_u64())
                        })
                        .unwrap_or_else(|| {
                            eprintln!("rex: no user `{who}` on this site (see `rex wp … user list`)");
                            exit(1);
                        })
                })
            };
            let user_id = resolve(who);
            // From `words`, NOT `rest`: `rest` is built by filtering every `--`
            // word out (see the top of this fn), so searching it for a flag
            // matches nothing, ever. That made both forks unreadable — the
            // refusal below fired on EVERY call and `wp user delete` shipped
            // uncompletable. The L0 tests held the server's rule and the
            // resolver; nothing exercised this parse, which is what the live
            // leg was for.
            let reassign_to = flag_value(words, "--reassign").map(|w| resolve(&w));
            let delete_posts = words.iter().any(|w| w == "--delete-posts");
            // The fork is the confirmation: deleting a user decides what happens
            // to their POSTS, and neither answer is safe to assume. Refusing here
            // keeps the round trip honest, and the server refuses again anyway.
            if reassign_to.is_some() == delete_posts {
                // BOTH is a different mistake from NEITHER, and one message for
                // the two sent a user who had over-specified looking for the
                // flag they had already typed. Found live, 3 Sep 2026.
                if delete_posts {
                    eprintln!(
                        "rex: `--reassign` and `--delete-posts` are the two answers to the same \
                         question about {who}'s posts — pick one"
                    );
                } else {
                    eprintln!(
                        "rex: say what happens to {who}'s posts: `--reassign <login|id>` to keep \
                         them under another account, or `--delete-posts` to delete them too"
                    );
                }
                exit(1);
            }
            let r = request(
                "wp.user.delete",
                json!({
                    "id": id,
                    "userId": user_id,
                    "reassign": reassign_to,
                    "deletePosts": delete_posts,
                }),
            );
            if json_output {
                return print_json(&r);
            }
            match reassign_to {
                Some(to) => println!("✓ deleted {who}; their posts now belong to user {to}"),
                None => println!("✓ deleted {who} and their posts"),
            }
        }
        (Some("user"), Some(act @ ("set-password" | "set-role"))) => {
            let Some(who) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> user {act} <login|id> [role]");
                exit(1);
            };
            // Accept a login or a numeric id; resolve via the app's own list.
            let user_id = who.parse::<u64>().ok().unwrap_or_else(|| {
                request("wp.users", json!({ "id": id }))["users"]
                    .as_array()
                    .and_then(|users| {
                        users.iter().find(|u| u["login"] == json!(who)).and_then(|u| u["id"].as_u64())
                    })
                    .unwrap_or_else(|| {
                        eprintln!("rex: no user `{who}` on this site (see `rex wp … user list`)");
                        exit(1);
                    })
            });
            if act == "set-password" {
                let password = generate_password();
                let r = request(
                    "wp.user.password",
                    json!({ "id": id, "userId": user_id, "password": password }),
                );
                if json_output {
                    return print_json(&r);
                }
                println!("✓ password reset for {who}\n  password: {password}   (shown once — store it now)");
            } else {
                let Some(role) = rest.get(1) else {
                    eprintln!("rex: usage: rex wp <domain> user set-role <login|id> <role>");
                    exit(1);
                };
                let r = request("wp.user.role", json!({ "id": id, "userId": user_id, "role": role }));
                if json_output {
                    return print_json(&r);
                }
                println!("✓ {who} is now {role}");
            }
        }
        _ => {
            eprintln!("rex: usage: {WP_USAGE}\n\n{USAGE}");
            exit(1);
        }
    }
}

// ── db export / import ───────────────────────────────────────────────────────

fn cmd_db_export(words: &[String], json_output: bool) {
    let site = find_site(words, "rex db export <domain>");
    if !json_output {
        println!("exporting {}…", site["domain"].as_str().unwrap_or("?"));
    }
    let data = request("db.export", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    println!("✓ exported → {}", data["path"].as_str().unwrap_or("?"));
}

fn cmd_db_import(words: &[String], json_output: bool) {
    reject_unknown_flags(
        words,
        "db import",
        &["--yes"],
        "rex: usage: rex db import <domain> <file.sql> [--yes]",
    );
    let site = find_site(words, "rex db import <domain> <file.sql> [--yes]");
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    let Some(file) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex db import <domain> <file.sql> [--yes]");
        exit(1);
    };
    // Absolute path client-side: the APP resolves relative paths against ITS
    // cwd, not this shell's.
    let file = match std::fs::canonicalize(file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rex: cannot read {file}: {e}");
            exit(1);
        }
    };
    if !words.iter().any(|w| w == "--yes") {
        eprint!(
            "import into {domain}? The dump's tables OVERWRITE existing ones \
             (tip: `rex db export {domain}` first). [y/N] "
        );
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err()
            || !matches!(answer.trim(), "y" | "Y" | "yes")
        {
            eprintln!("aborted (nothing imported)");
            exit(1);
        }
    }
    let result = request(
        "db.import",
        json!({ "id": site["id"], "path": file.to_string_lossy() }),
    );
    if json_output {
        return print_json(&result);
    }
    println!("✓ imported {} into {domain}", file.display());
}

// ── db reset / versions ──────────────────────────────────────────────────────

fn cmd_db_reset(words: &[String], json_output: bool) {
    reject_unknown_flags(
        words,
        "db reset",
        &["--confirm"],
        "rex: usage: rex db reset <domain> [--confirm <domain>]",
    );
    let site = find_site(words, "rex db reset <domain>");
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    // Nuclear: drop + reinstall. Typed confirmation (the UI's model), never
    // just --yes; scripts pass --confirm <domain>.
    let confirmed = flag_value(words, "--confirm").is_some_and(|c| c == domain) || {
        eprint!(
            "RESET {domain}? This DROPS the database and reinstalls WordPress.\n\
             Type the domain to confirm: "
        );
        let mut a = String::new();
        std::io::stdin().read_line(&mut a).is_ok() && a.trim() == domain
    };
    if !confirmed {
        eprintln!("aborted (nothing reset)");
        exit(1);
    }
    let r = request("db.reset", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&r);
    }
    println!("✓ {domain} reset — fresh WordPress install");
}

fn cmd_db_versions(words: &[String], json_output: bool) {
    if let (Some(engine), Some(version)) = (
        words.iter().position(|w| w == "--set").and_then(|i| words.get(i + 1)),
        words.iter().position(|w| w == "--set").and_then(|i| words.get(i + 2)),
    ) {
        let r = request("db.version.set", json!({ "key": engine, "version": version }));
        if json_output {
            return print_json(&r);
        }
        return println!("✓ {engine} → {version} (engine restarted if it was running)");
    }
    let data = request("db.versions", Value::Null);
    if json_output {
        return print_json(&data);
    }
    for e in data["engines"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let key = e["key"].as_str().unwrap_or("?");
        let avail = data["available"][key]
            .as_array()
            .map(|v| v.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        println!(
            "{:<10} {:<9} {:<8} available: {avail}",
            key,
            e["version"].as_str().unwrap_or("?"),
            if e["running"] == json!(true) { "running" } else { "idle" },
        );
    }
}

// ── doctor ───────────────────────────────────────────────────────────────────

/// Exit 0 = healthy; exit 1 = at least one finding (scriptable gate).
/// The `Resolvers` line's verdict — `(ok, warn, message)` for `line`.
///
/// THREE states, not two. A field that is ABSENT is not the same fact as an
/// empty list, and collapsing them would make an older app — one whose doctor
/// payload predates this field — report a confident ✓ for a check it never ran.
/// That is the shape this codebase keeps finding: a guard reading as passed
/// because nothing answered. The CLI already knows it can outrun the app it
/// talks to ("this rex may be newer than the running app"), so it says so here
/// too.
fn resolver_verdict(field: Option<&Value>) -> (bool, bool, String) {
    let Some(value) = field.filter(|v| !v.is_null()) else {
        return (
            false,
            true, // a warning: unknown, not broken
            "not reported by this rexenv (an older app) — check rexenv → Import".into(),
        );
    };
    let drifted: Vec<&str> =
        value.as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if drifted.is_empty() {
        return (true, false, "no TLD taken back by another tool".into());
    }
    // Named, and with the move that fixes it — the Import screen is the only
    // place a takeover can be redone; there is no `rex` command for it.
    let names = drifted.iter().map(|t| format!(".{t}")).collect::<Vec<_>>().join(", ");
    (
        false,
        false,
        format!(
            "{names} taken back by Valet or Herd — rexenv sites on {} no longer resolve\n            \
             take {} over again in rexenv → Import, or move those sites to .rex",
            if drifted.len() == 1 { "it" } else { "them" },
            if drifted.len() == 1 { "it" } else { "them" },
        ),
    )
}

fn cmd_doctor(json_output: bool) {
    let data = request("doctor", Value::Null);
    if json_output {
        print_json(&data);
        // json mode still gates the exit code so CI can use it
    }
    let mut findings = 0;
    let mut line = |ok: bool, warn: bool, label: &str, msg: String| {
        let mark = if ok { "✓" } else if warn { "⚠" } else { "✗" };
        if !ok {
            findings += 1;
        }
        if !json_output {
            println!("{mark} {label:<9} {msg}");
        }
    };

    let app = &data["app"];
    if !json_output {
        println!("rexenv {} ({})", app["version"].as_str().unwrap_or("?"), app["platform"].as_str().unwrap_or("?"));
    }

    let dns = &data["dns"];
    let dns_running = dns["running"] == json!(true);
    let mode = dns["mode"].as_str().unwrap_or("down");
    let resolver = dns["resolverInstalled"] == json!(true);
    let ca = dns["caTrusted"] == json!(true);
    line(
        dns_running && mode == "agent" && resolver && ca,
        dns_running, // running-but-degraded = warning, not failure
        "DNS",
        if !dns_running {
            "not answering — sites won't resolve (open the app / check Settings)".into()
        } else {
            format!(
                "{} · resolver {} · CA {}",
                if mode == "agent" { "agent (always on)".to_string() } else { format!("{mode} — stops when the app quits") },
                if resolver { "installed" } else { "MISSING (run system setup)" },
                if ca { "trusted" } else { "NOT TRUSTED (Settings → Re-trust)" },
            )
        },
    );

    // A borrowed resolver file another tool reclaimed is INVISIBLE to every
    // other probe on this screen: ours keeps answering on :15353, so the DNS
    // line above stays ✓ while the sites simply stop resolving. It was
    // backend-only until 13 Aug 2026 — a startup `log::warn!` nobody reads, an
    // IPC binding with no callers, and this field, emitted since the doctor
    // payload existed and rendered by nothing.
    let (ok, warn, msg) = resolver_verdict(data.get("resolverDrift"));
    line(ok, warn, "Resolvers", msg);

    // A TLD a site ANSWERS on that this machine cannot resolve. Broader than
    // the drift line above, which only covers files rexenv BORROWED from Valet
    // or Herd: this catches a resolver we installed ourselves and lost (a
    // cleanup script, an OS update, a tidied `/etc/resolver`) and a TLD only an
    // EXTRA domain uses — the newest way to hold a hostname nginx serves and
    // DNS never reaches. Nothing else notices, because our own resolver keeps
    // answering for every other TLD.
    if let Some(rows) = data["unresolvableTlds"].as_array().filter(|r| !r.is_empty()) {
        // The two halves need different SENTENCES and different FIXES, and the
        // first version printed one fix for both: `rex tld --repair` refuses a
        // foreign file by design (it will not take over another tool's
        // resolver), so the row it was prescribed for could never be fixed by
        // it — the circle the split exists to break. And it named only the
        // first of N, so with three TLDs down the user repaired one, read
        // "sites on it should load now", and the other two stayed dark.
        let tld_of = |r: &Value| r["tld"].as_str().unwrap_or("?").to_string();
        let (foreign, absent): (Vec<&Value>, Vec<&Value>) =
            rows.iter().partition(|r| r["foreign"] == json!(true));
        let mut msg = String::new();
        if !absent.is_empty() {
            let names = absent.iter().map(|r| format!(".{}", tld_of(r))).collect::<Vec<_>>();
            msg.push_str(&format!(
                "{} — no resolver file, so sites on {} do not resolve on this machine, however \
                 well they are served",
                names.join(", "),
                if names.len() == 1 { "it" } else { "them" }
            ));
            for r in &absent {
                msg.push_str(&format!("\n            fix: rex tld --repair {}", tld_of(r)));
            }
        }
        if !foreign.is_empty() {
            let names = foreign.iter().map(|r| format!(".{}", tld_of(r))).collect::<Vec<_>>();
            if !msg.is_empty() {
                msg.push_str("\n            ");
            }
            // Not "does not resolve": a Valet or Herd resolver for `.test` does
            // resolve — to THAT tool's server, which is not rexenv's. The
            // honest sentence is who answers, not that nobody does.
            msg.push_str(&format!(
                "{} — another tool owns the resolver file, so {} reach whatever it answers, \
                 not rexenv; `rex tld --repair` will not take a file it was never given \
                 (hand the TLD over in the app's import, or use .rex)",
                names.join(", "),
                if names.len() == 1 { "sites on it" } else { "sites on them" }
            ));
        }
        line(false, false, "TLDs in use", msg);
    } else {
        line(true, false, "TLDs in use", "every TLD your sites answer on resolves here".into());
    }

    // A NOTE, not a finding. The bundled 8.x builds link c-ares, whose curl
    // cannot resolve a `.rex` host — WordPress is covered (the mu-plugin patches
    // the HTTP API), a plugin's raw `curl_init()` and any non-WordPress PHP app
    // are not. Nothing here is broken or fixable by the user, so it must not
    // colour the verdict or the exit code: `rex doctor` going red on every
    // normal install would teach people to ignore it. It is printed because the
    // alternative is an unexplained "Could not resolve host" inside somebody's
    // plugin, with nothing on this machine willing to say why.
    if !json_output {
        let cr = &data["curlResolver"];
        let minors: Vec<String> = cr["aresMinors"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !minors.is_empty() {
            let n = cr["sites"].as_u64().unwrap_or(0);
            let pad = " ".repeat(12);
            println!(
                // Continuation lines align under the MESSAGE column (2 for the
                // mark + 9 for the label + 1 space), so the note reads as one
                // block rather than a stray paragraph.
                "· {:<9} PHP {} bundle curl with c-ares, which cannot resolve .rex hosts\n\
                 {pad}({n} site{} on them). WordPress is patched; a plugin's raw curl_init()\n\
                 {pad}and non-WordPress PHP are not — use the WP HTTP API, or CURLOPT_RESOLVE.",
                "PHP curl",
                minors.join(", "),
                if n == 1 { "" } else { "s" },
            );
        }
    }

    let edge = &data["edge"];
    if edge["running"] == json!(true) {
        if edge["wireOurs"] == json!(true) {
            line(true, false, "Edge", "answering as rexenv on :443".into());
        } else if edge["silent"] == json!(true) {
            // Our edge is up and NOTHING is on the port. Not a conflict — there
            // is no one to quit, and saying there is sends the reader hunting
            // for a program that is not running.
            line(
                false,
                false,
                "Edge",
                "running, but nothing answers :443 — rexenv's own edge isn't serving \n                             (its log has the reason; Stop all then Start all rebuilds it)"
                    .into(),
            );
        } else {
            let holder = edge["conflict"]["holder"].as_str().unwrap_or("another proxy");
            let fix = edge["conflict"]["fix"].as_str().map(|f| format!("\n            $ {f}")).unwrap_or_default();
            line(false, false, "Edge", format!("{holder} answers :443 IN FRONT of rexenv — sites unreachable{fix}"));
        }
    } else {
        line(false, true, "Edge", "not running — Start all to serve sites".into());
    }

    let services = data["services"].as_array().cloned().unwrap_or_default();
    let up = services.iter().filter(|s| s["running"] == json!(true)).count();
    line(true, false, "Services", format!("{up}/{} running", services.len()));

    let conflicts = data["portConflicts"].as_array().cloned().unwrap_or_default();
    if conflicts.is_empty() {
        line(true, false, "Ports", "no foreign holders on rexenv ports".into());
    } else {
        for c in &conflicts {
            let fix = c["fix"].as_str().map(|f| format!("\n            $ {f}")).unwrap_or_default();
            line(
                false,
                false,
                "Ports",
                format!(
                    "port {} (needed by {}) held by {}{fix}",
                    c["port"],
                    c["service"].as_str().unwrap_or("?"),
                    c["holder"].as_str().unwrap_or("an unknown process"),
                ),
            );
        }
    }

    let cli = &data["cli"];
    if cli.is_object() {
        line(
            cli["current"] == json!(true),
            true, // absent/stale link is a warning, not a fault
            "CLI",
            if cli["current"] == json!(true) {
                format!("{} → this app", cli["linkPath"].as_str().unwrap_or("?"))
            } else if cli["installed"] == json!(true) {
                "rex on PATH points at a different copy (Settings → Reinstall)".into()
            } else {
                "rex not on PATH (Settings → Command-line tool → Install)".into()
            },
        );
    }

    if findings > 0 {
        if !json_output {
            println!("\n{findings} finding{}", if findings == 1 { "" } else { "s" });
        }
        exit(1);
    }
}

// ── logs ─────────────────────────────────────────────────────────────────────

/// Print a tail, then (--follow) poll every second and print only the lines
/// beyond the largest tail/head overlap of consecutive windows — the same
/// near-real-time model as the app's Logs tab. Repeated identical lines can
/// fool the overlap occasionally; fine for a log follower.
fn tail_loop(base_args: Value, lines: u64, follow: bool) {
    let fetch = |n: u64| -> Vec<String> {
        let mut a = base_args.clone();
        a["lines"] = json!(n);
        request("logs.tail", a)["lines"]
            .as_array()
            .map(|v| v.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let mut prev = fetch(lines);
    for l in &prev {
        println!("{l}");
    }
    if !follow {
        return;
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let new = fetch(200);
        let overlap = (1..=prev.len().min(new.len()))
            .rev()
            .find(|&k| prev[prev.len() - k..] == new[..k])
            .unwrap_or(0);
        for l in &new[overlap..] {
            println!("{l}");
        }
        prev = new;
    }
}

fn lines_flag(words: &[String]) -> u64 {
    flag_value(words, "--lines").and_then(|v| v.parse().ok()).unwrap_or(100)
}

fn cmd_logs(words: &[String], json_output: bool) {
    let key = words.first().filter(|w| !w.starts_with("--"));
    let Some(key) = key else {
        let data = request("logs.list", Value::Null);
        if json_output {
            return print_json(&data);
        }
        let Some(files) = data["files"].as_array() else { return println!("(no logs)") };
        for f in files {
            println!("{:>9}  {}", format!("{} B", f["bytes"].as_u64().unwrap_or(0)), f["key"].as_str().unwrap_or("?"));
        }
        return;
    };
    if json_output {
        return print_json(&request("logs.tail", json!({ "key": key, "lines": lines_flag(words) })));
    }
    tail_loop(json!({ "key": key }), lines_flag(words), words.iter().any(|w| w == "--follow"));
}

fn cmd_site_logs(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site logs <domain> [--source K] [--lines N] [--follow]");
    let id = site["id"].clone();
    let Some(source) = flag_value(words, "--source") else {
        let data = request("logs.targets", json!({ "id": id }));
        if json_output {
            return print_json(&data);
        }
        let Some(targets) = data["targets"].as_array() else { return println!("(no sources)") };
        println!("sources (pass one via --source):");
        for t in targets {
            println!("  {:<28} {}", t["key"].as_str().unwrap_or("?"), t["label"].as_str().unwrap_or(""));
        }
        return;
    };
    // `id` rides along for the wp-debug pseudo-source (docroot-based tail).
    let base = json!({ "key": source, "id": id });
    if json_output {
        let mut a = base;
        a["lines"] = json!(lines_flag(words));
        return print_json(&request("logs.tail", a));
    }
    tail_loop(base, lines_flag(words), words.iter().any(|w| w == "--follow"));
}

fn cmd_site_delete(words: &[String], json_output: bool) {
    const DELETE_USAGE: &str = "rex: usage: rex site delete <domain> [--yes]";
    // Here a typo fails SAFE — a misspelt `--yes` leaves the prompt in place —
    // so this is defence in depth rather than a fix. It is still worth having:
    // the failure it prevents is the reverse one, someone who meant a flag this
    // command does not have (`--force`, `--keep-db`) and got a delete anyway.
    reject_unknown_flags(words, "site delete", &["--yes"], DELETE_USAGE);
    let Some(domain) = words.first() else {
        eprintln!("{DELETE_USAGE}");
        exit(1);
    };
    // Resolve domain → id through the app (same list the UI shows). By the
    // PRIMARY only, on purpose: a delete is the one verb where "the site that
    // answers on this name" is the wrong resolution — typing an extra name
    // most likely means "stop this name", not "destroy the site and its
    // database" — so an alias is named for what it is, with both ways out.
    let domain = normalize_hostname(domain);
    let data = request("site.list", Value::Null);
    let site = data["sites"]
        .as_array()
        .and_then(|sites| sites.iter().find(|s| s["domain"] == json!(domain)))
        .cloned();
    let Some(site) = site else {
        if let Some((_, primary)) = alias_owner(&data, &domain) {
            eprintln!(
                "rex: `{domain}` is an extra domain of `{primary}` — delete the site as \
                 `rex site delete {primary}`, or drop just this name with \
                 `rex site domains {primary} --remove {domain}`"
            );
        } else {
            eprintln!("rex: no site with domain `{domain}` (see `rex site list`)");
        }
        exit(1);
    };
    // Destructive: database + docroot go away. Ask unless --yes (and always
    // require --yes when stdin isn't a terminal-driven human).
    if !words.iter().any(|w| w == "--yes") {
        // The folder is only ours to delete when we created it — say which.
        let folder = if site["docrootManaged"].as_bool() == Some(false) {
            format!("Your folder at {} is left in place.", site["path"].as_str().unwrap_or("?"))
        } else {
            "This also removes its folder.".to_string()
        };
        eprint!("delete {domain}? This drops its database. {folder} [y/N] ");
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err()
            || !matches!(answer.trim(), "y" | "Y" | "yes")
        {
            eprintln!("aborted (nothing deleted)");
            exit(1);
        }
    }
    let result = request("site.delete", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&result);
    }
    // Derived the same way the confirm prompt above derives its warning, and for
    // a reason found by running it (23 Aug 2026): this line said "database +
    // files removed" for EVERY site, including a linked one whose folder rexenv
    // correctly did not touch. The prompt already got this right — but `--yes`
    // skips the prompt, so for the user most likely to be scripting, the false
    // sentence was the only thing printed.
    if site["docrootManaged"].as_bool() == Some(false) {
        println!(
            "✓ deleted {domain} (database removed; your folder at {} is untouched)",
            site["path"].as_str().unwrap_or("?")
        );
    } else {
        println!("✓ deleted {domain} (database + files removed)");
    }
}

// ── status ───────────────────────────────────────────────────────────────────

fn cmd_status(json_output: bool) {
    let data = request("status", Value::Null);
    if json_output {
        return print_json(&data);
    }
    if let Some(dns) = data["dns"].as_object() {
        let running = dns["running"] == json!(true);
        let mode = dns["mode"].as_str().unwrap_or("?");
        let port = &dns["port"];
        let resolver = dns["resolverInstalled"] == json!(true);
        let ca = dns["caTrusted"] == json!(true);
        println!(
            "DNS      {} ({mode}, udp {port}) · resolver {} · CA {}",
            if running { "answering" } else { "DOWN" },
            if resolver { "installed" } else { "MISSING" },
            if ca { "trusted" } else { "NOT TRUSTED" },
        );
    }
    // One line, and only when there IS one — an update line on every status
    // call would be noise on the 99 runs where nothing is offered. It names
    // where to go rather than what to type: `rex` deliberately has no way to
    // install one (ledger #530).
    if let Some(u) = data["update"].as_object() {
        if let Some(v) = u["version"].as_str() {
            println!("update   rexenv {v} can be installed from Settings → About");
        }
    }
    let Some(services) = data["services"].as_array() else {
        return println!("(no services reported)");
    };
    let name_w = services
        .iter()
        .filter_map(|s| s["name"].as_str())
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!("{:<name_w$}  {:<8} {:>7}  {:>6}  {:>6}  {:>8}", "NAME", "STATE", "PID", "PORT", "CPU%", "RAM");
    for s in services {
        let running = s["running"] == json!(true);
        let pid = s["pid"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        println!(
            "{:<name_w$}  {:<8} {:>7}  {:>6}  {:>6.1}  {:>6} MB",
            s["name"].as_str().unwrap_or("?"),
            if running { "running" } else { "idle" },
            pid,
            s["port"].as_u64().unwrap_or(0),
            s["cpuPercent"].as_f64().unwrap_or(0.0),
            s["ramMb"].as_u64().unwrap_or(0),
        );
    }
}

#[cfg(test)]
mod tests {

    /// `--repair` is a DIFFERENT verb from `--set`, and doctor names it.
    ///
    /// `--set` decides what NEW sites are called; `--repair` writes a
    /// root-owned file under `/etc/resolver` behind a privileged prompt.
    /// Conflating them would make "change my default TLD" quietly install a
    /// system file, which is not what that flag promises.
    ///
    /// And the diagnosis has to name its own fix: `doctor` is where a missing
    /// resolver is FOUND, and a finding with no next step is a finding people
    /// learn to scroll past.
    #[test]
    fn the_resolver_repair_is_its_own_flag_and_doctor_points_at_it() {
        const ME: &str = include_str!("main.rs");
        let tld = ME
            .split("fn cmd_tld(")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("cmd_tld");
        let repair = tld.find("\"--repair\"").expect("`rex tld --repair` is gone");
        let set = tld.find("\"--set\"").expect("`rex tld --set` is gone");
        assert!(
            tld.contains("\"--remove\"") && tld.contains("tld.remove"),
            "`rex tld --remove` is gone — a resolver file for a TLD nothing uses then has no \
             way out but hand-editing /etc/resolver as root"
        );
        assert!(
            repair < set,
            "`--set` is matched before `--repair`; if either ever shares a code path the \
             DEFAULT-TLD flag starts installing system files"
        );
        assert!(
            tld.contains("tld.repair") && tld.contains("tld.set"),
            "the two flags no longer send different commands"
        );

        let doctor = ME
            .split("fn cmd_doctor(")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("cmd_doctor");
        assert!(
            doctor.contains("rex tld --repair"),
            "the unresolvable-TLD finding does not name the command that fixes it — a finding \
             with no next step is one people learn to scroll past"
        );
    }

    /// `site info` answers with EVERY name the site has.
    ///
    /// It is the "tell me everything about this site" verb. A site answering on
    /// three hostnames while `info` prints one is the same half-answer the
    /// Sites list carried until extra domains were marked there — and the CLI
    /// is where a user checks after adding one.
    ///
    /// The line appears only when there ARE extras: an "also:" with nothing
    /// after it is a field that teaches people to skip fields.
    #[test]
    fn site_info_lists_every_name_the_site_answers_on() {
        const ME: &str = include_str!("main.rs");
        let body = ME
            .split("fn cmd_site_info(")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("cmd_site_info");
        assert!(
            body.contains("data[\"domains\"]"),
            "`site info` no longer reads the domains the server sends — a site that answers on \
             three names would print one"
        );
        assert!(
            body.contains("if !extra.is_empty()"),
            "the extra-domains line is printed unconditionally — an empty `also` is a field \
             that teaches people to skip fields"
        );
        // The PRIMARY keeps its own line: it is the name the site's files,
        // database and certificate folder are keyed to, and folding it into a
        // list would lose that.
        let primary = body.find("field(\"domain\"").expect("the domain field");
        let also = body.find("\"also\"").expect("the also field");
        assert!(primary < also, "the extras are printed above the primary");
    }

    /// **A site is findable by ANY name it answers on.**
    ///
    /// Extra domains (v42) are served by nginx and the edge, so a user who
    /// types one has every reason to expect `rex` to know it. Resolving only
    /// the site's OWN domain made them a thing the app served and the CLI could
    /// not name: `rex site info shop.rex` answered "no site with domain
    /// shop.rex" about a site that answers on exactly that.
    ///
    /// A source guard because `find_site` talks to a running app: what is
    /// asserted is that the resolution CONSULTS the alias map the server now
    /// sends, and that the primary is still tried first — an alias shadowing a
    /// primary would resolve the wrong site for a name that is somebody's
    /// actual domain.
    #[test]
    fn a_site_is_findable_by_any_of_its_domains() {
        const ME: &str = include_str!("main.rs");
        let body = ME
            .split("fn find_site(")
            .nth(1)
            .and_then(|b| b.split("\nfn ").next())
            .expect("find_site");
        let primary = body
            .find("s[\"domain\"] == json!(domain)")
            .expect("`find_site` no longer matches the site's own domain");
        let alias = body.find("alias_owner(").expect(
            "`find_site` never consults the alias map — a site's extra domains are served by \
             nginx and the edge, and the CLI cannot name them",
        );
        assert!(
            primary < alias,
            "the alias map is consulted BEFORE the site's own domain — an alias would shadow a \
             primary, and a name that is somebody's actual domain would resolve to another site"
        );
        // The argument is normalised the way the app normalised it on the way
        // in: `dig` prints a trailing dot, people type capitals, and both were
        // accepted by the add — so both must find the site.
        assert!(
            body.find("normalize_hostname(").is_some_and(|n| n < primary),
            "`find_site` compares the raw argument — `rex site info Shop.rex.` fails for a \
             name the app accepted as `shop.rex`"
        );

        // And the helper itself, behaviourally: the owner of an alias is the
        // site whose id the map lists it under, by primary; a primary is not
        // an alias; an unknown name is nobody's.
        let data = json!({
            "sites": [
                {"id": "a", "domain": "acme.rex"},
                {"id": "b", "domain": "beta.rex"}
            ],
            "aliases": {"b": ["shop.rex", "www.beta.rex"]}
        });
        assert_eq!(
            alias_owner(&data, "shop.rex"),
            Some(("b".to_string(), "beta.rex".to_string()))
        );
        assert_eq!(alias_owner(&data, "acme.rex"), None);
        assert_eq!(alias_owner(&data, "nobody.rex"), None);
        assert_eq!(normalize_hostname(" Shop.REX. "), "shop.rex");
    }

    /// **Every subcommand the CLI dispatches is completable, and every
    /// completion word dispatches — for every GROUP, not just the one that was
    /// broken.**
    ///
    /// Completions are how a terminal user DISCOVERS this tool: `rex site <tab>`
    /// is the only place most people will ever see that a verb exists. A verb
    /// that dispatches without a completion word shipped invisible — which is
    /// exactly what happened to `site restart`, `site domains` and
    /// `service restart` (2 Sep 2026); all three worked and nothing offered
    /// them. A completion word nothing dispatches is the same rot pointing the
    /// other way: a tab into an error.
    ///
    /// **The first version of this test checked `site` alone**, which is the
    /// guard-covers-claimed-surface shape this repo keeps paying for — written,
    /// on the same day, into a test about a surface. It walks every group with
    /// a completion list now.
    #[test]
    fn every_dispatched_subcommand_is_offered_by_completions() {
        const ME: &str = include_str!("main.rs");

        /// (group, the dispatch block's opening line, the completion constant).
        /// `mail`, `tunnel` and `repo` dispatch inside their own `cmd_*` fn, so
        /// the anchor is that fn's `match`.
        const GROUPS: &[(&str, &str, &str)] = &[
            (
                "site",
                "Some(\"site\") => match words.get(1).map(String::as_str) {",
                "const SITE: &str = \"",
            ),
            (
                "db",
                "Some(\"db\") => match words.get(1).map(String::as_str) {",
                "const DB: &str = \"",
            ),
        ];

        // The arms of one dispatch block, scoped to its own closing brace: the
        // first version ran past `site`'s and picked up `php default`, naming a
        // command that does not exist at that path.
        let arms = |after: &str| -> Vec<String> {
            let block = ME
                .split(after)
                .nth(1)
                .unwrap_or_else(|| panic!("`{after}` is gone — if the dispatch moved, move this guard"));
            let block = block.split("\n        },").next().unwrap_or(block);
            let mut out: Vec<String> = Vec::new();
            for (i, _) in block.match_indices("Some(\"") {
                if let Some(name) = block[i + 6..].split('"').next() {
                    if !name.is_empty() && !out.contains(&name.to_string()) {
                        out.push(name.to_string());
                    }
                }
            }
            out
        };
        let words_of = |const_start: &str| -> Vec<String> {
            ME.split(const_start)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or_else(|| panic!("the completion constant `{const_start}…` is gone"))
                .split_whitespace()
                .map(str::to_string)
                .collect()
        };

        for (group, anchor, const_start) in GROUPS {
            let dispatched = arms(anchor);
            let offered = words_of(const_start);
            assert!(
                dispatched.len() > 3,
                "only {dispatched:?} parsed from the `{group}` dispatch — the scan is broken, and \
                 a scan that reads nothing agrees with everything"
            );
            for verb in &dispatched {
                assert!(
                    offered.iter().any(|w| w == verb),
                    "`rex {group} {verb}` dispatches but is not in the completion list — a \
                     terminal user's only way to discover it is reading the source"
                );
            }
            for word in &offered {
                assert!(
                    dispatched.iter().any(|v| v == word),
                    "completions offer `rex {group} {word}` and nothing dispatches it — \
                     tab-completing into an error is worse than not being offered"
                );
            }
        }

        // `php`, `mail` and `tunnel` dispatch inside their own `cmd_*` fn, so
        // their verbs are read from that fn's body instead of a match arm at
        // the top level.
        for (group, func, const_start) in [
            ("php", "fn cmd_php(", "const PHP: &str = \""),
        ] {
            let body = ME
                .split(func)
                .nth(1)
                .and_then(|b| b.split("\nfn ").next())
                .unwrap_or_else(|| panic!("`{func}` is gone"));
            let offered = words_of(const_start);
            for word in &offered {
                assert!(
                    body.contains(&format!("\"{word}\"")),
                    "completions offer `rex {group} {word}` and `{func}…` never matches it"
                );
            }
        }

        // …and the HELP text, which is the other half of discovery: `rex` with
        // no arguments prints it, and a verb absent from it is invisible to
        // anyone who does not tab-complete. Yesterday's three new verbs made it
        // into USAGE and not into the completions, which is why both are
        // checked rather than either standing in for the other.
        let usage = ME
            .split("const USAGE: &str =")
            .nth(1)
            .and_then(|b| b.split("\";").next())
            .expect("the USAGE constant");
        for (group, anchor, _) in GROUPS {
            for verb in arms(anchor) {
                assert!(
                    usage.contains(&format!("{group} {verb}")),
                    "`rex {group} {verb}` dispatches and the help text never mentions it — a \
                     user reading `rex` with no arguments cannot know it exists"
                );
            }
        }

        // `wp` and `repo` take their verb at a DEEPER position (`rex wp <domain>
        // plugin …`, `rex repo <domain> status …`), and their dispatches nest —
        // `repo watch` has its own `start`/`stop` arms, and `repo` matches four
        // git ops through one `op @ ("pull" | "fetch" | …)` pattern. Reading
        // "every verb this dispatches" out of that needs a parser, so only the
        // direction that is unambiguous is checked here: every word the
        // completion OFFERS must appear in the dispatching function. A stale
        // list still fails; a new verb missing from the list does not, and
        // saying so beats a guard that pretends otherwise.
        for (group, func, const_start) in [
            ("wp", "fn cmd_wp(", "const WPA: &str = \""),
            ("repo", "fn cmd_repo(", "const REPO: &str =\n        \""),
        ] {
            let body = ME
                .split(func)
                .nth(1)
                .and_then(|b| b.split("\nfn ").next())
                .unwrap_or_else(|| panic!("`{func}` is gone"));
            let offered = words_of(const_start);
            assert!(offered.len() > 5, "the `{group}` completion list parsed as {offered:?}");
            for word in &offered {
                assert!(
                    body.contains(&format!("\"{word}\"")),
                    "completions offer `rex {group} … {word}` and `{func}…` never matches it — \
                     tab-completing into an error"
                );
            }
        }

        // `service` dispatches inside `cmd_service` and its three verbs are
        // asserted against BOTH shells' strings — a zsh-only fix leaves a bash
        // user unable to discover the web tier's only verb.
        for shell_list in ["compadd start stop restart", "compgen -W \\\"start stop restart"] {
            assert!(
                ME.contains(shell_list),
                "`rex service restart` is missing from a completion list ({shell_list}) — the \
                 web tier's only verb, undiscoverable in that shell"
            );
        }
    }

    #[test]
    fn the_native_version_prints_its_own_commit_before_it_asks_the_app() {
        let src = include_str!("main.rs");
        let arm = src
            .find(r#""-v" | "-V" | "--version" =>"#)
            .expect("the --version arm");
        let body = &src[arm..arm + 700];

        let own = body.find("REX_GIT_COMMIT").expect(
            "`rex --version` no longer prints the CLI's OWN commit — a downloaded artefact \
             cannot then say what built it, which is the whole reason this exists",
        );
        if let Some(ask) = body.find("soft_request") {
            assert!(
                own < ask,
                "the CLI's own commit is printed only after asking the app — with no app \
                 running, the artefact answers with nothing"
            );
        }
        assert!(
            body.contains("app rexenv"),
            "the app half is no longer LABELLED as the app's. Two version numbers side by \
             side with one commit under them invites reading the commit as either's, which \
             is the ambiguity this replaced"
        );

        // The stamp itself must be real: a short sha, optionally -dirty, or the
        // documented `unknown` fallback. An empty value would print `rex 0.3.0 ()`
        // and read as "no commit" rather than as a broken build script.
        let c = env!("REX_GIT_COMMIT");
        assert!(!c.is_empty(), "REX_GIT_COMMIT is empty — cli/build.rs did not stamp");
        assert!(
            c == "unknown"
                || c.trim_end_matches("-dirty").chars().all(|ch| ch.is_ascii_hexdigit()),
            "REX_GIT_COMMIT is not a short sha, -dirty sha, or `unknown`: {c:?}"
        );
        assert!(!env!("REX_BUILT_AT").is_empty(), "REX_BUILT_AT is empty");
    }
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Instant;

    /// The three states of the resolver line, which exists because a TLD taken
    /// back by Valet or Herd is invisible to every other probe on the screen:
    /// our resolver keeps answering, DNS reads ✓, and the sites stop resolving.
    #[test]
    fn a_reclaimed_resolver_is_a_finding_and_a_missing_field_is_not_a_pass() {
        // Checked, none: the only ✓.
        let (ok, warn, msg) = resolver_verdict(Some(&json!([])));
        assert!(ok && !warn, "an empty list means the check RAN and found nothing");
        assert!(msg.contains("no TLD taken back"));

        // Checked, found: a FINDING (not a warning) — the sites are dark, and
        // `cmd_doctor` counts anything not-ok toward the exit code.
        let (ok, warn, msg) = resolver_verdict(Some(&json!(["rex", "test"])));
        assert!(!ok && !warn, "a reclaimed TLD is a failure, not a warning — sites stop resolving");
        assert!(msg.contains(".rex") && msg.contains(".test"), "the TLDs are named: {msg}");
        assert!(
            msg.contains("rexenv → Import"),
            "the fix must name WHERE to redo the takeover — there is no `rex` command for it: {msg}"
        );
        assert!(msg.contains("no longer resolve"), "say the consequence, not just the state: {msg}");

        // NOT checked: an older app whose payload predates the field. Reporting
        // ✓ here would be a confident answer to a question nobody asked — the
        // shape this project keeps catching, one layer up.
        for absent in [None, Some(&Value::Null)] {
            let (ok, warn, msg) = resolver_verdict(absent);
            assert!(!ok && warn, "an absent field must not read as a clean check");
            assert!(msg.contains("older app"), "say WHY it is unknown: {msg}");
        }
    }

    /// A socket path this test owns, in the OS temp dir. No app data is touched:
    /// this crate cannot reach it (see the dependency note in Cargo.toml).
    fn fixture_socket(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rexenv-cli-test-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// THE 2026-08-12 failure, pinned: a listener that accepts and then says
    /// nothing. Before the deadline this hung in `recvfrom` forever, which is
    /// how `rex --version` — documented as working WITHOUT the app — printed
    /// nothing at all until the wedged app was killed.
    /// **"unknown command" is always a build mismatch, and the CLI can say which
    /// side is stale.**
    ///
    /// `cli_server` answers `unknown command: X (this rex may be newer than the
    /// running app)` — a hedge, because the app cannot know. A typo never gets
    /// that far: `rex`'s own match rejects an unknown subcommand client-side, so
    /// a command reaching the app and coming back unknown means the two builds
    /// disagree. They ship in the same cask, so a version difference IS the
    /// diagnosis.
    ///
    /// This is what `docs/CLI-ROADMAP.md` listed as a 🟡 protocol-version
    /// handshake. A protocol integer answers "is the wire contract compatible",
    /// which nobody is asking — and it would stay SILENT for the case that keeps
    /// happening, because adding a command is backward-compatible and would
    /// never bump it.
    #[test]
    fn a_version_difference_is_reported_and_a_match_stays_quiet() {
        // Different RELEASES.
        let skew = version_skew_between("0.3.0", "0.2.1", "aaa1111", "bbb2222")
            .expect("different builds must be named");
        assert!(skew.contains("0.3.0") && skew.contains("0.2.1"), "{skew}");
        assert!(skew.contains("stale"), "no diagnosis: {skew}");
        assert!(skew.contains("Quit and reopen"), "no way out offered: {skew}");

        // The reverse direction reads the same way on purpose — the message
        // names both numbers and does not claim to know which is newer, because
        // a higher version string is not proof of a newer BUILD on a dev machine.
        assert!(version_skew_between("0.2.1", "0.3.0", "a", "b").is_some());

        // **Same version, different COMMIT — the case that actually keeps
        // happening**, and the one the first version of this check missed. A
        // release carries one version number for weeks while every rebuild
        // changes the commit; measured live 23 Aug 2026 against an app built
        // from 460981c, where a version-only check said nothing.
        let dev = version_skew_between("0.3.0", "0.3.0", "86855cc", "460981c")
            .expect("same version, different commit is a mismatch");
        assert!(dev.contains("86855cc") && dev.contains("460981c"), "{dev}");
        assert!(dev.contains("different build"), "{dev}");
        // It must NOT tell the user which side to rebuild. A sha carries no
        // ordering, and the first version of this assumed the APP was behind —
        // then said so when the app was the NEWER one (observed 24 Aug 2026,
        // after rebuilding the app before the CLI). Naming both fixes is the
        // only honest form.
        assert!(dev.contains("whichever is behind"), "{dev}");
        assert!(dev.contains("rebuild `rex`"), "no CLI-is-stale branch offered: {dev}");

        // Matching builds must be silent: a warning on an unrelated bug sends
        // the reader at their install instead of at the bug.
        assert_eq!(version_skew_between("0.3.0", "0.3.0", "abc1234", "abc1234"), None);
        // An app that could not be asked. Guessing here would put the warning on
        // every failure from an app that is simply not running.
        assert_eq!(version_skew_between("0.3.0", "", "a", "b"), None);
        // A MISSING stamp is not a different one. `unknown` is what both build
        // scripts emit when git cannot answer (a tarball build, no .git), and
        // treating that as a mismatch would warn every such user on every error.
        assert_eq!(version_skew_between("0.3.0", "0.3.0", "unknown", "abc1234"), None);
        assert_eq!(version_skew_between("0.3.0", "0.3.0", "abc1234", "unknown"), None);
        assert_eq!(version_skew_between("0.3.0", "0.3.0", "abc1234", ""), None);
    }

    /// **`rex` offers to start the app; it never starts it.**
    ///
    /// The message names the command (C3), and the binary must contain no way
    /// to run it itself. Auto-spawning would launch a second rexenv behind the
    /// user's back — adopting services, opening the database as a second
    /// writer, taking over both sockets — as a side effect of `rex status` in a
    /// shell script. The one `open` this CLI does run is `open_url`, which
    /// opens a SITE in a browser at the user's explicit request.
    ///
    /// Text-level, and honest about it: it reads the source for a spawn that
    /// names the app. It cannot prove no spawn exists — it can keep the obvious
    /// one from being added, which is the drift worth catching, because
    /// "helpfully" launching the app is a two-line change that looks kind.
    #[test]
    /// **When rexenv goes away mid-call, the bridge answers the pending request
    /// itself, in-band, with a sentence the model can act on — and it reads
    /// ids and nothing else.**
    fn the_bridge_answers_a_pending_request_in_band_when_rexenv_stops() {
        const ME: &str = include_str!("main.rs");
        // What counts as a request (id + method), and what does not.
        assert_eq!(request_id(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{}}"#), Some(serde_json::json!(7)));
        assert_eq!(request_id(r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#), Some(serde_json::json!("abc")));
        assert_eq!(request_id(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None, "a notification has nothing to answer");
        assert_eq!(request_id(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#), None, "a reply is not a request");
        assert_eq!(request_id("not json"), None, "garbage passes through untouched, unread");
        assert_eq!(reply_id(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#), Some(serde_json::json!(7)));
        assert_eq!(reply_id(r#"{"jsonrpc":"2.0","id":7,"error":{"code":1,"message":"x"}}"#), Some(serde_json::json!(7)));
        assert_eq!(reply_id(r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{}}"#), None);

        // The ledger of what is unanswered.
        let p = PendingIds::default();
        p.sent(serde_json::json!(1));
        p.sent(serde_json::json!(2));
        p.answered(&serde_json::json!(1));
        assert_eq!(p.drain(), vec![serde_json::json!(2)], "only the unanswered one is drained");
        assert!(p.drain().is_empty(), "drained once");

        // The in-band error: a JSON-RPC error REPLY to the pending id — routed
        // by the client to the waiting call, not a protocol-level failure —
        // whose message says what happened, what is unknown, and what to do.
        let line = stopped_error(&serde_json::json!(2));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 2);
        assert_eq!(v["error"]["code"], -32000);
        let msg = v["error"]["message"].as_str().unwrap();
        for must in ["rexenv stopped", "unknown", "open rexenv again", "before retrying"] {
            assert!(msg.contains(must), "the in-band error must say {must:?}: {msg}");
        }
        assert!(!line.contains('\n'), "one line — the framing is newline-delimited");

        // The bridge still constructs no request: the only JSON it BUILDS is
        // the stopped error, and it never sends anything toward the socket
        // that it did not read from stdin.
        let prod = ME.split("\n#[cfg(test)]").next().unwrap_or(ME);
        let bridge = &prod[prod.find("fn run_mcp_bridge()").unwrap()..];
        let bridge = &bridge[..bridge.find("\n}\n").unwrap()];
        assert!(!bridge.contains("json!("), "the bridge body builds no JSON of its own");
        assert_eq!(prod.matches("stopped_error(").count(), 2, "defined once, used once — in the socket-EOF drain");
    }

    #[test]
    fn the_cli_names_the_start_command_and_never_runs_it() {
        let src = include_str!("main.rs");
        assert!(
            NOT_RUNNING.contains("rexenv isn't running"),
            "the reason must survive whatever else the line says: {NOT_RUNNING}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            NOT_RUNNING.contains("open -a rexenv"),
            "the user must be told what to run: {NOT_RUNNING}"
        );
        // Every spawn in the file, with its argument — none may launch rexenv.
        for (i, line) in src.lines().enumerate() {
            if !line.contains("Command::new") {
                continue;
            }
            assert!(
                !line.contains("rexenv") && !line.contains("-a "),
                "line {}: the CLI must not launch the app — it names the command instead: {}",
                i + 1,
                line.trim()
            );
        }
        // And nothing may pass the app to `open` a line or two later, which is
        // how the spawn above would actually be written.
        assert!(
            !src.contains("\"-a\""),
            "an `-a` argument in this binary is an app launch"
        );
    }

    #[test]
    fn soft_request_gives_up_on_a_listener_that_accepts_and_never_answers() {
        let path = fixture_socket("deaf");
        let listener = UnixListener::bind(&path).expect("bind");
        let deaf = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            // Read the request, then deliberately never reply. Hold the
            // connection open — a closed one would end the read by itself and
            // prove nothing about the deadline.
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf);
            std::thread::sleep(Duration::from_secs(3));
        });

        let started = Instant::now();
        let got = soft_request_at(&path, "version", Duration::from_millis(200));
        let waited = started.elapsed();

        assert!(got.is_none(), "a silent app must degrade to None, not data");
        assert!(
            waited < Duration::from_secs(2),
            "soft_request must be bounded by its deadline; waited {waited:?}"
        );
        let _ = std::fs::remove_file(&path);
        let _ = deaf.join();
    }

    #[test]
    fn soft_request_returns_the_data_when_the_app_answers() {
        let path = fixture_socket("answers");
        let listener = UnixListener::bind(&path).expect("bind");
        let app = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf);
            stream
                .write_all(b"{\"ok\":true,\"data\":{\"version\":\"9.9.9\"}}\n")
                .expect("reply");
        });

        let got = soft_request_at(&path, "version", Duration::from_secs(2));

        assert_eq!(got.expect("data")["version"], json!("9.9.9"));
        let _ = std::fs::remove_file(&path);
        let _ = app.join();
    }

    /// A missing socket is the ordinary "app isn't running" case and must stay
    /// instant — the deadline is for a listener that exists, not for ENOENT.
    #[test]
    fn soft_request_is_immediate_when_nothing_is_listening() {
        let path = fixture_socket("absent");
        let started = Instant::now();
        assert!(soft_request_at(&path, "version", Duration::from_secs(30)).is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn stall_notice_speaks_when_the_reply_is_late() {
        let (tx, rx) = mpsc::channel();
        let _flag = stall_notice(Duration::from_millis(50), move || {
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok(), "the notice must fire");
    }

    /// The common case: the reply lands first, so the user never sees a scary
    /// line about a wedged app for a command that worked.
    #[test]
    fn stall_notice_stays_quiet_when_the_reply_lands_first() {
        let (tx, rx) = mpsc::channel();
        let flag = stall_notice(Duration::from_millis(100), move || {
            let _ = tx.send(());
        });
        flag.store(true, Ordering::Relaxed);
        assert!(
            rx.recv_timeout(Duration::from_millis(400)).is_err(),
            "a finished command must not print a stall notice"
        );
    }
    /// **The roadmap may not call a verb unbuilt while `rex` dispatches it.**
    ///
    /// `docs/CLI-ROADMAP.md` is the file a reader opens to learn what the CLI
    /// can do, and its TABLE is the part they trust. On 3 Sep 2026 two rows
    /// there said `mail mark-read` had "no CLI verb yet" and `mail list`'s
    /// `--unread` was "not wired yet" — while the same file's Infrastructure
    /// and In-app-verifies sections recorded both shipping on 23 Aug and being
    /// verified live the same day. The file contradicted itself for ten days,
    /// and the wrong half is the half that sends someone to build what exists.
    ///
    /// So: a row whose Tag is not ✓, or whose Notes still carry a not-yet
    /// phrase, must not name something the CLI actually has. "Has" is read from
    /// this file — the completion constants for a subcommand, the literal flag
    /// string for a flag — never from a second list kept beside the doc.
    ///
    /// This proves a row is not FALSE. It cannot prove a row is complete: a
    /// verb nobody has written a row for is invisible here, and that is what
    /// `every_dispatched_subcommand_is_offered_by_completions` is for.
    #[test]
    fn no_roadmap_row_calls_unbuilt_a_thing_the_cli_already_dispatches() {
        const ME: &str = include_str!("main.rs");
        let doc = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/CLI-ROADMAP.md"),
        )
        .expect("docs/CLI-ROADMAP.md");

        // Phrases that assert absence. Each one is a sentence some row has
        // actually carried past its own shipping date.
        const NOT_YET: [&str; 5] =
            ["no CLI verb yet", "not wired yet", "unreachable from the CLI", "no `rex` verb", "not built"];

        let mut rows = 0usize;
        let mut stale: Vec<String> = Vec::new();
        for line in doc.lines() {
            let line = line.trim();
            if !line.starts_with("| `") {
                continue; // headers, separators, prose
            }
            // `\|` is an escaped pipe INSIDE a cell (alternations like
            // `start\|stop`), not a column break — hide it before splitting.
            let hidden = line.replace("\\|", "\u{1}");
            let cells: Vec<&str> = hidden.trim_matches('|').split('|').map(str::trim).collect();
            if cells.len() < 4 {
                continue;
            }
            rows += 1;
            let (cmd, tag, notes) = (cells[0], cells[2], cells[3]);
            let claims_absent =
                !tag.contains('✓') || NOT_YET.iter().any(|p| notes.contains(p));
            if !claims_absent {
                continue;
            }

            // A row naming FLAGS is a claim about those flags; otherwise it is a
            // claim about the subcommand path.
            let flags: Vec<&str> = cmd
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .filter(|w| w.starts_with("--") && w.len() > 3)
                .collect();
            if !flags.is_empty() {
                for f in flags {
                    if ME.contains(&format!("\"{f}\"")) {
                        stale.push(format!("{cmd} — the CLI parses `{f}` already"));
                    }
                }
                continue;
            }
            // `\`group sub …\`` — the first two bare words of the command cell.
            let words: Vec<&str> = cmd
                .trim_start_matches('|')
                .split('`')
                .nth(1)
                .unwrap_or("")
                .split_whitespace()
                .map(|w| w.trim_start_matches("rex").trim())
                .filter(|w| !w.is_empty())
                .collect();
            if let (Some(group), Some(sub)) = (words.first(), words.get(1)) {
                if sub.starts_with('<') || sub.starts_with('[') || sub.starts_with("--") {
                    continue; // `rex status`-shaped: no subcommand to look up
                }
                // The completion constants ARE the built surface — the sibling
                // test holds them equal to the dispatch in both directions.
                let offered = ME
                    .lines()
                    .filter(|l| l.contains(&format!("{group})")) || l.contains("const "))
                    .any(|l| l.split_whitespace().any(|w| w.trim_matches('"') == *sub));
                if offered {
                    stale.push(format!("{cmd} — `rex {group} {sub}` is offered by completions"));
                }
            }
        }

        assert!(rows > 40, "only {rows} roadmap rows parsed — the table shape moved");
        assert!(
            stale.is_empty(),
            "docs/CLI-ROADMAP.md calls these unbuilt, and the CLI has them:\n  {}\nA table row \
             that outlived its own shipping date is the half a reader trusts — fix the row in \
             the commit that ships the thing",
            stale.join("\n  ")
        );
    }

    /// **A forgotten flag value is an ABSENT flag, never the next flag.**
    ///
    /// `site create` gained its first switch (`--starter-db`), and until this
    /// guard `flag_value` took whatever word came next: `--name --starter-db`
    /// created a site literally named `"--starter-db"`, with no error and
    /// nothing to undo it by. Absent is a shape every caller already handles.
    #[test]
    fn a_flag_never_swallows_the_flag_that_follows_it() {
        let w = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        let words = w("app.rex --type php --name --starter-db");
        assert_eq!(flag_value(&words, "--type").as_deref(), Some("php"));
        assert_eq!(
            flag_value(&words, "--name"),
            None,
            "a flag with no value took the NEXT FLAG as its value — the site would be named for it"
        );
        // The switch is still seen where it is: dropping the value must not
        // drop the flag that was mistaken for one.
        assert!(words.iter().any(|x| x == "--starter-db"));
        // A trailing flag has no next word at all.
        assert_eq!(flag_value(&w("app.rex --name"), "--name"), None);
    }

    /// **`site create` refuses a flag it does not know, and knows every flag it
    /// reads.**
    ///
    /// The refusal is only safe while the known set is COMPLETE: a flag added
    /// to the body and forgotten in the list would be rejected on the command
    /// line it was just built for. So the list is not trusted — it is measured
    /// against the flag literals the function actually reads, and against the
    /// usage line the refusal prints, which is what a user retypes from.
    #[test]
    fn site_create_knows_every_flag_it_reads_and_says_so_in_its_usage() {
        const ME: &str = include_str!("main.rs");
        // What the command READS, from a source the known-list is not built
        // from: the value-taking table, plus every `w == "--x"` switch test in
        // the body. The first version of this scan collected every quoted
        // literal in the same window the list is written in, so removing an
        // entry removed the evidence with it — the plant came back green.
        let body = ME
            .split("fn cmd_site_create(")
            .nth(1)
            .and_then(|b| b.split("\nfn cmd_site_info").next())
            .expect("cmd_site_create");
        let mut read: Vec<String> =
            CREATE_FLAGS.iter().map(|(f, _)| (*f).to_string()).collect();
        for part in body.split("== \"--").skip(1) {
            if let Some(rest) = part.split('"').next() {
                read.push(format!("--{rest}"));
            }
        }
        assert!(
            read.len() > CREATE_FLAGS.len(),
            "no `== \"--flag\"` switch found in the body — the scan sees only the value table, \
             so a switch could be unknown and this test would not say so"
        );

        let known = create_known_flags();
        for f in &read {
            assert!(
                known.contains(&f.as_str()),
                "`site create` reads `{f}` but the unknown-flag check does not know it — the \
                 command would refuse the flag it was just given the code to handle"
            );
            assert!(
                CREATE_USAGE.contains(f.as_str()),
                "`{f}` is accepted but missing from the usage line the refusal prints, which \
                 is the only place a user finds out what IS accepted"
            );
        }
        // And nothing is advertised that the command never reads — a flag
        // accepted and silently dropped is the failure this file already had.
        for f in known {
            assert!(read.contains(&f.to_string()), "`{f}` is accepted but nothing reads it");
        }
    }

    /// **No flag is ever looked for in a list that has flags filtered out.**
    ///
    /// `cmd_wp` builds `rest` as the POSITIONAL words —
    /// `words.iter().skip(3).filter(|w| !w.starts_with("--"))` — so searching
    /// `rest` for a `--flag` matches nothing, ever. It is not a subtle bug: the
    /// branch is dead, and whatever it guards takes its default forever.
    ///
    /// It shipped. `wp user delete` read BOTH of its forks out of `rest`, so
    /// `--reassign` and `--delete-posts` were invisible, the "say what happens
    /// to their posts" refusal fired on every call, and the verb could not be
    /// completed by any combination of arguments. L0 held the server's rule and
    /// the login-or-id resolver; nothing exercised the parse. A live run on
    /// 3 Sep 2026 found it in four commands.
    ///
    /// One case would be a regression test. This is the class: a filtered list
    /// and a flag lookup must never meet, in any command.
    #[test]
    fn a_flag_is_never_looked_for_in_the_positional_list() {
        const ME: &str = include_str!("main.rs");

        // PRODUCTION code only, both halves. A dead branch inside a test is
        // not shipped behaviour, and scanning the test module makes a scan
        // capable of matching its own assertion text — which is how the
        // sibling guard below first "found" itself.
        let prod = ME.split("\n#[cfg(test)]").next().unwrap_or(ME);
        assert!(prod.len() > 50_000, "the test-module split ate the file — the scan is blind");

        // Every name bound to a `--`-filtered collection.
        let mut filtered: Vec<&str> = Vec::new();
        for line in prod.lines() {
            if !line.contains("!w.starts_with(\"--\")") {
                continue;
            }
            if let Some(name) = line.split("let ").nth(1).and_then(|r| {
                r.split([':', ' ', '=']).next()
            }) {
                if !name.is_empty() {
                    filtered.push(name);
                }
            }
        }
        assert!(
            !filtered.is_empty(),
            "no `--`-filtered list found — the scan is looking at the wrong shape, and a green \
             here would mean nothing"
        );

        let mut dead: Vec<String> = Vec::new();
        for (i, line) in prod.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if !code.contains("\"--") {
                continue;
            }
            for name in &filtered {
                for probe in [
                    format!("{name}.iter()"),
                    format!("{name}.contains("),
                    format!("{name}.first()"),
                ] {
                    if code.contains(&probe) {
                        dead.push(format!("line {}: {}", i + 1, code.trim()));
                    }
                }
            }
        }
        assert!(
            dead.is_empty(),
            "a flag is being looked for in a list the `--` words were filtered OUT of, so the \
             branch is dead and whatever it guards keeps its default forever:\n  {}\nRead the \
             flag from `words`.",
            dead.join("\n  ")
        );
    }

    /// **No roadmap row says a command is unrun that the same file records as
    /// run.**
    ///
    /// Live-run status has ONE owner — the in-app-verifies list — and the table
    /// rows kept a second copy. On 3 Sep 2026 four of them ("Not live-run yet")
    /// were contradicted by a paragraph in that list naming the same commands,
    /// written the same hour: `site domains`, `site restart`, `service restart`
    /// and `wp user delete` had all just been driven against a live app.
    ///
    /// This is the sibling of
    /// `no_roadmap_row_calls_unbuilt_a_thing_the_cli_already_dispatches`, and a
    /// DIFFERENT claim: that gate asks whether the CLI has the verb (answerable
    /// from `main.rs`); this one asks whether it has been exercised live, which
    /// only the file's own record can answer. So the two halves are checked
    /// against each other rather than against the code.
    #[test]
    fn no_row_calls_a_command_unrun_that_the_verifies_list_records_as_run() {
        let doc = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/CLI-ROADMAP.md"),
        )
        .expect("docs/CLI-ROADMAP.md");
        // Only what is INSIDE the markers. The owed list and the run record sit
        // in the same item and read alike to a scanner — matching the whole
        // section reported `mail clear` and `tunnel start` as contradictions
        // when the file had them right, so the boundary is marked, not guessed.
        // BOTH markers must be present. Taking "everything after the open
        // marker" when the close is missing silently widens the region to
        // include the OWED list — which is the exact confusion the markers
        // exist to end, so a missing close has to be an error, not a default.
        let after = doc
            .split("<!-- live-run-record")
            .nth(1)
            .expect("the live-run record's opening marker");
        let end = after
            .find("<!-- /live-run-record -->")
            .expect("the live-run record's CLOSING marker — without it the region would run \
                     on into the owed list and this gate would stop seeing contradictions");
        let verifies = &after[..end];
        assert!(
            verifies.contains("VERIFIED") || verifies.contains("RUN LIVE"),
            "nothing inside the live-run markers records a run — the markers moved, and a \
             green here would mean nothing"
        );

        let mut stale: Vec<String> = Vec::new();
        for line in doc.lines() {
            let line = line.trim();
            if !line.starts_with("| `") || !line.contains("ot live-run") {
                continue;
            }
            let cmd = line
                .replace("\\|", "\u{1}")
                .split('`')
                .nth(1)
                .unwrap_or("")
                .to_string();
            // The bare verb words: placeholders and option groups are not part
            // of a command's name.
            let words: Vec<String> = cmd
                .split_whitespace()
                .filter(|w| !w.starts_with('<') && !w.starts_with('[') && !w.starts_with("--"))
                .flat_map(|w| w.split('\u{1}').map(str::to_string).collect::<Vec<_>>())
                .filter(|w| !w.is_empty())
                .collect();
            // Every 2- and 3-word phrase the row's name can make. A row is
            // stale if the verifies list names any of them.
            for n in [2usize, 3] {
                for w in words.windows(n) {
                    let phrase = w.join(" ");
                    if verifies.contains(&phrase) {
                        stale.push(format!("`{cmd}` — the verifies list names `{phrase}`"));
                    }
                }
            }
        }
        assert!(
            stale.is_empty(),
            "these rows say a command has not been run live, and the in-app-verifies list in \
             the SAME FILE says it has:\n  {}\nLive-run status has one owner — correct the row, \
             or say what part is still owed",
            stale.join("\n  ")
        );
    }

    /// **`wp search-replace` refuses a flag it does not know, and will not take
    /// a flag as the text to write.**
    ///
    /// The worst flag set in the CLI. `--dry-runn --yes` leaves `dry` false and
    /// removes the prompt in one stroke, so a rehearsal runs as a real replace
    /// across every table; and `search-replace old --dry-run new` reads `to` as
    /// the flag and writes the literal string `--dry-run` into every row it
    /// matches. Both were silent, and `db export` is the only way back from
    /// either.
    #[test]
    fn search_replace_refuses_an_unknown_flag_and_a_flag_as_a_value() {
        const ME: &str = include_str!("main.rs");
        let arm = ME
            .split("(Some(\"search-replace\"), from_word) => {")
            .nth(1)
            .and_then(|b| b.split("\n        (Some(").next())
            .expect("the search-replace arm");

        // What the arm READS, from the `== \"--x\"` comparisons — a different
        // syntactic form from the declared list, so deleting one does not
        // delete the evidence for the other.
        let reads: Vec<String> = arm
            .split("== \"--")
            .skip(1)
            .filter_map(|p| p.split('"').next())
            .map(|r| format!("--{r}"))
            .collect();
        assert!(!reads.is_empty(), "no flag comparison found in the arm — the scan moved");

        // What it DECLARES, from the call site.
        let declared: Vec<String> = arm
            .split("reject_unknown_flags(words, \"wp search-replace\", &[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .expect("search-replace must call reject_unknown_flags")
            .split(',')
            .map(|t| t.trim().trim_matches('"').to_string())
            .filter(|t| t.starts_with("--"))
            .collect();

        for r in &reads {
            assert!(
                declared.contains(r),
                "the arm reads `{r}` but does not declare it, so the command would refuse the \
                 flag it was given the code to handle"
            );
        }
        assert!(
            declared.iter().any(|d| d == "--dry-run"),
            "`--dry-run` must be accepted — refusing it would make the rehearsal unreachable"
        );
        // The positional guard, which is the other half of the same danger.
        assert!(
            arm.contains("from.starts_with(\"--\")") && arm.contains("to.starts_with(\"--\")"),
            "a flag can still be taken as <from> or <to>, and the replacement WRITES it"
        );
    }

    /// The predicate behind every unknown-flag refusal.
    #[test]
    fn an_unknown_flag_is_found_and_a_known_one_is_not() {
        let w = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        let known = ["--dry-run", "--yes"];
        assert_eq!(unknown_flag(&w("old new --dry-run --yes"), &known), None);
        assert_eq!(
            unknown_flag(&w("old new --dry-runn --yes"), &known).map(String::as_str),
            Some("--dry-runn"),
            "a one-letter typo is the whole failure — it must be the thing that is named"
        );
        // Values are never flags, however flag-shaped the text is.
        assert_eq!(unknown_flag(&w("old new"), &known), None);
        assert_eq!(unknown_flag(&w("-x old"), &known), None, "a single dash is not our shape");
    }

    /// **The CLI never rebuilds a log key the app already sends.**
    ///
    /// `repo watch --tail` follows a file named by a server-side rule
    /// (`repo-<domain>-<dir>-watch.log`). Rebuilding that name here would work
    /// until the day the rule changes, and then `--tail` would follow a file
    /// nobody writes — silently, because an absent log tails as empty.
    /// `site_resources_check` re-derived a database name exactly this way and
    /// failed on every install that had an imported site (ledger #390), which
    /// is why the key rides in the watcher SNAPSHOT instead.
    #[test]
    fn the_watch_tail_reads_its_log_key_and_never_composes_one() {
        const ME: &str = include_str!("main.rs");
        assert!(
            ME.contains("w[\"logKey\"]"),
            "`--tail` must take the log key from the watcher snapshot"
        );
        // PRODUCTION code only — the scan matched its own assertion line, which
        // is the same self-agreement trap as #463's first version wearing a
        // different hat: a test that can satisfy itself proves nothing.
        let prod = ME.split("\n#[cfg(test)]").next().unwrap_or(ME);
        assert!(prod.len() > 50_000, "the test-module split ate the file — the scan is blind");
        for (i, line) in prod.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !(code.contains("format!") && code.contains("-watch.log")),
                "line {}: the CLI is composing a watch log name. The app sends it as `logKey` — \
                 compose it here and a rename in commands/repo.rs leaves `--tail` following a \
                 file nobody writes: {}",
                i + 1,
                code.trim()
            );
        }
        // And the older-app case is answered, not guessed at.
        assert!(
            ME.contains("does not report the watcher's log file"),
            "an app that predates `logKey` must be told apart from a watcher with no output"
        );
    }

}
