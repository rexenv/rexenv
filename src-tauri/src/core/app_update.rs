//! core::app_update — the signed descriptor that says a newer rexenv exists.
//!
//! # What moves, and what must not
//!
//! `core::updates` moved PHP's digest anchor from a compiled-in `const` to a
//! signed document whose public key is a compiled-in `const`. This module does
//! the same for the app itself, and the grant is strictly larger: a key-holder
//! who can name a PHP tarball can run native code as the user, and one who can
//! name an app bundle gets that PLUS the binary that re-execs as the DNS agent
//! and the tunnel guard, PLUS the process that enforces the agent-access dial
//! and `settings_access`. Everything below exists because of that sentence.
//!
//! So the rules are the same rules, deliberately:
//!
//! - **One key.** [`crate::core::updates::release_pubkey`], verified through the
//!   ONE ed25519 seam ([`crate::core::updates::verify_signed_bytes`]). A second
//!   key would live on the same laptop, in the same reviewer-gated Environment,
//!   approved by the same person — a ceremony, not a custodian. The honest cost
//!   is that one compromise takes PHP, Adminer and the app together (ledger
//!   #536), and it is written down rather than implied.
//! - **Re-verified on every read.** `rexenv.db` is `-rw-r--r--`, so a stored
//!   "verified" flag would put the verdict where the attacker is. [`cached`]
//!   re-runs the signature check and degrades to `None`, never to an error: this
//!   feeds a card and an optional button and must not be able to fail a screen.
//! - **A monotonic serial of its own.** Older than the high-water mark → refused
//!   before any write; equal → accepted and nothing written; newer → written.
//!   Without it, a host that keeps serving an older validly-signed descriptor
//!   holds a machine on a known-bad build forever, and the signature does not
//!   stop that.
//! - **Location is never trust.** The artifact URL must sit under one of the
//!   compiled-in [`ALLOWED_RELEASE_PREFIXES`], checked on the URL as written —
//!   GitHub's final hop is a signed CDN URL that expires in about an hour, so a
//!   check after the redirect would be checking the wrong thing.
//!
//! # Why the app is not a `Family` in the PHP manifest
//!
//! `updates::Family` locks that document to artifacts that flow through
//! `binaries::resolve*`, and `only_the_declared_families_are_nameable` pins the
//! names to exactly `php`, `php-fpm` and `adminer`. An unknown name there
//! resolves as `Shape::Single` — chmod, `prepare_binary`, spawn as a service —
//! which is the wrong shape for a bundle and the wrong grant for the app. A
//! separate document keeps that guard intact and lets this one carry fields the
//! PHP rows have no slot for (size, minimum macOS, notes).
//!
//! # What the signature does NOT attest
//!
//! That these are the bytes the maintainer signed. Never that the build is
//! safe. Rotation is an app release, which is the property that makes a stolen
//! key survivable — and the reason the public half is a `const` and not a
//! setting.
//!
//! The design record is `docs/PLAN-self-update.md`; the swap and relaunch this
//! descriptor eventually drives live behind the `AppBundle` platform trait (T3).

use crate::core::updates;
use crate::error::{Error, Result};
use crate::state::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Where the descriptor lives: two files on `rexenv/runtimes`' default branch,
/// the same repo and the same shape as the PHP manifest.
///
/// A commit, not a release asset. A moved release tag already broke this project
/// in production once (ledger #368: GitHub burns a tag name when an immutable
/// release on it is deleted, and delete-then-create has no manifest at all in
/// between). A committed file is atomic, and git keeps every descriptor ever
/// published, so "what was signed, and when" stays answerable.
///
/// It also never flips. When `rexenv/rexenv` goes public the cask's `url` and
/// the tap's `SOURCE_REPO` move; this URL does not, because runtimes is public
/// today and stays — and the artifact's location is signed DATA, not a constant.
pub const APP_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/rexenv/runtimes/main/app-manifest.json";
pub const APP_MANIFEST_SIG_URL: &str =
    "https://raw.githubusercontent.com/rexenv/runtimes/main/app-manifest.json.sig";

/// Path prefixes an app artifact may be downloaded from.
///
/// PREFIXES, not hosts. `https://github.com/` alone would let any GitHub account
/// serve the bytes this app replaces itself with, and anyone can create an
/// account. Two entries because the dmg ships from the public tap while
/// `rexenv/rexenv` is private, and moves back when it is not — both are ours.
pub const ALLOWED_RELEASE_PREFIXES: &[&str] = &[
    "https://github.com/rexenv/homebrew-tap/releases/download/",
    "https://github.com/rexenv/rexenv/releases/download/",
];

/// The verified descriptor and its detached signature, stored together.
pub const DOC_KEY: &str = "app_update_release";
pub const SIG_KEY: &str = "app_update_release_sig";
/// Highest serial ever accepted — stored apart from the document precisely so it
/// survives the document being replaced.
pub const SERIAL_KEY: &str = "app_update_release_serial";
/// `{checkedAt, offered}` in ONE value, written only after a successful check.
/// Two keys would let a crash between the writes leave "checked just now" over
/// yesterday's answer — the rule `php_upstream` already learned.
pub const CHECK_KEY: &str = "app_update_check";
/// A version string the user chose to skip, compared LIVE against the offer.
pub const SKIP_KEY: &str = "app_update_skipped";
/// Absent or unparseable = on. Only the exact string `false` turns it off.
pub const AUTO_CHECK_KEY: &str = "app_update_auto_check";
/// `{from, to, at}`, written before the swap and consumed once by the NEXT
/// process, which reports the version it reads from ITSELF.
pub const NOTICE_KEY: &str = "app_update_notice";

/// Refuse a descriptor larger than this before parsing it. This is a poll of a
/// document with one release in it, not a download.
const MAX_DOC: usize = 64 * 1024;

/// The largest artifact this will ever be asked to fetch. The universal dmg is
/// ~29 MB and the bundle archive is the same order; a signed document naming a
/// 4 GB "update" is a mistake or an attack, and either way not something to
/// start streaming to a user's disk.
pub const MAX_ARTIFACT_BYTES: u64 = 200 * 1024 * 1024;

/// The release a descriptor describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRelease {
    /// Three numeric segments. A prerelease (`0.6.0-rc.1`) is not representable
    /// here by construction — see [`well_formed_version`].
    pub version: String,
    /// Must start with one of [`ALLOWED_RELEASE_PREFIXES`].
    pub url: String,
    /// Lowercase 64-hex SHA-256 of the bytes at `url`.
    pub sha256: String,
    /// Size in bytes, as signed. Compared against `Content-Length` before the
    /// download starts and against what actually arrived after it ends.
    pub size_bytes: u64,
    /// The oldest rexenv that may take this update, if the swap or staging logic
    /// changed. Empty = no floor.
    #[serde(default)]
    pub min_app_version: String,
    /// The macOS this build needs. An update that cannot launch is worse than no
    /// update — the app's floor has already moved once (11.0 → 15.0).
    #[serde(default)]
    pub minimum_system_version: String,
    /// Release notes, shown as TEXT. Never rendered as HTML or Markdown, never
    /// interpolated into a native menu title: it is attacker-controlled the
    /// moment the host is.
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub published_at: String,
}

/// The document, as signed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    /// Monotonic. See the module doc's serial rule.
    pub serial: u64,
    #[serde(default)]
    pub generated_at: String,
    pub release: AppRelease,
}

/// A release that passed every rule, against THIS build, on THIS Mac, right now.
///
/// Deliberately a separate type from [`AppRelease`]: a value of this type has
/// been compared live, and a value of that one has only been signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub notes: String,
    pub published_at: String,
}

/// Why there is nothing to offer. Every arm is a sentence the UI or the log can
/// print, because "no update" and "we could not tell" are different facts and
/// the card is not allowed to blur them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoOffer {
    /// The signed version is this build's, or older.
    NotNewer { latest: String, running: String },
    /// The user skipped exactly this version. Compared live, never stored as a
    /// flag — a stored "skipped" boolean hides every later release too.
    Skipped { version: String },
    /// The build needs a macOS this Mac does not have.
    NeedsNewerMacos { needs: String, host: String },
    /// The build needs a newer rexenv than this one to install it safely.
    NeedsNewerApp { needs: String },
    /// This Mac's macOS version could not be read, so the floor cannot be
    /// checked. Fails closed: no offer, rather than an update that may not boot.
    HostVersionUnknown,
    /// The descriptor is signed but structurally wrong. Says which rule, because
    /// this one is a publisher bug and somebody has to fix it.
    Malformed(&'static str),
}

impl NoOffer {
    pub fn reason(&self) -> String {
        match self {
            Self::NotNewer { latest, running } => {
                format!("the newest signed release is {latest} and this is {running}")
            }
            Self::Skipped { version } => format!("{version} was skipped"),
            Self::NeedsNewerMacos { needs, host } => {
                format!("{needs} needs a newer macOS than this Mac's {host}")
            }
            Self::NeedsNewerApp { needs } => {
                format!("it needs rexenv {needs} or newer to install")
            }
            Self::HostVersionUnknown => {
                "this Mac's macOS version could not be read, so the release's floor \
                 could not be checked"
                    .into()
            }
            Self::Malformed(why) => format!("the signed descriptor is malformed: {why}"),
        }
    }
}

/// Whether this build can trust any descriptor at all.
///
/// The dark state — an empty key — must keep working: it is what every build
/// before the key ceremony did, and what a build whose key was rotated out does.
/// With no key nothing is fetched, nothing is offered, and no button renders.
pub fn enabled() -> bool {
    !updates::release_pubkey().is_empty()
}

/// Three all-digit segments, and nothing else.
///
/// This is where prereleases are refused. Not with a `-rc` test — by refusing
/// anything that is not three numbers, so a channel cannot be smuggled in with a
/// spelling nobody predicted. semver would rank `0.6.0-rc.1` above `0.5.0` and
/// push a release candidate to everyone; here it simply is not a version.
pub fn well_formed_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The structural limits on a signed release. Unlike the PHP manifest — which
/// DROPS a bad row so one malformed entry cannot deny every other update — a bad
/// row here is the whole document, so it is reported rather than dropped.
fn structural_check(r: &AppRelease) -> std::result::Result<(), NoOffer> {
    if !well_formed_version(&r.version) {
        return Err(NoOffer::Malformed(
            "the version is not three numeric segments (a prerelease is never offered)",
        ));
    }
    if !ALLOWED_RELEASE_PREFIXES.iter().any(|p| r.url.starts_with(p)) {
        return Err(NoOffer::Malformed(
            "the artifact URL is not under a rexenv releases/download prefix",
        ));
    }
    if r.sha256.len() != 64
        || !r.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(NoOffer::Malformed("the sha256 is not lowercase 64-hex"));
    }
    if r.size_bytes == 0 || r.size_bytes > MAX_ARTIFACT_BYTES {
        return Err(NoOffer::Malformed("the size is zero or implausibly large"));
    }
    Ok(())
}

/// Is this signed release something to offer the person running THIS build on
/// THIS Mac, right now?
///
/// Every input is passed in rather than read here, so the whole rule is one pure
/// function a test can drive: the running version, the host macOS, and the
/// skipped version as it is at this moment. **The skip is a live comparison** —
/// stored as a version string and compared to the offer — because a stored
/// "skipped" boolean is a one-time answer to a question that keeps changing, and
/// would hide every release after the one that was skipped.
pub fn offer_for(
    release: &AppRelease,
    running: &str,
    host_macos: Option<(u32, u32, u32)>,
    skipped: Option<&str>,
) -> std::result::Result<Offer, NoOffer> {
    structural_check(release)?;

    if updates::version_segments(&release.version) <= updates::version_segments(running) {
        return Err(NoOffer::NotNewer {
            latest: release.version.clone(),
            running: running.to_string(),
        });
    }
    if updates::newer_app_required(&release.min_app_version) {
        return Err(NoOffer::NeedsNewerApp { needs: release.min_app_version.clone() });
    }
    if !release.minimum_system_version.is_empty() {
        let Some(host) = host_macos else {
            return Err(NoOffer::HostVersionUnknown);
        };
        let Some(need) = crate::core::macho::parse_version(&release.minimum_system_version) else {
            return Err(NoOffer::Malformed("minimumSystemVersion is not a version"));
        };
        if crate::core::macho::newer_than(need, host) {
            return Err(NoOffer::NeedsNewerMacos {
                needs: release.minimum_system_version.clone(),
                host: format!("{}.{}.{}", host.0, host.1, host.2),
            });
        }
    }
    if skipped == Some(release.version.as_str()) {
        return Err(NoOffer::Skipped { version: release.version.clone() });
    }

    Ok(Offer {
        version: release.version.clone(),
        url: release.url.clone(),
        sha256: release.sha256.clone(),
        size_bytes: release.size_bytes,
        notes: release.notes.clone(),
        published_at: release.published_at.clone(),
    })
}

/// Verify a descriptor's signature against the compiled-in key and parse it.
pub fn verify(doc: &[u8], sig_hex: &str) -> Result<AppManifest> {
    verify_with(updates::release_pubkey(), doc, sig_hex)
}

/// [`verify`] against an explicit key.
///
/// The seam exists for the same reason `updates::verify_with` does: tests have no
/// private half of the real key and must never have one, so without it the
/// ed25519 path, the tamper rejection and the serial rule would have zero
/// coverage and only "a wrong key is refused" would be proven.
fn verify_with(pubkey_hex: &str, doc: &[u8], sig_hex: &str) -> Result<AppManifest> {
    if doc.len() > MAX_DOC {
        return Err(Error::Other(format!(
            "the app update descriptor is {} bytes, which is not a descriptor",
            doc.len()
        )));
    }
    updates::verify_signed_bytes(pubkey_hex, doc, sig_hex)?;
    serde_json::from_slice(doc)
        .map_err(|e| Error::Other(format!("the app update descriptor did not parse: {e}")))
}

/// Accept a fetched descriptor: verify it, enforce the serial rule, persist the
/// document AND its signature.
///
/// The stale-serial refusal happens BEFORE any write, so a replayed older
/// descriptor cannot displace a newer one. Equal is not stale — it is the
/// ordinary state on every launch after the first, and it writes nothing.
pub fn accept(conn: &Connection, doc: &[u8], sig_hex: &str) -> Result<AppManifest> {
    accept_with(updates::release_pubkey(), conn, doc, sig_hex)
}

fn accept_with(
    pubkey_hex: &str,
    conn: &Connection,
    doc: &[u8],
    sig_hex: &str,
) -> Result<AppManifest> {
    let m = verify_with(pubkey_hex, doc, sig_hex)?;
    let highest: u64 = store::get_setting(conn, SERIAL_KEY)
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if m.serial < highest {
        return Err(Error::Other(format!(
            "this app update descriptor's serial ({}) is OLDER than the highest already \
             accepted ({highest}) — refusing a replay, which is how a host would hold this \
             machine on a superseded build",
            m.serial
        )));
    }
    if m.serial == highest {
        return Ok(m);
    }
    let text = std::str::from_utf8(doc)
        .map_err(|_| Error::Other("the app update descriptor is not valid UTF-8".into()))?;
    store::set_setting(conn, DOC_KEY, text)?;
    store::set_setting(conn, SIG_KEY, sig_hex.trim())?;
    store::set_setting(conn, SERIAL_KEY, &m.serial.to_string())?;
    Ok(m)
}

/// The stored descriptor. **No network, and the signature is re-checked on every
/// call** — every failure reads as "nothing stored", never as an error.
pub fn cached(conn: &Connection) -> Option<AppManifest> {
    let doc = store::get_setting(conn, DOC_KEY).ok().flatten()?;
    let sig = store::get_setting(conn, SIG_KEY).ok().flatten()?;
    verify(doc.as_bytes(), &sig).ok()
}

/// The version the user chose to skip, if any.
pub fn skipped_version(conn: &Connection) -> Option<String> {
    store::get_setting(conn, SKIP_KEY).ok().flatten().filter(|s| !s.is_empty())
}

/// Skip exactly this version, or clear the skip.
///
/// Clearing DELETES the row rather than writing an empty string: absent and
/// empty are different states everywhere settings are read, and writing `""` to
/// mean "unset" is how they stop being.
pub fn set_skipped(conn: &Connection, version: Option<&str>) -> Result<()> {
    match version {
        Some(v) => store::set_setting(conn, SKIP_KEY, v),
        None => store::delete_setting(conn, SKIP_KEY),
    }
}

/// Whether the app may check for updates on its own.
///
/// Absent, empty or unparseable reads as ON — the opposite of
/// `start_services_on_launch`, and deliberately. There the safe direction is
/// "nothing starts"; here it is "the user hears about a security release",
/// because the failure mode of the other direction is a machine that silently
/// stops being told. Only the exact string `false` turns it off, which is what
/// the toggle writes.
pub fn auto_check_enabled(conn: &Connection) -> bool {
    match store::get_setting(conn, AUTO_CHECK_KEY).ok().flatten() {
        Some(v) => v.trim() != "false",
        None => true,
    }
}

/// Why the app may not replace itself right now.
///
/// A separate type from [`NoOffer`] because they answer different questions: one
/// is "is there anything newer", the other is "could we install it if there
/// were". A machine can be perfectly up to date AND unable to update, and a card
/// that collapses the two tells the second user nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotABundle,
    ReadOnlyVolume { path: String },
    Translocated { path: String },
    NotInApplications { parent: String },
    SymlinkedPath { path: String },
    ParentNotWritable { parent: String },
    ForeignOwner { bundle: String },
    NotEnoughSpace { need: u64, have: u64 },
}

impl Refusal {
    /// What the user is told. Every one of these names the CONSEQUENCE and, where
    /// a fix exists, ends with a `$ ` line — which `toastBackendError` renders as
    /// a copyable command block.
    ///
    /// **None of them is a prompt.** An unwritable folder does not become an
    /// admin dialog: root does not bypass App Management anyway, a privileged op
    /// outside `PrivilegeManager` is against the rule this project holds, and the
    /// plugin's version of exactly this branch is a root `rm -rf` with no backup.
    /// A refusal with a command in it is a fix the user can read before running.
    pub fn message(&self) -> String {
        match self {
            Self::NotABundle => "rexenv is not running from an app bundle, so there is \
                 nothing to replace. Open the installed rexenv and update from there."
                .into(),
            Self::ReadOnlyVolume { path } => format!(
                "rexenv is running from a read-only volume ({path}). Drag rexenv.app into \
                 Applications in Finder, open it from there, then update. Nothing was changed."
            ),
            Self::Translocated { path } => format!(
                "macOS is running rexenv from a temporary read-only copy ({path}), because \
                 rexenv.app was never moved out of the folder it was downloaded to. Drag \
                 rexenv.app into Applications in Finder, open it from there, then update."
            ),
            Self::NotInApplications { parent } => format!(
                "rexenv is running from {parent}, not an Applications folder. Move rexenv.app \
                 into Applications in Finder, open it from there, then update."
            ),
            Self::SymlinkedPath { path } => format!(
                "rexenv was opened through a link ({path}), so the copy that is running and \
                 the copy that would be replaced may not be the same one. Open the real copy \
                 and update from there."
            ),
            Self::ParentNotWritable { parent } => format!(
                "rexenv can't replace itself: {parent} is not writable by this account. Ask \
                 an admin to update rexenv, or take ownership of the folder first:\n\
                 $ sudo chown -R \"$USER\" {parent}"
            ),
            Self::ForeignOwner { bundle } => format!(
                "{bundle} belongs to another account, so rexenv could not clean up after \
                 replacing it. Have that account update rexenv, or take ownership first:\n\
                 $ sudo chown -R \"$USER\" {bundle}"
            ),
            Self::NotEnoughSpace { need, have } => format!(
                "Not enough free space to install the update: it needs about {} MB free and \
                 there is {} MB. Nothing was downloaded.",
                need / 1_000_000,
                have / 1_000_000
            ),
        }
    }
}

/// May this installation replace itself — decided from facts alone.
///
/// Pure, so every branch is testable without a Mac, an installed app or a real
/// `/Volumes`. The platform gathers the facts; this decides what they mean, and
/// keeps the two apart because a decision buried in a syscall wrapper is a
/// decision nobody can drive.
///
/// Called BEFORE any byte is downloaded. A refusal that arrives after a 30 MB
/// download is a refusal that wasted the user's morning.
///
/// The space rule wants room for the archive AND the extracted copy AND the
/// previous bundle, which is why it is three times the archive rather than one.
pub fn preflight(
    facts: &crate::platform::traits::BundleFacts,
    archive_bytes: u64,
) -> std::result::Result<(), Refusal> {
    use crate::platform::traits::InstallKind as K;
    match facts.kind {
        K::DevBuild => return Err(Refusal::NotABundle),
        K::DiskImage => {
            return Err(Refusal::ReadOnlyVolume { path: facts.bundle.display().to_string() })
        }
        K::Translocated => {
            return Err(Refusal::Translocated { path: facts.bundle.display().to_string() })
        }
        K::Elsewhere => {
            return Err(Refusal::NotInApplications { parent: facts.parent.display().to_string() })
        }
        K::Applications | K::UserApplications => {}
    }
    if facts.read_only {
        return Err(Refusal::ReadOnlyVolume { path: facts.parent.display().to_string() });
    }
    if !facts.canonical {
        return Err(Refusal::SymlinkedPath { path: facts.bundle.display().to_string() });
    }
    if !facts.parent_writable {
        return Err(Refusal::ParentNotWritable { parent: facts.parent.display().to_string() });
    }
    if !facts.owned_by_me {
        return Err(Refusal::ForeignOwner { bundle: facts.bundle.display().to_string() });
    }
    let need = archive_bytes.saturating_mul(3);
    if facts.free_parent_bytes < need {
        return Err(Refusal::NotEnoughSpace { need, have: facts.free_parent_bytes });
    }
    Ok(())
}

/// What a staged bundle must be, for THIS app at `version`.
///
/// One place, so the swap's idea of "a rexenv bundle" cannot drift from the
/// running app's. `rex` is in the required list because a bundle without it
/// silently breaks every terminal the user has open on the CLI.
pub fn staged_expect(version: &str) -> crate::platform::traits::StagedExpect {
    use crate::platform::traits::{Arch, StagedExpect};
    StagedExpect {
        version: version.to_string(),
        identifier: "dev.rexenv.rexenv".into(),
        executable: "rexenv".into(),
        archs: vec![Arch::X86_64, Arch::Arm64],
        required_binaries: vec!["rex".into()],
        codesign: true,
    }
}

/// What the check cache holds: when it was taken, and what it found.
///
/// **One key, not two.** A separate timestamp would let a crash between the two
/// writes leave "checked just now" sitting over yesterday's answer, which is
/// exactly the dishonesty the `checked N ago` line exists to prevent — the rule
/// `php_upstream` already paid for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckCache {
    /// `db_now()` at the moment of a SUCCESSFUL check. Never written on failure,
    /// so a failed check can never age into a success.
    pub checked_at: String,
    /// What that check decided. `None` is a real answer — "we looked, there is
    /// nothing for this Mac" — and is why the offer is stored rather than
    /// recomputed: the card must be able to say when it last looked even when
    /// the descriptor has since been rewritten.
    pub offered: Option<Offer>,
}

/// The last check, if one has ever succeeded.
pub fn cached_check(conn: &Connection) -> Option<CheckCache> {
    let raw = store::get_setting(conn, CHECK_KEY).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

/// Record a check that SUCCEEDED. Failures write nothing, deliberately.
pub fn store_check(conn: &Connection, offered: Option<Offer>) -> Result<CheckCache> {
    let check = CheckCache { checked_at: store::db_now(conn)?, offered };
    let blob = serde_json::to_string(&check)
        .map_err(|e| Error::Other(format!("could not cache the update check: {e}")))?;
    store::set_setting(conn, CHECK_KEY, &blob)?;
    Ok(check)
}

/// Fetch the descriptor and its detached signature. **Takes no `Connection`**,
/// so no caller can hold the database lock across this await.
///
/// Verification happens in [`accept`]; this is deliberately dumb about trust.
pub async fn fetch() -> Result<(Vec<u8>, String)> {
    if !enabled() {
        return Err(Error::Other(
            "this build has no update key pinned, so it does not check for app updates".into(),
        ));
    }
    updates::fetch_signed_pair(APP_MANIFEST_URL, APP_MANIFEST_SIG_URL, MAX_DOC).await
}

/// Everything the About card renders, in one answer.
///
/// `running` and `offered` are two different facts and stay two fields: one is
/// what this process IS, read from its own `CARGO_PKG_VERSION`, and the other is
/// what a verified document says exists. Nothing here is composed from what the
/// UI hopes shipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateState {
    /// This build. Read from the binary, never from the descriptor.
    pub running: String,
    /// Whether this build can trust a descriptor at all (a key is pinned).
    pub enabled: bool,
    /// Automatic checking, as the setting says right now.
    pub auto_check: bool,
    /// A live offer, if the stored descriptor still passes every rule against
    /// THIS build on THIS Mac. Recomputed on every call rather than read from
    /// the cache, so a skip, an OS upgrade or a newer running build changes the
    /// answer the moment it happens.
    pub offered: Option<Offer>,
    /// Why there is no offer, when there is a stored descriptor to judge. `None`
    /// when nothing has ever been accepted — which is a different sentence, and
    /// the card says so.
    pub no_offer_reason: Option<String>,
    /// When the last SUCCESSFUL check ran (`db_now` format), for `checked N ago`.
    pub checked_at: Option<String>,
    /// The version the user skipped, if any.
    pub skipped: Option<String>,
}

/// Assemble [`AppUpdateState`] from the database and this machine. Pure reads:
/// no network, no writes, and every failure degrades to "nothing to offer"
/// rather than an error, because this feeds a card that must not be able to fail.
pub fn state(conn: &Connection) -> AppUpdateState {
    let running = env!("CARGO_PKG_VERSION").to_string();
    let skipped = skipped_version(conn);
    let check = cached_check(conn);
    let mut offered = None;
    let mut no_offer_reason = None;
    if let Some(m) = cached(conn) {
        match offer_for(&m.release, &running, crate::core::macho::host_macos(), skipped.as_deref())
        {
            Ok(o) => offered = Some(o),
            Err(no) => no_offer_reason = Some(no.reason()),
        }
    }
    AppUpdateState {
        running,
        enabled: enabled(),
        auto_check: auto_check_enabled(conn),
        offered,
        no_offer_reason,
        checked_at: check.map(|c| c.checked_at),
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair() -> (String, ring::signature::Ed25519KeyPair) {
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let hex = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        (hex, kp)
    }

    fn sign(kp: &ring::signature::Ed25519KeyPair, doc: &[u8]) -> String {
        kp.sign(doc).as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn release(version: &str) -> AppRelease {
        AppRelease {
            version: version.into(),
            url: format!(
                "https://github.com/rexenv/homebrew-tap/releases/download/v{version}/\
                 rexenv_{version}_universal.app.tar.gz"
            ),
            sha256: "a".repeat(64),
            size_bytes: 31_000_000,
            min_app_version: String::new(),
            minimum_system_version: "15.0".into(),
            notes: String::new(),
            published_at: "2026-09-06T00:00:00Z".into(),
        }
    }

    fn doc(serial: u64, version: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "serial": serial,
            "generatedAt": "2026-09-06T00:00:00Z",
            "release": release(version),
        }))
        .unwrap()
    }

    fn db() -> Connection {
        crate::state::db::open_in_memory().unwrap()
    }

    const HOST: Option<(u32, u32, u32)> = Some((26, 6, 2));

    #[test]
    fn an_app_release_is_offered_only_when_strictly_newer_than_this_build() {
        let r = release("0.6.0");
        assert!(offer_for(&r, "0.5.0", HOST, None).is_ok());
        // Equal is not newer, and neither is older. Both are the ordinary state
        // of a machine that is already up to date, so neither may produce a
        // button that would reinstall what is already running.
        assert!(matches!(
            offer_for(&r, "0.6.0", HOST, None),
            Err(NoOffer::NotNewer { .. })
        ));
        assert!(matches!(
            offer_for(&r, "0.7.0", HOST, None),
            Err(NoOffer::NotNewer { .. })
        ));
        // Numeric per segment: a lexical compare puts 0.10.0 BEFORE 0.9.0, and
        // this project has already shipped a double-digit segment elsewhere.
        assert!(offer_for(&release("0.10.0"), "0.9.0", HOST, None).is_ok());
        assert!(matches!(
            offer_for(&release("0.9.0"), "0.10.0", HOST, None),
            Err(NoOffer::NotNewer { .. })
        ));
    }

    #[test]
    fn a_prerelease_is_never_offered_to_a_release_build() {
        // semver would rank 0.6.0-rc.1 ABOVE 0.5.0 and push a release candidate
        // to every user. Here it is refused for not being a version at all, so
        // no channel can arrive by a spelling nobody predicted.
        for v in ["0.6.0-rc.1", "0.6.0-beta", "v0.6.0", "0.6", "0.6.0.1", "0.6.x"] {
            let mut r = release("0.6.0");
            r.version = v.into();
            assert!(
                matches!(offer_for(&r, "0.5.0", HOST, None), Err(NoOffer::Malformed(_))),
                "{v} should not be offerable"
            );
        }
    }

    #[test]
    fn a_skipped_version_is_compared_live_against_the_offer_not_stored_as_a_flag() {
        let r = release("0.6.0");
        assert!(matches!(
            offer_for(&r, "0.5.0", HOST, Some("0.6.0")),
            Err(NoOffer::Skipped { .. })
        ));
        // The whole point: skipping 0.6.0 must not hide 0.6.1. A boolean would.
        assert!(offer_for(&release("0.6.1"), "0.5.0", HOST, Some("0.6.0")).is_ok());
    }

    #[test]
    fn a_release_needing_a_newer_macos_is_not_offered_and_an_unknown_host_fails_closed() {
        let mut r = release("0.6.0");
        r.minimum_system_version = "27.0".into();
        assert!(matches!(
            offer_for(&r, "0.5.0", HOST, None),
            Err(NoOffer::NeedsNewerMacos { .. })
        ));
        r.minimum_system_version = "26.6.2".into();
        assert!(offer_for(&r, "0.5.0", HOST, None).is_ok(), "equal to the host is fine");
        // An update that cannot launch is worse than no update, so a host version
        // we could not read refuses rather than guesses.
        r.minimum_system_version = "15.0".into();
        assert!(matches!(offer_for(&r, "0.5.0", None, None), Err(NoOffer::HostVersionUnknown)));
        // …but a descriptor that names no floor has nothing to check.
        r.minimum_system_version = String::new();
        assert!(offer_for(&r, "0.5.0", None, None).is_ok());
    }

    #[test]
    fn a_release_that_needs_a_newer_rexenv_to_install_is_not_offered() {
        let mut r = release("99.0.0");
        r.min_app_version = "99.0.0".into();
        assert!(matches!(
            offer_for(&r, "0.5.0", HOST, None),
            Err(NoOffer::NeedsNewerApp { .. })
        ));
    }

    #[test]
    fn the_artifact_url_must_sit_under_a_release_download_prefix_not_a_bare_host() {
        // `https://github.com/` alone would let ANY account serve the bytes this
        // app replaces itself with — the same argument the PHP manifest's host
        // allowlist is built on.
        for url in [
            "https://github.com/someone-else/rexenv/releases/download/v0.6.0/x.tar.gz",
            "https://github.com/rexenv/homebrew-tap/archive/v0.6.0.tar.gz",
            "http://github.com/rexenv/homebrew-tap/releases/download/v0.6.0/x.tar.gz",
            "https://evil.example/rexenv/releases/download/v0.6.0/x.tar.gz",
        ] {
            let mut r = release("0.6.0");
            r.url = url.into();
            assert!(
                matches!(offer_for(&r, "0.5.0", HOST, None), Err(NoOffer::Malformed(_))),
                "{url} should be refused"
            );
        }
        for prefix in ALLOWED_RELEASE_PREFIXES {
            assert!(prefix.starts_with("https://github.com/rexenv/"), "{prefix}");
            assert!(prefix.ends_with("/releases/download/"), "{prefix} is not a path prefix");
        }
    }

    #[test]
    fn a_malformed_digest_or_size_is_refused_rather_than_downloaded() {
        type Mangle = fn(&mut AppRelease);
        let cases: &[(&str, Mangle)] = &[
            ("short digest", |r| r.sha256 = "abc".into()),
            ("uppercase digest", |r| r.sha256 = "A".repeat(64)),
            ("non-hex digest", |r| r.sha256 = "z".repeat(64)),
            ("zero size", |r| r.size_bytes = 0),
            ("absurd size", |r| r.size_bytes = MAX_ARTIFACT_BYTES + 1),
        ];
        for (name, mangle) in cases {
            let mut r = release("0.6.0");
            mangle(&mut r);
            assert!(
                matches!(offer_for(&r, "0.5.0", HOST, None), Err(NoOffer::Malformed(_))),
                "{name} should be refused"
            );
        }
    }

    #[test]
    fn the_app_descriptor_serial_is_its_own_high_water_mark() {
        let (pk, kp) = keypair();
        let conn = db();
        let newer = doc(7, "0.7.0");
        accept_with(&pk, &conn, &newer, &sign(&kp, &newer)).unwrap();
        assert_eq!(
            store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(),
            Some("7")
        );

        // Older: refused BEFORE any write, so a replay cannot displace what is
        // stored — that is the whole reason the mark is a separate key.
        let older = doc(6, "0.6.0");
        let err = accept_with(&pk, &conn, &older, &sign(&kp, &older)).unwrap_err().to_string();
        assert!(err.contains("refusing a replay"), "{err}");
        let stored = store::get_setting(&conn, DOC_KEY).unwrap().unwrap();
        assert!(stored.contains("0.7.0"), "the replay overwrote the newer descriptor");
        assert!(!stored.contains("0.6.0"));

        // Equal is the ordinary every-launch state: accepted, writes nothing.
        let same = doc(7, "0.7.0");
        assert!(accept_with(&pk, &conn, &same, &sign(&kp, &same)).is_ok());
        assert_eq!(
            store::get_setting(&conn, SERIAL_KEY).unwrap().as_deref(),
            Some("7")
        );

        // The PHP manifest's serial is a DIFFERENT high-water mark. One document
        // moving must never move the other's floor.
        assert!(store::get_setting(&conn, "php_update_manifest_serial").unwrap().is_none());
    }

    #[test]
    fn a_tampered_app_descriptor_cache_is_refused_on_every_read_not_trusted_from_a_flag() {
        let (pk, kp) = keypair();
        let conn = db();
        let d = doc(1, "0.6.0");
        accept_with(&pk, &conn, &d, &sign(&kp, &d)).unwrap();
        // The stored pair verifies against the key it was signed with…
        assert!(verify_with(&pk, &d, &sign(&kp, &d)).is_ok());
        // …and `cached` uses the COMPILED-IN key, which did not sign this, so a
        // document planted in the database by anything but a real publish reads
        // as nothing at all rather than as an offer.
        assert!(cached(&conn).is_none());

        // Edit one byte of a validly signed document: refused.
        let mut tampered = d.clone();
        let i = tampered.iter().position(|b| *b == b'6').unwrap();
        tampered[i] = b'7';
        assert!(verify_with(&pk, &tampered, &sign(&kp, &d)).is_err());
    }

    #[test]
    fn both_manifest_modules_verify_through_one_seam() {
        // Two ed25519 checks would be two places for a `map_err` to swallow a
        // failure, and the second copy is the one nobody re-reads. Plant a
        // flipped byte and require BOTH documents to refuse it.
        let (pk, kp) = keypair();
        let app_doc = doc(1, "0.6.0");
        let php_doc = br#"{"serial":1,"generatedAt":"x","minAppVersion":"","artifacts":[]}"#;

        let app_sig = sign(&kp, &app_doc);
        let php_sig = sign(&kp, php_doc);
        assert!(verify_with(&pk, &app_doc, &app_sig).is_ok());
        assert!(updates::verify_signed_bytes(&pk, php_doc, &php_sig).is_ok());

        let mut flipped = app_doc.clone();
        flipped[10] ^= 0x01;
        assert!(verify_with(&pk, &flipped, &app_sig).is_err());

        let mut php_flipped = php_doc.to_vec();
        php_flipped[10] ^= 0x01;
        assert!(updates::verify_signed_bytes(&pk, &php_flipped, &php_sig).is_err());

        // A signature from a different key never verifies either document.
        let (_, other) = keypair();
        assert!(verify_with(&pk, &app_doc, &sign(&other, &app_doc)).is_err());
        assert!(updates::verify_signed_bytes(&pk, php_doc, &sign(&other, php_doc)).is_err());
    }

    #[test]
    fn a_descriptor_bigger_than_a_descriptor_is_refused_before_it_is_parsed() {
        let (pk, kp) = keypair();
        let huge = vec![b' '; MAX_DOC + 1];
        let err = verify_with(&pk, &huge, &sign(&kp, &huge)).unwrap_err().to_string();
        assert!(err.contains("not a descriptor"), "{err}");
    }

    #[test]
    fn the_skip_is_a_version_string_and_clearing_it_removes_the_row() {
        let conn = db();
        assert!(skipped_version(&conn).is_none());
        set_skipped(&conn, Some("0.6.0")).unwrap();
        assert_eq!(skipped_version(&conn).as_deref(), Some("0.6.0"));
        set_skipped(&conn, None).unwrap();
        // Absent, not empty: everything that reads settings treats them
        // differently, and `""` to mean unset is how that stops being true.
        assert!(store::get_setting(&conn, SKIP_KEY).unwrap().is_none());
    }

    #[test]
    fn the_auto_check_setting_is_on_unless_it_says_exactly_false() {
        let conn = db();
        assert!(auto_check_enabled(&conn), "absent means on");
        for junk in ["", "no", "0", "FALSE", "maybe"] {
            store::set_setting(&conn, AUTO_CHECK_KEY, junk).unwrap();
            assert!(auto_check_enabled(&conn), "{junk:?} should read as on");
        }
        store::set_setting(&conn, AUTO_CHECK_KEY, "false").unwrap();
        assert!(!auto_check_enabled(&conn));
    }

    /// Every key this module writes is ruled on, and the two that carry the
    /// trust are Denied to `rex config`.
    ///
    /// The rulings live in `settings_access` — one policy file, deny by default —
    /// but they are asserted HERE, beside the keys, because that file cannot know
    /// which of its rows belong to this document. A key added to this module next
    /// month and forgotten there is refused by the default rather than silently
    /// writable, and this test says so out loud instead of trusting it.
    #[test]
    fn every_key_this_module_owns_is_ruled_on_and_the_signed_chain_is_denied() {
        use crate::core::settings_access::{cli_access, CliAccess};
        for key in [DOC_KEY, SIG_KEY, SERIAL_KEY] {
            assert!(
                matches!(cli_access(key), CliAccess::Denied(_)),
                "{key} names or protects the bytes rexenv replaces itself with — a shell \
                 must not write it"
            );
        }
        for key in [CHECK_KEY, NOTICE_KEY] {
            assert!(matches!(cli_access(key), CliAccess::ReadOnly(_)), "{key}");
        }
        for key in [SKIP_KEY, AUTO_CHECK_KEY] {
            assert_eq!(cli_access(key), CliAccess::ReadWrite, "{key} is a preference");
        }
        // The app's serial is NOT the PHP manifest's serial. Two documents, two
        // high-water marks, and neither may move the other's floor.
        assert_ne!(SERIAL_KEY, "php_update_manifest_serial");
    }

    /// One value, and only after a success.
    ///
    /// Two keys — a timestamp and an answer — would let a crash between the
    /// writes leave "checked just now" sitting over yesterday's answer, which is
    /// the exact dishonesty the line exists to prevent. Writing on failure would
    /// do the same thing without needing a crash.
    fn facts(kind: crate::platform::traits::InstallKind) -> crate::platform::traits::BundleFacts {
        crate::platform::traits::BundleFacts {
            bundle: "/Applications/rexenv.app".into(),
            parent: "/Applications".into(),
            kind,
            homebrew: false,
            parent_writable: true,
            owned_by_me: true,
            read_only: false,
            canonical: true,
            free_parent_bytes: 10_000_000_000,
        }
    }

    /// Running from a mounted image or a translocated copy is refused BEFORE
    /// anything is downloaded, and the message is the same one in both cases
    /// because the user's move is the same: drag it to Applications.
    ///
    /// Translocation has no supported detection beyond the path, so the check is
    /// the path — and the T0 quarantine leg measured that this is exactly the
    /// state a downloaded-but-never-moved copy launches in.
    #[test]
    fn a_volumes_or_translocated_path_is_refused_with_the_move_to_applications_fix() {
        use crate::platform::traits::InstallKind as K;
        let mut f = facts(K::DiskImage);
        f.bundle = "/Volumes/rexenv/rexenv.app".into();
        let m = preflight(&f, 1).unwrap_err().message();
        assert!(m.contains("read-only volume") && m.contains("Applications"), "{m}");

        let mut f = facts(K::Translocated);
        f.bundle = "/private/var/folders/x/T/AppTranslocation/UUID/d/rexenv.app".into();
        let m = preflight(&f, 1).unwrap_err().message();
        assert!(m.contains("temporary read-only copy") && m.contains("Finder"), "{m}");

        // A read-only PARENT is the same refusal reached a different way — a
        // check on the kind alone would miss a read-only mount at /Applications.
        let mut f = facts(K::Applications);
        f.read_only = true;
        assert!(matches!(preflight(&f, 1), Err(Refusal::ReadOnlyVolume { .. })));
    }

    #[test]
    fn a_symlink_ancestor_is_refused_because_the_running_copy_may_not_be_the_replaced_one() {
        let mut f = facts(crate::platform::traits::InstallKind::Applications);
        f.canonical = false;
        let m = preflight(&f, 1).unwrap_err().message();
        assert!(m.contains("through a link"), "{m}");
    }

    /// The one that must NEVER become a prompt.
    ///
    /// The Tauri updater turns exactly this state into `osascript … rm -rf …
    /// with administrator privileges`, and because Rust folds EACCES and EPERM
    /// into one error kind it does so for policy refusals privileges cannot fix.
    /// Here it is a sentence with a command in it, which the user can read
    /// before running.
    #[test]
    fn an_unwritable_parent_is_refused_with_a_chown_fix_never_a_prompt() {
        let mut f = facts(crate::platform::traits::InstallKind::Applications);
        f.parent_writable = false;
        let m = preflight(&f, 1).unwrap_err().message();
        assert!(m.contains("not writable"), "{m}");
        assert!(m.contains("\n$ sudo chown"), "the fix must be a copyable command: {m}");
        for forbidden in ["administrator", "osascript", "privileges", "password"] {
            assert!(!m.to_lowercase().contains(forbidden), "{m} — this is not a prompt");
        }

        // A bundle another login owns: an admin could rename it, but nothing
        // could clean up afterwards, so it is refused with the same shape.
        let mut f = facts(crate::platform::traits::InstallKind::Applications);
        f.owned_by_me = false;
        assert!(preflight(&f, 1).unwrap_err().message().contains("another account"));
    }

    #[test]
    fn a_dev_build_or_a_bundle_outside_applications_is_refused_and_the_ordinary_case_is_not() {
        use crate::platform::traits::InstallKind as K;
        assert!(matches!(preflight(&facts(K::DevBuild), 1), Err(Refusal::NotABundle)));
        assert!(matches!(
            preflight(&facts(K::Elsewhere), 1),
            Err(Refusal::NotInApplications { .. })
        ));
        // Anti-vacuity: the states this refuses are refused because of what they
        // are, not because `preflight` refuses everything.
        assert!(preflight(&facts(K::Applications), 1).is_ok());
        assert!(preflight(&facts(K::UserApplications), 1).is_ok());
        // Homebrew is not a refusal — the bundle is the user's either way.
        let mut f = facts(K::Applications);
        f.homebrew = true;
        assert!(preflight(&f, 1).is_ok());
    }

    #[test]
    fn space_is_checked_for_the_archive_the_copy_and_the_previous_bundle() {
        let mut f = facts(crate::platform::traits::InstallKind::Applications);
        f.free_parent_bytes = 100_000_000;
        // Three times, not once: the archive, what it extracts to, and the
        // bundle being replaced all sit on that volume at the same moment.
        assert!(preflight(&f, 30_000_000).is_ok());
        let m = preflight(&f, 40_000_000).unwrap_err().message();
        assert!(m.contains("Not enough free space") && m.contains("Nothing was downloaded"), "{m}");
    }

    #[test]
    fn what_a_staged_bundle_must_be_is_stated_once_and_includes_the_rex_sidecar() {
        let e = staged_expect("0.6.0");
        assert_eq!(e.version, "0.6.0");
        assert_eq!(e.identifier, "dev.rexenv.rexenv");
        assert_eq!(e.executable, "rexenv");
        // A bundle without `rex` silently breaks every terminal the user has
        // open on the CLI, which is the kind of thing nobody notices in a test.
        assert!(e.required_binaries.iter().any(|b| b == "rex"));
        assert_eq!(e.archs.len(), 2, "the shipped bundle is universal");
        assert!(e.codesign);
    }

    #[test]
    fn the_check_cache_is_one_value_written_only_on_success() {
        let conn = db();
        assert!(cached_check(&conn).is_none(), "nothing checked yet is its own state");
        assert!(state(&conn).checked_at.is_none());

        let offer = Offer {
            version: "0.6.0".into(),
            url: ALLOWED_RELEASE_PREFIXES[0].to_string() + "v0.6.0/x.tar.gz",
            sha256: "a".repeat(64),
            size_bytes: 1,
            notes: String::new(),
            published_at: String::new(),
        };
        let first = store_check(&conn, Some(offer.clone())).unwrap();
        assert!(!first.checked_at.is_empty());
        assert_eq!(cached_check(&conn).unwrap().offered, Some(offer));

        // The timestamp and the answer live in ONE row, so no failure can
        // separate them.
        let raw = store::get_setting(&conn, CHECK_KEY).unwrap().unwrap();
        assert!(raw.contains("checkedAt") && raw.contains("offered"), "{raw}");
        assert_eq!(
            store::get_setting(&conn, "app_update_checked_at").unwrap(),
            None,
            "a second key is what this shape exists to avoid"
        );

        // A check that finds nothing is still a check that RAN: the timestamp
        // moves and the offer is cleared, which is why the offer is stored
        // rather than recomputed.
        let second = store_check(&conn, None).unwrap();
        assert!(cached_check(&conn).unwrap().offered.is_none());
        assert!(!second.checked_at.is_empty());

        // Nothing here writes on failure — there is no path that can: the only
        // writer takes the answer, and the caller reaches it only after the
        // fetch and the verification have both succeeded.
        let unreadable = "not json";
        store::set_setting(&conn, CHECK_KEY, unreadable).unwrap();
        assert!(cached_check(&conn).is_none(), "a corrupt cache reads as never-checked");
        assert!(state(&conn).checked_at.is_none());
    }

    #[test]
    fn the_dark_state_offers_nothing_and_does_not_pretend_to_be_broken() {
        // A build with no key pinned must resolve exactly like a build with no
        // update feature: `enabled()` false, and nothing verifies.
        assert!(verify_with("", b"{}", "00").is_err());
        // The real build does have a key; this asserts the switch reads it
        // rather than a separate flag somebody could set.
        assert_eq!(enabled(), !updates::release_pubkey().is_empty());
    }
}
