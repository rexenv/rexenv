//! core::adminer — serve the bundled Adminer DB browser as an internal vhost (§5.2).
//!
//! Adminer is a single `adminer.php` (§5.1). We serve it through the shared stack
//! on a fixed internal host (`adminer.rexenv.rex`) rooted at an isolated docroot,
//! behind the edge (TLS, local CA). It is deliberately NOT a `Site` in the DB —
//! so it can never be selected as a public tunnel origin (§9): tunnels are scoped
//! to a single site's Host, and this internal vhost isn't one.

use crate::core::binaries;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::path::PathBuf;

/// Custom webview scheme the in-app Database Browser loads Adminer through
/// (`rexdb://localhost/…`). The scheme handler proxies to the stack over
/// loopback with a RUST-SIDE cookie jar ([`forward`]) because WebKit withholds
/// third-party cookies inside a cross-site `<iframe>` (ITP): the session cookie
/// never survived the login POST, so every login — auto or manual — bounced
/// straight back to the form. With the jar in Rust, browser cookie policy is
/// out of the picture entirely. (Windows webviews expect `http://rexdb.localhost/`
/// instead — Phase 4.)
pub const PROXY_SCHEME: &str = "rexdb";

/// Internal host Adminer is served on (resolved by the embedded DNS (backbone `.rex` resolver)).
/// NEVER a public tunnel origin (§9) — it isn't a site.
pub const ADMINER_HOST: &str = "adminer.rexenv.rex";

/// FLOOR for the largest SQL dump the Adminer vhost accepts, in bytes — nginx's
/// `client_max_body_size` for the vhost AND the PHP upload/post limits it runs
/// with ([`import_php_value`]), set together from one number so nginx never 413s
/// a body PHP would take. Before this cap the vhost inherited the 128m
/// http-level default and 413'd every larger dump — with no setting anywhere
/// that could raise it.
///
/// A FLOOR, not the value: the vhost takes the LARGER of this and the default
/// pool's own configured limit (`php::nginx_body_limits`), so it can only ever
/// raise what the user already asked for. A flat cap looked simpler and was
/// wrong — a user running the default pool at `upload_max_filesize = 6G` would
/// have had Adminer, alone, silently held to 2G by the thing meant to unblock
/// imports. Importing a big dump is this vhost's whole job, and it's
/// loopback-only (§5.2), so it gets the generous floor for free.
pub const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The vhost's actual cap: the floor, or the default pool's own configured
/// limit when that's larger (`None` = the pool stores neither upload key, so
/// PHP's small defaults apply and only the floor matters).
pub fn import_cap(pool_limit: Option<u64>) -> u64 {
    pool_limit.unwrap_or(0).max(MAX_IMPORT_BYTES)
}

/// Per-request `PHP_VALUE` ini lines for the Adminer vhost, for a cap of
/// `bytes` — the SAME number the vhost's `client_max_body_size` gets, derived
/// once so nginx and PHP cannot disagree. php-fpm applies these over the pool's
/// OVERRIDABLE `php_value[…]` lines, so the shared default pool — and every site
/// on it — keeps the user's own upload limits.
///
/// Plain byte integers, not `2048M`: the cap is a `u64` and PHP's ini parser
/// takes a bare number as bytes, so no shorthand rounding sits between the two
/// values.
pub fn import_php_value(bytes: u64) -> String {
    format!("upload_max_filesize={bytes}\npost_max_size={bytes}")
}

/// FastCGI read/send timeout for the Adminer vhost, in seconds.
///
/// Replaying a large dump takes minutes; nginx's own default is 60s, so an
/// import 504'd while php-fpm kept importing — the user got a failure page over
/// a job still running, and a database left in an unknown middle state (SQL
/// replay is not wrapped in a transaction). nginx must never be the component
/// that gives up here: php-fpm's `request_terminate_timeout` is the guard that
/// actually ENDS the work.
///
/// The value is the SettingKind bound on `max_execution_time` (`php::SETTINGS`),
/// not a guess: `request_terminate_timeout` is `max(max_execution_time, 300)`,
/// and `max_execution_time` can't be stored above 86400 — so this is ≥ every
/// terminate value the app can produce, for every version, whatever the user
/// sets later. A per-version derived timeout would have to be recomputed on
/// every settings change and would silently under-shoot if that recompute were
/// ever missed; this can't drift, and a test pins it to the spec's own maximum.
pub const IMPORT_TIMEOUT_SECS: u64 = 86_400;

/// Query flag a rexenv deep-link sets to request a one-click scoped session (§11.4).
pub const AUTOLOGIN_FLAG: &str = "rexenv_auto";

/// The deep-link wrapper served as `index.php`. It customizes Adminer via the
/// `adminer_object()` hook (defined in the GLOBAL namespace — Adminer checks the
/// global function name) so a rexenv "Open database" link lands the user straight
/// in the site's DB instead of on a manual login screen:
///   - `login()` accepts a passwordless login, but ONLY for the loopback engines
///     rexenv manages (MySQL root / Postgres have no local password) — never a
///     remote host;
///   - `loginForm()` auto-submits Adminer's OWN rendered form (which already
///     carries the CSRF token + CSP nonce) when `?rexenv_auto` is set, guarded by
///     sessionStorage so a failed login can't loop;
///   - `headers()`/`csp()` replace Adminer's blanket `X-Frame-Options: deny` with
///     `frame-ancestors` scoped to the rexenv webview origins, so the in-app
///     Database Browser `<iframe>` renders while any OTHER site framing this
///     passwordless vhost (clickjacking from a page in the user's browser —
///     `adminer.rexenv.rex` resolves locally for them too) stays blocked;
///   - a `header_register_callback` rewrites every `Set-Cookie` to
///     `SameSite=None; Secure; Partitioned`: inside the app the iframe is a
///     cross-site embed (webview origin ≠ `adminer.rexenv.rex`), so Adminer's
///     default `SameSite=lax` session/key cookies are withheld from the login
///     POST and every login bounces back to the form. `Partitioned` (CHIPS)
///     keeps the embedded jar isolated per top-level site, and Adminer's own
///     CSRF token still guards every state-changing request.
///
/// SECURITY: Adminer here is an INTERNAL vhost (`adminer.rexenv.rex` → 127.0.0.1)
/// behind the local edge and is NEVER a public tunnel origin (§9), so passwordless
/// loopback access stays confined to the local machine.
///
/// `pub` so `examples/adminer_login_gate_check` can extract the loopback gate
/// (between the `rexenv-loopback-gate` markers) and prove it through real PHP.
pub const WRAPPER_INDEX_PHP: &str = r#"<?php
// rexenv Adminer deep-link wrapper (§11.4) — generated; do not edit by hand.

// The Database Browser embeds this vhost in a cross-site <iframe> (webview
// origin != adminer.rexenv.rex), where SameSite=lax cookies are withheld and
// the login POST loses its session. Rewrite every cookie to
// "SameSite=None; Secure; Partitioned" at flush time (covers the PHP session
// cookie and Adminer's own key/permanent cookies alike). CSRF stays covered by
// Adminer's per-session token; framing stays limited by frame-ancestors below.
header_register_callback(function () {
    $rewritten = array();
    foreach (headers_list() as $h) {
        if (stripos($h, 'Set-Cookie:') === 0) {
            $v = preg_replace('/;\s*SameSite=\w+/i', '', trim(substr($h, 11)));
            if (stripos($v, 'secure') === false) {
                $v .= '; Secure';
            }
            $rewritten[] = $v . '; SameSite=None; Partitioned';
        }
    }
    if ($rewritten) {
        header_remove('Set-Cookie');
        foreach ($rewritten as $v) {
            header('Set-Cookie: ' . $v, false);
        }
    }
});

// rexenv-loopback-gate:start
// Passwordless login is granted ONLY when the target server's HOST is EXACTLY a
// loopback name. \Adminer\SERVER is request-controlled (auth[server] / ?server= /
// ?pgsql=), so a PREFIX test (a starts-with match on the SERVER string) once
// accepted "127.0.0.1.evil.com" and handed a passwordless session to a REMOTE
// server — a malicious server can then use `LOAD DATA LOCAL INFILE` to read local
// files off this machine. Strip an optional :port (or MySQL :socket, or bracketed
// IPv6) and compare the bare host to the allow-list. See docs/archive/CODEBASE-REVIEW.md B1.
// (examples/adminer_login_gate_check runs this exact function through PHP.)
if (!function_exists('rexenv_is_loopback')) {
    function rexenv_is_loopback($server) {
        $server = (string) $server;
        if ($server === '') {
            return false;
        }
        if ($server[0] === '[') {
            // Bracketed IPv6 literal, optional ":port": [::1] or [::1]:3306.
            $end = strpos($server, ']');
            $host = $end === false ? '' : substr($server, 1, $end - 1);
        } elseif (substr_count($server, ':') === 1) {
            // host:port (or host:/socket for MySQL) — take the host.
            $host = substr($server, 0, strpos($server, ':'));
        } else {
            // Bare host, or a bare IPv6 literal (::1) which has more than one colon.
            $host = $server;
        }
        return in_array(strtolower($host), array('127.0.0.1', '::1', 'localhost'), true);
    }
}
// rexenv-loopback-gate:end

function adminer_object() {
    if (!class_exists('RexenvAdminer')) {
        class RexenvAdminer extends \Adminer\Adminer {
            function login($login, $password) {
                // Passwordless login for rexenv's loopback engines ONLY — the host
                // must match a loopback name EXACTLY (see the rexenv-loopback-gate
                // above). SERVER is request-controlled, so a prefix match would let
                // "127.0.0.1.evil.com" reach a remote server passwordless (B1).
                return rexenv_is_loopback(\Adminer\SERVER);
            }
            function loginForm() {
                parent::loginForm();
                if (isset($_GET['rexenv_auto'])) {
                    // Auto-submit once PER TARGET SERVER. Guards: never on a page
                    // already showing a login error (prevents a submit loop when
                    // the engine is down); sessionStorage is best-effort — it can
                    // throw on the app's custom-scheme origin, and a thrown guard
                    // must not kill the submit. The guard key MUST include the
                    // target server: the embedded webview is ONE permanent tab
                    // (one sessionStorage per origin), so a global one-shot let
                    // the first engine's auto-login permanently suppress every
                    // other engine's (observed live: embedded MariaDB never
                    // auto-logged-in after MySQL had). The param name is part of
                    // the key so MySQL-protocol (server=) and Postgres (pgsql=)
                    // targets can never collide.
                    $target = isset($_GET['pgsql'])
                        ? 'pgsql:' . $_GET['pgsql']
                        : 'server:' . (isset($_GET['server']) ? $_GET['server'] : '');
                    echo "<script" . \Adminer\nonce() . ">"
                       . "(function(){"
                       . "if(document.querySelector('.error'))return;"
                       . "var key=" . json_encode('rexenv_autologin:' . $target) . ";"
                       . "var seen=null;"
                       . "try{seen=sessionStorage.getItem(key);}catch(e){}"
                       . "if(seen)return;"
                       . "try{sessionStorage.setItem(key,'1');}catch(e){}"
                       . "var f=document.querySelector('[name=\"auth[driver]\"]');"
                       . "if(f&&f.form){f.form.submit();}"
                       . "})();"
                       . "</script>";
                }
            }
            function headers() {
                // Adminer hardcodes "X-Frame-Options: deny", which blanks the
                // rexenv Database Browser <iframe>. Drop it; the csp() hook below
                // re-adds frame protection scoped to the app's webview origins.
                header_remove('X-Frame-Options');
            }
            function css() {
                // The console follows the APP's theme, not the OS's.
                //
                // This is Adminer's own switch, not a paint job over it. Adminer
                // decides its scheme from what css() returns: values naming only
                // 'dark' make it load dark.css WITHOUT the
                // prefers-color-scheme media query and emit
                // <meta name="color-scheme" content="dark">; values naming only
                // 'light' drop dark.css entirely. Returning nothing leaves its
                // default — both, media-gated — which is what shipped, and why
                // the console stayed on the OS palette while rexenv sat in the
                // other one.
                //
                // The choice is read from a FILE rather than a query parameter
                // because Adminer's own links carry no parameter of ours: one
                // click inside the console (Create database, a table) would have
                // dropped it and snapped the page back to the OS scheme.
                $file = __DIR__ . '/.rexenv-theme';
                $want = is_file($file) ? trim((string) @file_get_contents($file)) : '';
                if ($want !== 'dark' && $want !== 'light') {
                    // Absent or unreadable = say nothing and let Adminer do what
                    // it always did. A half-written file must not become a
                    // console with no stylesheet at all.
                    return parent::css();
                }
                // The value carries the scheme; the file itself is empty. It has
                // to EXIST though — Adminer emits a <link> for whatever key is
                // returned, and a 404 in the console's own <head> is a defect
                // report waiting to happen.
                return array('rexenv-theme.css' => $want);
            }
            function csp(array $csp) {
                // Embeddable ONLY by the rexenv app webview (prod macOS/Linux +
                // prod Windows origins) — every other ancestor stays blocked. The
                // Vite dev origin is appended by Rust ONLY in debug builds
                // (__REXENV_DEV_ANCESTOR__), so a shipped Adminer isn't frameable
                // by a local process squatting :1420 (B14).
                $csp[0]['frame-ancestors'] =
                    'tauri://localhost https://tauri.localhost__REXENV_DEV_ANCESTOR__';
                return $csp;
            }
        }
    }
    return new \RexenvAdminer();
}
require __DIR__ . '/.adminer.php';
"#;

/// The Vite dev server origin, allowed to frame Adminer ONLY in dev builds so
/// the embedded Database Browser works under `npm run tauri dev`. In release
/// builds it's empty — a shipped Adminer must not be frameable by any local
/// process squatting `:1420` (B14). The packaged app frames from
/// `tauri://localhost`, which stays allowed regardless.
#[cfg(debug_assertions)]
const DEV_FRAME_ANCESTOR: &str = " http://localhost:1420";
#[cfg(not(debug_assertions))]
const DEV_FRAME_ANCESTOR: &str = "";

/// The wrapper written to disk, with the dev-only frame-ancestor resolved for
/// this build profile (the `__REXENV_DEV_ANCESTOR__` placeholder in
/// [`WRAPPER_INDEX_PHP`]).
pub fn wrapper_index_php() -> String {
    WRAPPER_INDEX_PHP.replace("__REXENV_DEV_ANCESTOR__", DEV_FRAME_ANCESTOR)
}


/// The binding probe, run through the bundled PHP before an Adminer version is
/// ever allowed to become the one this machine serves.
///
/// See [`verify_pair`] for what it proves and what it cannot.
const PROBE_PHP: &str = r####"<?php
// rexenv Adminer binding probe (generated; see core/adminer.rs — do not edit).
//
// The verdict comes from a SHUTDOWN handler, not from code after the require:
// Adminer exits during its own bootstrap on most requests, so anything asserted
// afterwards would simply never run and the probe would pass by not executing.
register_shutdown_function(function () {
    $need = ['login', 'loginForm', 'headers', 'csp'];
    $miss = [];
    if (!class_exists('\\Adminer\\Adminer')) {
        $miss[] = 'class Adminer\\Adminer';
    } else {
        $r = new ReflectionClass('\\Adminer\\Adminer');
        foreach ($need as $m) {
            if (!$r->hasMethod($m)) { $miss[] = "override $m has nothing to override"; }
        }
    }
    if (!function_exists('\\Adminer\\nonce')) { $miss[] = 'function Adminer\\nonce'; }
    $e = error_get_last();
    if ($e && ($e['type'] & (E_ERROR | E_PARSE | E_COMPILE_ERROR | E_CORE_ERROR))) {
        $miss[] = 'fatal: ' . $e['message'];
    }
    fwrite(STDERR, $miss ? ('REXENV-PROBE-FAIL ' . implode('; ', $miss)) : 'REXENV-PROBE-OK');
});
// The same subclass SHAPE the real wrapper declares. Declaring it is half the
// test: if the base class moved, this is where PHP fatals.
function adminer_object() {
    class RexenvProbeAdminer extends \Adminer\Adminer {
        function login($login, $password) { return false; }
        function loginForm() { parent::loginForm(); }
        function headers() {}
        function csp(array $csp) { return $csp; }
    }
    return new \RexenvProbeAdminer();
}
require __DIR__ . '/.adminer.php';
"####;

/// Marker the probe writes on success. Compared exactly — a probe that printed
/// nothing (killed, PHP missing, a fatal before the handler) must read as a
/// failure, and "no output" is the shape that would otherwise read as fine.
const PROBE_OK: &str = "REXENV-PROBE-OK";

/// **Does this Adminer still bind to rexenv's wrapper?** Run before an Adminer
/// version becomes the one this machine serves.
///
/// # Why this exists at all
///
/// rexenv's security controls for the Adminer console live inside Adminer's OWN
/// plugin API: [`WRAPPER_INDEX_PHP`] subclasses `\Adminer\Adminer` and overrides
/// `login` (the loopback gate that stops a passwordless session reaching a remote
/// server), `headers` (removing `X-Frame-Options: deny`) and `csp` (the
/// frame-ancestors bound), and it calls `\Adminer\nonce()`. A PHP update cannot
/// switch off a control rexenv wrote; **an Adminer update can**, by renaming the
/// hook it hangs on — and it would do it SILENTLY, with the console still
/// serving. That is the one axis on which Adminer is worse than PHP, and this is
/// the answer to it.
///
/// # What it proves
///
/// The base class resolves, each of the four overrides shadows a method that
/// still EXISTS on the parent, `\Adminer\nonce()` is there, and requiring the
/// file raises no fatal. Static inspection cannot do any of this: the released
/// `adminer-<v>-en.php` is a compressed stub, so grepping it for `class Adminer`
/// finds nothing in ANY version, including the one running right now.
///
/// # What it does NOT prove, stated rather than implied
///
/// That Adminer still CALLS those methods. A version that kept `csp()` on the
/// class and stopped consulting it would pass here while `headers()` still
/// strips `X-Frame-Options` — clickjacking on a database console. Reflection
/// cannot see a call site that is gone. The probe converts the FATAL cases (a
/// renamed or re-namespaced base class, an override with nothing to override,
/// a truncated file) into a loud refusal; the never-called case is
/// `docs/SMOKE-TEST.md`'s.
///
/// Probed in a throwaway directory, never in the live docroot: a version that
/// fails must not have been served for the duration of the check.
pub fn verify_pair(php_bin: &std::path::Path, adminer_php: &std::path::Path) -> Result<()> {
    let dir = std::env::temp_dir().join(format!(
        "rexenv-adminer-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir)?;
    let cleanup = |d: &std::path::Path| {
        let _ = std::fs::remove_dir_all(d);
    };
    let run = (|| -> Result<std::process::Output> {
        std::fs::copy(adminer_php, dir.join(STAGED_ADMINER))?;
        let probe = dir.join(".rexenv-probe.php");
        std::fs::write(&probe, PROBE_PHP)?;
        Ok(std::process::Command::new(php_bin)
            .arg("-d")
            .arg("display_errors=0")
            // Adminer starts a session at include time; keep it in the throwaway
            // directory rather than wherever this machine's default points.
            .arg("-d")
            .arg(format!("session.save_path={}", dir.display()))
            .arg(&probe)
            .current_dir(&dir)
            .output()?)
    })();
    let out = match run {
        Ok(o) => o,
        Err(e) => {
            cleanup(&dir);
            return Err(e);
        }
    };
    cleanup(&dir);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains(PROBE_OK) {
        return Ok(());
    }
    let why = stderr
        .lines()
        .find(|l| l.contains("REXENV-PROBE-FAIL"))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| {
            // No verdict at all: killed, or a fatal before the handler was
            // registered. Never treated as a pass.
            format!("the probe produced no verdict (exit {:?})", out.status.code())
        });
    Err(Error::Other(format!(
        "this Adminer build does not bind to rexenv's wrapper, so the database console's \
         login gate and frame protections would not be applied — {why}"
    )))
}

/// Where the user's chosen Adminer version is stored.
///
/// A settings ROW, not a column and not a table. One fact with no existing row to
/// hang it on is exactly what migration v36 already deleted once, and
/// `php_versions.selected_patch` is a column only because a row per minor already
/// existed carrying `fpm_port`/`installed`/`is_default`.
///
/// Absent means "follow the pin" — never backfilled, so a user who never presses
/// Update keeps receiving the version their app ships as releases move it.
pub const VERSION_KEY: &str = "adminer_version";

/// **The ONE answer to "which Adminer version runs".** Everything that resolves,
/// stages, plans or displays Adminer asks this — never `binaries::ADMINER_VERSION`.
///
/// Three things in order, and each is load-bearing:
///
/// 1. the stored selection, if any;
/// 2. FLOORED by the compiled-in pin ([`updates::adminer_floored`]), so a stale
///    choice can never hold a user below the version their app ships;
/// 3. **VOUCHED** — filtered on read through `binaries::manifest`, which consults
///    the pin first and only then the verified catalog. A selection whose entry
///    has since left the manifest (a truncating publish, a rotated key, a
///    downgraded app) is not resolvable, and a version that cannot resolve must
///    read as "we are on the pin" rather than as a version nothing can produce.
///
/// Filtering on READ is deliberate and mirrors `DbEngine::effective_version`: it
/// makes the generic `set_setting` IPC door inert for this key with no write-path
/// code to get wrong, which matters because that door's gating guard does not
/// cover the surface its comment used to claim (`docs/TODO.md`).
pub fn effective_version(platform: &dyn Platform, conn: &Connection) -> String {
    effective_version_for(conn, platform.binaries().arch())
}

/// [`effective_version`] against an explicit arch — the seam the tests use, so
/// the VOUCHING step is exercised without standing up a `Platform`.
fn effective_version_for(conn: &Connection, arch: crate::platform::traits::Arch) -> String {
    let selected = crate::state::store::get_setting(conn, VERSION_KEY).ok().flatten();
    let want = crate::core::updates::adminer_floored(selected.as_deref());
    if binaries::manifest("adminer", &want, std::env::consts::OS, arch).is_some() {
        return want;
    }
    binaries::ADMINER_VERSION.to_string()
}

/// Record the user's chosen Adminer version, or clear it back to the pin.
pub fn set_selected_version(conn: &Connection, version: Option<&str>) -> Result<()> {
    match version {
        Some(v) => crate::state::store::set_setting(conn, VERSION_KEY, v),
        None => crate::state::store::delete_setting(conn, VERSION_KEY),
    }
}

/// Which version the docroot is ACTUALLY serving, or `None` before the first
/// start has staged anything.
///
/// `None` is a state, not a zero value: "nothing staged yet" and "staged, and it
/// is 5.4.2" are different sentences, and a row that renders them the same is
/// the honest-UI rule in `docs/DESIGN.md` being broken quietly. Read from the
/// marker rather than measured from the file, for the reason [`needs_restage`]
/// gives.
pub fn staged_version(platform: &dyn Platform) -> Option<String> {
    let dir = docroot(platform).ok()?;
    let v = std::fs::read_to_string(dir.join(STAGED_VERSION)).ok()?;
    let v = v.trim().to_string();
    // A marker whose file is gone describes nothing.
    (!v.is_empty() && dir.join(STAGED_ADMINER).is_file()).then_some(v)
}

/// Web docroot for Adminer, isolated from the binary cache.
pub fn docroot(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("adminer"))
}

/// The real Adminer's name inside the docroot — a DOTFILE, deliberately.
///
/// Every `.php` in this docroot is directly executable: the vhost is built as
/// `RewriteMode::Single` and the generated nginx block ends in
/// `location ~ \.php$`. So while it was staged as `adminer.php`,
/// `https://adminer.rexenv.rex/adminer.php` served **raw Adminer with no
/// wrapper** — no `login()` override (the loopback gate), no `csp()`
/// (frame-ancestors), no `headers()` (the `X-Frame-Options: deny` removal). Every
/// control rexenv installs for this console lives in the wrapper, and that URL
/// went around all of them.
///
/// A dotfile costs nothing and closes it with a guard that already exists:
/// `NGINX_DOTFILE_DENY` is emitted BEFORE the php location and is ordering-
/// asserted for all three rewrite modes. Better than a `location = /adminer.php
/// { deny all; }` — that adds a rule to keep true, while this removes the path.
pub(crate) const STAGED_ADMINER: &str = ".adminer.php";

/// The palette the console must render in — `dark` or `light`, written by
/// rexenv whenever the app's own theme resolves or changes, read by the wrapper
/// on EVERY request.
///
/// A dotfile, so the same `NGINX_DOTFILE_DENY` that hides the console's source
/// hides this; nothing about it is servable and nothing needs to be.
pub(crate) const THEME_FILE: &str = ".rexenv-theme";

/// The stylesheet Adminer `<link>`s when rexenv is driving the scheme. Empty by
/// design: its VALUE in [`WRAPPER_INDEX_PHP`]'s `css()` return is the whole
/// signal, and Adminer needs a real file behind the key it emits.
pub(crate) const THEME_CSS: &str = "rexenv-theme.css";

const THEME_CSS_BODY: &str =
    "/* rexenv — generated. Deliberately empty: Adminer reads the SCHEME from
      * this file's entry in the wrapper's css() return, not from any rule here. */
";

/// Record which palette the console must use. `dark` and `light` only —
/// "system" is resolved by the app before it gets here, so the console matches
/// what rexenv is actually rendering rather than re-consulting the OS and
/// possibly disagreeing with it.
///
/// Published by RENAME: the wrapper reads this file on every request, and a
/// half-written one would fall back to Adminer's default mid-navigation.
pub fn set_theme(platform: &dyn Platform, theme: &str) -> Result<()> {
    write_theme(&docroot(platform)?, theme)
}

/// [`set_theme`] against a GIVEN directory — split out for the reason
/// [`needs_restage`] is: the rule is then testable with a temp dir instead of a
/// `Platform`, and the rule is the part that can be wrong.
pub(crate) fn write_theme(dir: &std::path::Path, theme: &str) -> Result<()> {
    if theme != "dark" && theme != "light" {
        return Err(Error::Other(format!(
            "unknown console theme \"{theme}\" — the app resolves \"system\" before this point"
        )));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(THEME_FILE);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(theme) {
        return Ok(());
    }
    let tmp = dir.join(format!("{THEME_FILE}.new"));
    std::fs::write(&tmp, theme)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Records which version the staged copy IS.
///
/// Staleness used to be a FILE SIZE comparison against the cached source, which
/// is a proxy for the question rather than the question — two Adminer releases
/// can be the same length, and the answer that matters after an update is
/// "which version is this", not "is it a different size". A dotfile too, so the
/// same deny rule covers it.
const STAGED_VERSION: &str = ".adminer-version";

/// Whether the docroot copy has to be rewritten — **a value, not a condition
/// inside an async fn that downloads**, so the rule is testable without a
/// `Platform`, a cache or a network.
///
/// Both halves are load-bearing. The MARKER answers "which version is this",
/// which is the question an update asks; the file's EXISTENCE is checked too
/// because a marker without its file is a docroot that 500s on every request
/// while claiming to be up to date. The predicate this replaced compared FILE
/// SIZES against the cached source — a proxy for the question rather than the
/// question, and two Adminer releases can be the same length.
fn needs_restage(marker: Option<&str>, staged_exists: bool, want: &str) -> bool {
    marker != Some(want) || !staged_exists
}

/// Ensure the Adminer docroot exists: the bundled Adminer as [`STAGED_ADMINER`]
/// plus the rexenv deep-link wrapper as `index.php` (the served entrypoint).
/// Downloads + caches the file (§5.1) on first use.
///
/// `version` is [`effective_version`]'s answer, passed in rather than read here —
/// so the download PLANNER and this stager cannot disagree. They must not: the
/// planner is what login-start's strictly-offline guard reads, and a plan for the
/// pin while this resolves the selection clears a start that then downloads
/// inside the services lock (ledger #175).
///
/// Idempotent: the Adminer copy is restaged only when the recorded version
/// differs, and the wrapper only when its content differs (so a wrapper update
/// redeploys on next start).
pub async fn ensure(platform: &dyn Platform, version: &str) -> Result<PathBuf> {
    let dir = docroot(platform)?;
    std::fs::create_dir_all(&dir)?;

    // The real Adminer, copied beside the wrapper (the wrapper `require`s it).
    let adminer_php = dir.join(STAGED_ADMINER);
    let marker = dir.join(STAGED_VERSION);
    let staged = std::fs::read_to_string(&marker).ok();
    if needs_restage(staged.as_deref(), adminer_php.is_file(), version) {
        let src = binaries::resolve_file(platform, "adminer", version).await?;
        // Publish by RENAME so a reader never sees a half-written console: the
        // wrapper `require`s this path on every request, and a truncated PHP
        // file is a fatal error on a page the user is looking at.
        let tmp = dir.join(".adminer.php.new");
        std::fs::copy(&src, &tmp)?;
        std::fs::rename(&tmp, &adminer_php)?;
        // The marker LAST: it means "the file beside me is this version", so
        // writing it before the file would claim a version that is not there.
        std::fs::write(&marker, version)?;
    }
    // The pre-dotfile name, if this install predates the move. Removing it is
    // stronger than making it unroutable — there is then nothing to route to.
    let legacy = dir.join("adminer.php");
    if legacy.is_file() {
        let _ = std::fs::remove_file(&legacy);
    }

    // The served entrypoint: our deep-link wrapper (rewrite only if changed).
    // Rendered per build profile — the dev frame-ancestor is present only in
    // debug builds (B14).
    let index = dir.join("index.php");
    let rendered = wrapper_index_php();
    if std::fs::read_to_string(&index).ok().as_deref() != Some(rendered.as_str()) {
        std::fs::write(&index, &rendered)?;
    }

    // The empty stylesheet the wrapper's css() points at when rexenv drives the
    // scheme. Staged here rather than written on demand, because it must exist
    // BEFORE the first themed request — Adminer emits the <link> whether or not
    // anything is behind it.
    let theme_css = dir.join(THEME_CSS);
    if std::fs::read_to_string(&theme_css).ok().as_deref() != Some(THEME_CSS_BODY) {
        std::fs::write(&theme_css, THEME_CSS_BODY)?;
    }
    Ok(dir)
}

/// Build the upstream Adminer URL from a request's path+query. Guard: it MUST
/// start with `/`. Otherwise a value like `@evil.com/` would parse the host as
/// `evil.com` (with `ADMINER_HOST` demoted to userinfo), and because the proxy
/// client pins ONLY `ADMINER_HOST` to loopback AND disables cert checks, that
/// would be an SSRF to an arbitrary host. Requests always arrive as
/// `rexdb://localhost/…`, so a legit path always starts with `/` — a `//path`
/// still resolves to the pinned host (the authority ends at the first `/`). B8,
/// defense-in-depth (not reachable today).
fn proxy_url(path_and_query: &str) -> Result<reqwest::Url> {
    if !path_and_query.starts_with('/') {
        return Err(Error::Other(format!(
            "adminer proxy: refusing non-absolute request path {path_and_query:?}"
        )));
    }
    reqwest::Url::parse(&format!("https://{ADMINER_HOST}{path_and_query}"))
        .map_err(|e| Error::Other(format!("adminer proxy: bad url: {e}")))
}

/// A response ready to hand back to the webview's custom-scheme responder.
pub struct ProxiedResponse {
    pub status: u16,
    /// Pass-through headers (hop-by-hop, cookie and length headers already stripped).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Shared client for the Database Browser proxy: pinned to the loopback edge
/// (no DNS), with a persistent RUST-SIDE cookie jar so Adminer's session
/// survives the login POST no matter what cookie policy the webview applies to
/// cross-site iframes. Redirect policy is `none` so [`forward`] can resolve
/// Adminer's POST-redirect-GET itself — WKWebView does NOT follow redirects
/// returned by a custom-scheme handler (`WKURLSchemeTask` has no redirect
/// mechanism), so a passed-through 302 dead-ends as a blank frame. The jar
/// lives for the app run, so the session persists across iframe remounts.
fn proxy_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .cookie_store(true)
            // Loopback-pinned; our local-CA leaf won't chain for reqwest's
            // store (same trade-off as `proxy::edge_answers_as_ours`).
            .danger_accept_invalid_certs(true)
            .resolve(
                ADMINER_HOST,
                std::net::SocketAddr::from(([127, 0, 0, 1], crate::core::proxy::DEFAULT_HTTPS_PORT)),
            )
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("adminer proxy client")
    })
}

/// Response headers that must NOT be replayed to the webview: cookies stay in
/// the Rust jar (the whole point), and framing/length headers are rebuilt by
/// the responder.
const STRIPPED_RESPONSE_HEADERS: &[&str] = &[
    "set-cookie",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "alt-svc",
];

/// Forward one Database Browser request (`rexdb://localhost<path_and_query>`)
/// to the Adminer vhost through the edge and return the response for the
/// webview. `content_type` is the request's Content-Type (form POSTs).
///
/// Redirects are followed HERE (≤5 hops, same-vhost only): Adminer is strictly
/// POST-redirect-GET, and WKWebView never follows a redirect returned by a
/// custom-scheme handler — replaying the 302 to the webview leaves the iframe
/// on a dead blank frame (observed live as "Save does nothing" / blank first
/// open). Per browser form semantics 301/302/303 become a body-less GET;
/// 307/308 keep the method + body.
pub async fn forward(
    method: &str,
    path_and_query: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Result<ProxiedResponse> {
    let mut url = proxy_url(path_and_query)?;
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| Error::Other(format!("adminer proxy: bad method: {e}")))?;
    let mut content_type = content_type.map(str::to_string);
    let mut body = body;

    let mut hops = 0u8;
    let resp = loop {
        let mut req = proxy_client().request(method.clone(), url.clone());
        if let Some(ct) = &content_type {
            req = req.header(reqwest::header::CONTENT_TYPE, ct);
        }
        if !body.is_empty() {
            req = req.body(body.clone());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Other(format!("adminer proxy: {e}")))?;

        if resp.status().is_redirection() && hops < 5 {
            let target = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|loc| url.join(loc).ok())
                // Never follow OFF the vhost — anything else replays to the
                // webview below (where frame policy applies).
                .filter(|next| next.host_str() == Some(ADMINER_HOST));
            if let Some(next) = target {
                hops += 1;
                url = next;
                let s = resp.status();
                if s != reqwest::StatusCode::TEMPORARY_REDIRECT
                    && s != reqwest::StatusCode::PERMANENT_REDIRECT
                {
                    method = reqwest::Method::GET;
                    body = Vec::new();
                    content_type = None;
                }
                continue;
            }
        }
        break resp;
    };

    let status = resp.status().as_u16();
    let mut headers = Vec::new();
    for (name, value) in resp.headers() {
        let n = name.as_str().to_ascii_lowercase();
        if STRIPPED_RESPONSE_HEADERS.contains(&n.as_str()) {
            continue;
        }
        let Ok(v) = value.to_str() else { continue };
        // Absolute self-redirects must stay on the proxy origin; relative ones
        // (Adminer's norm) already resolve against `rexdb://localhost`.
        if n == "location" {
            let v = v
                .strip_prefix(&format!("https://{ADMINER_HOST}"))
                .unwrap_or(v);
            headers.push((n, v.to_string()));
            continue;
        }
        headers.push((n, v.to_string()));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("adminer proxy: read body: {e}")))?
        .to_vec();
    Ok(ProxiedResponse { status, headers, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_an_internal_subdomain_on_the_backbone_tld() {
        // Lives on the backbone TLD (whose resolver onboarding installs), so
        // Adminer never depends on any other TLD being present.
        assert!(ADMINER_HOST.ends_with(&format!(".rexenv.{}", crate::core::tld::BACKBONE_TLD)));
        // Not a wildcard / not a user site domain.
        assert_eq!(ADMINER_HOST, "adminer.rexenv.rex");
    }

    /// The palette wiring is three names that must agree across two languages:
    /// the file rexenv WRITES, the file the wrapper READS, and the stylesheet
    /// key Adminer is handed. A drift in any one of them is a console that
    /// silently goes back to following the OS — which is the bug this exists to
    /// close, and it would look exactly like nothing happening.
    #[test]
    fn the_wrapper_reads_the_theme_file_rexenv_writes() {
        assert!(
            WRAPPER_INDEX_PHP.contains("function css()"),
            "the css() override is gone — Adminer decides its scheme from that return"
        );
        assert!(
            WRAPPER_INDEX_PHP.contains(&format!("__DIR__ . '/{THEME_FILE}'")),
            "the wrapper no longer reads the file `set_theme` writes"
        );
        assert!(
            WRAPPER_INDEX_PHP.contains(&format!("'{THEME_CSS}' => $want")),
            "the wrapper hands Adminer a stylesheet `ensure` does not stage"
        );
        // Adminer's own contract: the VALUE is the scheme, and only these two
        // words make it commit. Anything else leaves it media-gated.
        assert!(WRAPPER_INDEX_PHP.contains("$want !== 'dark' && $want !== 'light'"));
        // The two files sit on opposite sides of the dotfile deny rule ON
        // PURPOSE: the choice must not be readable over HTTP, and the
        // stylesheet must — Adminer emits a <link> for it either way.
        assert!(THEME_FILE.starts_with('.'), "{THEME_FILE} is web-readable");
        assert!(!THEME_CSS.starts_with('.'), "{THEME_CSS} is denied by the dotfile rule → 404 in <head>");
    }

    /// `set_theme` takes the RESOLVED palette only. "system" is the app's word
    /// for "ask the OS", and the whole point here is that the console stops
    /// asking — if it ever reached this function it would mean the caller
    /// forgot to resolve, and writing it would put the string "system" in a
    /// file the wrapper then ignores, silently restoring the bug.
    #[test]
    fn set_theme_takes_a_resolved_palette_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("rexenv-theme-rule-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for good in ["dark", "light"] {
            write_theme(&dir, good).expect("a resolved palette is written");
            assert_eq!(std::fs::read_to_string(dir.join(THEME_FILE)).unwrap(), good);
            // Idempotent: the second call must not churn a file the console
            // reads on every request.
            write_theme(&dir, good).expect("idempotent");
        }
        for bad in ["system", "", "DARK", "moonlight"] {
            assert!(write_theme(&dir, bad).is_err(), "{bad:?} was accepted");
        }
        // ...and the last good value survives every refusal: a rejected call
        // must not leave the console with no answer.
        assert_eq!(std::fs::read_to_string(dir.join(THEME_FILE)).unwrap(), "light");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrapper_defines_the_deeplink_hook_and_loopback_gate() {
        // Global-namespace hook (Adminer checks the global function name).
        assert!(WRAPPER_INDEX_PHP.contains("function adminer_object()"));
        assert!(WRAPPER_INDEX_PHP.contains("extends \\Adminer\\Adminer"));
        // Passwordless login is gated to loopback only (never a remote host).
        assert!(WRAPPER_INDEX_PHP.contains("function login("));
        assert!(WRAPPER_INDEX_PHP.contains("127.0.0.1"));
        // Auto-submit is keyed on the deep-link flag + carries the CSP nonce.
        assert!(WRAPPER_INDEX_PHP.contains(AUTOLOGIN_FLAG));
        assert!(WRAPPER_INDEX_PHP.contains("\\Adminer\\nonce()"));
        // It serves the real Adminer beside it.
        // The literal the wrapper requires and the name `ensure` stages must be
        // the same string. Two spellings of one filename is a blank console.
        assert!(
            WRAPPER_INDEX_PHP.contains(&format!("require __DIR__ . '/{STAGED_ADMINER}'")),
            "the wrapper requires a file `ensure` does not stage"
        );
        // …and it is a DOTFILE, which is what keeps raw Adminer unroutable.
        assert!(STAGED_ADMINER.starts_with('.'), "{STAGED_ADMINER} is directly executable");
        assert!(STAGED_VERSION.starts_with('.'), "{STAGED_VERSION} is web-readable");
    }

    #[test]
    fn login_gate_matches_loopback_host_exactly_not_by_prefix() {
        // B1 regression: the passwordless gate must compare the target HOST to a
        // loopback allow-list EXACTLY. The OLD prefix test
        // (`strpos(\Adminer\SERVER, '127.0.0.1') === 0`) accepted
        // "127.0.0.1.evil.com" and granted a passwordless session to a REMOTE
        // server (→ LOAD DATA LOCAL INFILE local-file read). The behavioral proof
        // over a real PHP interpreter lives in examples/adminer_login_gate_check.
        assert!(WRAPPER_INDEX_PHP.contains("function rexenv_is_loopback("));
        assert!(WRAPPER_INDEX_PHP.contains("return rexenv_is_loopback(\\Adminer\\SERVER)"));
        assert!(WRAPPER_INDEX_PHP.contains(
            "in_array(strtolower($host), array('127.0.0.1', '::1', 'localhost'), true)"
        ));
        // The vulnerable prefix gate must be GONE — both halves of it.
        assert!(!WRAPPER_INDEX_PHP.contains("strpos(\\Adminer\\SERVER, '127.0.0.1') === 0"));
        assert!(!WRAPPER_INDEX_PHP.contains("strpos(\\Adminer\\SERVER, 'localhost') === 0"));
        // Extraction markers the example relies on stay present.
        assert!(WRAPPER_INDEX_PHP.contains("rexenv-loopback-gate:start"));
        assert!(WRAPPER_INDEX_PHP.contains("rexenv-loopback-gate:end"));
    }

    #[test]
    fn autologin_guard_is_keyed_per_target_server() {
        // The embedded webview is one permanent tab (one sessionStorage per
        // origin) — a global one-shot guard let the first engine's auto-login
        // suppress every other engine's. The key must carry the target server,
        // for BOTH param families we expose (server= for MySQL/MariaDB,
        // pgsql= for Postgres), with the param name in the key so the two
        // families can never collide.
        assert!(WRAPPER_INDEX_PHP.contains("'rexenv_autologin:' . $target"));
        assert!(WRAPPER_INDEX_PHP.contains("'pgsql:' . $_GET['pgsql']"));
        assert!(WRAPPER_INDEX_PHP.contains("'server:' . (isset($_GET['server'])"));
        // Same-server loop protection stays: the key is still set before the
        // submit and checked before firing.
        assert!(WRAPPER_INDEX_PHP.contains("sessionStorage.getItem(key)"));
        assert!(WRAPPER_INDEX_PHP.contains("sessionStorage.setItem(key,'1')"));
        // No global (unkeyed) guard left behind.
        assert!(!WRAPPER_INDEX_PHP.contains("getItem('rexenv_autologin')"));
    }

    #[test]
    fn wrapper_allows_framing_only_from_the_app_webview() {
        // Adminer's blanket deny is dropped (it blanks the in-app iframe)…
        assert!(WRAPPER_INDEX_PHP.contains("header_remove('X-Frame-Options')"));
        // …and replaced by frame-ancestors scoped to rexenv webview origins.
        assert!(WRAPPER_INDEX_PHP.contains("function csp("));
        assert!(WRAPPER_INDEX_PHP.contains("'frame-ancestors'"));
        assert!(WRAPPER_INDEX_PHP.contains("tauri://localhost"));
        // No wildcard — never embeddable by arbitrary origins.
        assert!(!WRAPPER_INDEX_PHP.contains("frame-ancestors *"));

        // The Vite dev origin is present ONLY in debug builds (B14): a shipped
        // Adminer must not be frameable by a local process squatting :1420. The
        // placeholder is always resolved — it must never ship literally.
        let rendered = wrapper_index_php();
        assert!(!rendered.contains("__REXENV_DEV_ANCESTOR__"));
        assert!(WRAPPER_INDEX_PHP.contains("__REXENV_DEV_ANCESTOR__"));
        assert!(rendered.contains("tauri://localhost"));
        #[cfg(debug_assertions)]
        assert!(rendered.contains("http://localhost:1420"));
        #[cfg(not(debug_assertions))]
        assert!(!rendered.contains("http://localhost:1420"));
    }

    #[test]
    fn proxy_url_requires_an_absolute_path_and_pins_the_host() {
        // Legit paths always arrive as rexdb://localhost/… → host stays pinned.
        assert_eq!(
            proxy_url("/index.php?server=127.0.0.1").unwrap().host_str(),
            Some(ADMINER_HOST)
        );
        assert_eq!(proxy_url("/").unwrap().host_str(), Some(ADMINER_HOST));
        // A `//path` is still on the pinned host (authority ends at the first
        // `/`) — a legit-looking shape the guard must NOT reject.
        assert_eq!(proxy_url("//evil.com/x").unwrap().host_str(), Some(ADMINER_HOST));
        // The SSRF shape (`@evil.com/` would smuggle a host via userinfo) and any
        // other non-absolute lead are refused before a request is ever built.
        assert!(proxy_url("@evil.com/").is_err());
        assert!(proxy_url("evil.com").is_err());
        assert!(proxy_url("*").is_err());
    }

    #[test]
    fn relative_locations_resolve_on_the_vhost() {
        // Adminer redirects with bare query-string Locations ("?select=notes");
        // the follow loop must resolve them onto the vhost so they get followed
        // in Rust (WKWebView can't follow custom-scheme redirects itself).
        let url =
            reqwest::Url::parse(&format!("https://{ADMINER_HOST}/?edit=notes&where%5Bid%5D=1"))
                .unwrap();
        let next = url.join("?select=notes").unwrap();
        assert_eq!(next.host_str(), Some(ADMINER_HOST));
        assert_eq!(next.query(), Some("select=notes"));
        // An absolute off-vhost Location must NOT be followed.
        let foreign = url.join("https://example.com/x").unwrap();
        assert_ne!(foreign.host_str(), Some(ADMINER_HOST));
    }

    #[test]
    fn import_php_limits_never_exceed_the_nginx_cap() {
        // The 413 this pair exists to kill comes back the moment PHP is told it
        // may accept MORE than nginx will pass, or LESS than the dump — so check
        // both keys land on EXACTLY the cap they're rendered for (and that both
        // are present, or a typo'd key reverts to PHP's 2M/8M with nothing
        // failing). Across the floor and a user-raised cap alike.
        for cap in [MAX_IMPORT_BYTES, 10 * 1024 * 1024 * 1024] {
            let rendered = import_php_value(cap);
            let mut seen = 0;
            for line in rendered.lines() {
                let (key, value) = line.split_once('=').expect("key=value");
                let bytes = crate::core::php::parse_php_size(value).expect("php size");
                assert_eq!(bytes, cap, "{key} ({value}) does not match the nginx cap {cap}");
                assert!(
                    matches!(key, "upload_max_filesize" | "post_max_size"),
                    "unexpected key {key}"
                );
                seen += 1;
            }
            assert_eq!(seen, 2, "both upload_max_filesize and post_max_size must be set");
        }
    }

    #[test]
    fn the_import_cap_can_only_raise_what_the_pool_already_allows() {
        // The floor unblocks a default pool…
        assert_eq!(import_cap(None), MAX_IMPORT_BYTES);
        assert_eq!(import_cap(Some(64 << 20)), MAX_IMPORT_BYTES);
        // …but a user running the pool at 6G keeps 6G here. Holding Adminer —
        // and only Adminer — below the user's own configured limit would be the
        // same silent-cap bug this const exists to remove, wearing a bigger number.
        let six_gb = 6 * 1024 * 1024 * 1024;
        assert_eq!(import_cap(Some(six_gb)), six_gb);
        assert_eq!(import_cap(Some(MAX_IMPORT_BYTES)), MAX_IMPORT_BYTES);
    }

    #[test]
    fn the_vhost_timeout_outlasts_every_terminate_timeout_the_app_can_produce() {
        // php-fpm's request_terminate_timeout is max(max_execution_time, 300) —
        // so nginx outlasting the SPEC'S MAXIMUM max_execution_time means nginx
        // can never 504 a request php-fpm would still be serving, for any
        // version, at any setting the user can save. Raising that spec bound
        // without raising this const brings the 504 back: fail here if it moves.
        let spec = crate::core::php::SETTINGS
            .iter()
            .find(|s| s.key == "max_execution_time")
            .expect("whitelisted");
        let max = match spec.kind {
            crate::core::php::SettingKind::Int { max, .. } => u64::try_from(max).expect("positive"),
            _ => panic!("max_execution_time is an Int setting"),
        };
        assert!(
            IMPORT_TIMEOUT_SECS >= max.max(300),
            "nginx would give up at {IMPORT_TIMEOUT_SECS}s while php-fpm serves until {max}s"
        );
    }

    #[test]
    fn wrapper_rewrites_cookies_for_the_cross_site_iframe() {
        // SameSite=lax cookies are withheld from the embedded iframe's login
        // POST; the wrapper rewrites them to None+Secure+Partitioned at flush.
        assert!(WRAPPER_INDEX_PHP.contains("header_register_callback"));
        assert!(WRAPPER_INDEX_PHP.contains("SameSite=None; Partitioned"));
        assert!(WRAPPER_INDEX_PHP.contains("Secure"));
    }

    /// **The chosen version wins, the pin is a FLOOR, and a version nothing
    /// vouches for reads as the pin.**
    ///
    /// The third leg is the one that is easy to skip. A selection can outlive
    /// its manifest entry — a truncating publish, a rotated key, an app
    /// downgrade — and a version that cannot resolve must read as "we are on the
    /// pin", not as a version nothing can produce. Filtering on READ is also what
    /// makes the generic `set_setting` IPC door inert for this key: there is no
    /// write path to get wrong.
    #[test]
    fn the_effective_adminer_version_is_the_choice_floored_by_the_pin_and_vouched() {
        use crate::platform::traits::Arch;
        let _catalog = binaries::catalog_test_lock();
        let conn = crate::state::db::open_in_memory().unwrap();
        let pin = binaries::ADMINER_VERSION;

        // No choice → the pin, byte for byte.
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin);

        // A choice nothing vouches for → still the pin. This is the state after a
        // publish that dropped the entry, and it must not be a dangling version.
        set_selected_version(&conn, Some("6.0.1")).unwrap();
        assert_eq!(
            effective_version_for(&conn, Arch::Arm64),
            pin,
            "an unvouched selection became the effective version"
        );

        // Vouched by the catalog → honoured.
        binaries::install_catalog(crate::core::updates::catalog_for_tests(&[(
            "adminer",
            "6.0.1",
            crate::core::updates::ANY_ARCH,
            "https://github.com/vrana/adminer/releases/download/v6.0.1/adminer-6.0.1-en.php",
            &"c".repeat(64),
        )]));
        assert_eq!(effective_version_for(&conn, Arch::Arm64), "6.0.1");
        // Arch-free: the other Mac gets the same answer.
        assert_eq!(effective_version_for(&conn, Arch::X86_64), "6.0.1");

        // OLDER than the pin → ignored. The pin is a floor, not a default.
        //
        // The old version has to be VOUCHED for this to test the floor at all:
        // with only 6.0.1 catalogued, an unfloored 4.8.1 falls back to the pin
        // through the vouching step instead, and the assertion passes for the
        // wrong reason. Found by planting — the floor was removed and nothing
        // failed. Two guards covering one case is fine; a test that cannot tell
        // them apart is not.
        binaries::install_catalog(crate::core::updates::catalog_for_tests(&[
            (
                "adminer",
                "6.0.1",
                crate::core::updates::ANY_ARCH,
                "https://github.com/vrana/adminer/releases/download/v6.0.1/adminer-6.0.1-en.php",
                &"c".repeat(64),
            ),
            (
                "adminer",
                "4.8.1",
                crate::core::updates::ANY_ARCH,
                "https://github.com/vrana/adminer/releases/download/v4.8.1/adminer-4.8.1-en.php",
                &"d".repeat(64),
            ),
        ]));
        assert!(
            binaries::manifest("adminer", "4.8.1", std::env::consts::OS, Arch::Arm64).is_some(),
            "the fixture must VOUCH for the old version, or this tests the wrong guard"
        );
        set_selected_version(&conn, Some("4.8.1")).unwrap();
        assert_eq!(
            effective_version_for(&conn, Arch::Arm64),
            pin,
            "a stale choice held the machine below the version this app ships"
        );

        // Cleared → the pin, and ABSENT rather than an empty string.
        set_selected_version(&conn, None).unwrap();
        assert!(crate::state::store::get_setting(&conn, VERSION_KEY).unwrap().is_none());
        assert_eq!(effective_version_for(&conn, Arch::Arm64), pin);
        binaries::install_catalog(Default::default());
    }


    /// **The restage rule asks which VERSION is staged, not how big it is.**
    ///
    /// The size compare it replaced was a proxy for the question: two Adminer
    /// releases can be the same length, and after an update the thing that
    /// matters is which one this is. The existence half is not redundant — a
    /// marker with no file beside it is a docroot that 500s on every request
    /// while claiming to be current.
    #[test]
    fn the_restage_rule_is_a_version_compare_and_a_file_check() {
        assert!(needs_restage(None, false, "5.4.2"), "a fresh docroot must stage");
        assert!(needs_restage(None, true, "5.4.2"), "an unmarked file is an unknown version");
        assert!(needs_restage(Some("5.4.2"), false, "5.4.2"), "the marker outlived its file");
        assert!(needs_restage(Some("5.4.2"), true, "6.0.1"), "an update must restage");
        assert!(needs_restage(Some("6.0.1"), true, "5.4.2"), "a revert must restage too");
        assert!(!needs_restage(Some("5.4.2"), true, "5.4.2"), "an unchanged start must not rewrite");
    }

    /// **Raw Adminer has no URL, because the file has no servable name.**
    ///
    /// Every `.php` in this docroot is directly executable (the vhost is
    /// `RewriteMode::Single` and the generated block ends in `location ~
    /// \.php$`), so while the real file was staged as `adminer.php`,
    /// `/adminer.php` served Adminer with NO wrapper: no `login()` override —
    /// the loopback gate — no `csp()`, no `headers()`. Every control rexenv
    /// installs for this console was bypassable by dropping `index.php` from the
    /// URL. That the deny rule actually covers this name is asserted in
    /// `services.rs`, against the generated config.
    #[test]
    fn the_staged_files_have_no_servable_name() {
        for name in [STAGED_ADMINER, STAGED_VERSION] {
            assert!(name.starts_with('.'), "{name} is a directly executable path");
        }
    }


    /// **The probe refuses a build the wrapper cannot bind to — proven against
    /// real PHP and hand-built Adminers, not asserted about code.**
    ///
    /// Only runs where the bundled PHP is already cached; otherwise it SKIPS
    /// loudly rather than passing, because a probe test that silently no-ops is
    /// a probe nobody is testing.
    #[test]
    fn the_binding_probe_accepts_a_bound_adminer_and_refuses_every_broken_one() {
        let Some(php) = cached_php_for_tests() else {
            eprintln!("SKIP the_binding_probe: no cached PHP CLI on this machine");
            return;
        };
        let dir = std::env::temp_dir().join(format!("rexenv-probe-t-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |body: &str| {
            let p = dir.join("candidate.php");
            std::fs::write(&p, body).unwrap();
            p
        };

        // A minimal Adminer carrying the whole surface the wrapper needs.
        let good = write(
            "<?php namespace Adminer; class Adminer { function login($a,$b){} \
             function loginForm(){} function headers(){} function csp(array $c){return $c;} } \
             function nonce(){} adminer_object();",
        );
        assert!(verify_pair(&php, &good).is_ok(), "a bound Adminer was refused");

        // Every way a real upstream change breaks the binding.
        for (label, body) in [
            (
                "csp() renamed",
                "<?php namespace Adminer; class Adminer { function login($a,$b){} \
                 function loginForm(){} function headers(){} } function nonce(){}",
            ),
            ("the class re-namespaced", "<?php namespace Adminer2; class Adminer {} "),
            (
                "nonce() gone",
                "<?php namespace Adminer; class Adminer { function login($a,$b){} \
                 function loginForm(){} function headers(){} function csp(array $c){return $c;} }",
            ),
            ("a truncated file", "<?php namespace Adminer; class Adminer { function login("),
            ("an empty file", ""),
        ] {
            let bad = write(body);
            let err = verify_pair(&php, &bad).unwrap_err().to_string();
            assert!(
                err.contains("does not bind to rexenv's wrapper"),
                "{label} was accepted — the console's login gate would not be applied"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bundled PHP CLI, if this machine already has one cached. Never
    /// downloads: a unit test must not reach the network.
    fn cached_php_for_tests() -> Option<std::path::PathBuf> {
        let base = std::path::PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support/dev.rexenv.rexenv/bin");
        let mut best: Option<std::path::PathBuf> = None;
        for e in std::fs::read_dir(&base).ok()?.flatten() {
            let p = e.path();
            let name = p.file_name()?.to_string_lossy().into_owned();
            if name.starts_with("php-8.") && p.join("php").is_file() {
                best = Some(p.join("php"));
            }
        }
        best
    }

}
