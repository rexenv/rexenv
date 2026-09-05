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
/// (`docs/archive/PLAN-php-74-support.md` retires that). The moment 7.4 gains a pinned
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
/// forbids (`docs/archive/PLAN-php-74-support.md` §4.3).
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

/// The `(major, minor)` a port offset is derived from, or `None` when this
/// scheme cannot express it.
///
/// # The ten-slot limit, and why it is a refusal rather than a wider formula
///
/// Both port schemes are `base + major * 10 + minor`, which gives each major
/// exactly TEN slots — so `fpm_port("8.10")` and `fpm_port("9.0")` both land on
/// 9790. PHP has never shipped an x.10 minor, so this is unreachable today; it
/// stops being unreachable by the calendar alone, not by anything rexenv does.
///
/// **Widening the formula was rejected, because every existing port would move.**
/// `adopt_startup`, the managed-port set and the orphaned-worker sweep all
/// enumerate by CALLING these functions, so a moved port strands a running
/// survivor that is neither adoptable nor sweepable while `ports::ensure_free`
/// happily binds the new one — the user's sites keep being served by a master
/// rexenv can no longer see, until something needs the port. That is a worse
/// failure than the one being fixed, and it would land on every user at once.
///
/// So the scheme keeps its ten slots and REFUSES the eleventh. A minor this
/// cannot express resolves to `None`, `seed_registry` fails loudly by name, and
/// `every_shipped_minor_has_a_unique_pool_port` fails at `cargo test` — before
/// any release, which is the only place this can be fixed cheaply. The message
/// there says what to change.
fn port_offset(minor: &str) -> Option<u16> {
    let mut parts = minor.split('.');
    let major: u16 = parts.next()?.parse().ok()?;
    let min: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None; // exactly major.minor, not a patch string
    }
    // The eleventh slot would alias the next major's first.
    (min < 10).then_some(major * 10 + min)
}

/// Deterministic loopback FastCGI port for a minor series (`"8.3"` → `9783`), or
/// `None` if `minor` isn't exactly `major.minor` numeric, or is a minor this
/// port scheme cannot express ([`port_offset`]).
pub fn fpm_port(minor: &str) -> Option<u16> {
    Some(FPM_PORT_BASE + port_offset(minor)?)
}

/// Deterministic loopback port for a minor's DEBUG (Xdebug) pool, or `None`
/// when the toggle isn't available for that minor — gated on
/// [`binaries::xdebug_supported`], so an unsupported minor (8.0: its static
/// build can't dlopen any .so) can never grow a debug pool by construction.
///
/// Shares [`port_offset`] with [`fpm_port`] so the ten-slot rule lives in ONE
/// place: it used to be two copies of the same arithmetic, which is how a fix to
/// one would have left the other aliasing.
pub fn debug_fpm_port(minor: &str) -> Option<u16> {
    if !binaries::xdebug_supported(minor) {
        return None;
    }
    Some(DEBUG_FPM_PORT_BASE + port_offset(minor)?)
}

/// Seed/refresh the `php_versions` registry from the pinned build set. Idempotent
/// and safe to call on every app start.
///
/// **On re-run it updates exactly one column: `fpm_port`.** Everything else the
/// row carries is either the user's (`installed`, `is_default`) or gone
/// (`patch`, dropped in v36). This sentence has been wrong twice — it used to
/// promise that the seed "updates `patch`/`fpm_port`/`is_default` while
/// preserving the user's `installed` choices", which described two shipped bugs
/// as intended behaviour and is how a reader would put #340 back by implementing
/// what they read.
///
/// **The seed never sets `is_default`, not even on INSERT.** New rows arrive with
/// it false and the zero-default backstop below elects the pin, so a fresh
/// database still ends with exactly one default and an existing user's choice is
/// never joined by a second. Setting it on INSERT was the third defect in this
/// one statement (#344): a release that adds a NEW minor and moves the pin to it
/// takes the INSERT arm for that row, so the user's default survived via
/// `ON CONFLICT` and the new pin arrived beside it claiming to be default too.
///
/// Returns nothing: there is no stored patch to compare against any more.
///
/// **Which minors need a pool restart after a pin moves is a LIVE question now**,
/// answered after adoption by comparing each running master's executable against
/// the pin ([`patch_of_exe`], [`PhpFpmPools::running_patches`]) rather than by a
/// column this function used to write. That column was a mirror of the pin, and
/// the seed both detected the bump and committed it in the same statement — so a
/// failed prefetch consumed the signal and the retry never happened (#339), and
/// `is_default` beside it was overwritten from the pin on every launch (#340).
/// Deriving the fact makes both unrepresentable rather than fixed.
pub fn seed_registry(conn: &Connection) -> Result<()> {
    let default_minor = minor_of(binaries::PHP_VERSION);
    for minor in all_minors() {
        // Resolvable-ness is still asserted here even though the patch is no
        // longer stored: a minor in `all_minors()` with no pinned build is a
        // packaging mistake, and finding it at seed time beats finding it when
        // a user presses Start.
        patch_for_minor(&minor)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))?;
        let port =
            fpm_port(&minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        let is_pin = minor == default_minor;
        store::upsert_php_version(
            conn,
            &PhpVersion {
                minor: minor.clone(),
                fpm_port: port,
                // On first insert the pinned minor is enabled; others are available
                // but off until the user installs them (Phase 2 §1.5). Preserved on
                // update.
                installed: is_pin,
                // NEVER here — the backstop below elects it. See the doc comment:
                // a new minor that is also the new pin takes this arm while the
                // user's existing default survives via ON CONFLICT, so setting it
                // here produces TWO defaults (#344).
                is_default: false,
                // A new row follows the pin. Only the user's Update sets this,
                // through `store::set_php_selected_patch` — the seed must never
                // write a column the user owns (#345).
                selected_patch: None,
            },
        )?;
    }
    // Exactly-one-default is an invariant the whole registry leans on
    // (`scratch.rs` and `available_minors` both do `find(|v| v.is_default)`), and
    // the seed sets it NOWHERE else — so this is the only thing that elects one.
    // It fires on a fresh database (no row is default yet) and stands down on
    // every launch after, which is what keeps the user's choice untouched.
    if !store::list_php_versions(conn)?.iter().any(|v| v.is_default) {
        store::set_default_php_version(conn, &default_minor)?;
    }
    Ok(())
}

/// The patch a minor should RUN: the user's selection, floored by this build's
/// pin.
///
/// The one place that question is answered, so the pool, the download planner,
/// the UI and the cache sweep cannot answer it differently. `updates::floored`
/// makes the floor a version comparison rather than a flag: with no selection the
/// answer is today's pin byte for byte, and a selection older than the pin is
/// ignored rather than honoured — so a stale choice can never hold a user below
/// the patch their app ships.
pub fn effective_patch(conn: &Connection, minor: &str) -> Result<Option<String>> {
    let selected = store::list_php_versions(conn)?
        .into_iter()
        .find(|v| v.minor == minor)
        .and_then(|v| v.selected_patch);
    Ok(crate::core::updates::floored(minor, selected.as_deref()))
}

/// [`effective_patch`] or an error naming the minor — the shape every caller that
/// RUNS php actually wants.
///
/// Added because eleven call sites reached for `patch_for_minor` (the PIN) to get a
/// binary to execute: the site terminal, every WP-CLI invocation, the repo
/// composer steps, the agent scratch tools and the ini `-t` gate. After an in-app
/// update each of them ran the OLD interpreter while the site's pool served the
/// new one — so `php -v` in a site's own terminal disagreed with the site.
pub fn patch_to_run(conn: &Connection, minor: &str) -> Result<String> {
    effective_patch(conn, minor)?
        .ok_or_else(|| Error::Other(format!("unknown PHP version: {minor}")))
}

/// The registry's default minor, or `None` when nothing is marked default.
///
/// Its own accessor because three callers wanted only this and reached for
/// [`list_versions`] with an EMPTY catalog to get it. That builds a full row
/// whose `updatable` is `None` by construction — a value that is not "no update
/// available" but "nobody asked", and one `?` away from being rendered.
pub fn default_minor(conn: &Connection) -> Result<Option<String>> {
    Ok(store::list_php_versions(conn)?.into_iter().find(|v| v.is_default).map(|v| v.minor))
}

/// Every INSTALLED minor paired with the patch it will run — what the launch
/// cache repair sweeps over. See [`default_minor`] for why this is not a
/// filtered [`list_versions`].
pub fn installed_effective(conn: &Connection) -> Result<Vec<(String, String)>> {
    Ok(store::list_php_versions(conn)?
        .into_iter()
        .filter(|v| v.installed)
        .filter_map(|v| {
            crate::core::updates::floored(&v.minor, v.selected_patch.as_deref())
                .map(|p| (v.minor, p))
        })
        .collect())
}

/// Every minor's effective patch, for the snapshot the ServiceManager is handed
/// before a start (the same shape as `set_php_settings` / `set_db_versions`).
pub fn effective_patches(conn: &Connection) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    for v in store::list_php_versions(conn)? {
        if let Some(p) = crate::core::updates::floored(&v.minor, v.selected_patch.as_deref()) {
            out.insert(v.minor, p);
        }
    }
    Ok(out)
}

/// The PHP patch a `php-fpm-<patch>/php-fpm` executable path names, if it does.
///
/// Pure so the parse is testable without a process: the cache layout
/// (`bin/php-fpm-8.3.31/php-fpm`) is the only thing that says which patch a
/// running master is actually serving.
pub fn patch_of_exe(exe: &std::path::Path) -> Option<String> {
    let dir = exe.parent()?.file_name()?.to_str()?;
    let patch = dir.strip_prefix("php-fpm-").or_else(|| dir.strip_prefix("php-"))?;
    let parts: Vec<&str> = patch.split('.').collect();
    (parts.len() == 3
        && parts.iter().all(|p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
    .then(|| patch.to_string())
}

/// All registered PHP versions (installed + available), for the UI.
///
/// The stored row plus what is DERIVED from the pinned build set — Xdebug
/// availability and, where offered, the release that minor's debug pool loads.
/// Derived here rather than stored, so a pin change moves the UI on the next
/// read and there is no column to migrate; and returned as a distinct view type
/// so no caller can hold a [`PhpVersion`] whose derived fields were never
/// filled (see [`PhpVersionView`]).
/// `catalog` is the VERIFIED update catalog — passed in, not read here.
///
/// It used to be a hidden `updates::cached(conn)` call, which made the field it
/// feeds untestable: a test could not put a catalog in scope without a signed
/// document, so `updatable` was always `None` and the first test written for it
/// asserted nothing at all. A parameter is the difference between a test that
/// passes and a test that proves something.
pub fn list_versions(
    conn: &Connection,
    running: &[String],
    catalog: &crate::core::updates::VersionCatalog,
    arch: &str,
) -> Result<Vec<PhpVersionView>> {
    let upstream = crate::core::php_upstream::cached(conn);
    let checked_at = (!upstream.checked_at.is_empty()).then(|| upstream.checked_at.clone());
    Ok(store::list_php_versions(conn)?
        .into_iter()
        .map(|v| {
            // THE BASELINE, and getting it wrong produced three wrong fields at
            // once. Every question this row answers is relative to what the minor
            // WILL RUN — the user's selection floored by the pin — NOT to the pin.
            // Comparing against the pin meant that after a successful update the
            // row showed the old patch, painted the correct new pool as a
            // discrepancy, and kept offering an update already applied.
            let effective = crate::core::updates::floored(&v.minor, v.selected_patch.as_deref())
                .unwrap_or_default();
            PhpVersionView {
                xdebug_supported: binaries::xdebug_supported(&v.minor),
                xdebug_unavailable_reason: binaries::xdebug_unavailable_reason(&v.minor),
                xdebug_version: binaries::xdebug_version_for(&v.minor),
                eol_since: eol_since(&v.minor),
                // What the live pool is EXECUTING, said only when it differs from
                // what this minor SHOULD be running — i.e. a restart is still
                // pending. A pool already on the chosen patch is not a
                // disagreement, and calling it one is how a correct state got
                // painted amber.
                serving: running
                    .iter()
                    .find(|p| minor_of(p) == v.minor && **p != effective)
                    .cloned(),
                // php.net's newest, only when it is newer than what we will run.
                upstream: upstream
                    .latest
                    .get(&v.minor)
                    .filter(|u| crate::core::php_upstream::is_newer(u, &effective))
                    .cloned(),
                upstream_checked_at: checked_at.clone(),
                // Offered only when the catalog has something newer than what we
                // will run. Against the pin, this kept offering a patch the user
                // had already installed.
                // `arch`, because a manifest carrying only the OTHER Mac's
                // binaries would otherwise render a button that downloads
                // ~100 MB and then fails at the last resolve. No minor: the
                // catalog derives it from `effective`, which IS a patch of it.
                updatable: catalog.newer_than(
                    crate::core::updates::Family::Php,
                    &effective,
                    arch,
                ),
                patch: effective,
                minor: v.minor,
                fpm_port: v.fpm_port,
                installed: v.installed,
                is_default: v.is_default,
            }
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
    /// The mail catch-all baked into every pool's config so a site's mail — PHP
    /// `mail()` via the shim, a Laravel app via `env[MAIL_*]` — lands in Mailpit
    /// (§2.2). Set by `ServiceManager` once Mailpit's binary is resolved; `None`
    /// ⇒ pools mail exactly as each site is configured to, which is what the
    /// user asked for when they turned the catch-all off.
    catch: Option<super::mail::Catch>,
    /// Per-minor ini settings (whitelisted, pre-validated — see [`SETTINGS`])
    /// written as `php_value[key]` lines into that pool's config. Set by
    /// `ServiceManager` from the SQLite `php_settings` table.
    settings: std::collections::HashMap<String, Vec<(String, String)>>,
    /// Per-minor EFFECTIVE patch — the user's Update choice, already floored by
    /// the pin (`php::effective_patches`). Set by `ServiceManager` from the
    /// registry, exactly like `settings` above, so this module stays off the
    /// database: a pool manager that could read SQLite would be a second place
    /// answering "which patch runs".
    ///
    /// An ABSENT minor falls back to the compiled-in pin, which is what makes an
    /// unset snapshot behave as it did before selections existed.
    patches: std::collections::HashMap<String, String>,
}

impl PhpFpmPools {
    /// The patch `minor` should run: the snapshot's answer, else the pin.
    ///
    /// ONE resolution point for both pools, so the normal and debug pool of a
    /// minor can never execute different bytes — they share a binary by design,
    /// and two copies of this lookup is how they would stop.
    fn effective(&self, minor: &str) -> Result<String> {
        if let Some(p) = self.patches.get(minor) {
            return Ok(p.clone());
        }
        patch_for_minor(minor)
            .map(str::to_string)
            .ok_or_else(|| Error::Other(format!("no pinned PHP build for {minor}")))
    }

    /// Mirror the registry's effective patches (`php::effective_patches`).
    pub fn set_patches(&mut self, patches: std::collections::HashMap<String, String>) {
        self.patches = patches;
    }
    /// Set the mail catch-all used when (re)writing pool configs. Applies to
    /// pools started afterward (a running pool keeps its config until restarted).
    ///
    /// ONE setter for both halves on purpose: see [`super::mail::Catch`] — a
    /// pool holding the shim but not the environment catches WordPress and
    /// delivers Laravel, which is the bug this whole path exists to end.
    pub fn set_mail_catch(&mut self, catch: Option<super::mail::Catch>) {
        self.catch = catch;
    }

    /// The catch as it stands — for the OVERRIDE backends (#514), which are not
    /// pools but must carry the same two halves: `ServiceManager` renders it
    /// into a FrankenPHP config (`php_ini sendmail_path`) and that backend's
    /// process environment. One value, read here, so the pools and the override
    /// sites cannot disagree about whether mail is caught.
    pub fn mail_catch(&self) -> Option<&super::mail::Catch> {
        self.catch.as_ref()
    }

    /// Set the per-minor ini settings used when (re)writing pool configs. Like
    /// the mail catch-all, applies to pools started afterward — the caller
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
        let patch = self.effective(minor)?;
        let port =
            fpm_port(minor).ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
        ports::ensure_free(platform, port, ports::Proto::Tcp, "PHP-FPM")?;
        let bin = binaries::resolve(platform, "php-fpm", &patch).await?;
        let settings = self.settings.get(minor).map(Vec::as_slice).unwrap_or(&[]);
        let conf = services::write_fpm_config(
            platform,
            minor,
            port,
            self.catch.as_ref(),
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
        let patch = self.effective(minor)?;
        let port = debug_fpm_port(minor).ok_or_else(|| {
            Error::Other(format!("Xdebug is not available for PHP {minor}"))
        })?;
        ports::ensure_free(platform, port, ports::Proto::Tcp, "PHP-FPM (Xdebug)")?;
        let bin = binaries::resolve(platform, "php-fpm", &patch).await?;
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
            self.catch.as_ref(),
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

    /// The patches the LIVE pools are actually executing, or `None` when any of
    /// them cannot be identified.
    ///
    /// The live fact behind the GC keep-set (ledger #338/#341). Read from each
    /// master's EXECUTABLE path, never from its argv: php-fpm rewrites its
    /// process title, so `ps -o comm=` shows the conf path — the minor, never
    /// the patch. An adopted survivor from a previous app version is precisely
    /// the case that matters here and precisely the case argv cannot answer.
    ///
    /// `None` on ANY unidentifiable pool rather than a partial list: the caller
    /// deletes what is not in this set, so a short answer is a delete-the-wrong-
    /// tree answer. Not-knowing must cost a skipped sweep, never a lost tree.
    pub fn running_patches(&self, platform: &dyn Platform) -> Option<Vec<String>> {
        let mut out = Vec::new();
        for p in &self.pools {
            let patch = platform
                .supervisor()
                .pid_exe(p.child.id())
                .as_deref()
                .and_then(patch_of_exe)?;
            if !out.contains(&patch) {
                out.push(patch);
            }
        }
        Some(out)
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

    /// **Every shipped minor gets a port, and no two pool ports collide — across
    /// normal pools, debug pools, and every other fixed port in the app.**
    ///
    /// The whole-surface version, because the collision that motivated it was
    /// arithmetic nobody had computed: `base + major * 10 + minor` gives each
    /// major TEN slots, so `fpm_port("8.10")` and `fpm_port("9.0")` both land on
    /// 9790. Unreachable only because `PHP_VERSIONS` is a curated list — the
    /// calendar reaches it on its own.
    ///
    /// This is the gate that makes the refusal cheap. The day a maintainer adds
    /// an x.10 minor, `port_offset` returns `None`, this fails at `cargo test`,
    /// and the message below says what to change — instead of the collision
    /// reaching a user as two pools fighting over one port.
    #[test]
    fn every_shipped_minor_has_a_unique_pool_port() {
        let mut seen: std::collections::BTreeMap<u16, String> = std::collections::BTreeMap::new();
        let mut claim = |port: u16, who: String| {
            if let Some(prev) = seen.insert(port, who.clone()) {
                panic!(
                    "port {port} is claimed by BOTH `{prev}` and `{who}`.\n\n\
                     The pool port schemes are `base + major * 10 + minor`, which gives each \
                     major exactly TEN slots — so an x.10 minor aliases the next major's .0 \
                     (8.10 and 9.0 both want 9790).\n\n\
                     Widening the formula is NOT the cheap fix: `adopt_startup`, the managed-port \
                     set and the orphaned-worker sweep all enumerate by CALLING `fpm_port` / \
                     `debug_fpm_port`, so moving existing ports strands every running master \
                     where rexenv can no longer see it while `ensure_free` binds the new one. \
                     Whatever you change must keep every port that exists today exactly where \
                     it is — a second base for the wide range, or an explicit table."
                );
            }
        };
        for minor in all_minors() {
            let port = fpm_port(&minor).unwrap_or_else(|| {
                panic!(
                    "PHP {minor} ships but has no pool port. `port_offset` refuses minors this \
                     scheme cannot express (minor >= 10) — see the collision note above; the \
                     scheme needs extending before this minor can be offered."
                )
            });
            claim(port, format!("php-fpm {minor}"));
            if let Some(debug) = debug_fpm_port(&minor) {
                claim(debug, format!("php-fpm {minor} (debug)"));
            }
        }
        // …and against every other fixed port rexenv binds. A pool landing on
        // MySQL's port would be a mutual-refusal at start with no obvious cause.
        for (port, who) in [
            (services::PHP_FPM_PORT, "the Phase-1 single pool"),
            (services::NGINX_HTTP_PORT, "nginx"),
            (crate::core::proxy::DEFAULT_HTTPS_PORT, "the edge"),
            (crate::core::mail::MAILPIT_SMTP_PORT, "Mailpit SMTP"),
            (crate::core::mail::MAILPIT_HTTP_PORT, "Mailpit HTTP"),
            (crate::core::dns::DEFAULT_DNS_PORT, "DNS"),
        ] {
            // PHP_FPM_PORT is 8.3's pool BY DESIGN — it is the same port, not a
            // collision, so it is the one allowed overlap.
            if port == services::PHP_FPM_PORT {
                assert_eq!(fpm_port("8.3"), Some(port), "the Phase-1 port must stay 8.3's");
                continue;
            }
            assert!(!seen.contains_key(&port), "a PHP pool port collides with {who} ({port})");
        }
        // The collision itself, asserted directly so the rule cannot quietly
        // loosen: an x.10 minor must not resolve at all.
        assert_eq!(fpm_port("8.10"), None, "8.10 must be refused, not aliased onto 9.0");
        assert_eq!(fpm_port("9.0"), Some(9790));
        assert_eq!(debug_fpm_port("8.10"), None);
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
        // (docs/archive/PLAN-php-74-support.md), so this list is a floor, not a ceiling.
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
        let rows = list_versions(&conn, &[], &Default::default(), "arm64").unwrap();
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
        let rows = list_versions(&conn, &[], &Default::default(), "arm64").unwrap();
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
            // And so does the REASON, added 23 Aug 2026 when it turned out the
            // card still hardcoded one sentence for every kind of absence. It is
            // present exactly when support is absent — a row carrying both, or
            // neither, renders either a reason for a working toggle or a
            // disabled toggle that will not say why.
            assert_eq!(
                r.xdebug_unavailable_reason,
                binaries::xdebug_unavailable_reason(&r.minor),
                "{} disagrees with core about WHY",
                r.minor
            );
            assert_eq!(
                r.xdebug_supported,
                r.xdebug_unavailable_reason.is_none(),
                "{}: support and a reason-for-no-support cannot both be true",
                r.minor
            );
        }
        // …and the set is not uniformly true, so an accessor stubbed to a
        // constant could not pass this. 8.0 ships and has no Xdebug.
        assert!(rows.iter().any(|r| !r.xdebug_supported), "no unsupported row — the fixture is too tidy");
        assert!(rows.iter().any(|r| r.xdebug_supported));
    }

    /// The seed refreshes what it OWNS and touches nothing the user owns.
    ///
    /// Replaces the two tests that guarded `php_versions.patch` (ledger #339):
    /// the column is gone as of migration v36, so a patch bump is no longer a
    /// stored comparison that can be consumed — it is asked of the running
    /// masters after adoption (`running_patches`, #342). What still matters
    /// here is that re-seeding preserves the two USER facts beside it.
    #[test]
    fn the_seed_preserves_every_user_choice_and_returns_nothing_to_consume() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();

        let pinned = minor_of(binaries::PHP_VERSION);
        let chosen = all_minors().into_iter().find(|m| *m != pinned).expect("a second minor");
        set_installed(&conn, &chosen, true).unwrap();
        set_default(&conn, &chosen).unwrap();

        // Re-seed twice: idempotent, and neither user fact moves.
        seed_registry(&conn).unwrap();
        seed_registry(&conn).unwrap();

        let rows = store::list_php_versions(&conn).unwrap();
        let row = rows.iter().find(|v| v.minor == chosen).unwrap();
        assert!(row.installed, "the seed cleared the user's install choice");
        assert!(row.is_default, "the seed cleared the user's default choice");
        assert_eq!(rows.iter().filter(|v| v.is_default).count(), 1);
        // The derived port IS refreshed — that one the app owns.
        assert_eq!(row.fpm_port, fpm_port(&chosen).unwrap());
    }

    /// **The effective patch is the user's choice, and the pin is a FLOOR.**
    ///
    /// One resolution point for the pool, the planner, the UI and the sweep. The
    /// floor matters most for the case nobody would test by hand: a selection
    /// made months ago, then an app update that ships a NEWER pin. The pin wins,
    /// so a stale choice can never hold someone below what their app ships.
    /// #342 — **the running patch is read from the executable PATH, never from
    /// a process title.**
    ///
    /// A php-fpm master's title says `php-fpm: master process (…)` and its argv
    /// carries a MINOR at best — so an argv-derived answer can never name the
    /// patch actually serving requests, which is the one thing "is my update
    /// live?" needs. The cache layout (`bin/php-fpm-8.3.31/php-fpm`) is what
    /// knows, and this parse is where that is turned into an answer.
    ///
    /// **This test is a restoration, not a new claim.** The ledger row cited a
    /// test of this name since 16 Aug 2026; the name existed only in the row
    /// (found 2 Sep 2026 by the guard that now checks every citation), so the
    /// claim had been reading as proven for two weeks with nothing behind it.
    #[test]
    fn the_patch_is_parsed_from_the_executable_path_not_the_title() {
        use std::path::Path;
        // The real cache layout, both spellings the tree has used.
        assert_eq!(patch_of_exe(Path::new("/x/bin/php-fpm-8.3.31/php-fpm")).as_deref(), Some("8.3.31"));
        assert_eq!(patch_of_exe(Path::new("/x/bin/php-8.4.2/php-fpm")).as_deref(), Some("8.4.2"));

        // A MINOR is not a patch. This is exactly what an argv- or title-derived
        // answer would produce, and accepting it would let the UI say "8.3" is
        // running when the question is which 8.3.x.
        assert_eq!(patch_of_exe(Path::new("/x/bin/php-fpm-8.3/php-fpm")), None);
        // Not our directory shape at all: a system php-fpm, or a title mistaken
        // for a path.
        for foreign in [
            "/opt/homebrew/bin/php-fpm",
            "/x/bin/php-fpm/php-fpm",
            "php-fpm: master process (/x/etc/php-fpm.conf)",
            "/x/bin/php-fpm-8.3.31.2/php-fpm",
            "/x/bin/php-fpm-8.3.x/php-fpm",
            "/x/bin/php-fpm-/php-fpm",
        ] {
            assert_eq!(patch_of_exe(Path::new(foreign)), None, "`{foreign}` parsed as a patch");
        }
        // The FILE name is not consulted — only its directory. A binary renamed
        // in place still reports the cache directory's patch, which is the one
        // that was staged.
        assert_eq!(patch_of_exe(Path::new("/x/bin/php-fpm-8.3.31/anything")).as_deref(), Some("8.3.31"));
    }

    #[test]
    fn the_effective_patch_honours_a_selection_but_never_below_the_pin() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let minor = minor_of(binaries::PHP_VERSION);
        let pin = patch_for_minor(&minor).unwrap();

        // No selection → the pin, byte for byte. This is the whole install base.
        assert_eq!(effective_patch(&conn, &minor).unwrap().as_deref(), Some(pin));
        assert_eq!(effective_patches(&conn).unwrap().get(&minor).map(String::as_str), Some(pin));

        // A NEWER selection is honoured.
        let up = format!("{minor}.9999");
        store::set_php_selected_patch(&conn, &minor, Some(&up)).unwrap();
        assert_eq!(effective_patch(&conn, &minor).unwrap().as_deref(), Some(up.as_str()));

        // An OLDER selection is ignored — the pin is a floor, not a default.
        store::set_php_selected_patch(&conn, &minor, Some(&format!("{minor}.0"))).unwrap();
        assert_eq!(
            effective_patch(&conn, &minor).unwrap().as_deref(),
            Some(pin),
            "a stale selection held the minor below the patch this app ships"
        );

        // Cleared → back to the pin.
        store::set_php_selected_patch(&conn, &minor, None).unwrap();
        assert_eq!(effective_patch(&conn, &minor).unwrap().as_deref(), Some(pin));
        // And the seed never touches it (#345's per-column rule).
        seed_registry(&conn).unwrap();
        store::set_php_selected_patch(&conn, &minor, Some(&up)).unwrap();
        seed_registry(&conn).unwrap();
        assert_eq!(
            effective_patch(&conn, &minor).unwrap().as_deref(),
            Some(up.as_str()),
            "the seed cleared the user's selected patch"
        );
    }

    /// **The seed cannot set a user's selected patch even if it tries.**
    ///
    /// Written because a plant FAILED and taught me the property held for a
    /// different reason than I assumed. I planted `selected_patch:
    /// patch_for_minor(..)` into the seed's row expecting the floor test to
    /// catch it; nothing failed, because `upsert_php_version`'s SQL never NAMES
    /// that column — the struct field is silently dropped. That is the behaviour
    /// we want and it was entirely implicit, so the next reader "fixing" the
    /// apparent omission by adding it to the INSERT would reintroduce exactly the
    /// #339/#340 family with every test still green. This asserts the real
    /// mechanism instead of the one I guessed at.
    #[test]
    fn the_seed_cannot_write_a_selected_patch_even_when_handed_one() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let minor = minor_of(binaries::PHP_VERSION);

        // Hand `upsert_php_version` a row that DOES carry a selection, on a fresh
        // row (the INSERT arm) and on an existing one (the conflict arm).
        for m in [minor.clone(), "9.9".to_string()] {
            let _ = store::upsert_php_version(
                &conn,
                &PhpVersion {
                    minor: m.clone(),
                    fpm_port: 9999,
                    installed: true,
                    is_default: false,
                    selected_patch: Some("6.6.6".into()),
                },
            );
            let got = store::list_php_versions(&conn)
                .unwrap()
                .into_iter()
                .find(|v| v.minor == m)
                .and_then(|v| v.selected_patch);
            assert_eq!(
                got, None,
                "the seed wrote a selected patch for {m} — that column is the user's alone"
            );
        }
    }

    /// A pool with NO snapshot runs the pin — the behaviour every install had
    /// before selections existed, and the fallback the launch path relies on.
    #[test]
    fn a_pool_with_no_patch_snapshot_runs_the_compiled_in_pin() {
        let pools = PhpFpmPools::default();
        let minor = minor_of(binaries::PHP_VERSION);
        assert_eq!(pools.effective(&minor).unwrap(), patch_for_minor(&minor).unwrap());
        // A minor with no pin at all is an error, not a guess.
        assert!(pools.effective(unshipped_minor()).is_err());

        // With a snapshot, BOTH the normal and debug pool read the same answer —
        // they share a binary by design, and two lookups is how they would stop.
        let mut pools = PhpFpmPools::default();
        pools.set_patches(std::collections::HashMap::from([(minor.clone(), "8.3.9999".to_string())]));
        assert_eq!(pools.effective(&minor).unwrap(), "8.3.9999");
    }

    /// **A release that adds a new minor AND moves the pin to it must not create
    /// a second default.**
    ///
    /// The third defect in one statement (#344), and the one the previous two
    /// fixes could not see: #339 and #340 both corrected the `ON CONFLICT DO
    /// UPDATE SET` list, while the INSERT arm went unread. A new minor has no row
    /// yet, so it takes that arm — and it arrived with `is_default = 1` while the
    /// user's existing default survived, untouched, via the conflict clause.
    ///
    /// **Not a rare shape: it fires for every user the first time the pin moves
    /// to a newly-added minor**, which is exactly what shipping 7.4 did. The
    /// user's own default need not have been chosen — a row that has been default
    /// since the first seed collides just the same.
    ///
    /// The fixture deletes the pinned minor's row and re-seeds, which is what a
    /// release adding that minor looks like from the database's side. The
    /// surviving regression test cannot see this: it seeds first, so only the
    /// conflict arm is ever exercised.
    #[test]
    fn a_newly_pinned_minor_does_not_arrive_claiming_to_be_a_second_default() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let pinned = minor_of(binaries::PHP_VERSION);
        let chosen = all_minors().into_iter().find(|m| *m != pinned).expect("a second minor");
        set_installed(&conn, &chosen, true).unwrap();
        set_default(&conn, &chosen).unwrap();

        // The pinned minor arrives with this release: no row for it yet.
        conn.execute("DELETE FROM php_versions WHERE minor = ?1", [&pinned]).unwrap();
        seed_registry(&conn).unwrap();

        let rows = store::list_php_versions(&conn).unwrap();
        let defaults: Vec<&str> =
            rows.iter().filter(|v| v.is_default).map(|v| v.minor.as_str()).collect();
        assert_eq!(
            defaults,
            vec![chosen.as_str()],
            "the new pin joined the user's default instead of leaving it alone"
        );
        // The new row still exists and is still seeded installed — the fix must
        // not have removed the row, only its claim to be default.
        assert!(rows.iter().any(|v| v.minor == pinned), "the new minor lost its row");
    }

    /// A registry that somehow carries NO default gets one back, rather than
    /// leaving `find(|v| v.is_default)` returning `None` to every caller.
    ///
    /// **Restored 17 Aug 2026 — it was deleted with the v36 migration and not
    /// replaced, and ledger #340 went on citing it.** That mattered more after
    /// the fix above than before it: the backstop is now the ONLY thing that ever
    /// elects a default, so it went from a belt-and-braces repair to the
    /// mechanism, with zero coverage in between.
    #[test]
    fn a_registry_with_no_default_regains_the_pinned_one() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        conn.execute("UPDATE php_versions SET is_default = 0", []).unwrap();

        seed_registry(&conn).unwrap();

        let rows = store::list_php_versions(&conn).unwrap();
        let defaults: Vec<&str> =
            rows.iter().filter(|v| v.is_default).map(|v| v.minor.as_str()).collect();
        assert_eq!(defaults, vec![minor_of(binaries::PHP_VERSION).as_str()]);
    }

    /// A FRESH database ends with exactly one default, which is the pin — the
    /// case the fix above moved from the INSERT arm to the backstop, so it is
    /// asserted directly rather than assumed.
    #[test]
    fn a_fresh_registry_ends_with_exactly_one_default_and_it_is_the_pin() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let rows = store::list_php_versions(&conn).unwrap();
        let defaults: Vec<&str> =
            rows.iter().filter(|v| v.is_default).map(|v| v.minor.as_str()).collect();
        assert_eq!(defaults, vec![minor_of(binaries::PHP_VERSION).as_str()]);
        // …and the pinned minor is the one seeded installed.
        let pin = rows.iter().find(|v| v.minor == minor_of(binaries::PHP_VERSION)).unwrap();
        assert!(pin.installed, "a fresh install must have its default minor enabled");
    }

    /// **After a successful update the row shows the new patch, no discrepancy,
    /// and NO update button.**
    ///
    /// The test class that was missing, and its absence is why three user-visible
    /// bugs shipped in a row. Every existing test asked "does this function
    /// return the right thing for these inputs". None asked **"after the user
    /// does X, what does the screen say"** — so `list_versions` comparing three
    /// fields against the PIN instead of the effective patch passed everything
    /// while producing a row that showed 8.2.31, painted the correct 8.2.32 pool
    /// as a problem, and offered an update already applied.
    ///
    /// Written from the user's report, in their order: apply, then look.
    #[test]
    fn after_an_update_the_row_shows_the_new_patch_and_offers_nothing() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let minor = "8.2".to_string();
        let pin = patch_for_minor(&minor).unwrap().to_string();
        let target = format!("{minor}.9999");
        // A catalog offering the newer patch — PASSED IN, so the field it feeds is
        // actually under test. The first version of this read the catalog from the
        // database, where a test cannot put one without a signed document, so
        // `updatable` was `None` for both halves and every assertion about it was
        // vacuous. That is the same trap as three earlier guards in this session.
        // BOTH binaries, because an apply resolves both and `newer_than` now
        // refuses a half-published version. A one-entry fixture offered a button
        // no real apply could have completed.
        let sha = "a".repeat(64);
        let url = "https://dl.static-php.dev/x";
        let catalog = crate::core::updates::catalog_for_tests(&[
            ("php", &target, "arm64", url, &sha),
            ("php-fpm", &target, "arm64", url, &sha),
        ]);
        let row = |c: &Connection, running: &[String]| {
            list_versions(c, running, &catalog, "arm64")
                .unwrap()
                .into_iter()
                .find(|r| r.minor == minor)
                .unwrap()
        };

        // BEFORE: on the pin, pool on the pin — and the button IS offered, which
        // is what makes the "after" assertions mean something.
        let before = row(&conn, std::slice::from_ref(&pin));
        assert_eq!(before.patch, pin);
        assert_eq!(before.serving, None, "a pool on the pin is not a discrepancy");
        assert_eq!(
            before.updatable.as_deref(),
            Some(target.as_str()),
            "the fixture offers nothing — every assertion below would be vacuous"
        );

        // THE UPDATE: the selection is recorded and the pool moves onto it.
        store::set_php_selected_patch(&conn, &minor, Some(&target)).unwrap();

        // AFTER — the three things the user read off the screen and reported.
        let after = row(&conn, std::slice::from_ref(&target));
        assert_eq!(after.patch, target, "the row still showed the old patch");
        assert_eq!(
            after.serving, None,
            "the row painted the pool it was ASKED to run as a discrepancy"
        );
        assert_eq!(
            after.updatable, None,
            "the row kept offering an update the user had already applied"
        );

        // AND the pending-restart case must STILL be reported: selection moved,
        // pool has not. That is the one time `serving` should appear.
        let pending = row(&conn, std::slice::from_ref(&pin));
        assert_eq!(pending.patch, target);
        assert_eq!(
            pending.serving.as_deref(),
            Some(pin.as_str()),
            "a pool still on the old patch after a selection MUST be visible"
        );

        // A stale selection below the pin never moves the row (the floor), and the
        // button comes back, because the catalog is still newer than the pin.
        store::set_php_selected_patch(&conn, &minor, Some(&format!("{minor}.0"))).unwrap();
        let stale = row(&conn, std::slice::from_ref(&pin));
        assert_eq!(stale.patch, pin);
        assert_eq!(stale.updatable.as_deref(), Some(target.as_str()));
    }

    /// **The row says what is PINNED and, separately, what is SERVING.**
    ///
    /// This is what makes deleting `php_versions.patch` a simplification rather
    /// than a cover-up. A view that could only show the pin would render 8.3.32
    /// while the pool served 8.3.31 — the identical silent lie ledger #339 was
    /// shipped to end, just moved somewhere harder to see. `serving` is `None`
    /// unless there is a real disagreement, so the UI gains a line only when
    /// there is something to say.
    #[test]
    fn the_view_says_both_the_pinned_patch_and_the_one_actually_running() {
        let conn = db::open_in_memory().unwrap();
        seed_registry(&conn).unwrap();
        let minor = minor_of(binaries::PHP_VERSION);
        let pinned = patch_for_minor(&minor).unwrap().to_string();
        let row = |rows: Vec<PhpVersionView>| rows.into_iter().find(|r| r.minor == minor).unwrap();

        // Nothing running: pinned only, and no invented disagreement.
        let r = row(list_versions(&conn, &[], &Default::default(), "arm64").unwrap());
        assert_eq!(r.patch, pinned);
        assert_eq!(r.serving, None);

        // A pool running the pinned patch is not a disagreement either.
        let r = row(
            list_versions(&conn, std::slice::from_ref(&pinned), &Default::default(), "arm64")
                .unwrap(),
        );
        assert_eq!(r.serving, None, "running the pin is not worth a second line");

        // A pool running something else IS, and the row carries both.
        let stale = format!("{minor}.0");
        assert_ne!(stale, pinned);
        let r = row(
            list_versions(&conn, std::slice::from_ref(&stale), &Default::default(), "arm64")
                .unwrap(),
        );
        assert_eq!(r.patch, pinned, "the pin is still the pin");
        assert_eq!(r.serving, Some(stale), "…and the row must say what is serving");

        // Another minor's pool never leaks into this row.
        let other = all_minors().into_iter().find(|m| *m != minor).unwrap();
        let r = row(list_versions(&conn, &[format!("{other}.0")], &Default::default(), "arm64").unwrap());
        assert_eq!(r.serving, None);
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
                starter_db: false,
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

    /// The watchdog/Start-all race (docs/archive/SHIPPED-2026-07.md): a pool spawned by an
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

    /// Every use of the PIN, by name, checked against what it is FOR.
    ///
    /// The whole in-app-update feature failed on this once: `selected_patch` was
    /// honoured in two places while eleven others asked `patch_for_minor` for a
    /// binary to EXECUTE. After an update the pool served the new patch while the
    /// site's own terminal, WP-CLI, composer, the agent tools and the `-t` gate all
    /// still ran the old one — every layer individually correct, the system wrong,
    /// and `php -v` in a site's terminal disagreeing with the site it belongs to.
    /// A rename or a review cannot catch that class; only a rule that fires on the
    /// call itself can, because the tempting call is the one that compiles.
    ///
    /// So the pin has exactly three legitimate uses, and this test is the list:
    ///   1. **Existence** — "is this a minor rexenv ships?" (`.is_some()`,
    ///      `.is_none()`, `Some(_)`). Never yields a patch to run, so an update
    ///      cannot make it wrong.
    ///   2. **The floor** — `updates::floored`, and the ONE planner seam below it.
    ///   3. This module, which defines both.
    ///
    /// Anything that RUNS php uses [`patch_to_run`]. If you came here to add a
    /// fourth: you almost certainly want `patch_to_run`.
    #[test]
    fn the_pin_is_never_used_as_a_patch_to_run() {
        // file → why its calls are not "what patch should I run" questions.
        const ALLOWED: &[(&str, &str)] = &[
            ("core/php.rs", "defines the pin and the effective-patch accessors"),
            ("core/updates.rs", "`floored` IS the floor — the pin's one derivation"),
            (
                "core/downloads.rs",
                "`planned_patch`: the single planner seam that falls back to the pin",
            ),
        ];

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        walk(&root, &mut files);
        files.sort();
        assert!(
            files.len() > 40,
            "the scan found {} files — it is not walking the tree",
            files.len()
        );

        let mut offences: Vec<String> = Vec::new();
        let mut checked = 0usize;
        let mut saw_existence = false;
        for path in &files {
            let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let body = std::fs::read_to_string(path).unwrap();
            // Test code asserting against the pin is legitimate, so test modules
            // come out — by BRACE DEPTH, never by cutting to end-of-file. In
            // `mcp_server/scratch.rs` the test module sits in the MIDDLE, and the
            // naive split silently shrank this scan past 900 production lines on
            // the first run of this very test.
            let prod = crate::core::copy_scan::production_lines(&body);
            for (n, line) in &prod {
                if !line.contains("patch_for_minor(") {
                    continue;
                }
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with('*') {
                    continue; // prose naming it, not calling it
                }
                checked += 1;
                if ALLOWED.iter().any(|(f, _)| *f == rel) {
                    continue;
                }
                // An existence check puts its predicate on the call's own line or,
                // for a `match`, in the arm just below it.
                let here = prod.iter().position(|(m, _)| m == n).unwrap();
                let window: String = prod[here..(here + 3).min(prod.len())]
                    .iter()
                    .map(|(_, l)| *l)
                    .collect::<Vec<_>>()
                    .join(" ");
                if window.contains(".is_some()")
                    || window.contains(".is_none()")
                    || window.contains("Some(_)")
                {
                    saw_existence = true;
                } else {
                    offences.push(format!("{rel}:{n} — {}", t.trim_end()));
                }
            }
        }

        // Both directions, as `copy_scan`'s contract requires. The comment canary
        // is assembled at runtime: written as one literal it would survive
        // stripping as code and prove only its own presence.
        assert!(checked >= 6, "only {checked} pin uses found — the scan's matcher is broken");
        assert!(saw_existence, "no existence check classified — the classifier is dead code");
        let comment_only = format!("{} {} {}", "eleven", "call", "sites reached for");
        assert!(
            std::fs::read_to_string(root.join("core/php.rs")).unwrap().contains(&comment_only),
            "canary phrase moved — this test can no longer prove comments are stripped"
        );
        let scanned_php = crate::core::copy_scan::production_source(
            &std::fs::read_to_string(root.join("core/php.rs")).unwrap(),
        );
        assert!(
            scanned_php.contains("pub fn patch_to_run"),
            "the stripper ate production code in core/php.rs"
        );

        assert!(
            offences.is_empty(),
            "these use the compiled-in PIN where the question is \"what patch should run\":\n  {}\n\
             After an in-app PHP update the pin is NOT what the pool serves — use \
             `php::patch_to_run(&conn, minor)`. If a call only asks whether rexenv ships the \
             minor, say so with `.is_some()`/`.is_none()`; if it is a genuine fourth use, add \
             the file to ALLOWED with the reason.",
            offences.join("\n  ")
        );
    }
}
