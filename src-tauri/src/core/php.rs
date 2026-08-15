//! core::php — multiple PHP versions: per-version FPM pools + the installed
//! registry (Phase 2 task 1.2).
//!
//! One php-fpm master per PHP **minor** series (8.1, 8.2, 8.3) — never per site;
//! all sites on a version share its pool. Each pool listens on a deterministic
//! loopback port ([`fpm_port`]), is started via `ProcessSupervisor::spawn_logged`
//! (through [`crate::core::services`]), and is gated on `core::ports::ensure_free`.
//! Which versions are enabled is recorded in SQLite (the `php_versions` table, via
//! `state::store`); the manager is told which minors to start so it stays
//! DB-agnostic (mirroring how `ServiceManager` is handed the site list).

use crate::core::{binaries, ports, services};
use crate::core::proc::Proc;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::{models::{PhpVersion, PhpVersionView}, store};
use rusqlite::Connection;


/// Base for per-version FPM ports: `9700 + major*10 + minor`, so 8.1 → 9781,
/// 8.2 → 9782, 8.3 → 9783 (keeps the Phase-1 port for 8.3).
const FPM_PORT_BASE: u16 = 9700;

/// Base for per-version DEBUG pool ports (same scheme: 8.4 → 9984). A debug
/// pool is the SAME static php-fpm binary with the minor's pinned `xdebug.so`
/// loaded via `-d zend_extension` (§8.2) — sites with the Xdebug toggle route
/// here; every other site on the version keeps the normal pool, unaffected.
const DEBUG_FPM_PORT_BASE: u16 = 9900;

/// The minor series of a (patch) version string: `"8.3.31"` → `"8.3"`.
pub fn minor_of(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.to_string(),
    }
}

/// All PHP minor series that have a pinned build (derived from
/// [`binaries::PHP_VERSIONS`]), in the same order (newest last).
pub fn all_minors() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for patch in binaries::PHP_VERSIONS {
        let m = minor_of(patch);
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// The pinned patch build for a minor series (`"8.3"` → `"8.3.31"`), or `None`.
/// Every PHP minor rexenv ships, oldest first — `["8.0", "8.1", …]`.
///
/// Derived from [`binaries::PHP_VERSIONS`] rather than listed again: a refusal
/// that names the available set is only useful while the set it names is the
/// real one, and a second hand-maintained list is how that stops being true.
pub fn available_minors() -> Vec<String> {
    binaries::PHP_VERSIONS.iter().map(|p| minor_of(p)).collect()
}

pub fn patch_for_minor(minor: &str) -> Option<&'static str> {
    binaries::PHP_VERSIONS
        .iter()
        .copied()
        .find(|p| minor_of(p) == minor)
}

/// A PHP minor rexenv does **not** ship — the fixture every "refused by name"
/// test and probe asks for. **Derived, never a literal**: it returns the first
/// candidate `patch_for_minor` cannot resolve, so the day that minor gains a
/// pinned build this PANICS instead of quietly testing nothing.
///
/// Why this is a function and not a `"7.4"` in each test. `7.4` was hardcoded as
/// this fixture in eight asserts, one live example and two manual steps, on the
/// strength of a comment saying static-php.dev would never publish it
/// (`docs/PLAN-php-74-support.md` retires that). The moment 7.4 gains a pinned
/// build, `is_outdated_php_cache("php-7.4.33")` keeps **passing** — for the
/// OPPOSITE reason: 7.4.33 becomes the pinned patch, so "not outdated" is
/// trivially true and the unpinned-minor branch it was written to cover goes
/// untested. A literal fixture is a snapshot of a mutable fact; this is the fact
/// itself, which is the same rule `available_minors` above already follows.
///
/// Public rather than `#[cfg(test)]` because `examples/mcp_scratch_check.rs` is a
/// separate crate and needs the same answer — a second copy of the candidate list
/// over there is precisely the drift being fixed.
///
/// The candidates are upstream-EOL minors OLDER than any floor rexenv plans, so a
/// panic here is a real "we now ship PHP 7.0" alarm, not routine churn.
pub fn unshipped_minor() -> &'static str {
    const CANDIDATES: &[&str] = &["7.2", "7.1", "7.0", "5.6"];
    CANDIDATES
        .iter()
        .copied()
        .find(|m| patch_for_minor(m).is_none())
        .expect("every unshipped-minor fixture candidate now has a pinned build — add an older one")
}

/// A full `x.y.z` in [`unshipped_minor`]'s series, for the rules that parse a
/// patch string (cache-dir names). `.0` is a real release in every candidate.
pub fn unshipped_patch() -> String {
    format!("{}.0", unshipped_minor())
}

/// The date upstream **security support ends** for a PHP minor — php.net's
/// published schedule, `YYYY-MM-DD`.
///
/// A DATE, not a status. "Is this version dead?" is then computed against today
/// ([`eol_since`]) instead of stored, so the answer becomes true on the day it
/// becomes true and nobody has to remember to flip a bool. The table only ever
/// grows — a new row when a new minor ships — and
/// [`every_shipped_minor_declares_its_support_end`] fails the build if that step
/// is skipped, which is the whole reason this can be a hand-written table at all.
///
/// Why it exists: rexenv said NOTHING about EOL anywhere. PHP 8.0 died 26 Nov
/// 2023, 8.1 on 31 Dec 2025, and the app offered both with the same face as 8.4.
/// Adding 7.4 would have made it a third silently-dead runtime, which
/// `docs/DESIGN.md`'s "the sentence in front of the button that starts it" rule
/// forbids (`docs/PLAN-php-74-support.md` §4.3).
fn security_end(minor: &str) -> Option<&'static str> {
    Some(match minor {
        "7.4" => "2022-11-28",
        "8.0" => "2023-11-26",
        "8.1" => "2025-12-31",
        "8.2" => "2026-12-31",
        "8.3" => "2027-12-31",
        "8.4" => "2028-12-31",
        "8.5" => "2029-12-31",
        _ => return None,
    })
}

/// The date a minor's upstream security support ENDED, or `None` while it is
/// still supported (or unknown). `Some` = no more security fixes, ever.
pub fn eol_since(minor: &str) -> Option<&'static str> {
    eol_since_on(minor, time::OffsetDateTime::now_utc().date())
}

/// [`eol_since`] against an explicit date — the pure core, so the rule is
/// testable without waiting for a calendar.
fn eol_since_on(minor: &str, today: time::Date) -> Option<&'static str> {
    let end = security_end(minor)?;
    let parsed = time::Date::parse(
        end,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()?;
    (today > parsed).then_some(end)
}

/// Deterministic loopback FastCGI port for a minor series (`"8.3"` → `9783`), or
/// `None` if `minor` isn't exactly `major.minor` numeric.
pub fn fpm_port(minor: &str) -> Option<u16> {
    let mut parts = minor.split('.');
    let major: u16 = parts.next()?.parse().ok()?;
    let min: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None; // exactly major.minor, not a patch string
    }
    Some(FPM_PORT_BASE + major * 10 + min)
}

/// Deterministic loopback port for a minor's DEBUG (Xdebug) pool, or `None`
/// when the toggle isn't available for that minor — gated on
/// [`binaries::xdebug_supported`], so an unsupported minor (8.0: its static
/// build can't dlopen any .so) can never grow a debug pool by construction.
pub fn debug_fpm_port(minor: &str) -> Option<u16> {
    if !binaries::xdebug_supported(minor) {
        return None;
    }
    let mut parts = minor.split('.');
    let major: u16 = parts.next()?.parse().ok()?;
    let min: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(DEBUG_FPM_PORT_BASE + major * 10 + min)
}

/// Seed/refresh the `php_versions` registry from the pinned build set. Idempotent:
/// inserts unknown versions (the default minor enabled, others available), and on
/// re-run updates `patch`/`fpm_port`/`is_default` while **preserving** the user's
/// `installed` choices. Safe to call on every app start.
///
/// Returns the minors whose stored patch CHANGED — a pin bump riding an app
/// release (Option A patch updates: pins only move with a release, there is no
/// in-app updater). The startup task restarts those minors' live pools so an
/// adopted survivor doesn't keep serving the old patch, and GCs the old caches.
pub fn seed_registry(conn: &Connection) -> Result<Vec<String>> {
    let default_minor = minor_of(binaries::PHP_VERSION);
    let existing: std::collections::HashMap<String, String> = store::list_php_versions(conn)?
        .into_iter()
        .map(|v| (v.minor, v.patch))
        .collect();
    let mut bumped = Vec::new();
    for minor in all_minors() {
        let patch = patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port =
            fpm_port(&minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        if existing.get(&minor).is_some_and(|old| old != patch) {
            bumped.push(minor.clone());
        }
        let is_default = minor == default_minor;
        store::upsert_php_version(
            conn,
            &PhpVersion {
                minor: minor.clone(),
                patch: patch.to_string(),
                fpm_port: port,
                // On first insert the default is enabled; others are available but
                // off until the user installs them (Phase 2 §1.5). Preserved on update.
                installed: is_default,
                is_default,
            },
        )?;
    }
    Ok(bumped)
}

/// All registered PHP versions (installed + available), for the UI.
///
/// The stored row plus what is DERIVED from the pinned build set — Xdebug
/// availability and, where offered, the release that minor's debug pool loads.
/// Derived here rather than stored, so a pin change moves the UI on the next
/// read and there is no column to migrate; and returned as a distinct view type
/// so no caller can hold a [`PhpVersion`] whose derived fields were never
/// filled (see [`PhpVersionView`]).
pub fn list_versions(conn: &Connection) -> Result<Vec<PhpVersionView>> {
    Ok(store::list_php_versions(conn)?
        .into_iter()
        .map(|v| PhpVersionView {
            xdebug_supported: binaries::xdebug_supported(&v.minor),
            xdebug_version: binaries::xdebug_version_for(&v.minor),
            eol_since: eol_since(&v.minor),
            minor: v.minor,
            patch: v.patch,
            fpm_port: v.fpm_port,
            installed: v.installed,
            is_default: v.is_default,
        })
        .collect())
}

/// Enable (install) or disable (remove) a PHP version. Guards on removal: the
/// default version can't be removed, nor can one a site currently uses (it would
/// silently fall back to the default pool). The version must be in the registry.
pub fn set_installed(conn: &Connection, minor: &str, installed: bool) -> Result<()> {
    if !installed {
        let versions = store::list_php_versions(conn)?;
        if versions.iter().any(|v| v.minor == minor && v.is_default) {
            return Err(Error::Other(format!(
                "cannot remove the default PHP version ({minor})"
            )));
        }
        let in_use = crate::core::sites::list(conn)?
            .iter()
            .any(|s| minor_of(&s.php_version) == minor);
        if in_use {
            return Err(Error::Other(format!(
                "PHP {minor} is in use by a site — switch those sites first"
            )));
        }
    }
    if !store::set_php_installed(conn, minor, installed)? {
        return Err(Error::Other(format!("unknown PHP version: {minor}")));
    }
    Ok(())
}

/// Make `minor` the default PHP version for new sites. Guards: the version must be
/// in the registry AND installed (defaulting to an uninstalled version would point
/// new sites at a pool that never starts). Exactly one default always remains.
pub fn set_default(conn: &Connection, minor: &str) -> Result<()> {
    let versions = store::list_php_versions(conn)?;
    let v = versions
        .iter()
        .find(|v| v.minor == minor)
        .ok_or_else(|| Error::Other(format!("unknown PHP version: {minor}")))?;
    if !v.installed {
        return Err(Error::Other(format!(
            "install PHP {minor} before making it the default"
        )));
    }
    store::set_default_php_version(conn, minor)?;
    Ok(())
}

/// The minor series the app should start pools for: every registry row marked
/// `installed`. Falls back to the default minor if none are (so there is always a
/// working pool).
pub fn installed_minors(conn: &Connection) -> Result<Vec<String>> {
    let mut minors: Vec<String> = store::list_php_versions(conn)?
        .into_iter()
        .filter(|v| v.installed)
        .map(|v| v.minor)
        .collect();
    if minors.is_empty() {
        minors.push(minor_of(binaries::PHP_VERSION));
    }
    Ok(minors)
}

// ── Per-version PHP ini settings ────────────────────────────────────────────
//
// Curated, default-deny whitelist (mirroring the site-options editor): only
// these keys are ever written into a pool config, so a stored value can never
// smuggle an arbitrary directive. Values are syntax-validated BEFORE persisting
// and the rewritten config is gated on `php-fpm -t` before the pool restarts,
// so a bad value can never brick a pool. Written as `php_value[key]` (not
// `php_admin_value`) so WordPress can still `ini_set()` at runtime.

/// How a whitelisted ini value is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// PHP size shorthand: digits + optional K/M/G suffix (e.g. `512M`).
    Size,
    /// Like `Size` but also accepts `-1` (unlimited) — memory_limit only.
    SizeOrUnlimited,
    /// Plain integer within `[min, max]`.
    Int { min: i64, max: i64 },
}

/// One whitelisted setting: key, validation kind, and PHP's compiled default
/// (what actually applies when unset — our static builds load NO php.ini).
pub struct SettingSpec {
    pub key: &'static str,
    pub kind: SettingKind,
    pub default: &'static str,
}

/// The editable per-version ini settings. `max_execution_time` default is the
/// server-SAPI 30 (CLI's 0 doesn't apply to pools); `max_input_time` -1 means
/// "use max_execution_time".
pub const SETTINGS: &[SettingSpec] = &[
    SettingSpec { key: "memory_limit", kind: SettingKind::SizeOrUnlimited, default: "128M" },
    SettingSpec { key: "upload_max_filesize", kind: SettingKind::Size, default: "2M" },
    SettingSpec { key: "post_max_size", kind: SettingKind::Size, default: "8M" },
    SettingSpec {
        key: "max_execution_time",
        kind: SettingKind::Int { min: 0, max: 86_400 },
        default: "30",
    },
    SettingSpec {
        key: "max_input_time",
        kind: SettingKind::Int { min: -1, max: 86_400 },
        default: "-1",
    },
    SettingSpec {
        key: "max_input_vars",
        kind: SettingKind::Int { min: 1, max: 1_000_000 },
        default: "1000",
    },
];

fn setting_spec(key: &str) -> Option<&'static SettingSpec> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// Parse a PHP size-shorthand string (`64M`, `1G`, `524288`) to bytes, or `None`
/// if malformed. Suffix is case-insensitive; only K/M/G exist in PHP.
pub fn parse_php_size(v: &str) -> Option<u64> {
    let v = v.trim();
    let (digits, mult) = match v.chars().last()? {
        'k' | 'K' => (&v[..v.len() - 1], 1u64 << 10),
        'm' | 'M' => (&v[..v.len() - 1], 1u64 << 20),
        'g' | 'G' => (&v[..v.len() - 1], 1u64 << 30),
        _ => (v, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(mult)
}

fn validate_one(spec: &SettingSpec, value: &str) -> Result<()> {
    let ok = match spec.kind {
        SettingKind::Size => parse_php_size(value).is_some(),
        SettingKind::SizeOrUnlimited => value.trim() == "-1" || parse_php_size(value).is_some(),
        SettingKind::Int { min, max } => value
            .trim()
            .parse::<i64>()
            .is_ok_and(|n| n >= min && n <= max),
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Other(match spec.kind {
            SettingKind::Int { min, max } => {
                format!("{}: '{value}' is not an integer in {min}..={max}", spec.key)
            }
            _ => format!(
                "{}: '{value}' is not a PHP size (digits + optional K/M/G, e.g. 512M)",
                spec.key
            ),
        }))
    }
}

/// Validate a settings set for one minor: every key must be whitelisted, every
/// value must parse for its kind, keys must be unique, and the cross-field
/// gotcha is enforced — `upload_max_filesize` must not exceed the EFFECTIVE
/// `post_max_size` (stored or default 8M), else PHP silently caps uploads at
/// `post_max_size` and the setting lies. Returns normalized (trimmed) pairs.
pub fn validate_settings(pairs: &[(String, String)]) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        let spec = setting_spec(key)
            .ok_or_else(|| Error::Other(format!("unknown PHP setting: {key}")))?;
        if out.iter().any(|(k, _)| k == key) {
            return Err(Error::Other(format!("duplicate PHP setting: {key}")));
        }
        let value = value.trim().to_string();
        validate_one(spec, &value)?;
        out.push((key.clone(), value));
    }
    let effective = |key: &str| -> Option<u64> {
        let v = out
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or(setting_spec(key).expect("whitelisted").default);
        parse_php_size(v)
    };
    if let (Some(upload), Some(post)) = (effective("upload_max_filesize"), effective("post_max_size")) {
        if upload > post {
            return Err(Error::Other(format!(
                "upload_max_filesize ({}) exceeds post_max_size ({}) — PHP caps uploads at \
                 post_max_size, so raise post_max_size too",
                display_size(upload),
                display_size(post),
            )));
        }
    }
    Ok(out)
}

fn display_size(bytes: u64) -> String {
    if bytes >= 1 << 20 && bytes % (1 << 20) == 0 {
        format!("{}M", bytes >> 20)
    } else {
        format!("{bytes} bytes")
    }
}

/// The nginx `client_max_body_size` each minor needs so nginx never 413s a body
/// PHP would accept: max(effective upload_max_filesize, effective post_max_size),
/// in bytes, for every minor that stores either key. Minors with neither key set
/// are absent — the config's global default applies.
pub fn nginx_body_limits(
    settings: &std::collections::HashMap<String, Vec<(String, String)>>,
) -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    for (minor, pairs) in settings {
        let stored = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| parse_php_size(v))
        };
        let upload = stored("upload_max_filesize");
        let post = stored("post_max_size");
        if upload.is_none() && post.is_none() {
            continue;
        }
        let eff = |set: Option<u64>, key: &str| {
            set.or_else(|| parse_php_size(setting_spec(key).expect("whitelisted").default))
                .unwrap_or(0)
        };
        out.insert(
            minor.clone(),
            eff(upload, "upload_max_filesize").max(eff(post, "post_max_size")),
        );
    }
    out
}

/// A running php-fpm pool's status (for the Services view / metrics).
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub minor: String,
    pub port: u16,
    pub pid: u32,
    pub running: bool,
    /// Whether this is the minor's DEBUG (Xdebug) pool.
    pub debug: bool,
}

struct Pool {
    minor: String,
    port: u16,
    child: Proc,
    /// Debug (Xdebug) pool — same binary, own port/config, `-d zend_extension`.
    debug: bool,
    /// Consecutive `reap_dead` polls that read dead-looking (B29b). A probe
    /// result is not positive evidence — a transient failed port probe on a
    /// loaded box, or a momentary pid-table hiccup on an adopted master, must
    /// not cost the user a serving pool. Reset to 0 by any healthy poll.
    misses: u32,
}

/// Consecutive dead-looking polls before a pool is reaped on PROBE evidence
/// (B29b). Same value and rationale as the adopted-service reap's
/// `ADOPTED_MISS_LIMIT` (B29): the watchdog ticks every 10s, so 2 ≈ 20s —
/// past any transient hiccup, still prompt for a genuinely dead pool. A
/// spawned master whose `try_wait` reports exit is positive evidence and is
/// reaped immediately, counter or no counter.
const POOL_MISS_LIMIT: u32 = 2;

/// What one poll could see of a pool's MASTER.
enum MasterSight {
    /// A spawned child whose `try_wait` returned an exit status — positive
    /// evidence of death, not a probe. (The reverse, `Ok(None)`, is equally
    /// positive: it is our own child handle, no recycle trap exists.)
    ChildExited,
    /// Probe evidence: for a spawned child, "alive" (true); for an ADOPTED
    /// master, whether the pid POSITIVELY identifies as php-fpm — `kill -0`
    /// alone would keep a dead pool alive on a recycled pid (the B29 trap).
    Probed(bool),
}

/// One pool's fate from one poll (B29b). Returns `(new_misses, reap)`.
///
/// - [`MasterSight::ChildExited`] reaps immediately — a crash during start
///   must restart, grace or no grace, and `try_wait` cannot flap.
/// - Everything else is a probe: healthy = master identified AND (the port
///   serves OR the child is within its start grace). The miss arithmetic is
///   [`service_manager::adopted_reap_decision`] — the same contract, one
///   definition. Note the AND: a `php-fpm`-titled listener on the port with
///   the master GONE is the orphan-worker failure ("UI shows running, every
///   site hangs") and must keep accruing misses even though the port answers.
fn pool_fate(master: MasterSight, serving: bool, starting: bool, misses: u32) -> (u32, bool) {
    match master {
        MasterSight::ChildExited => (misses, true),
        MasterSight::Probed(master_ok) => {
            let healthy = master_ok && (serving || starting);
            crate::core::service_manager::adopted_reap_decision(healthy, misses, POOL_MISS_LIMIT)
        }
    }
}

/// Owns one php-fpm master per PHP version. Held by the `ServiceManager`.
#[derive(Default)]
pub struct PhpFpmPools {
    pools: Vec<Pool>,
    /// `php_admin_value[sendmail_path]` baked into every pool's config so site PHP
    /// `mail()` is routed to Mailpit (§2.2). Set by `ServiceManager` once Mailpit's
    /// binary is resolved; `None` ⇒ pools use PHP's default sendmail.
    sendmail_path: Option<String>,
    /// Per-minor ini settings (whitelisted, pre-validated — see [`SETTINGS`])
    /// written as `php_value[key]` lines into that pool's config. Set by
    /// `ServiceManager` from the SQLite `php_settings` table.
    settings: std::collections::HashMap<String, Vec<(String, String)>>,
}

impl PhpFpmPools {
    /// Set the mail-routing shim used when (re)writing pool configs. Applies to
    /// pools started afterward (a running pool keeps its config until restarted).
    pub fn set_sendmail_path(&mut self, sendmail_path: Option<String>) {
        self.sendmail_path = sendmail_path;
    }

    /// Set the per-minor ini settings used when (re)writing pool configs. Like
    /// the sendmail shim, applies to pools started afterward — the caller
    /// restarts an affected running pool to make new values live.
    pub fn set_settings(
        &mut self,
        settings: std::collections::HashMap<String, Vec<(String, String)>>,
    ) {
        self.settings = settings;
    }

    /// Start a pool for `minor` if one isn't already running. Idempotent: resolves
    /// (downloads on first use) the version's `php-fpm`, gates on a free port, then
    /// writes the pool config and spawns the foreground master.
    pub async fn ensure(&mut self, platform: &dyn Platform, minor: &str) -> Result<()> {
        if self.has(minor, false) {
            return Ok(());
        }
        let patch = patch_for_minor(minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port =
            fpm_port(minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        ports::ensure_free(platform, port, ports::Proto::Tcp, "PHP-FPM")?;
        let bin = binaries::resolve(platform, "php-fpm", patch).await?;
        let settings = self.settings.get(minor).map(Vec::as_slice).unwrap_or(&[]);
        let conf = services::write_fpm_config(
            platform,
            minor,
            port,
            self.sendmail_path.as_deref(),
            settings,
        )?;
        let child = services::start_fpm(platform, &bin, &conf)?;
        self.pools.push(Pool {
            minor: minor.to_string(),
            port,
            child: child.into(),
            debug: false,
            misses: 0,
        });
        Ok(())
    }

    /// Start the DEBUG (Xdebug) pool for `minor` if one isn't already running.
    /// Same shape as [`Self::ensure`] plus: resolves the minor's pinned
    /// `xdebug.so` bundle, then GATES on a real load probe (`php-fpm -m` must
    /// list the module) — PHP treats a failed `zend_extension` as a warning and
    /// starts anyway, so without the gate a bad .so would serve sites with the
    /// toggle silently OFF. Refused for minors without Xdebug support (8.0).
    pub async fn ensure_debug(&mut self, platform: &dyn Platform, minor: &str) -> Result<()> {
        if self.has(minor, true) {
            return Ok(());
        }
        let patch = patch_for_minor(minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port = debug_fpm_port(minor).ok_or_else(|| {
            Error::Other(format!("Xdebug is not available for PHP {minor}"))
        })?;
        ports::ensure_free(platform, port, ports::Proto::Tcp, "PHP-FPM (Xdebug)")?;
        let bin = binaries::resolve(platform, "php-fpm", patch).await?;
        let (bundle, bundle_version) = binaries::xdebug_bundle_id(minor)
            .ok_or_else(|| Error::Other(format!("Xdebug is not available for PHP {minor}")))?;
        let so = binaries::resolve_bundle(platform, &bundle, bundle_version)
            .await?
            .join("xdebug.so");
        services::assert_fpm_loads_xdebug(platform, &bin, &so)?;
        let settings = self.settings.get(minor).map(Vec::as_slice).unwrap_or(&[]);
        let conf = services::write_fpm_config(
            platform,
            &format!("{minor}-debug"),
            port,
            self.sendmail_path.as_deref(),
            settings,
        )?;
        let child = services::start_fpm_xdebug(platform, &bin, &conf, &so)?;
        self.pools.push(Pool {
            minor: minor.to_string(),
            port,
            child: child.into(),
            debug: true,
            misses: 0,
        });
        Ok(())
    }

    /// Adopt a pool master surviving from a prior app session (services outlive
    /// the app; see `ServiceManager::adopt_startup`). The pool is then managed
    /// exactly like a spawned one: listed in status, skipped by `ensure`,
    /// stopped by `stop_all`.
    pub fn adopt(&mut self, minor: &str, port: u16, pid: u32, debug: bool) {
        if self.has(minor, debug) {
            return;
        }
        self.pools.push(Pool {
            minor: minor.to_string(),
            port,
            child: Proc::Adopted(pid),
            debug,
            misses: 0,
        });
    }

    /// Ensure a pool is running for each minor in `minors`.
    pub async fn start(&mut self, platform: &dyn Platform, minors: &[String]) -> Result<()> {
        for m in minors {
            self.ensure(platform, m).await?;
        }
        Ok(())
    }

    /// Drop pools that are dead — port closed OR master process gone — reaping
    /// the child and any ORPHANED WORKERS still squatting on the pool port.
    /// Returns the affected `(minor, debug)` pairs so the health watchdog can
    /// `ensure`/`ensure_debug` them again (a fresh spawn, same config path).
    ///
    /// The master-alive check matters: php-fpm workers outlive a SIGKILLed
    /// master, keep the inherited listen socket accepting (so the port probe
    /// stays green), but can't scale or recover — the exact "UI shows running,
    /// every site hangs" failure. Workers also rewrite their process title to
    /// `php-fpm: pool www` (no app-data path), so the marker-gated orphan sweep
    /// can't see them; on OUR fixed pool port, a `php-fpm`-titled listener is
    /// ours — kill it so the respawn's port gate passes.
    pub fn reap_dead(&mut self, platform: &dyn Platform) -> Vec<(String, bool)> {
        let mut dead = Vec::new();
        for mut p in std::mem::take(&mut self.pools) {
            // A dead MASTER is always reaped. A closed port alone is not — a
            // just-spawned master (Start-all still awaiting readiness outside
            // the lock) hasn't bound yet; killing it here is the watchdog/
            // Start-all race. Within the start grace, alive + not-listening
            // means "starting", not "dead".
            //
            // B29b: probe evidence goes through a MISS COUNTER, never a single
            // poll. Only a spawned child's `try_wait` exit — positive evidence
            // — reaps immediately. An ADOPTED master is identified by its
            // command line, not `kill -0`: a recycled pid must read as a miss.
            let master = if p.child.is_adopted() {
                let ours = platform
                    .supervisor()
                    .pid_command(p.child.id())
                    .is_some_and(|cmd| cmd.contains("php-fpm"));
                MasterSight::Probed(ours)
            } else if !p.child.alive() {
                MasterSight::ChildExited
            } else {
                MasterSight::Probed(true)
            };
            let (misses, reap) = pool_fate(
                master,
                services::fpm_running(p.port),
                p.child.starting(),
                p.misses,
            );
            p.misses = misses;
            if reap {
                dead.push(p);
            } else {
                self.pools.push(p);
            }
        }
        dead.into_iter()
            .map(|mut p| {
                let _ = services::stop(platform, p.child.id()); // no-op if already gone
                p.child.kill();
                p.child.wait();
                for pid in platform.supervisor().owned_listeners(p.port, "php-fpm") {
                    let _ = platform.supervisor().stop(pid);
                }
                (p.minor, p.debug)
            })
            .collect()
    }

    /// Stop ONE pool (for a settings-change restart), reaping the master and any
    /// orphaned workers still on the pool port — same sweep as [`Self::reap_dead`],
    /// so the follow-up `ensure`'s port gate passes. Returns whether that pool
    /// was actually running (false ⇒ nothing to restart).
    pub fn stop_one(&mut self, platform: &dyn Platform, minor: &str, debug: bool) -> bool {
        let Some(i) = self
            .pools
            .iter()
            .position(|p| p.minor == minor && p.debug == debug)
        else {
            return false;
        };
        let mut p = self.pools.remove(i);
        let _ = services::stop(platform, p.child.id());
        p.child.wait();
        for pid in platform.supervisor().owned_listeners(p.port, "php-fpm") {
            let _ = platform.supervisor().stop(pid);
        }
        true
    }

    /// Stop and clear every pool. Stack guard: a non-app process (live-check
    /// example) skips ADOPTED pools — they are the user's serving stack.
    pub fn stop_all(&mut self, platform: &dyn Platform) {
        let may_foreign = crate::core::stack_guard::may_control_real_stack();
        for mut p in std::mem::take(&mut self.pools) {
            if may_foreign || !p.child.is_adopted() {
                let _ = services::stop(platform, p.child.id());
                p.child.wait();
            } else {
                log::warn!(
                    "rexenv: stack guard — leaving adopted php-fpm {} pool running",
                    p.minor
                );
            }
        }
    }

    /// Whether no pools are currently managed.
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

    /// Whether a pool for `minor` (normal or debug) is currently managed
    /// (spawned or adopted).
    pub fn has(&self, minor: &str, debug: bool) -> bool {
        self.pools.iter().any(|p| p.minor == minor && p.debug == debug)
    }

    /// Per-pool status, ordered by minor series (a minor's normal pool before
    /// its debug pool).
    pub fn status(&self) -> Vec<PoolStatus> {
        let mut out: Vec<PoolStatus> = self
            .pools
            .iter()
            .map(|p| PoolStatus {
                minor: p.minor.clone(),
                port: p.port,
                pid: p.child.id(),
                running: services::fpm_running(p.port),
                debug: p.debug,
            })
            .collect();
        out.sort_by(|a, b| (&a.minor, a.debug).cmp(&(&b.minor, b.debug)));
        out
    }
}

impl Drop for PhpFpmPools {
    fn drop(&mut self) {
        // Best-effort stop so no pool is orphaned if stop_all wasn't called.
        // SIGTERM (graceful), NOT SIGKILL: a SIGKILLed fpm master leaks its
        // workers, which keep the pool port accepting forever — the "UI shows
        // running, every site hangs" failure.
        for p in &mut self.pools {
            p.child.terminate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;

    fn pairs(kv: &[(&str, &str)]) -> Vec<(String, String)> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn parse_php_size_handles_shorthand_and_bytes() {
        assert_eq!(parse_php_size("512M"), Some(512 << 20));
        assert_eq!(parse_php_size("64m"), Some(64 << 20));
        assert_eq!(parse_php_size("1G"), Some(1 << 30));
        assert_eq!(parse_php_size("2048K"), Some(2048 << 10));
        assert_eq!(parse_php_size("524288"), Some(524_288));
        for bad in ["banana", "", "M", "12MB", "1.5G", "-1", "12 M"] {
            assert_eq!(parse_php_size(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn validate_settings_accepts_good_values() {
        let out = validate_settings(&pairs(&[
            ("memory_limit", "512M"),
            ("upload_max_filesize", "64M"),
            ("post_max_size", "64M"),
            ("max_execution_time", "600"),
            ("max_input_vars", "5000"),
        ]))
        .unwrap();
        assert_eq!(out.len(), 5);
        // memory_limit may be unlimited.
        validate_settings(&pairs(&[("memory_limit", "-1")])).unwrap();
    }

    #[test]
    fn validate_settings_rejects_bad_input_before_any_write() {
        // The "banana" gate: a malformed value errors in validation, long before
        // any config write or pool restart.
        assert!(validate_settings(&pairs(&[("memory_limit", "banana")])).is_err());
        assert!(validate_settings(&pairs(&[("max_execution_time", "-5")])).is_err());
        assert!(validate_settings(&pairs(&[("upload_max_filesize", "-1")])).is_err());
        // Default-deny whitelist: unknown keys never reach a pool config.
        assert!(validate_settings(&pairs(&[("disable_functions", "exec")])).is_err());
        assert!(validate_settings(&pairs(&[
            ("memory_limit", "1M"),
            ("memory_limit", "2M")
        ]))
        .is_err());
    }

    #[test]
    fn validate_settings_catches_upload_exceeding_post_max_size() {
        // upload > EFFECTIVE post (default 8M when unset) → PHP would silently cap
        // uploads at post_max_size, so the pair must be rejected as a set.
        assert!(validate_settings(&pairs(&[("upload_max_filesize", "64M")])).is_err());
        assert!(validate_settings(&pairs(&[
            ("upload_max_filesize", "64M"),
            ("post_max_size", "32M")
        ]))
        .is_err());
        // Equal or covered by post → fine.
        validate_settings(&pairs(&[
            ("upload_max_filesize", "64M"),
            ("post_max_size", "64M"),
        ]))
        .unwrap();
        validate_settings(&pairs(&[("upload_max_filesize", "8M")])).unwrap();
    }

    #[test]
    fn nginx_body_limits_mirror_effective_upload_and_post() {
        let mut map = std::collections::HashMap::new();
        map.insert("8.3".to_string(), pairs(&[("upload_max_filesize", "64M"), ("post_max_size", "80M")]));
        // Only post set: effective upload stays at its 2M default → limit = post.
        map.insert("8.2".to_string(), pairs(&[("post_max_size", "16M")]));
        // Neither body key set: no per-server limit (global default applies).
        map.insert("8.1".to_string(), pairs(&[("memory_limit", "1G")]));
        let limits = nginx_body_limits(&map);
        assert_eq!(limits.get("8.3"), Some(&(80u64 << 20)));
        assert_eq!(limits.get("8.2"), Some(&(16u64 << 20)));
        assert_eq!(limits.get("8.1"), None);
    }

    #[test]
    fn minor_of_strips_patch() {
        assert_eq!(minor_of("8.3.31"), "8.3");
        assert_eq!(minor_of("8.1.34"), "8.1");
        assert_eq!(minor_of("8.3"), "8.3");
    }

    #[test]
    fn fpm_port_is_deterministic_and_keeps_phase1_port() {
        assert_eq!(fpm_port("8.0"), Some(9780));
        assert_eq!(fpm_port("8.1"), Some(9781));
        assert_eq!(fpm_port("8.2"), Some(9782));
        // 8.3 must equal the Phase-1 single-pool port.
        assert_eq!(fpm_port("8.3"), Some(9783));
        assert_eq!(fpm_port("8.3"), Some(services::PHP_FPM_PORT));
        assert_eq!(fpm_port("8.4"), Some(9784));
        assert_eq!(fpm_port("8.5"), Some(9785));
        // Distinct per version.
        assert_ne!(fpm_port("8.1"), fpm_port("8.2"));
        // Rejects a patch string or junk.
        assert_eq!(fpm_port("8.3.31"), None);
        assert_eq!(fpm_port("x.y"), None);
    }

    #[test]
    fn debug_fpm_port_covers_supported_minors_and_refuses_80() {
        // Same 9900-based scheme, disjoint from the normal pool range.
        assert_eq!(debug_fpm_port("8.1"), Some(9981));
        assert_eq!(debug_fpm_port("8.4"), Some(9984));
        assert_eq!(debug_fpm_port("8.5"), Some(9985));
        for minor in all_minors() {
            if let (Some(d), Some(n)) = (debug_fpm_port(&minor), fpm_port(&minor)) {
                assert_ne!(d, n);
            }
        }
        // 8.0's static build can't dlopen — no debug port EXISTS for it, so no
        // caller can ever route a site there.
        assert_eq!(debug_fpm_port("8.0"), None);
        assert_eq!(debug_fpm_port("8.3.31"), None);
        assert_eq!(debug_fpm_port("banana"), None);
    }

    #[test]
    fn minors_and_patches_track_pinned_builds() {
        let minors = all_minors();
        // The offered set today. A minor JOINING it is a deliberate pin change
        // (docs/PLAN-php-74-support.md), so this list is a floor, not a ceiling.
        for want in ["8.0", "8.1", "8.2", "8.3", "8.4", "8.5"] {
            assert!(minors.contains(&want.to_string()), "missing {want}");
        }
        for m in &minors {
            // Every minor maps back to a pinned patch in the same series.
            let patch = patch_for_minor(m).unwrap();
            assert_eq!(&minor_of(patch), m);
            assert!(fpm_port(m).is_some());
        }
        assert!(patch_for_minor(unshipped_minor()).is_none());
    }

    /// The fixture must be a fact, not a literal — this is what makes every
    /// "refused by name" assert below mean something.
    #[test]
    fn the_unshipped_fixture_is_derived_from_the_pinned_set() {
        let m = unshipped_minor();
        assert!(patch_for_minor(m).is_none(), "{m} is pinned — not a valid fixture");
        assert!(!available_minors().contains(&m.to_string()), "{m}");
        // …and it parses as a minor everywhere a real one would, so a refusal
        // reads naturally and the port/cache rules exercise their real branches.
        assert!(fpm_port(m).is_some(), "{m} must be port-derivable like any minor");
        assert_eq!(minor_of(&unshipped_patch()), m);
    }

    #[test]
    /// The one thing that keeps a hand-written date table honest: a minor
    /// cannot enter `PHP_VERSIONS` without declaring when its security support
    /// ends. Without this the failure is silent and in the safe-looking
    /// direction — `eol_since` returns `None`, the UI shows no warning, and a
    /// dead runtime reads as a supported one.
    #[test]
    fn every_shipped_minor_declares_its_support_end() {
        for minor in all_minors() {
            assert!(
                security_end(&minor).is_some(),
                "PHP {minor} ships with no security-support end date — add it to `security_end`"
            );
        }
    }

    /// EOL is computed against TODAY, so it becomes true on the day it becomes
    /// true. A stored bool would need somebody to remember; nobody did, which is
    /// why 8.0 was offered silently from Nov 2023.
    #[test]
    fn eol_is_derived_from_the_date_not_remembered() {
        use time::macros::date;
        // Before, on, and after 8.0's end date (26 Nov 2023).
        assert_eq!(eol_since_on("8.0", date!(2023 - 11 - 25)), None);
        assert_eq!(eol_since_on("8.0", date!(2023 - 11 - 26)), None, "the end date itself is still supported");
        assert_eq!(eol_since_on("8.0", date!(2023 - 11 - 27)), Some("2023-11-26"));
        // 8.1 died 31 Dec 2025 — rexenv ships it and said nothing.
        assert_eq!(eol_since_on("8.1", date!(2025 - 12 - 31)), None);
        assert_eq!(eol_since_on("8.1", date!(2026 - 01 - 01)), Some("2025-12-31"));
        // A minor still in support, and one with no declared date.
        assert_eq!(eol_since_on("8.4", date!(2026 - 08 - 14)), None);
        assert_eq!(eol_since_on(unshipped_minor(), date!(2026 - 08 - 14)), None);
        // The same future date makes EVERY shipped minor EOL — proving the rule
        // is the comparison and not a per-version literal someone typed.
        for minor in all_minors() {
            assert!(eol_since_on(&minor, date!(2099 - 01 - 01)).is_some(), "{minor}");
        }
    }

    /// The tell reaches the row the UI actually renders.
    #[test]
    fn the_version_list_carries_the_eol_date() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let rows = list_versions(&conn).unwrap();
        for r in &rows {
            assert_eq!(r.eol_since, eol_since(&r.minor), "{}", r.minor);
        }
        // Not uniformly null — 8.0 and 8.1 are past their end dates today, so a
        // field stubbed to None could not pass this.
        assert!(
            rows.iter().any(|r| r.eol_since.is_some()),
            "no EOL row — either the table is wrong or this test has gone vacuous"
        );
        assert!(rows.iter().any(|r| r.eol_since.is_none()), "every shipped minor is EOL?");
    }

    /// The UI's Xdebug rule is CORE's, carried down — never re-decided in the
    /// client. `SiteDetail` disabled the toggle on a literal `minor === "8.0"`,
    /// so the frontend held a second copy of `binaries::xdebug_supported` that
    /// was free to disagree with it (four guards of that shape have bitten
    /// here). This asserts the carried value equals the source for EVERY row,
    /// so the copy cannot come back by drifting — it has to come back by
    /// deleting this.
    #[test]
    fn the_version_list_carries_cores_xdebug_rule_rather_than_the_ui_guessing() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let rows = list_versions(&conn).unwrap();
        assert!(!rows.is_empty());
        for r in &rows {
            assert_eq!(
                r.xdebug_supported,
                binaries::xdebug_supported(&r.minor),
                "{} disagrees with core",
                r.minor
            );
            assert_eq!(r.xdebug_version, binaries::xdebug_version_for(&r.minor), "{}", r.minor);
            // The two travel together: a version without support, or support
            // without a version, would each render a control that lies.
            assert_eq!(r.xdebug_supported, r.xdebug_version.is_some(), "{}", r.minor);
        }
        // …and the set is not uniformly true, so an accessor stubbed to a
        // constant could not pass this. 8.0 ships and has no Xdebug.
        assert!(rows.iter().any(|r| !r.xdebug_supported), "no unsupported row — the fixture is too tidy");
        assert!(rows.iter().any(|r| r.xdebug_supported));
    }

    #[test]
    fn seed_registry_reports_patch_bumps_and_preserves_installed() {
        let conn = db::open_in_memory().unwrap();
        // First seed (fresh DB) and a same-pin re-seed report no bumps.
        assert!(seed_registry(&conn).unwrap().is_empty());
        assert!(seed_registry(&conn).unwrap().is_empty());
        // Simulate a prior app release: 8.3 installed at an older patch.
        conn.execute(
            "UPDATE php_versions SET patch = '8.3.30', installed = 1 WHERE minor = '8.3'",
            [],
        )
        .unwrap();
        // This app's seed bumps the pin: reported, patch updated, installed kept.
        assert_eq!(seed_registry(&conn).unwrap(), vec!["8.3".to_string()]);
        let row = store::list_php_versions(&conn)
            .unwrap()
            .into_iter()
            .find(|v| v.minor == "8.3")
            .unwrap();
        assert_eq!(row.patch, patch_for_minor("8.3").unwrap());
        assert!(row.installed);
        // And the bump is one-shot: the next launch reports nothing.
        assert!(seed_registry(&conn).unwrap().is_empty());
    }

    #[test]
    fn seed_registry_marks_default_installed_and_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();

        let rows = store::list_php_versions(&conn).unwrap();
        assert_eq!(rows.len(), all_minors().len());
        let default_minor = minor_of(binaries::PHP_VERSION);
        let def = rows.iter().find(|v| v.minor == default_minor).unwrap();
        assert!(def.is_default && def.installed);
        assert!(def.fpm_port == fpm_port(&default_minor).unwrap());
        // Non-default versions exist but aren't installed by default.
        assert!(rows.iter().any(|v| !v.is_default && !v.installed));

        // installed_minors reflects the default.
        assert_eq!(installed_minors(&conn).unwrap(), vec![default_minor.clone()]);

        // A user enables 8.1; re-seeding must NOT clobber that choice.
        store::set_php_installed(&conn, "8.1", true).unwrap();
        seed_registry(&conn).unwrap();
        let mut got = installed_minors(&conn).unwrap();
        got.sort();
        assert!(got.contains(&"8.1".to_string()));
        assert!(got.contains(&default_minor));
    }

    #[test]
    fn set_default_switches_exclusively_and_requires_installed() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let default_minor = minor_of(binaries::PHP_VERSION); // 8.3, installed by seed

        // Can't default to an uninstalled version.
        assert!(set_default(&conn, "8.1").is_err());
        // Unknown version errors.
        assert!(set_default(&conn, unshipped_minor()).is_err());

        // Install 8.1, then make it the default → exactly one default, and it moved.
        set_installed(&conn, "8.1", true).unwrap();
        set_default(&conn, "8.1").unwrap();
        let rows = store::list_php_versions(&conn).unwrap();
        assert_eq!(rows.iter().filter(|v| v.is_default).count(), 1);
        assert!(rows.iter().find(|v| v.minor == "8.1").unwrap().is_default);
        assert!(!rows.iter().find(|v| v.minor == default_minor).unwrap().is_default);
    }

    #[test]
    fn set_installed_guards_default_and_in_use() {
        use crate::core::sites;
        use crate::state::models::{NewSite, SiteType, WebServer};

        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let default_minor = minor_of(binaries::PHP_VERSION); // "8.3"

        // The default version can't be removed.
        assert!(set_installed(&conn, &default_minor, false).is_err());

        // Install 8.1, then put a site on it → it can't be removed.
        set_installed(&conn, "8.1", true).unwrap();
        sites::create(
            &conn,
            NewSite {
                name: "S".into(),
                domain: "s.test".into(),
                site_type: SiteType::Php,
                php_version: "8.1".into(),
                web_server: WebServer::Nginx,
                path: "~/Sites/s".into(),
                db_engine: crate::state::models::SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
            },
        )
        .unwrap();
        assert!(set_installed(&conn, "8.1", false).is_err());

        // An unknown version is rejected.
        assert!(set_installed(&conn, "9.9", true).is_err());
    }

    #[test]
    fn installed_minors_falls_back_to_default() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        // Disable everything → still returns the default minor.
        for v in store::list_php_versions(&conn).unwrap() {
            store::set_php_installed(&conn, &v.minor, false).unwrap();
        }
        assert_eq!(
            installed_minors(&conn).unwrap(),
            vec![minor_of(binaries::PHP_VERSION)]
        );
    }

    #[test]
    fn pools_start_empty() {
        let pools = PhpFpmPools::default();
        assert!(pools.is_empty());
        assert!(pools.status().is_empty());
    }

    /// The watchdog/Start-all race (docs/TODO.md): a pool spawned by an
    /// in-flight Start-all hasn't bound its port when a watchdog tick lands —
    /// alive + within the start grace must NOT be reaped. A dead master must
    /// still be reaped immediately, grace or no grace (a crash during start
    /// has to restart).
    use crate::platform::traits::*;

    /// Test supervisor: `pid_command` answers with a fixed string, so a test
    /// chooses whether an adopted master identifies as php-fpm, something
    /// recycled, or gone (`None`).
    struct StubSupervisor {
        cmd: Option<&'static str>,
    }
    impl ProcessSupervisor for StubSupervisor {
        fn spawn(&self, _: &std::path::Path, _: &[String]) -> crate::error::Result<std::process::Child> {
            unimplemented!()
        }
        fn spawn_logged(
            &self,
            _: &std::path::Path,
            _: &[String],
            _: &std::path::Path,
        ) -> crate::error::Result<std::process::Child> {
            unimplemented!()
        }
        fn stop(&self, _pid: u32) -> crate::error::Result<()> {
            Ok(())
        }
        fn pid_command(&self, _pid: u32) -> Option<String> {
            self.cmd.map(String::from)
        }
    }
    struct StubPlatform(StubSupervisor);
    impl Platform for StubPlatform {
        fn supervisor(&self) -> &dyn ProcessSupervisor {
            &self.0
        }
        fn paths(&self) -> &dyn Paths { unimplemented!() }
        fn dns(&self) -> &dyn DnsManager { unimplemented!() }
        fn cert_trust(&self) -> &dyn CertTrustManager { unimplemented!() }
        fn privileges(&self) -> &dyn PrivilegeManager { unimplemented!() }
        fn autostart(&self) -> &dyn AutostartManager { unimplemented!() }
        fn permissions(&self) -> &dyn PermissionManager { unimplemented!() }
        fn shell(&self) -> &dyn ShellRunner { unimplemented!() }
        fn binaries(&self) -> &dyn BinaryProvider { unimplemented!() }
        fn edge(&self) -> &dyn EdgeSupervisor { unimplemented!() }
        fn dns_agent(&self) -> &dyn DnsAgentManager { unimplemented!() }
    }

    #[test]
    fn reap_dead_spares_a_starting_child_but_reaps_a_dead_master() {
        let platform = StubPlatform(StubSupervisor { cmd: None });

        // Port 1 is never listening on a dev box without root — both pools
        // read "port closed"; only liveness + grace differ.
        let alive = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let mut dead = std::process::Command::new("true").spawn().unwrap();
        let _ = dead.wait();

        let mut pools = PhpFpmPools::default();
        pools.pools.push(Pool {
            minor: "8.4".into(),
            port: 1,
            child: alive.into(), // freshly stamped → within START_GRACE
            debug: false,
            misses: 0,
        });
        pools.pools.push(Pool {
            minor: "8.3".into(),
            port: 1,
            child: dead.into(),
            debug: false,
            misses: 0,
        });

        let reaped = pools.reap_dead(&platform);
        assert_eq!(reaped, vec![("8.3".to_string(), false)], "dead master reaped despite grace");
        assert!(pools.has("8.4", false), "starting child must survive the sweep");

        // Kill (not stop_all) — the stub's no-op `stop` would leave `wait`
        // blocking the suite for the sleep's full 30s.
        for p in &mut pools.pools {
            p.child.kill();
            p.child.wait();
        }
    }

    /// B29b, the decision table. The one-miss bug: a single failed port probe
    /// on a loaded box reaped a serving pool. Probe evidence now accrues
    /// misses; only a spawned child's `try_wait` exit reaps on sight.
    #[test]
    fn pool_fate_takes_consecutive_misses_for_probes_and_one_exit_for_evidence() {
        // One failed port probe (master fine, past grace) is a MISS, not a reap.
        assert_eq!(
            pool_fate(MasterSight::Probed(true), false, false, 0),
            (1, false),
            "a single failed port probe reaped a live pool — the B29b one-miss bug"
        );
        // The limit is real: the second consecutive miss reaps.
        assert_eq!(pool_fate(MasterSight::Probed(true), false, false, 1), (2, true));
        // Health RESETS the count — misses are consecutive, never cumulative.
        assert_eq!(pool_fate(MasterSight::Probed(true), true, false, 1), (0, false));
        // The start grace counts as health (the watchdog/Start-all race).
        assert_eq!(pool_fate(MasterSight::Probed(true), false, true, 1), (0, false));
        // A spawned child's exit is positive evidence: reaped at zero misses,
        // grace or no grace.
        assert!(pool_fate(MasterSight::ChildExited, true, true, 0).1);
        // Orphan workers: the port SERVES but the master no longer identifies
        // — that is the "UI shows running, every site hangs" failure, and a
        // green port must not reset the count.
        assert_eq!(
            pool_fate(MasterSight::Probed(false), true, false, 1),
            (2, true),
            "a serving port with an unidentified master must keep accruing misses"
        );
    }

    /// B29b wiring: `reap_dead` really consults the counter (a live child past
    /// its grace with a closed port survives poll one, dies on poll two), and
    /// an ADOPTED master is judged by POSITIVE IDENTIFICATION, not `kill -0` —
    /// a pid whose command line is not php-fpm is reaped even while the port
    /// serves (recycled pid / orphaned workers), while an identified master on
    /// a serving port never accrues a miss.
    #[test]
    fn reap_dead_needs_two_misses_and_identifies_adopted_masters_positively() {
        // A real listener makes fpm_running(port) true without any php-fpm.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let serving_port = listener.local_addr().unwrap().port();

        // Poll 1 vs poll 2 for a probe-dead spawned pool.
        let platform = StubPlatform(StubSupervisor { cmd: None });
        let alive = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let backdated =
            std::time::Instant::now() - Proc::START_GRACE - std::time::Duration::from_secs(1);
        let mut pools = PhpFpmPools::default();
        pools.pools.push(Pool {
            minor: "8.4".into(),
            port: 1, // never listening without root
            child: Proc::Child(alive, backdated),
            debug: false,
            misses: 0,
        });
        assert!(
            pools.reap_dead(&platform).is_empty(),
            "first failed probe must keep the pool (miss 1 of {POOL_MISS_LIMIT})"
        );
        assert!(pools.has("8.4", false));
        let reaped = pools.reap_dead(&platform);
        assert_eq!(reaped, vec![("8.4".to_string(), false)], "second miss reaps");

        // Adopted, port serving, but the pid identifies as NOT php-fpm
        // (recycled) — reaped after the limit despite the green port.
        let recycled = StubPlatform(StubSupervisor { cmd: Some("totally-not-fpm --flag") });
        let mut pools = PhpFpmPools::default();
        pools.adopt("8.3", serving_port, 4242, false);
        assert!(pools.reap_dead(&recycled).is_empty(), "miss 1: kept");
        assert!(!pools.reap_dead(&recycled).is_empty(), "recycled pid reaped at the limit");

        // Adopted, port serving, pid identifies as php-fpm — healthy forever.
        let ours = StubPlatform(StubSupervisor { cmd: Some("php-fpm: master process (conf)") });
        let mut pools = PhpFpmPools::default();
        pools.adopt("8.3", serving_port, 4242, false);
        for _ in 0..4 {
            assert!(pools.reap_dead(&ours).is_empty(), "an identified serving master never reaps");
        }
        assert_eq!(pools.pools[0].misses, 0, "health must RESET the count, not just not-reap");
    }
}
