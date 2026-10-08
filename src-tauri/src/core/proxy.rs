//! core::proxy — edge router (Caddy) config generation + supervision (task 4.2).
//!
//! Caddy terminates TLS for every local site using the per-site certs issued
//! by our local CA (3.2) via explicit `tls <cert> <key>` directives. Caddy's
//! automatic HTTPS (ACME + its own internal CA) is therefore never used — the
//! chain the browser sees is signed by the CA trusted in 3.4. Caddy proxies to
//! the shared Nginx (one upstream per site by Host).

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;

pub const CADDYFILE: &str = "Caddyfile";
pub const DEFAULT_HTTP_PORT: u16 = 80;
pub const DEFAULT_HTTPS_PORT: u16 = 443;
/// rexenv drives Caddy's admin API over a **unix socket** (not the default
/// unauthenticated TCP `:2019`) so the ROOT edge exposes no local-TCP control
/// surface — any local process reaching `:2019` could otherwise POST config to a
/// root Caddy = arbitrary file read/write as root (task 2.3 / H5). The socket lives
/// in our config dir with owner-only (0600) perms; for the privileged edge it is
/// chown'd to the invoking user so reload/stop stay promptless (see
/// [`start_privileged`]). A fixed path so a leftover edge is found deterministically.
pub const ADMIN_SOCKET_FILE: &str = "caddy-admin.sock";

/// Response header only OUR edge emits (stamped into every generated site
/// block) — the positive wire-identity marker for [`edge_answers_as_ours`].
pub const EDGE_MARKER_HEADER: &str = "X-Rexenv-Edge";

/// Path the wire probe requests (see [`edge_answers_as_ours`]). The internal
/// Adminer site block answers it AT the edge (`respond 204`, marker already
/// stamped) so the every-10s health probe never reaches nginx/PHP — no probe
/// noise drowning real requests in the user-visible nginx access log, and no
/// Adminer page render per tick. Only the internal tooling vhost carries the
/// route: user sites reserve no paths.
pub const EDGE_PROBE_PATH: &str = "/__rexenv-probe";

/// Path of Caddy's admin unix socket under the platform config dir.
pub fn admin_socket_path(platform: &dyn Platform) -> Result<PathBuf> {
    // Deliberately NOT length-checked here. Computing this path is not the same
    // as binding it: `admin_alive` probes it, `reload`/`stop` connect to it, and
    // a sandboxed edge runs with admin OFF and never binds it at all. A check at
    // construction turned a working example into a hard failure (14 Aug 2026) —
    // the guard belongs at the bind, in `start_privileged`.
    Ok(platform.paths().config_dir()?.join(ADMIN_SOCKET_FILE))
}

/// Longest unix-socket path that will bind on macOS: `sun_path[104]` counts the
/// NUL, so 103 bytes of path. **Measured 14 Aug 2026** by binding at increasing
/// lengths until it failed, not read off a header — the number that matters is
/// the one the kernel enforces.
pub const MAX_UNIX_SOCKET_PATH: usize = 103;

/// Refuse a socket path the kernel cannot bind, WITH the numbers.
///
/// The alternative is what a user actually gets today: the edge fails to start
/// and caddy says `bind: invalid argument`. That names nothing — not the path,
/// not the limit, not the fact that a length is the problem — and it reaches the
/// user as "rexenv won't start" with nothing to search for. It cost an hour to
/// diagnose from a log; from a UI it is undiagnosable.
///
/// Headroom is not generous. The path is
/// `<home>/Library/Application Support/dev.rexenv.rexenv/config/caddy-admin.sock`
/// — 70 bytes after `/Users/<name>`, so a home directory name over 26 characters
/// overflows. Short account names are fine; `firstname.lastname` homes and
/// network/AD mounts are the ones that get close.
pub fn check_unix_socket_len(path: &Path) -> Result<()> {
    // `OsStr::len` is the byte length of the platform encoding — on unix exactly
    // `as_bytes().len()` — and, unlike `OsStrExt`, it compiles for Windows.
    let len = path.as_os_str().len();
    if len <= MAX_UNIX_SOCKET_PATH {
        return Ok(());
    }
    Err(crate::error::Error::Other(format!(
        "the edge's admin socket path is {len} bytes and macOS cannot bind more than \
         {MAX_UNIX_SOCKET_PATH}:\n  {}\n\nThis is a PATH LENGTH problem, not a permissions \
         one — the kernel reports it as \"bind: invalid argument\". rexenv keeps this socket \
         beside its application-support data, so the length is driven by your home directory. \
         Moving rexenv's data directory to a shorter path fixes it.",
        path.display()
    )))
}

/// What is on the other end of loopback `:443`.
///
/// Three states, because "not ours" is two different problems with two
/// different fixes, and every caller of the boolean below has been telling the
/// user the wrong one. `commands/valet_import.rs` is the case that forced this:
/// it probes without checking whether rexenv's own stack is running, so
/// importing before Start-all told people *"another app is answering port 443 —
/// quit it"* when nothing was answering at all. Advice to quit a program that
/// does not exist is worse than no message.
///
/// # The ambiguous shape resolves toward `Foreign`, deliberately
///
/// The two failure directions are not equal. A false `NoAnswer` HIDES a real
/// blocker — the user is told the port is free while someone else serves every
/// site. A false `Foreign` merely sends them looking for a program that is not
/// there, which the holder lookup then fails to name. So anything that is not
/// provably "nothing is listening" is reported as `Foreign`.
///
/// What that means concretely is settled by measurement, not by reading
/// reqwest's docs — see `examples/edge_wire_check.rs`, which puts a real
/// refused port, a real bare TCP listener and a real foreign HTTPS responder in
/// front of this and asserts the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeWire {
    /// The marker header — our edge, positively identified.
    Ours,
    /// Something answered, or something is listening and did not complete an
    /// exchange we could read. Includes every ambiguous case.
    Foreign,
    /// Nothing is listening. The connection was refused.
    NoAnswer,
}

/// How long the listening probe waits for loopback to REFUSE before it stops
/// being able to prove the port empty.
///
/// Half a second is enormous on macOS loopback, where a closed port refuses
/// in microseconds. **Windows refuses a closed loopback port in ~2 s** — measured
/// 19 Sep 2026 on two machines (a clean Windows 11 24H2 VM: 2313 / 2033 /
/// 2058 ms for :443 / :18088 / :9999; the Windows 10 22H2 Dell, native x64:
/// 2271 / 2038 / 2034 ms), so the number is Windows', not the emulation's. With
/// the 500 ms wait every Windows install read a bare :443 as a TIMEOUT, the
/// timeout resolved toward `Foreign` as designed, and onboarding's last step
/// told every user "Another app is answering HTTPS on this PC" with nothing
/// there — seen on the first installed copy, `docs/SMOKE-TEST.md`. The ambiguity
/// rule is right; the wait simply has to be longer than the OS's own answer.
/// `the_listen_probe_outlasts_the_os_refusal` pins the two numbers.
pub const LISTEN_PROBE_WAIT: std::time::Duration =
    std::time::Duration::from_millis(if cfg!(target_os = "windows") { 3_000 } else { 500 });

/// Whether OUR edge is what actually ANSWERS loopback `:443` — the DNS
/// `answers_as_ours` pattern applied to HTTPS. `admin_alive()` proves our caddy
/// PROCESS runs; it cannot prove the wire is ours: on macOS a foreign proxy that
/// binds `127.0.0.1:443` SPECIFICALLY coexists with our wildcard `*:443` bind
/// (both binds succeed — no error anywhere) and the kernel hands loopback
/// connections to the most-specific listener. Observed live: Herd's nginx
/// answered every site with its own 404 while our edge sat green.
///
/// Probe: request [`EDGE_PROBE_PATH`] on `<host>` pinned to `127.0.0.1:443`
/// (no DNS involved) and require the [`EDGE_MARKER_HEADER`] our config stamps.
/// Any HTTP status counts: a 502 from OUR edge still proves the wire is ours. A
/// current edge answers the path itself (204 at the edge, no nginx round-trip);
/// an older surviving edge just proxies it through as a 404 that still carries
/// the marker — detection is config-version-independent. Connection failure or
/// a server without the marker → false.
///
/// MARKER-ONLY, deliberately — do NOT re-add a `Server: Caddy` fallback:
/// - It's a FALSE-POSITIVE: any foreign Caddy on loopback `:443` (a dev's own)
///   sends `Server: Caddy` and would be mis-identified as OUR edge — the exact
///   "never mistake a foreign Caddy for ours" (M1) violation this probe exists
///   to catch. The marker is a POSITIVE ID only our config stamps.
/// - It's unnecessary: our real edge always emits the marker on this exact
///   response — the `respond @rexenv_probe 204` short-circuit carries
///   `X-Rexenv-Edge: 1` because Caddy orders `header` before `respond`
///   (verified on the bundled Caddy). A "pre-marker rexenv edge" can't reach a
///   shipped user (the marker predates the first release), and adoption is keyed
///   on the private admin socket — not this probe — and reloads the marker-
///   bearing config, so nothing here gates an edge start on the result.
///
/// Probe loopback `:443` and say which of the three it is.
///
/// TWO probes, in this order, and the order is the finding. "Is anything
/// listening" is answered by a raw TCP connect, NOT by reqwest's error
/// taxonomy — measured 13 Aug 2026 (`edge_wire_check` leg C): a plaintext HTTP
/// server on the port makes reqwest report `is_connect() == true`, because the
/// TLS handshake is part of establishing the connection. Keying `NoAnswer` on
/// that filed a REAL BLOCKER as an empty port — the user would be told :443 is
/// free while another server answered every site. That is the failure direction
/// that hides something, which is why the mapping was measured before it was
/// trusted and why the check that measured it is permanent.
pub async fn edge_wire(host: &str, https_port: u16) -> EdgeWire {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], https_port));
    // A REFUSED connect is the only proof that nothing is listening. A timeout
    // proves nothing, so it resolves toward `Foreign` with every other
    // ambiguous case — which is why the wait must outlast how long THIS OS
    // takes to refuse (`LISTEN_PROBE_WAIT`).
    match tokio::time::timeout(LISTEN_PROBE_WAIT, tokio::net::TcpStream::connect(addr)).await {
        Ok(Err(_)) => return EdgeWire::NoAnswer,
        Ok(Ok(_)) => {}
        Err(_elapsed) => return EdgeWire::Foreign,
    }

    // Something is listening. The only question left is whether it is ours, and
    // the marker header is the sole positive authority.
    let url = format!("https://{host}:{https_port}{EDGE_PROBE_PATH}");
    let Ok(client) = reqwest::Client::builder()
        // Our local-CA leaf won't chain for reqwest's store; identity comes from
        // the marker header, not the chain.
        .danger_accept_invalid_certs(true)
        .resolve(host, addr)
        .timeout(std::time::Duration::from_secs(3))
        .build()
    else {
        return EdgeWire::Foreign;
    };
    match client.get(&url).send().await {
        Ok(resp) if probe_response_is_ours(resp.headers()) => EdgeWire::Ours,
        // Answered without the marker, or would not complete an exchange at all
        // — either way something holds the port and it is not us.
        _ => EdgeWire::Foreign,
    }
}

/// Is OUR edge what answers loopback `:443`?
///
/// The original question, kept because four callers ask exactly it. Both
/// not-ours states collapse to `false` here, which is EXACTLY what they did
/// before [`EdgeWire`] existed — `the_boolean_still_means_what_it_meant` pins
/// that, so introducing the richer state could not quietly change any of them.
pub async fn edge_answers_as_ours(host: &str, https_port: u16) -> bool {
    edge_wire(host, https_port).await == EdgeWire::Ours
}

/// A probe response is OUR edge iff it carries the marker header — the sole,
/// positive authority. Pure so the marker-only contract (and the removal of the
/// old `Server: Caddy` false-positive) is unit-testable without a TLS mock: a
/// re-added Server-based branch would flip a foreign-Caddy case and fail the test.
fn probe_response_is_ours(headers: &reqwest::header::HeaderMap) -> bool {
    headers.contains_key(EDGE_MARKER_HEADER)
}

/// Caddy admin address for the CLI `--address` / Caddyfile `admin` directive.
///
/// Caddy splits an address at its FIRST slash and takes everything after it as the path
/// (`SplitNetworkAddress`, Caddy 2.11.4). rexenv has always written `unix//` + an absolute Unix
/// path — `unix///Users/…`, whose path `//Users/…` Unix reads as `/Users/…` — and that is kept
/// byte-identical: a running edge's admin listener is keyed by the string, and an upgrade that
/// changed it would ask Caddy to rebind its own socket on the next reload. A Windows path does not
/// start with `/`, and the extra slash became part of it: measured on the Dell (14 Sep 2026,
/// `windows_edge_probe`), `unix//C:\…` named the socket `/C:\…` — Caddy refused to start ("cannot
/// reuse socket … already in use") and `caddy reload`/`stop` could not dial it ("An invalid argument
/// was supplied") — while `unix/C:\…` started, served and answered. So such a path gets ONE slash.
fn admin_address(sock: &Path) -> String {
    let path = sock.display().to_string();
    if path.starts_with('/') {
        format!("unix//{path}")
    } else {
        format!("unix/{path}")
    }
}

/// Whether OUR edge's admin socket is accepting connections (liveness). The socket
/// FILE persists after a crash, so we actually connect rather than stat the path.
/// Dialled through `LocalIpc` — the socket is OS-specific, and `core/` is not.
pub fn admin_alive(platform: &dyn Platform) -> bool {
    match admin_socket_path(platform) {
        Ok(sock) => platform.local_ipc().connect(&sock, None).is_ok(),
        Err(_) => false,
    }
}

/// One site's TLS termination + upstream.
#[derive(Debug, Clone)]
pub struct SiteRoute {
    /// Host served, e.g. `mysite.rex`.
    pub host: String,
    /// Also match `*.host` (subdomain multisite, §10.2). The wildcard sub-site
    /// hosts share this site's backend + wildcard cert; a more-specific exact
    /// host (any other site) still wins, so it can't overshadow `other.rex`.
    pub wildcard: bool,
    /// Extra hostnames this site also answers on (v42) — additional addresses
    /// on the SAME site block, sharing its cert and upstream.
    pub aliases: Vec<String>,
    /// Upstream `host:port` Caddy proxies to (the shared Nginx).
    pub upstream: String,
    /// The user stopped THIS site (v44): `Some(dir)` is the site's OWN
    /// stopped-page directory. The route is still emitted — with its
    /// certificate — and answers 503 from that page instead of proxying.
    ///
    /// Dropping the route instead would hand the browser a TLS failure or, for
    /// a neighbouring subdomain-multisite block, someone else's site: a
    /// "your machine is broken" screen for a state the user deliberately chose.
    /// A 503 that says so is the honest answer, and keeping the address here is
    /// also what stops the hostname falling through to a `*.` matcher.
    pub stopped: Option<PathBuf>,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Edge-router configuration.
#[derive(Debug, Clone)]
pub struct CaddyConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub routes: Vec<SiteRoute>,
    /// Admin API unix socket to bind (`Some` in production via [`admin_socket_path`]).
    /// `None` omits the `admin` directive (Caddy's default TCP admin) — tests only.
    pub admin_socket: Option<PathBuf>,
    /// Caddy's `default_bind` for the site ports, when the platform wants one
    /// (`EdgeSupervisor::default_bind`; Windows: `127.0.0.1`, ledger #611). `None` binds every
    /// interface, as macOS always has.
    pub default_bind: Option<String>,
}

impl Default for CaddyConfig {
    fn default() -> Self {
        Self {
            http_port: DEFAULT_HTTP_PORT,
            https_port: DEFAULT_HTTPS_PORT,
            routes: Vec::new(),
            admin_socket: None,
            default_bind: None,
        }
    }
}

/// Render the Caddyfile. Explicit per-site `tls` means Caddy never invokes its
/// internal issuer/ACME (it uses the loaded local-CA certs). Auto-HTTPS is left
/// on so Caddy also binds `http_port` and 308-redirects HTTP→HTTPS for every
/// site host — `http://site.rex` lands on `https://site.rex`.
pub fn generate_caddyfile(cfg: &CaddyConfig) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("\thttp_port {}\n", cfg.http_port));
    s.push_str(&format!("\thttps_port {}\n", cfg.https_port));
    if let Some(sock) = &cfg.admin_socket {
        // Bind the admin API to a unix socket instead of the default TCP :2019 (H5).
        // Quote the whole token — app-data paths contain spaces; `|0600` sets
        // owner-only perms (Caddy 2.7+; the privileged edge additionally chowns it).
        // The address is `admin_address`'s: one slash for a Windows path (measured, see there).
        s.push_str(&format!("\tadmin \"{}|0600\"\n", admin_address(sock)));
    }
    if let Some(bind) = &cfg.default_bind {
        // Windows: loopback only — an all-interfaces bind raised the firewall's allow prompt (#611).
        s.push_str(&format!("\tdefault_bind {bind}\n"));
    }
    s.push_str("}\n");
    for r in &cfg.routes {
        s.push('\n');
        // Subdomain multisite serves the apex plus every sub-site host; Caddy
        // matches the most-specific site address first, so the exact hosts of
        // other sites are never shadowed by this `*.host` matcher.
        let mut addrs = if r.wildcard {
            vec![format!("https://{host}", host = r.host), format!("https://*.{host}", host = r.host)]
        } else {
            vec![format!("https://{}", r.host)]
        };
        for alias in &r.aliases {
            addrs.push(format!("https://{alias}"));
            if r.wildcard {
                addrs.push(format!("https://*.{alias}"));
            }
        }
        let site_addr = addrs.join(", ");
        s.push_str(&format!("{site_addr} {{\n"));
        // Quote paths: app-data paths contain spaces ("Application Support").
        s.push_str(&format!(
            "\ttls \"{}\" \"{}\"\n",
            r.cert_path.display(),
            r.key_path.display()
        ));
        // Positive wire identity (see `edge_answers_as_ours`): only OUR edge
        // stamps this response header. Needed because a foreign proxy can bind
        // 127.0.0.1:443 SPECIFICALLY and shadow our wildcard :443 listener with
        // no bind error anywhere (observed live with Herd) — process liveness
        // alone can't detect that the wire belongs to someone else.
        s.push_str(&format!("\theader {EDGE_MARKER_HEADER} \"1\"\n"));
        // The health wire-probe polls EDGE_PROBE_PATH every 10s; the internal
        // tooling vhost answers it at the edge so the probe never reaches
        // nginx (`respond` orders before `reverse_proxy`). Marker is already
        // stamped above, so the 204 still carries the wire identity.
        if r.host == crate::core::adminer::ADMINER_HOST {
            s.push_str(&format!("\t@rexenv_probe path {EDGE_PROBE_PATH}\n"));
            s.push_str("\trespond @rexenv_probe 204\n");
        }
        if let Some(page_dir) = &r.stopped {
            // No upstream at all for a stopped site — not an unreachable proxy.
            // A `reverse_proxy` at a dead port answers 502 "Bad Gateway", which
            // reads as a broken machine.
            //
            // The page is a FILE, served through the error handler, because
            // Caddy cannot `respond` with one and a Caddyfile string is the
            // wrong home for HTML — every `{` in its CSS would be read as a
            // placeholder. `error` + `handle_errors` keeps the 503 status while
            // the body is the real page (`core::stopped_page`).
            s.push_str(&format!("\troot * \"{}\"\n", page_dir.display()));
            s.push_str("\terror * \"site stopped\" 503\n");
            s.push_str("\thandle_errors {\n");
            s.push_str(&format!("\t\trewrite * {}\n", crate::core::stopped_page::request_path()));
            s.push_str("\t\tfile_server\n");
            s.push_str("\t}\n");
        } else {
            s.push_str(&format!("\treverse_proxy {}\n", r.upstream));
        }
        s.push_str("}\n");
    }
    s
}

/// Write the Caddyfile under the platform config dir; returns its path.
pub fn write_caddyfile(platform: &dyn Platform, cfg: &CaddyConfig) -> Result<PathBuf> {
    let dir = platform.paths().config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(CADDYFILE);
    std::fs::write(&path, generate_caddyfile(cfg))?;
    Ok(path)
}

/// Start Caddy with the given Caddyfile via `ProcessSupervisor`; returns the
/// child handle (track its pid to `stop`).
pub fn start(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<Child> {
    let args = vec![
        "run".to_string(),
        "--config".to_string(),
        caddyfile.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
    ];
    // Caddy logs (JSON) to stderr — capture it to a per-service log file.
    let log = platform.paths().log_dir()?.join("caddy-stdout.log");
    platform.supervisor().spawn_logged(caddy_bin, &args, &log)
}

/// Stop a running Caddy by pid (for the non-privileged `start`).
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// Single-quote a path for the /bin/sh command (app-data paths contain spaces).
fn sh_quote(path: &Path) -> String {
    // Single-quote wrap for the shell (paths contain spaces), POSIX-escaping any
    // embedded `'` as `'\''` so it can't break OUT of the quotes into the root
    // command context. Identity for `'`-free paths (every rexenv path is), so
    // real commands are byte-identical — belt-and-suspenders on the root path
    // (B12), matching the cli.rs quote discipline.
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Start Caddy on privileged ports (:80/:443) as root via `PrivilegeManager`
/// (one admin prompt). `caddy start` backgrounds the server and returns once
/// it's up, so the prompt doesn't block. Afterwards, [`reload`] and [`stop_admin`]
/// drive it through Caddy's localhost admin API — no further prompts.
pub fn start_privileged(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path) -> Result<()> {
    let sock = admin_socket_path(platform)?;
    // THIS is where the path is bound, so this is where a length that cannot be
    // bound has to be refused — with the numbers, because the kernel's own answer
    // is "bind: invalid argument" and a user cannot diagnose that from a UI.
    check_unix_socket_len(&sock)?;
    let appdata = platform.paths().app_data_dir()?;
    // Start the edge as root (binds :80/:443), THEN hand its admin unix socket to the
    // invoking user with 0600 perms so rexenv can drive reload/stop with NO further
    // prompt while no other local process (or user) can reach it (task 2.3 / H5). The
    // uid comes from our app-data dir's owner (the invoking user), so it works for any
    // account. `&&` keeps a failed `caddy start` a failure (the chown group's exit
    // code must not mask it); the chown/chmod are best-effort (`|| true`).
    //
    // `caddy start` MUST have its output redirected away from osascript's pipes: the
    // detached `caddy run` it spawns inherits them and holds the write ends open for
    // its whole lifetime, and `do shell script` waits for EOF — so without the
    // redirect run_privileged blocks until the edge EXITS (the edge serves fine, but
    // start_services never finishes and the edge reads as stopped). A log file keeps
    // the start diagnostics without keeping the pipe; on failure it is replayed to
    // stderr (the pipe is safe then — a failed start leaves no live child) so the
    // surfaced error stays actionable.
    let start_log = platform.paths().log_dir()?.join("caddy-start.log");
    let cmd = format!(
        "{caddy} start --config {cfg} --adapter caddyfile >{log} 2>&1 \
         || {{ cat {log} 1>&2 ; exit 1 ; }} ; {{ \
         for i in 1 2 3 4 5 6 7 8 9 10; do [ -S {sock} ] && break; sleep 0.2; done ; \
         chown $(stat -f %u {appdata}) {sock} 2>/dev/null || true ; \
         chmod 600 {sock} 2>/dev/null || true ; }}",
        caddy = sh_quote(caddy_bin),
        cfg = sh_quote(caddyfile),
        sock = sh_quote(&sock),
        appdata = sh_quote(&appdata),
        log = sh_quote(&start_log),
    );
    platform.privileges().run_privileged(
        &cmd,
        &crate::platform::traits::PromptReason::new("start its HTTPS server on ports 80 and 443"),
    )?;
    Ok(())
}

/// Start the edge under the OS supervisor (macOS: a root LaunchDaemon with
/// `KeepAlive`) so it stays up across ANY death — external SIGTERM (the incident
/// this replaced), crash, sleep/wake, logout, reboot — with NO health-watchdog
/// restart (which can't clear the admin-password prompt a privileged start needs).
///
/// One privileged prompt: rexenv writes the plist + launcher CONTENTS to a staging
/// dir UNPRIVILEGED (plain writes — no shell-escaping of multi-line files), then the
/// single [`EdgeSupervisor::install_command`] `cp`s them into the root-owned tree
/// (binary locked `root:wheel`, never re-exec of the user-writable cache — LPE guard)
/// and bootstraps launchd. This runs on every non-adopted start (idempotent), so it
/// doubles as recovery: its `bootout` stops any prior/wedged edge — freeing `:443` —
/// before the fresh `bootstrap` rebinds, and it always deploys the current launcher
/// (self-healing a stale one). The launcher keeps the 0600 admin socket owned by the
/// invoking user for caddy's whole life, so reload/stop stay promptless.
///
/// `src_caddy` is our resolved caddy cache path (the install SOURCE). Waits for the
/// admin socket to answer before returning so the caller can mark the edge live.
pub fn start_edge_daemon(platform: &dyn Platform, src_caddy: &Path, caddyfile: &Path) -> Result<()> {
    let edge = platform.edge();
    let sock = admin_socket_path(platform)?;
    let appdata = platform.paths().app_data_dir()?;
    let start_log = platform.paths().log_dir()?.join("caddy-start.log");

    // Always (re)install: stage the launcher + plist unprivileged, then the ONE
    // privileged cp+bootstrap. This is idempotent AND self-healing — a first install,
    // a binary refresh after a version bump, and recovery of a WEDGED daemon (e.g. a
    // stale launcher whose socket rexenv can't reach) all take the same path. The
    // `install_command`'s `bootout` cleanly stops any prior edge FIRST, which frees
    // `:443` before the fresh `bootstrap` rebinds it — so a reinstall doubles as the
    // port-conflict recovery. The launcher execs the ROOT binary copy.
    let staging = platform.paths().config_dir()?.join("edge-daemon");
    std::fs::create_dir_all(&staging)?;
    let staged_wrapper = staging.join("edge-launch.sh");
    let staged_plist = staging.join("edge.plist");
    std::fs::write(
        &staged_wrapper,
        edge.wrapper_contents(&edge.daemon_binary_path(), caddyfile, &sock, &appdata),
    )?;
    std::fs::write(&staged_plist, edge.plist_contents(&edge.wrapper_path(), &start_log))?;
    platform.privileges().run_privileged(
        &edge.install_command(src_caddy, &staged_wrapper, &staged_plist),
        &crate::platform::traits::PromptReason::new("start its HTTPS server on ports 80 and 443"),
    )?;

    // Bootstrap/kickstart returns before caddy has bound the socket; wait for it
    // (the wrapper also needs a beat to chown the socket to us) so the edge reads
    // live to the caller — same bounded poll as the old privileged start.
    for _ in 0..25 {
        if admin_alive(platform) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    // Most common real-world cause: another local proxy (Herd — the target user's
    // other tool — ships its own nginx/caddy) already holds :443/:80, so our edge
    // crash-loops under launchd instead of binding. NAME the holder honestly; a
    // generic "socket never came up" cost a real user a confused debugging session.
    for port in [DEFAULT_HTTPS_PORT, DEFAULT_HTTP_PORT] {
        let help = platform.supervisor().port_conflict_help(port, false);
        if let Some(holder) = help.holder {
            let quit = help.app.unwrap_or_else(|| "that app".into());
            let fix = help.free_command.map(|c| format!("\n$ {c}")).unwrap_or_default();
            return Err(crate::error::Error::Other(format!(
                "the Caddy edge could not start: port {port} is already used by {holder}. \
                 Quit {quit} (or stop its proxy), then Start all again.{fix}"
            )));
        }
    }
    Err(crate::error::Error::Other(
        "the Caddy edge daemon was installed but its admin socket never came up — \
         check logs/caddy-start.log."
            .to_string(),
    ))
}

/// EXPLICITLY stop the edge daemon (Stop-all). With `KeepAlive` a graceful
/// `caddy stop` is instantly relaunched, so a real stop goes through the supervisor's
/// own switch (macOS: lower the ON-switch file the plist's `KeepAlive` watches, then
/// `kill` — the job stays loaded, so the next start registers nothing new with
/// Background Task Management, #773; a plist from before the switch is still
/// `disable` + `bootout`) — a privileged op (one prompt). Waits for the admin
/// socket to go quiet. No-op / best-effort if the daemon isn't installed.
pub fn stop_edge_daemon(platform: &dyn Platform) -> Result<()> {
    platform.privileges().run_privileged(
        &platform.edge().stop_command(),
        &crate::platform::traits::PromptReason::new("stop its HTTPS server on ports 80 and 443"),
    )?;
    for _ in 0..20 {
        if !admin_alive(platform) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(crate::error::Error::Other(
        "the Caddy edge daemon was stopped but its admin socket is still answering.".to_string(),
    ))
}

/// Reload Caddy's config via its admin API (no privilege needed even though
/// Caddy may run as root). Run after writing a new Caddyfile (e.g. site added).
///
/// `force` matters for **cert rotation**: cert paths are stable, so a re-issued
/// leaf leaves the Caddyfile byte-identical — and Caddy SKIPS a reload whose
/// config is unchanged, keeping the OLD cert in its in-memory cache until the
/// edge restarts. `--force` re-provisions the tls app so the file loaders
/// re-read the cert files. Pass `false` for config-shape changes (site
/// add/delete/switch), `true` after re-issuing certs.
pub fn reload(platform: &dyn Platform, caddy_bin: &Path, caddyfile: &Path, force: bool) -> Result<()> {
    let sock = admin_socket_path(platform)?;
    let mut args = vec![
        "reload".to_string(),
        "--config".to_string(),
        caddyfile.display().to_string(),
        "--adapter".to_string(),
        "caddyfile".to_string(),
        // Reach the admin over our unix socket, not the (now absent) TCP :2019.
        "--address".to_string(),
        admin_address(&sock),
    ];
    if force {
        args.push("--force".to_string());
    }
    wait_ok(platform.supervisor().spawn(caddy_bin, &args)?, "caddy reload")
}

/// Stop Caddy via its admin API (no privilege) over our unix socket.
pub fn stop_admin(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    let sock = admin_socket_path(platform)?;
    wait_ok(
        platform.supervisor().spawn(
            caddy_bin,
            &["stop".to_string(), "--address".to_string(), admin_address(&sock)],
        )?,
        "caddy stop",
    )
}

/// Recover from a LEFTOVER edge before starting ours (Phase 2 §7.3).
///
/// A Caddy orphaned by a prior run (commonly a **root** edge from a `:443` start)
/// keeps holding `:443` and its admin socket. rexenv's admin socket is chown'd to
/// the invoking user, so we can stop even a root edge without privilege: detect a
/// live leftover on our socket and stop it so a fresh start isn't blocked.
///
/// No-op when our admin socket has no live listener. Returns an actionable error if
/// the leftover edge can't be stopped. Ownership-gated: because we probe/stop OUR
/// private socket (never the default TCP `:2019`), a developer's own Caddy on `:2019`
/// is never touched by launch (task 2.4 / M1).
pub fn recover_stale_edge(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    if !admin_alive(platform) {
        return Ok(());
    }
    if !crate::core::stack_guard::may_control_real_stack() {
        // A live listener on the admin socket is most likely the USER'S serving
        // edge (root LaunchDaemon) — a live-check example must refuse to kill it
        // rather than take the fresh-start path over the shared socket.
        return Err(crate::error::Error::Other(format!(
            "a live rexenv edge is on the admin socket and this process is not the \
             rexenv app — refusing to stop it (stack guard). If this teardown is \
             deliberate, set {}=1 or call stack_guard::allow_real_stack_control().",
            crate::core::stack_guard::ALLOW_ENV
        )));
    }
    log::warn!(
        "rexenv: a leftover rexenv Caddy edge is live on its admin socket; \
         stopping it via the admin API so startup isn't blocked"
    );
    // Best-effort: `caddy stop` POSTs to the admin socket; judge success by the
    // socket actually going quiet below.
    let _ = stop_admin(platform, caddy_bin);
    for _ in 0..10 {
        if !admin_alive(platform) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(crate::error::Error::Other(
        "the leftover Caddy edge would not stop via its admin socket. Stop it and retry \
         (e.g. `caddy stop`, or kill the leftover caddy process)."
            .to_string(),
    ))
}

/// Stop the Caddy edge via its admin API (our unix socket) and wait for it to go
/// quiet. Works on a **root** edge (the socket is chown'd to us, so no privilege
/// needed) and on a stray edge we never tracked (a leftover from a prior run). No-op
/// if our socket has no live listener. This is what `stop_all` uses so "Stop all"
/// reliably frees `:443`/`:80`.
///
/// Ownership-gated (task 2.4 / M1): both steps act ONLY on rexenv's own edge — the
/// admin stop targets our private unix socket (a foreign Caddy on the default TCP
/// `:2019` is invisible to us), and the reap matches our own caddy binary path. A
/// developer's own Caddy is never stopped by launch or Stop-all.
pub fn stop_edge(platform: &dyn Platform, caddy_bin: &Path) -> Result<()> {
    if !crate::core::stack_guard::may_control_real_stack() {
        // Live-check example: the edge on the shared admin socket is the USER'S
        // — skip the stop instead of tearing down every site (stack guard).
        log::warn!(
            "rexenv: stack guard — not the rexenv app; leaving the live edge \
             running (set {}=1 to override)",
            crate::core::stack_guard::ALLOW_ENV
        );
        return Ok(());
    }
    // 1) Graceful: if our edge's admin socket is live, stop via the API + wait for it.
    if admin_alive(platform) {
        let _ = stop_admin(platform, caddy_bin);
        for _ in 0..10 {
            if !admin_alive(platform) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // 2) Reap any Caddy still running OUR binary — including a wedged, listener-less
    //    edge the admin API can't reach (e.g. a crashed start that lost its sockets).
    //    Best-effort: killing a ROOT edge needs privilege, so a root remnant may
    //    survive — it holds no ports, and we log it rather than fail.
    let marker = caddy_bin.display().to_string();
    for pid in platform.supervisor().owned_pids(&marker) {
        let _ = platform.supervisor().stop(pid);
    }
    let survivors = platform.supervisor().owned_pids(&marker);
    if !survivors.is_empty() {
        log::warn!(
            "rexenv: {} Caddy process(es) using our binary could not be reaped \
             (likely a root edge — needs privilege): {survivors:?}",
            survivors.len()
        );
    }

    // 3) If our admin socket is somehow STILL live, surface it.
    if admin_alive(platform) {
        return Err(crate::error::Error::Other(
            "the Caddy edge would not stop via its admin socket after `caddy stop`.".to_string(),
        ));
    }
    Ok(())
}

/// Upper bound for a `caddy reload`/`stop` admin CLI call. Healthy calls over the
/// local unix socket return near-instantly; a wedged edge (socket accepts but
/// never answers — the orphan-worker class) would otherwise hang forever.
const ADMIN_CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn wait_ok(child: Child, what: &str) -> Result<()> {
    wait_ok_within(child, what, ADMIN_CLI_TIMEOUT)
}

/// Wait for a `caddy` admin CLI child, BOUNDED. `admin_alive` only checks the
/// socket *accepts*; a wedged edge accepts the reload/stop connection then never
/// answers, and Go's admin HTTP client has no request timeout — so an unbounded
/// `child.wait()` hangs forever, freezing the caller and any lock it holds (B5).
/// Poll `try_wait` to a deadline (mirroring the file's `admin_alive` poll loops),
/// then kill + reap. Timeout is a parameter so it's unit-testable.
fn wait_ok_within(mut child: Child, what: &str, timeout: std::time::Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(crate::error::Error::Other(format!(
                    "{what} failed (exit {})",
                    crate::core::proc::exit_text(status.code())
                )))
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait(); // reap the killed child — no zombie
                    return Err(crate::error::Error::Other(format!(
                        "{what} timed out after {}s — the edge admin socket accepted the \
                         connection but never answered.",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn admin_socket_path_refuses_a_length_the_kernel_cannot_bind() {
        use super::{check_unix_socket_len, MAX_UNIX_SOCKET_PATH};
        use std::path::PathBuf;
        // The ceiling itself is MEASURED (14 Aug 2026: binding at increasing
        // lengths, 103 bound and 104 did not). This guard is about the message:
        // caddy's own words for this are "bind: invalid argument", which names
        // neither the path nor the limit and reaches a user as "rexenv won't
        // start". The real one is 82 bytes on a 5-character home directory, so
        // the headroom is ~26 characters of home-directory name — small enough
        // to reach a real person with a long one.
        let ok = PathBuf::from("/Users/wpdev/Library/Application Support/dev.rexenv.rexenv/config/caddy-admin.sock");
        assert!(ok.as_os_str().len() <= MAX_UNIX_SOCKET_PATH);
        assert!(check_unix_socket_len(&ok).is_ok(), "the ordinary path must pass");

        let long = PathBuf::from(format!("/Users/{}/Library/Application Support/dev.rexenv.rexenv/config/caddy-admin.sock", "n".repeat(40)));
        let e = check_unix_socket_len(&long).unwrap_err().to_string();
        // The message must carry the diagnosis, not just refuse: both numbers
        // and the path, or it is the same dead end with a different spelling.
        assert!(e.contains(&long.display().to_string()), "must name the path: {e}");
        assert!(e.contains(&MAX_UNIX_SOCKET_PATH.to_string()), "must name the limit: {e}");
        assert!(e.contains(&long.as_os_str().len().to_string()), "must name the actual length: {e}");
        assert!(e.contains("bind: invalid argument"), "must connect it to what the kernel says: {e}");

        // The boundary is the byte, not a round number near it.
        let at = PathBuf::from(format!("/{}", "a".repeat(MAX_UNIX_SOCKET_PATH - 1)));
        assert_eq!(at.as_os_str().len(), MAX_UNIX_SOCKET_PATH);
        assert!(check_unix_socket_len(&at).is_ok(), "exactly at the limit must pass");
        let over = PathBuf::from(format!("/{}", "a".repeat(MAX_UNIX_SOCKET_PATH)));
        assert!(check_unix_socket_len(&over).is_err(), "one byte over must fail");
    }

    use super::*;

    /// The onboarding notice's must-say list, and the ONE VOICE check across
    /// every place this app explains a `:443` conflict.
    ///
    /// Guard lives here because `proxy` owns the fact all five sentences are
    /// about (#197). Five surfaces say it — onboarding, the provision card, the
    /// import toast, the watchdog event and `verify_edge_wire` — deliberately
    /// in different words, because they are different situations. What they must
    /// NOT do is sound like five authors: one verb for the fix (quit), one
    /// consequence (won't load), and the real holder NAMED rather than guessed.
    /// The provision card said "most likely Herd" while every other surface
    /// named the actual process.
    #[test]
    fn the_onboarding_edge_notice_says_what_it_costs_and_that_continuing_is_fine() {
        let onboarding = crate::core::copy_scan::strip_ts_comments(include_str!(
            "../../../src/routes/Onboarding.tsx"
        ));
        assert!(
            onboarding.contains("function EdgeConflictNotice"),
            "the stripper ate the source — every check below would pass on an empty string"
        );
        assert!(
            !onboarding.contains("LOAD-BEARING"),
            "comment text survived the strip; prose can satisfy this guard again"
        );

        for (phrase, why) in [
            (
                "is answering HTTPS on ",
                "WHAT is wrong, in the reader's terms. At onboarding they have no sites and no \
                 mental model of an edge, so the Start-all sentence (\"services are running, \
                 but…\") is meaningless here",
            ),
            (
                "rexenv needs port 443 to serve sites",
                "WHY it matters — and it names the port, which is the vocabulary every other \
                 :443 message in the app uses",
            ),
            (
                "you can finish setting up",
                "LOAD-BEARING, and the clause a trim removes as reassurance. It is the \
                 warn-not-block decision MADE VISIBLE: onboarding needs nothing on :443, and \
                 someone trying rexenv with Herd running is in a deliberate state. Without it \
                 this reads as a wall in the first two minutes of the product",
            ),
            (
                "whenever you like",
                "the fix is not urgent and saying so is the point — the surfaces that DO need \
                 the port say it again when they need it",
            ),
        ] {
            assert!(
                onboarding.contains(phrase),
                "the onboarding edge notice no longer says {phrase:?} — {why}"
            );
        }

        // ONE VOICE. Every surface names the holder it was GIVEN; none invents
        // one, and none guesses.
        for (file, src) in [
            ("Onboarding.tsx", onboarding.as_str()),
            (
                "SiteProvisionCard.tsx",
                include_str!("../../../src/components/sites/SiteProvisionCard.tsx"),
            ),
        ] {
            assert!(
                !src.contains("most likely Herd"),
                "{file} guesses at the holder. Every other :443 message names the process the \
                 supervisor actually found; a guess in one of five is how a product starts \
                 sounding like five people."
            );
        }
    }

    /// Every caller of the TRI-STATE must say something about `NoAnswer`.
    ///
    /// The whole point of the enum is that "not ours" was two situations with
    /// two fixes, told to the user as one. A caller that takes `edge_wire` and
    /// then folds `NoAnswer` back into the foreign branch has re-created the
    /// bug while looking like it uses the richer state — so the tree is scanned
    /// rather than trusted. Each of the four says something DIFFERENT, because
    /// the same variant means a different thing in each: the stack isn't
    /// running (import), our edge is alive but silent (watchdog, doctor), our
    /// own start didn't take (verify_edge_wire).
    #[test]
    fn every_caller_of_the_tristate_handles_no_answer() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut callers: Vec<String> = Vec::new();
        fn walk(dir: &std::path::Path, callers: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, callers);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let body = crate::core::copy_scan::production_source(&text);
                // `proxy.rs` DEFINES it; everyone else calls it.
                if path.file_name().and_then(|f| f.to_str()) == Some("proxy.rs") {
                    continue;
                }
                if body.contains("edge_wire(") {
                    let named = path
                        .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    assert!(
                        body.contains("NoAnswer"),
                        "{named} asks `edge_wire` for the three states and then never mentions \
                         `NoAnswer`, so it is telling the user the foreign-proxy story when \
                         nothing is listening. That is the bug the enum exists to end — and on \
                         the import path it shipped, telling people to quit a program that was \
                         not running."
                    );
                    callers.push(named);
                }
            }
        }
        walk(&root, &mut callers);
        callers.sort();
        assert!(
            callers.len() >= 3,
            "the detection found {} tri-state callers — it has stopped working. Expected the \
             import path, the watchdog and doctor at least: {callers:?}",
            callers.len()
        );
    }

    /// The four callers that gate Start-all, login autostart, the watchdog and
    /// doctor ask the BOOLEAN. It has to stay one expression over the tri-state,
    /// not a second probe that agrees today and drifts later — the exact shape
    /// that put four different wrong messages in front of users in the first
    /// place. Structural, because no unit test can reach a socket: this asserts
    /// there is ONE probe.
    #[test]
    fn the_boolean_still_means_what_it_meant() {
        let src = crate::core::copy_scan::production_source(include_str!("proxy.rs"));
        let body = src
            .split("pub async fn edge_answers_as_ours(")
            .nth(1)
            .and_then(|b| b.split("\n}").next())
            .expect("edge_answers_as_ours");
        let dense: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            dense.contains("edge_wire(host,https_port).await==EdgeWire::Ours"),
            "`edge_answers_as_ours` is no longer `edge_wire(…) == Ours`. Four callers depend on \
             it meaning exactly what it meant before the tri-state existed, and a second \
             implementation is how they start disagreeing: {body}"
        );
        // …and only one place actually probes.
        assert_eq!(
            src.matches("reqwest::Client::builder()").count(),
            1,
            "a second probe client appeared in proxy.rs — `edge_wire` is the one place the \
             wire is identified"
        );
    }

    /// The probe's wait has to outlast how long the OS takes to REFUSE a closed
    /// loopback port, or "empty" is unprovable and every install reports a
    /// foreign proxy. Windows measured at 2.0–2.3 s on two machines (19 Sep
    /// 2026); macOS refuses at once. A wait that is shorter than the refusal is
    /// the bug this commit fixed, and a wait that grows on macOS slows four
    /// callers for nothing.
    #[test]
    fn the_listen_probe_outlasts_the_os_refusal() {
        let wait = LISTEN_PROBE_WAIT.as_millis();
        if cfg!(target_os = "windows") {
            assert!(wait >= 2_500, "Windows refuses loopback in ~2.3 s; the probe waits {wait} ms");
        } else {
            assert_eq!(wait, 500, "the macOS wait moved for no measured reason");
        }
    }

    #[test]
    fn probe_identity_is_marker_only_not_server_caddy() {
        use reqwest::header::{HeaderMap, HeaderValue, SERVER};
        let marker = reqwest::header::HeaderName::from_static("x-rexenv-edge");

        // Our edge: the marker is present → ours (the 204 short-circuit carries it).
        let mut ours = HeaderMap::new();
        ours.insert(&marker, HeaderValue::from_static("1"));
        assert!(probe_response_is_ours(&ours), "marker present → ours");

        // A foreign CADDY (Server: Caddy, NO marker): the false-positive the old
        // fallback caused — must now read FALSE (M1: never mistake a foreign Caddy
        // for ours). This is the regression guard for the dropped fallback.
        let mut foreign_caddy = HeaderMap::new();
        foreign_caddy.insert(SERVER, HeaderValue::from_static("Caddy"));
        assert!(!probe_response_is_ours(&foreign_caddy), "foreign Caddy → NOT ours");

        // Marker present with a NON-Caddy Server (e.g. reverse-proxied through
        // nginx): still ours — the marker is the sole authority, independent of
        // the Server line.
        let mut marker_via_nginx = HeaderMap::new();
        marker_via_nginx.insert(&marker, HeaderValue::from_static("1"));
        marker_via_nginx.insert(SERVER, HeaderValue::from_static("nginx"));
        assert!(probe_response_is_ours(&marker_via_nginx), "marker wins over Server: nginx");

        // A foreign nginx shadow (Herd-class: no marker, no Server: Caddy) →
        // false, matching the wire_probe_check example's shadow assertion.
        assert!(!probe_response_is_ours(&HeaderMap::new()), "no marker → not ours");
    }

    #[test]
    fn sh_quote_is_byte_identical_for_real_paths_and_escapes_a_quote() {
        // Every real rexenv path is single-quote-free → byte-identical wrap.
        for p in ["/usr/lib", "/App Support/dev.rexenv.rexenv/bin/caddy", "/a-b_c.d/e"] {
            assert_eq!(sh_quote(Path::new(p)), format!("'{p}'"), "unchanged for real paths");
        }
        // A path containing `'` can't break out — it's POSIX-escaped to `'\''`.
        assert_eq!(sh_quote(Path::new("/x/o'brien")), "'/x/o'\\''brien'");
    }

    #[test]
    fn wait_ok_within_times_out_and_kills_a_wedged_admin_cli() {
        // A `caddy reload`/`stop` whose admin socket accepts but never answers must
        // be bounded and killed, not waited on forever (B5). A `sleep 30` child
        // stands in for the wedged CLI.
        let child = crate::test_support::live_child();
        let start = std::time::Instant::now();
        let e = wait_ok_within(child, "caddy reload", std::time::Duration::from_millis(300))
            .unwrap_err();
        assert!(e.to_string().contains("timed out"), "{e}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "must not hang past the deadline: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn wait_ok_within_reports_success_and_failure() {
        let ok = crate::test_support::exiting_child(0);
        assert!(wait_ok_within(ok, "caddy reload", std::time::Duration::from_secs(5)).is_ok());

        let bad = crate::test_support::exiting_child(3);
        let e = wait_ok_within(bad, "caddy stop", std::time::Duration::from_secs(5)).unwrap_err();
        assert!(e.to_string().contains("failed"), "{e}");
    }

    fn sample() -> CaddyConfig {
        CaddyConfig {
            http_port: 8080,
            https_port: 8443,
            routes: vec![SiteRoute {
                aliases: Vec::new(),
                host: "proxytest.test".into(),
                wildcard: false,
                upstream: "127.0.0.1:9999".into(),
                cert_path: "/c/cert.pem".into(),
                key_path: "/c/key.pem".into(),
                stopped: None,
            }],
            admin_socket: None,
            default_bind: None,
        }
    }

    #[test]
    fn caddyfile_uses_explicit_tls_not_internal_issuer() {
        let f = generate_caddyfile(&sample());
        // Auto-HTTPS left on (no `disable_redirects`) so Caddy opens :80 and
        // redirects HTTP→HTTPS; explicit per-site tls still bypasses the issuer.
        assert!(!f.contains("disable_redirects"));
        assert!(f.contains("http_port 8080"));
        assert!(f.contains("https_port 8443"));
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("tls \"/c/cert.pem\" \"/c/key.pem\""));
        assert!(f.contains("reverse_proxy 127.0.0.1:9999"));
        // Every site block stamps the wire-identity marker — the ONLY reliable
        // way to tell our edge from a foreign proxy shadow-binding 127.0.0.1:443.
        assert!(f.contains("header X-Rexenv-Edge \"1\""));
        // Never falls back to Caddy's internal CA.
        assert!(!f.to_lowercase().contains("internal"));
    }

    #[test]
    fn admin_binds_unix_socket_not_tcp() {
        // No admin_socket → no admin directive emitted (tests only).
        let mut cfg = sample();
        assert!(!generate_caddyfile(&cfg).contains("admin "));
        // With a socket → admin bound to a quoted unix socket with 0600 perms, and
        // NOT the default unauthenticated TCP :2019 (task 2.3 / H5). The quoting also
        // covers app-data paths that contain spaces ("Application Support").
        cfg.admin_socket = Some("/x/app data/config/caddy-admin.sock".into());
        let f = generate_caddyfile(&cfg);
        assert!(
            f.contains("admin \"unix///x/app data/config/caddy-admin.sock|0600\""),
            "caddyfile:\n{f}"
        );
        assert!(!f.contains("2019"));
    }

    /// The admin address a Windows path gets (ledger #610): ONE slash. Caddy splits an address at
    /// its first slash, and `unix//C:\…` named the socket `/C:\…` — Caddy refused to start on it and
    /// its CLI could not dial it (measured on the Dell, 14 Sep 2026). The Unix string keeps every
    /// byte, and the Caddyfile's `admin` and the CLI's `--address` are the same string.
    #[test]
    fn a_windows_admin_path_gets_one_slash_and_a_unix_path_keeps_its_bytes() {
        let win = Path::new(r"C:\Users\x\AppData\Local\rexenv\config\caddy-admin.sock");
        assert_eq!(admin_address(win), r"unix/C:\Users\x\AppData\Local\rexenv\config\caddy-admin.sock");
        let mac = Path::new("/Users/x/Library/Application Support/dev.rexenv.rexenv/config/caddy-admin.sock");
        assert_eq!(
            admin_address(mac),
            "unix///Users/x/Library/Application Support/dev.rexenv.rexenv/config/caddy-admin.sock"
        );
        let mut cfg = sample();
        cfg.admin_socket = Some(win.to_path_buf());
        let f = generate_caddyfile(&cfg);
        assert!(f.contains(&format!("admin \"{}|0600\"", admin_address(win))), "caddyfile:\n{f}");
        assert!(!f.contains("unix//C:"), "caddyfile:\n{f}");
    }

    /// The Windows edge binds loopback only (ledger #611, owner ruling 14 Sep 2026): a platform
    /// `default_bind` becomes Caddy's global `default_bind`; without one the Caddyfile says nothing,
    /// so macOS keeps binding every interface exactly as before.
    #[test]
    fn a_default_bind_is_a_global_option_and_absent_otherwise() {
        let mut cfg = sample();
        let f = generate_caddyfile(&cfg);
        assert!(!f.contains("default_bind"), "no platform bind, no directive:\n{f}");
        cfg.default_bind = Some("127.0.0.1".into());
        let f = generate_caddyfile(&cfg);
        let global = f.split("\n}\n").next().unwrap_or("");
        assert!(global.lines().any(|l| l == "\tdefault_bind 127.0.0.1"), "not in the global block:\n{f}");
        assert_eq!(f.matches("default_bind").count(), 1, "one global option, not per site:\n{f}");
    }

    #[test]
    fn caddyfile_renders_each_route() {
        let mut cfg = sample();
        cfg.routes.push(SiteRoute {
            host: "two.test".into(),
            wildcard: false,
            upstream: "127.0.0.1:9001".into(),
            cert_path: "/c/2.pem".into(),
            key_path: "/c/2.key".into(),
            stopped: None,
            aliases: Vec::new(),
        });
        let f = generate_caddyfile(&cfg);
        assert!(f.contains("https://proxytest.test {"));
        assert!(f.contains("https://two.test {"));
        assert_eq!(f.matches("reverse_proxy").count(), 2);
    }

    /// **A stopped site keeps its address and its certificate, and proxies
    /// nowhere.**
    ///
    /// Three things are asserted together because each one alone is the wrong
    /// shape: no `reverse_proxy` (an upstream is what "stopped" removes), the
    /// `tls` line still there (a dropped route means a browser interstitial —
    /// the loudest failure this product has — for a state the user chose), and
    /// the neighbouring site's block untouched (stopping one site is the whole
    /// point, and the shared web server serves everyone else).
    #[test]
    fn a_stopped_site_answers_503_and_leaves_its_neighbour_alone() {
        let mut cfg = sample();
        cfg.routes[0].stopped = Some("/appdata/config/stopped/site-id".into());
        cfg.routes.push(SiteRoute {
            host: "neighbour.test".into(),
            wildcard: false,
            upstream: "127.0.0.1:9001".into(),
            cert_path: "/c/2.pem".into(),
            key_path: "/c/2.key".into(),
            stopped: None,
            aliases: Vec::new(),
        });
        let f = generate_caddyfile(&cfg);

        let stopped_block = f
            .split("https://proxytest.test {")
            .nth(1)
            .and_then(|b| b.split("\n}").next())
            .expect("the stopped site still has a block");
        assert!(
            !stopped_block.contains("reverse_proxy"),
            "a stopped site must proxy nowhere:\n{stopped_block}"
        );
        assert!(
            stopped_block.contains("503"),
            "a stopped site must answer 503 — the status is what every non-browser \
             client reads:\n{stopped_block}"
        );
        assert!(
            stopped_block.contains("tls \"/c/cert.pem\" \"/c/key.pem\""),
            "the certificate must survive being stopped — otherwise starting the site \
             again shows the browser a name it has never been given a cert for:\n{stopped_block}"
        );
        // The body is a PAGE — rexenv's own (`core::stopped_page`), served
        // through the error handler because Caddy cannot `respond` with a file
        // and a Caddyfile string cannot hold CSS. Asserted as the three
        // directives that make it work, since any one of them missing is a
        // different (and silent) failure: no root = 404 from the handler, no
        // handle_errors = Caddy's own bare error text, no rewrite = a directory
        // listing at the address of a site the user believes is switched off.
        assert!(stopped_block.contains("root * "), "{stopped_block}");
        assert!(stopped_block.contains("handle_errors"), "{stopped_block}");
        assert!(
            stopped_block.contains(&crate::core::stopped_page::request_path()),
            "{stopped_block}"
        );

        let neighbour = f
            .split("https://neighbour.test {")
            .nth(1)
            .and_then(|b| b.split("\n}").next())
            .expect("neighbour block");
        assert!(
            neighbour.contains("reverse_proxy 127.0.0.1:9001"),
            "stopping one site changed another one:\n{neighbour}"
        );
    }

    /// Extra domains are ADDRESSES on the site's own block — same cert, same
    /// upstream — never a second block.
    ///
    /// A second block would need its own certificate and its own upstream line,
    /// which is two places to keep in step for one site, and the failure is
    /// silent: the alias keeps serving the old backend after a server switch.
    #[test]
    fn extra_domains_are_addresses_on_the_same_site_block() {
        let mut cfg = sample();
        cfg.routes[0].aliases = vec!["shop.test".into()];
        let f = generate_caddyfile(&cfg);
        assert!(
            f.contains("https://proxytest.test, https://shop.test {"),
            "the alias must be an address on the same block: {f}"
        );
        assert_eq!(f.matches("reverse_proxy").count(), 1, "one site, one upstream: {f}");
        assert_eq!(f.matches("tls ").count(), 1, "one site, one certificate: {f}");

        // A subdomain network takes the wildcard for every one of its names,
        // matching what nginx's `server_name` does for the same site.
        cfg.routes[0].wildcard = true;
        let f = generate_caddyfile(&cfg);
        assert!(
            f.contains(
                "https://proxytest.test, https://*.proxytest.test, https://shop.test, \
                 https://*.shop.test {"
            ),
            "a network's alias needs its wildcard too: {f}"
        );
    }

    #[test]
    fn probe_route_only_on_internal_adminer_block() {
        let mut cfg = sample();
        cfg.routes.push(SiteRoute {
            host: crate::core::adminer::ADMINER_HOST.into(),
            wildcard: false,
            upstream: "127.0.0.1:18088".into(),
            cert_path: "/c/a.pem".into(),
            key_path: "/c/a.key".into(),
            stopped: None,
            aliases: Vec::new(),
        });
        let f = generate_caddyfile(&cfg);
        // The probe endpoint is answered at the edge, exactly once, on the
        // internal tooling vhost — the 10s health probe never reaches nginx
        // (no access-log noise), and user sites reserve no paths.
        assert_eq!(f.matches(&format!("@rexenv_probe path {EDGE_PROBE_PATH}")).count(), 1);
        assert_eq!(f.matches("respond @rexenv_probe 204").count(), 1);
        let adminer_at = f.find("https://adminer.rexenv.rex {").unwrap();
        let user_block = &f[..adminer_at];
        assert!(!user_block.contains("respond"), "user site block gained a probe route");
        // The adminer block still stamps the marker and proxies everything
        // else — probe replies carry the wire identity, Adminer keeps working.
        let adminer_block = &f[adminer_at..];
        assert!(adminer_block.contains("header X-Rexenv-Edge \"1\""));
        assert!(adminer_block.contains("reverse_proxy 127.0.0.1:18088"));
    }

    #[test]
    fn subdomain_multisite_route_matches_wildcard_without_shadowing_others() {
        let mut cfg = sample();
        // A subdomain-multisite site: apex + wildcard sub-site hosts.
        cfg.routes.push(SiteRoute {
            host: "mysite.test".into(),
            wildcard: true,
            upstream: "127.0.0.1:18088".into(),
            cert_path: "/c/m.pem".into(),
            key_path: "/c/m.key".into(),
            stopped: None,
            aliases: Vec::new(),
        });
        let f = generate_caddyfile(&cfg);
        // The site address carries both the apex and the wildcard sub-site host.
        assert!(f.contains("https://mysite.test, https://*.mysite.test {"));
        // The wildcard is scoped to mysite.test — a plain site keeps its own exact
        // (non-wildcard) address, so it is never shadowed by `*.mysite.test`.
        assert!(f.contains("https://proxytest.test {"));
        assert!(!f.contains("*.proxytest.test"));
    }
}
