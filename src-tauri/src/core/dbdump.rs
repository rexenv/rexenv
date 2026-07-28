//! core::dbdump — the preflight gate and the dump itself (Stage 2 step 5,
//! `docs/PLAN-valet-herd-db-import.md` §5).
//!
//! **The preflight refuses in cost order, and the order is structural.** The
//! checks that need no connection run first ([`gate`]: is-this-server-us, then
//! the compatibility verdict); the live checks ([`preflight_live`]: sign-in,
//! database-exists, size) and the dump itself each require the [`Cleared`]
//! witness the gate returns — a value with a private field that nothing else
//! can mint. A caller physically cannot reach a connection attempt, let alone a
//! dump, for a pairing that was refusable from the start. [`dump`] additionally
//! requires the [`Preflight`] that only [`check_disk`] produces, so the
//! disk-space answer can't be skipped either.
//!
//! **A partial dump is unrepresentable as an artifact.** The dump writes to
//! `<domain>.sql.partial`, renamed to `<domain>.sql` only after the tool exits
//! zero; the manifest is written only after the rename; and Half B loads an
//! artifact exclusively through [`load_manifest`], which refuses a missing
//! manifest and a byte-size mismatch. A cancel, a crash, or a full disk at any
//! point leaves either a `.partial` (never restorable — wrong name) or an
//! artifact without a manifest (refused) — never a truncated file that looks
//! whole.
//!
//! **Cancelling a dump is a read that stopped.** `--single-transaction` opens a
//! consistent-snapshot READ transaction; killing the client drops the
//! connection, and the server rolls the snapshot back and releases its metadata
//! locks as part of ordinary session teardown. We never use `FLUSH TABLES WITH
//! READ LOCK`, `--master-data`, or `--lock-all-tables` — the flags that take
//! locks a dead session's cleanup wouldn't already release. Nothing about their
//! server changes because a dump stopped early.

use crate::core::dbcompat::{Verdict, Version};
use crate::core::dbimport::DbConnection;
use crate::core::dbsource::{Identity, Vendor};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use rusqlite::Connection;
use std::io::BufRead;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Is this server us?
// ---------------------------------------------------------------------------

/// One of OUR running engines, snapshotted from `ServiceManager` by the caller
/// (core never reaches into the manager itself).
#[derive(Debug, Clone)]
pub struct OurEngine {
    pub port: u16,
    /// The engine's effective version, e.g. "8.4.6".
    pub version: String,
}

/// Positive identification, never a string match: the listener is ours when we
/// have a RUNNING owned engine on that port AND the thing that answered is the
/// version we run. Host spelling doesn't matter — `localhost` and `127.0.0.1`
/// reach the same listener, and the question is about the listener.
pub fn server_is_ours(host: &str, port: u16, identity: &Identity, ours: &[OurEngine]) -> bool {
    // A non-loopback host can't be our engine (we bind 127.0.0.1 only).
    let loopback = (host, port)
        .to_socket_addrs()
        .map(|mut a| a.all(|s| s.ip().is_loopback()))
        .unwrap_or(false);
    if !loopback {
        return false;
    }
    let Some(engine) = ours.iter().find(|e| e.port == port) else {
        return false;
    };
    // The handshake must match the engine we run there — two independent facts.
    let Identity::Handshake { version, .. } = identity else {
        return false;
    };
    match (Version::parse(version), Version::parse(&engine.version)) {
        (Some(a), Some(b)) => (a.major, a.minor, a.patch) == (b.major, b.minor, b.patch),
        _ => false,
    }
}

/// When the server IS ours, whose database is the site pointing at?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfImport {
    /// This site's own rexenv database — it's already here.
    ThisSite,
    /// Another rexenv site's database; carries that site's domain.
    OtherSite(String),
    /// A database on our engine that no site claims — the user's own.
    Unclaimed,
}

/// Classify a database name on OUR engine against the sites table — the
/// recorded authority for who owns a name (like `db_created`, never derived).
pub fn classify_self_import(
    conn: &Connection,
    site_id: &str,
    db_name: &str,
) -> Result<SelfImport> {
    match crate::state::store::site_with_db_name(conn, db_name)? {
        Some(site) if site.id == site_id => Ok(SelfImport::ThisSite),
        Some(site) => Ok(SelfImport::OtherSite(site.domain)),
        None => Ok(SelfImport::Unclaimed),
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// Witness that the no-connection checks passed. Private field: the only mint
/// is [`gate`], so the live preflight and the dump cannot run without it.
#[derive(Debug)]
pub struct Cleared(());

/// Why the gate refused. Each message teaches; none is a dead end.
#[derive(Debug, Clone)]
pub enum GateRefusal {
    SelfImport { kind: SelfImport, db_name: String },
    /// The compatibility verdict said no (or said "override" and the user
    /// hasn't). Render with `verdict.explain()`.
    Incompatible { verdict: Verdict },
}

impl GateRefusal {
    pub fn message(&self) -> String {
        match self {
            GateRefusal::SelfImport { kind, db_name } => match kind {
                SelfImport::ThisSite => format!(
                    "This site already reads and writes `{db_name}` on rexenv's own \
                     database engine — that IS the rexenv copy. There's nothing to import."
                ),
                SelfImport::OtherSite(domain) => format!(
                    "`{db_name}` on rexenv's database engine belongs to {domain}. \
                     Importing it here would either overwrite that site's data or fork it \
                     into two diverging copies. If the two sites are meant to share one \
                     database, point this site's config at it and skip the import."
                ),
                SelfImport::Unclaimed => format!(
                    "`{db_name}` already lives on rexenv's own database engine (no rexenv \
                     site claims it — likely created by hand). There's nothing to copy: \
                     the site's config already points at rexenv's server."
                ),
            },
            GateRefusal::Incompatible { verdict } => verdict.explain(),
        }
    }
}

/// The no-connection gate, in cost order: is-this-us first (nothing to copy),
/// then the compatibility verdict. `self_import` is `Some` when
/// [`server_is_ours`] said yes, carrying the classification;
/// `override_accepted` is the user's explicit acceptance of a
/// [`Verdict::NeedsOverride`], never a default.
pub fn gate(
    self_import: Option<(SelfImport, String)>,
    verdict: &Verdict,
    override_accepted: bool,
) -> std::result::Result<Cleared, GateRefusal> {
    if let Some((kind, db_name)) = self_import {
        return Err(GateRefusal::SelfImport { kind, db_name });
    }
    match verdict {
        Verdict::Proceed { .. } => Ok(Cleared(())),
        Verdict::NeedsOverride { .. } if override_accepted => Ok(Cleared(())),
        v => Err(GateRefusal::Incompatible { verdict: v.clone() }),
    }
}

// ---------------------------------------------------------------------------
// Credentials on disk, briefly and privately
// ---------------------------------------------------------------------------

/// The `--defaults-extra-file` handed to the MySQL-family tools: born 0600 via
/// `write_private` (B6 — created with the mode, never write-then-chmod) and
/// deleted on Drop, so it survives no exit path, including panic.
///
/// `connect-timeout` is deliberately NOT written here: the dump tools reject it
/// in every defaults group (verified) — the interactive client gets it on argv.
pub struct DefaultsFile {
    path: PathBuf,
}

impl DefaultsFile {
    pub fn create(platform: &dyn Platform, dir: &Path, conn: &DbConnection) -> Result<DefaultsFile> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(".connect.cnf");
        let contents = format!(
            "[client]\nhost={}\nport={}\nprotocol=TCP\nuser={}\npassword=\"{}\"\n",
            conn.host,
            conn.port,
            conn.user,
            escape_option_value(&conn.password),
        );
        platform.permissions().write_private(&path, contents.as_bytes())?;
        Ok(DefaultsFile { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DefaultsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// MySQL option-file quoting: inside double quotes, backslash and the quote
/// itself are escaped. Without this, a password containing `"` or `\` would be
/// read short — the same invisible-cause class as the wp-config truncation.
fn escape_option_value(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Live preflight
// ---------------------------------------------------------------------------

/// What one authenticated look at the source found.
#[derive(Debug, Clone)]
pub enum LiveCheck {
    Unreachable(String),
    SigninRefused(String),
    /// The named database isn't there; `available` is what is (user schemas).
    DatabaseMissing { available: Vec<String> },
    Ready(SourceSize),
}

/// The size answer the disk check needs.
#[derive(Debug, Clone, Copy)]
pub struct SourceSize {
    pub table_count: u64,
    /// Row data only — what a text dump approximates.
    pub data_bytes: u64,
    /// Data + indexes — what "how big is this database" means to a user.
    pub total_bytes: u64,
}

/// Ask the source, with the paired INTERACTIVE client (the only tool that
/// accepts `--connect-timeout`): can we sign in, is the database there, how big.
/// Requires the gate's witness — cheap refusals happen before any connection.
pub fn preflight_live(
    _cleared: &Cleared,
    client: &Path,
    defaults: &DefaultsFile,
    db: &str,
) -> Result<LiveCheck> {
    let list = match client_query(client, defaults, "SHOW DATABASES") {
        Ok(out) => out,
        Err(stderr) => {
            let s = stderr.to_lowercase();
            if s.contains("access denied") {
                return Ok(LiveCheck::SigninRefused(stderr));
            }
            if s.contains("can't connect") || s.contains("connection refused") {
                return Ok(LiveCheck::Unreachable(stderr));
            }
            return Err(Error::Other(format!("asking the source for its databases: {stderr}")));
        }
    };
    const SYSTEM: [&str; 5] = ["mysql", "information_schema", "performance_schema", "sys", "Database"];
    let user_dbs: Vec<String> = list
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !SYSTEM.contains(&l.as_str()))
        .collect();
    if !user_dbs.iter().any(|d| d == db) {
        return Ok(LiveCheck::DatabaseMissing { available: user_dbs });
    }

    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(data_length),0), COALESCE(SUM(data_length+index_length),0) \
         FROM information_schema.tables WHERE table_schema = '{}'",
        sql_escape(db)
    );
    let out = client_query(client, defaults, &sql)
        .map_err(|e| Error::Other(format!("asking the source for `{db}`'s size: {e}")))?;
    let mut cols = out.lines().last().unwrap_or_default().split('\t');
    let mut next = || cols.next().and_then(|c| c.trim().parse::<u64>().ok()).unwrap_or(0);
    Ok(LiveCheck::Ready(SourceSize { table_count: next(), data_bytes: next(), total_bytes: next() }))
}

/// One `-e` query through the interactive client. Batch mode, no header, and
/// `--connect-timeout=10` on argv (B25: bound the connect, never the transfer —
/// and only this tool accepts the flag at all). Shared with
/// `core::confverify`'s sign-in check, which authenticates the same way.
pub(crate) fn client_query(
    client: &Path,
    defaults: &DefaultsFile,
    sql: &str,
) -> std::result::Result<String, String> {
    let out = std::process::Command::new(client)
        .arg(format!("--defaults-extra-file={}", defaults.path().display())) // MUST be first
        .args(["--connect-timeout=10", "-N", "-B", "-e", sql])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Escape a string for a single-quoted SQL literal.
fn sql_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}

// ---------------------------------------------------------------------------
// Disk
// ---------------------------------------------------------------------------

/// The disk answer, produced only by [`check_disk`] — [`dump`] requires it.
#[derive(Debug, Clone, Copy)]
pub struct Preflight {
    pub size: SourceSize,
    pub estimated_dump_bytes: u64,
    pub free_bytes: u64,
}

/// Not enough room, with the numbers stated the way the refusal will say them.
#[derive(Debug, Clone, Copy)]
pub struct DiskShortfall {
    pub estimated_dump_bytes: u64,
    pub free_bytes: u64,
}

impl DiskShortfall {
    pub fn message(&self) -> String {
        format!(
            "Not enough disk space for the copy: it needs about {} and this Mac has {} \
             free. The estimate is deliberately generous — a text dump is often larger \
             than the database it copies — so freeing space a little past it is enough.",
            human_bytes(self.estimated_dump_bytes),
            human_bytes(self.free_bytes)
        )
    }
}

/// A text dump is INSERT statements: roughly the row data re-spelled, bigger
/// when blobs hex-encode (2×) or numbers widen, smaller when indexes dominate
/// (indexes aren't data in a dump). 2× the data length plus a fixed floor errs
/// generous on every shape we know; the refusal says it's an estimate.
pub fn estimated_dump_bytes(size: &SourceSize) -> u64 {
    size.data_bytes.saturating_mul(2) + 16 * 1024 * 1024
}

/// Free space on the volume holding `dir`, or `None` when it can't be read
/// (then we proceed — refusing to dump because we couldn't MEASURE the disk
/// would block real imports over a statistics failure; the dump itself still
/// fails cleanly on a full disk, into a `.partial` no one can restore).
pub fn free_space(dir: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// The last preflight step: is there room for the artifact where it will live?
pub fn check_disk(size: SourceSize, dest_dir: &Path) -> std::result::Result<Preflight, DiskShortfall> {
    let estimated = estimated_dump_bytes(&size);
    let free = free_space(dest_dir).unwrap_or(u64::MAX);
    if free < estimated {
        return Err(DiskShortfall { estimated_dump_bytes: estimated, free_bytes: free });
    }
    Ok(Preflight { size, estimated_dump_bytes: estimated, free_bytes: free })
}

fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{b} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

// ---------------------------------------------------------------------------
// The artifact
// ---------------------------------------------------------------------------

/// Everything Half B needs to trust an artifact, and NOTHING it must not carry:
/// the type has no password, user, or credential field to forget to strip —
/// the restore re-reads the site's own config live, so the manifest never
/// needed one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub domain: String,
    pub database: String,
    pub source_host: String,
    pub source_port: u16,
    pub source_vendor: Vendor,
    /// The source server's own version string, verbatim from its handshake.
    pub source_version: String,
    /// The engine + version this dump was judged compatible against. Half B
    /// re-checks: restoring into a different engine than the one the verdict
    /// approved would silently skip the compatibility gate.
    pub target_engine: String,
    pub target_version: String,
    /// Exact artifact size at write time; [`load_manifest`] refuses a mismatch.
    pub artifact_bytes: u64,
    pub table_count: u64,
    /// e.g. "mysqldump 8.4.6" — which tool wrote it.
    pub dump_tool: String,
    /// Unix seconds. A number, not a formatted date — nothing parses it back.
    pub created_at_unix: u64,
    /// Every table the copy CREATEs, read from the artifact itself. Half B
    /// verifies completeness by MEMBERSHIP — each of these must exist in the
    /// restored database — not by count equality, because a pre-existing
    /// target legitimately holds tables the dump never mentioned.
    pub tables: Vec<String>,
    pub findings: Findings,
}

/// The hygiene scan: things Half B must know (the sandbox line) or should
/// surface (the rest). Facts about the artifact, never blockers.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Findings {
    /// Line 1 is the May-2024 MariaDB sandbox form (`/*!999999\-`) that MySQL
    /// hard-fails on. Half B skips that line when feeding the restore.
    pub skip_sandbox_line: bool,
    /// `DEFINER=` clauses: restored as-is (we restore as root); orphan definers
    /// become use-time warnings.
    pub definer_count: u64,
    /// `IDENTIFIED WITH mysql_native_password` appears — see the 8.4 caution.
    pub native_password: bool,
    /// `NO_AUTO_CREATE_USER` appears in an sql_mode (5.7-era routines).
    pub no_auto_create_user: bool,
}

/// What one streaming pass over the artifact learned.
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub findings: Findings,
    /// The tables the dump CREATEs, in dump order.
    pub tables: Vec<String>,
}

/// Scan the artifact once, streaming — it can be gigabytes.
pub fn scan_artifact(path: &Path) -> Result<ScanReport> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut f = Findings::default();
    let mut tables = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if i == 0 && line.starts_with("/*!999999\\-") {
            f.skip_sandbox_line = true;
        }
        // mysqldump/mariadb-dump always backtick: CREATE TABLE `name` (
        if let Some(rest) = line.strip_prefix("CREATE TABLE `") {
            if let Some(end) = rest.find('`') {
                tables.push(rest[..end].to_string());
            }
        }
        f.definer_count += line.matches("DEFINER=").count() as u64;
        if line.contains("IDENTIFIED WITH mysql_native_password")
            || line.contains("IDENTIFIED WITH 'mysql_native_password'")
        {
            f.native_password = true;
        }
        if line.contains("NO_AUTO_CREATE_USER") {
            f.no_auto_create_user = true;
        }
    }
    Ok(ScanReport { findings: f, tables })
}

/// Where a domain's artifact and manifest live. One per domain BY NAME, so the
/// orphan set is bounded and enumerable (the resolver-backup reasoning).
pub fn artifact_path(dest_dir: &Path, domain: &str) -> PathBuf {
    dest_dir.join(format!("{domain}.sql"))
}
pub fn manifest_path(dest_dir: &Path, domain: &str) -> PathBuf {
    dest_dir.join(format!("{domain}.sql.manifest.json"))
}

/// Load an artifact FOR RESTORING — the only door Half B may use. Refuses an
/// artifact with no manifest (an interrupted dump renamed nothing / wrote no
/// manifest) and a size that moved since the manifest was written.
pub fn load_manifest(dest_dir: &Path, domain: &str) -> Result<(PathBuf, Manifest)> {
    let artifact = artifact_path(dest_dir, domain);
    let manifest_file = manifest_path(dest_dir, domain);
    if !artifact.is_file() {
        return Err(Error::Other(format!("no database copy for {domain} exists yet")));
    }
    let text = std::fs::read_to_string(&manifest_file).map_err(|_| {
        Error::Other(format!(
            "the copy for {domain} has no manifest — it looks like the dump that wrote \
             it never finished. Re-run the import to take a fresh copy."
        ))
    })?;
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|e| Error::Other(format!("reading the manifest for {domain}: {e}")))?;
    let actual = std::fs::metadata(&artifact)?.len();
    if actual != manifest.artifact_bytes {
        return Err(Error::Other(format!(
            "the copy for {domain} is {actual} bytes but its manifest says \
             {} — the file changed after it was written. Re-run the import to take a \
             fresh copy.",
            manifest.artifact_bytes
        )));
    }
    Ok((artifact, manifest))
}

// ---------------------------------------------------------------------------
// The dump
// ---------------------------------------------------------------------------

/// What to dump and what to record. Everything here is display/manifest data —
/// the credentials travel only in the [`DefaultsFile`].
pub struct DumpRequest<'a> {
    pub tool: &'a Path,
    /// Which family the TOOL is — decides the flag set (mariadb-dump has no
    /// `--set-gtid-purged` or `--column-statistics`).
    pub tool_vendor: Vendor,
    pub db: &'a str,
    pub domain: &'a str,
    pub source_host: &'a str,
    pub source_port: u16,
    pub source_vendor: Vendor,
    pub source_version: &'a str,
    pub target_engine: &'a str,
    pub target_version: &'a str,
    pub dump_tool_label: &'a str,
    /// Where the artifact and manifest land (`<app-data>/db-imports`).
    pub dest_dir: &'a Path,
}

/// How a dump ended.
pub enum DumpOutcome {
    Done { artifact: PathBuf, manifest: Box<Manifest> },
    /// Cancelled by the user. Their server saw a read stop; our `.partial` is
    /// gone; no manifest was ever written.
    Cancelled,
}

/// The dump tool's flag set, exactly as [`dump`] passes it (between the
/// defaults file, which must stay FIRST, and the per-run result-file/db args).
///
/// Public so `examples/db_dump_flags_check.rs` can feed the REAL bundled
/// binaries the REAL flags: whether a tool accepts a flag is the TOOL's fact,
/// not ours — `--connect-timeout` broke every export before the real binary
/// was asked. Neither this list nor [`DefaultsFile`] carries it: mysqldump
/// hard-errors on it, mariadb-dump warns and ignores it (both proven by the
/// example, which is also where that vendor split was discovered).
pub fn dump_tool_flags(tool_vendor: Vendor, source_version: &str) -> Vec<String> {
    let mut args: Vec<String> =
        ["--single-transaction", "--skip-lock-tables"].iter().map(|s| s.to_string()).collect();
    if tool_vendor == Vendor::Mysql {
        args.push("--set-gtid-purged=OFF".into());
        // An 8.x tool dumping a 5.x server writes histogram syntax the pairing
        // can't read back.
        if Version::parse(source_version).is_some_and(|v| v.major == 5) {
            args.push("--column-statistics=0".into());
        }
    }
    args
}

/// Run the dump. Requires the gate's witness AND the disk answer — the
/// signature is the preflight order.
///
/// `progress` receives the artifact's byte count as it grows (the only real
/// signal a dump emits); `cancel` is checked continuously.
pub fn dump(
    _cleared: &Cleared,
    platform: &dyn Platform,
    preflight: &Preflight,
    req: &DumpRequest<'_>,
    defaults: &DefaultsFile,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<DumpOutcome> {
    let dest_dir = req.dest_dir;
    std::fs::create_dir_all(dest_dir)?;
    let partial = dest_dir.join(format!("{}.sql.partial", req.domain));
    // Born 0600; the tool truncates in place, preserving the inode's mode (the
    // example asserts this against the real mysqldump).
    platform.permissions().write_private(&partial, b"")?;

    let mut args: Vec<String> =
        vec![format!("--defaults-extra-file={}", defaults.path().display())]; // MUST be first
    args.extend(dump_tool_flags(req.tool_vendor, req.source_version));
    args.push(format!("--result-file={}", partial.display()));
    args.push(req.db.to_string());

    let mut child = std::process::Command::new(req.tool)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("starting the dump tool: {e}")))?;
    // Drain stderr on a thread so a chatty tool can't block on a full pipe.
    let mut stderr_pipe = child.stderr.take();
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            use std::io::Read;
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            // SIGTERM first, poll, then SIGKILL — the Proc::terminate shape.
            // Either way their server just sees the connection drop; the
            // snapshot transaction rolls back in session teardown.
            let pid = child.id().to_string();
            let _ = std::process::Command::new("kill").arg(&pid).status();
            for _ in 0..20 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_thread.join();
            let _ = std::fs::remove_file(&partial);
            return Ok(DumpOutcome::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Ok(md) = std::fs::metadata(&partial) {
                    progress(md.len());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&partial);
                return Err(Error::Other(format!("waiting for the dump tool: {e}")));
            }
        }
    };
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() {
        // A failed dump leaves nothing that looks like a backup.
        let _ = std::fs::remove_file(&partial);
        return Err(Error::Other(format!(
            "the dump failed (exit {:?}): {}",
            status.code(),
            stderr.trim()
        )));
    }

    let bytes = std::fs::metadata(&partial)?.len();
    progress(bytes);
    let scan = scan_artifact(&partial)?;

    // Order matters: rename first, manifest second. A crash between the two
    // leaves an artifact with no manifest, which load_manifest refuses; the
    // reverse order could leave a manifest describing a file that isn't there.
    let artifact = artifact_path(dest_dir, req.domain);
    std::fs::rename(&partial, &artifact)?;
    let manifest = Manifest {
        domain: req.domain.to_string(),
        database: req.db.to_string(),
        source_host: req.source_host.to_string(),
        source_port: req.source_port,
        source_vendor: req.source_vendor,
        source_version: req.source_version.to_string(),
        target_engine: req.target_engine.to_string(),
        target_version: req.target_version.to_string(),
        artifact_bytes: bytes,
        table_count: preflight.size.table_count,
        dump_tool: req.dump_tool_label.to_string(),
        created_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        tables: scan.tables,
        findings: scan.findings,
    };
    platform
        .permissions()
        .write_private(&manifest_path(dest_dir, req.domain), serde_json::to_string_pretty(&manifest).map_err(|e| Error::Other(format!("writing the manifest: {e}")))?.as_bytes())?;
    Ok(DumpOutcome::Done { artifact, manifest: Box::new(manifest) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dbcompat::{compat, Source, Target};
    use crate::core::dbsource::Vendor;

    fn handshake(v: &str) -> Identity {
        Identity::Handshake {
            vendor: if v.contains("MariaDB") { Vendor::Mariadb } else { Vendor::Mysql },
            version: v.into(),
        }
    }

    #[test]
    fn ours_is_two_facts_and_never_a_string_match() {
        let ours = vec![OurEngine { port: 13306, version: "8.4.6".into() }];
        // Spelling doesn't matter: both reach the same listener.
        assert!(server_is_ours("127.0.0.1", 13306, &handshake("8.4.6"), &ours));
        assert!(server_is_ours("localhost", 13306, &handshake("8.4.6"), &ours));
        // Our port, but NOT the thing we run there — someone else's server on a
        // port we happen to own on paper. Not ours.
        assert!(!server_is_ours("127.0.0.1", 13306, &handshake("8.0.27"), &ours));
        // Right version, wrong port.
        assert!(!server_is_ours("127.0.0.1", 3306, &handshake("8.4.6"), &ours));
        // No handshake = no second fact; a declaration is not evidence here.
        assert!(!server_is_ours("127.0.0.1", 13306, &Identity::Unknown { note: None }, &ours));
        // Not running (empty snapshot) = not ours, whatever the port says.
        assert!(!server_is_ours("127.0.0.1", 13306, &handshake("8.4.6"), &[]));
    }

    #[test]
    fn the_gate_refuses_in_cost_order_and_mints_the_only_witness() {
        let proceed = compat(
            &Source { vendor: Some(Vendor::Mysql), version: Version::parse("8.0.27") },
            &Target { vendor: Vendor::Mysql, version: Version::parse("8.4.6").unwrap() },
        );
        assert!(proceed.runs_now());

        // Self-import outranks everything — even a Proceed verdict.
        let r = gate(Some((SelfImport::ThisSite, "ea".into())), &proceed, false).unwrap_err();
        assert!(r.message().contains("There's nothing to import"));

        let r = gate(Some((SelfImport::OtherSite("shop.test".into()), "ea".into())), &proceed, false)
            .unwrap_err();
        let m = r.message();
        assert!(m.contains("belongs to shop.test"), "{m}");
        assert!(m.contains("overwrite") && m.contains("fork"), "{m}");

        let r = gate(Some((SelfImport::Unclaimed, "scratch".into())), &proceed, false).unwrap_err();
        assert!(r.message().contains("no rexenv site claims it"));

        // A blocked verdict refuses even with override_accepted — there is no
        // override on Blocked, and the gate must not invent one.
        let blocked = compat(
            &Source { vendor: Some(Vendor::Mariadb), version: Version::parse("11.4.12-MariaDB") },
            &Target { vendor: Vendor::Mysql, version: Version::parse("8.4.6").unwrap() },
        );
        assert!(gate(None, &blocked, true).is_err());

        // NeedsOverride opens only with explicit acceptance.
        let needs = compat(
            &Source { vendor: Some(Vendor::Mysql), version: Version::parse("9.1.0") },
            &Target { vendor: Vendor::Mysql, version: Version::parse("8.4.6").unwrap() },
        );
        assert!(gate(None, &needs, false).is_err());
        assert!(gate(None, &needs, true).is_ok());

        // The happy path mints the witness.
        assert!(gate(None, &proceed, false).is_ok());
    }

    #[test]
    fn option_file_values_survive_passwords_with_quotes_and_backslashes() {
        assert_eq!(escape_option_value(r#"p"ss\word"#), r#"p\"ss\\word"#);
        assert_eq!(escape_option_value("plain"), "plain");
        // And SQL literals survive quotes the same way.
        assert_eq!(sql_escape("o'brien\\x"), "o''brien\\\\x");
    }

    #[test]
    fn the_dump_estimate_errs_generous_and_says_so() {
        let size = SourceSize { table_count: 10, data_bytes: 100 * 1024 * 1024, total_bytes: 160 * 1024 * 1024 };
        let est = estimated_dump_bytes(&size);
        assert!(est > size.data_bytes, "must exceed the data it copies");
        assert!(est >= 2 * size.data_bytes, "blob hex-encoding can double the text");
        let short = DiskShortfall { estimated_dump_bytes: est, free_bytes: 5 * 1024 * 1024 };
        let m = short.message();
        assert!(m.contains("needs about"), "{m}");
        assert!(m.contains("free"), "{m}");
        assert!(m.contains("estimate"), "{m}");
    }

    #[test]
    fn the_manifest_type_cannot_carry_a_credential() {
        // Not "we remembered to strip it" — the fields don't exist. This test
        // pins the full key set so a future field is a conscious decision.
        let m = Manifest {
            domain: "ea.test".into(),
            database: "ea".into(),
            source_host: "127.0.0.1".into(),
            source_port: 3306,
            source_vendor: Vendor::Mysql,
            source_version: "8.0.27".into(),
            target_engine: "mysql".into(),
            target_version: "8.4.6".into(),
            artifact_bytes: 42,
            table_count: 7,
            dump_tool: "mysqldump 8.4.6".into(),
            created_at_unix: 1_753_000_000,
            tables: vec!["wp_posts".into()],
            findings: Findings::default(),
        };
        let json: serde_json::Value = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "artifactBytes", "createdAtUnix", "database", "domain", "dumpTool",
                "findings", "sourceHost", "sourcePort", "sourceVendor", "sourceVersion",
                "tableCount", "tables", "targetEngine", "targetVersion",
            ],
            "a new manifest field must be added here knowingly — and never a credential"
        );
    }

    #[test]
    fn an_artifact_without_a_manifest_or_with_a_changed_size_is_refused() {
        let dir = std::env::temp_dir().join("rexenv-dbdump-manifest-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // No artifact at all.
        assert!(load_manifest(&dir, "ea.test").is_err());

        // Artifact, no manifest — the interrupted-dump shape.
        std::fs::write(artifact_path(&dir, "ea.test"), "-- half a dump").unwrap();
        let err = load_manifest(&dir, "ea.test").unwrap_err().to_string();
        assert!(err.contains("never finished"), "{err}");

        // Manifest present but the size moved.
        let m = Manifest {
            domain: "ea.test".into(),
            database: "ea".into(),
            source_host: "127.0.0.1".into(),
            source_port: 3306,
            source_vendor: Vendor::Mysql,
            source_version: "8.0.27".into(),
            target_engine: "mysql".into(),
            target_version: "8.4.6".into(),
            artifact_bytes: 999_999,
            table_count: 1,
            dump_tool: "mysqldump".into(),
            created_at_unix: 0,
            tables: vec![],
            findings: Findings::default(),
        };
        std::fs::write(manifest_path(&dir, "ea.test"), serde_json::to_string(&m).unwrap()).unwrap();
        let err = load_manifest(&dir, "ea.test").unwrap_err().to_string();
        assert!(err.contains("changed after it was written"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scan_reads_the_shapes_the_research_verified() {
        let dir = std::env::temp_dir().join("rexenv-dbdump-scan-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scan.sql");

        // The BREAKING May-2024 sandbox form on line 1.
        std::fs::write(
            &p,
            "/*!999999\\- enable the sandbox mode */\n\
             CREATE DEFINER=`root`@`localhost` PROCEDURE x() BEGIN END;\n\
             CREATE USER 'u'@'%' IDENTIFIED WITH mysql_native_password BY 'x';\n\
             SET sql_mode='NO_AUTO_CREATE_USER';\n",
        )
        .unwrap();
        let r = scan_artifact(&p).unwrap();
        assert!(r.findings.skip_sandbox_line);
        assert_eq!(r.findings.definer_count, 1);
        assert!(r.findings.native_password);
        assert!(r.findings.no_auto_create_user);

        // The CURRENT form (`/*M!`) is a plain comment to MySQL — not flagged.
        std::fs::write(&p, "/*M!999999\\- enable the sandbox mode */\nSELECT 1;\n").unwrap();
        let r = scan_artifact(&p).unwrap();
        assert!(!r.findings.skip_sandbox_line, "the /*M! form parses fine; only /*! must be skipped");

        // Table names are read from the artifact itself.
        std::fs::write(&p, "CREATE TABLE `wp_posts` (\n  `id` int\n);\nCREATE TABLE `wp_options` (x int);\n-- CREATE TABLE `commented` (x int);\n").unwrap();
        assert_eq!(scan_artifact(&p).unwrap().tables, vec!["wp_posts", "wp_options"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classify_self_import_reads_the_sites_table() {
        let conn = crate::state::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
             VALUES ('s1','A','a.test','wordpress','8.3','/x/a','ea'),
                    ('s2','B','b.test','wordpress','8.3','/x/b','shop')",
            [],
        )
        .unwrap();
        assert_eq!(classify_self_import(&conn, "s1", "ea").unwrap(), SelfImport::ThisSite);
        assert_eq!(
            classify_self_import(&conn, "s1", "shop").unwrap(),
            SelfImport::OtherSite("b.test".into())
        );
        assert_eq!(classify_self_import(&conn, "s1", "scratch").unwrap(), SelfImport::Unclaimed);
    }
}
