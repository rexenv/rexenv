//! core::database — database services (Phase 1: MySQL).
//!
//! One shared MySQL server on a loopback port. The datadir is initialized once
//! with `--initialize-insecure` (passwordless root for local dev), then the
//! `mysqld` master is supervised like the other services. `basedir` is the
//! extracted MySQL tree from `core::binaries::resolve_dir("mysql", …)`.

use crate::core::db::SqlClient;
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Loopback port for the shared MySQL server (non-default to avoid clashing with
/// a system MySQL on 3306).
pub const MYSQL_PORT: u16 = 13306;

/// `mysqld` executable inside the extracted MySQL tree.
pub fn mysqld_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mysqld")
}

/// `mysql` client inside the extracted MySQL tree.
///
/// `pub(crate)` deliberately: outside the crate the client is only ever
/// obtained as a [`crate::core::db::SqlClient`] from `sql_client_bins` /
/// `cached_sql_client`, so a bare path can no longer be handed to the
/// functions that exec it — the `&Path`-means-two-things class that left nine
/// examples red for a month (14 Aug 2026; docs/archive/SHIPPED-2026-08.md).
pub fn mysql_client_bin(basedir: &Path) -> PathBuf {
    basedir.join("bin/mysql")
}

/// MySQL data directory under app-data.
pub fn data_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("mysql").join("data"))
}

/// Unix socket path for the server.
pub fn socket_path(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("run").join("mysql.sock"))
}

/// A datadir is initialized once its system `mysql` schema dir exists.
pub fn is_initialized(datadir: &Path) -> bool {
    datadir.join("mysql").is_dir()
}

/// Initialize the datadir if needed (`mysqld --initialize-insecure`): creates a
/// fresh database with a passwordless `root@localhost`. Idempotent.
pub fn initialize(platform: &dyn Platform, basedir: &Path, datadir: &Path) -> Result<()> {
    if is_initialized(datadir) {
        return Ok(());
    }
    std::fs::create_dir_all(datadir)?;
    // Any failure below must remove the half-written datadir: mysqld creates the
    // `mysql/` system-schema dir EARLY, before init completes, so a leftover
    // would satisfy is_initialized on the next run and start the server on a
    // corrupt datadir (B22). The wrapper covers the nonzero exit AND every `?`.
    let result = (|| -> Result<()> {
        let log = platform.paths().log_dir()?.join("mysql-init.log");
        std::fs::create_dir_all(platform.paths().log_dir()?)?;

        let args = vec![
            "--no-defaults".to_string(),
            "--initialize-insecure".to_string(),
            format!("--basedir={}", basedir.display()),
            format!("--datadir={}", datadir.display()),
            format!("--log-error={}", log.display()),
        ];
        let mut child = platform.supervisor().spawn(&mysqld_bin(basedir), &args)?;
        let status = child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "mysqld --initialize-insecure failed (exit {}); see {}",
                crate::core::proc::exit_text(status.code()),
                log.display()
            )))
        }
    })();
    crate::core::db::clean_datadir_on_init_failure(datadir, result)
}

/// Guard for identifiers we interpolate into SQL: DB names are derived from a
/// validated site domain (`wordpress::db_name_for`), and this is the backstop.
pub(crate) fn validate_db_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::Other(format!("invalid database name {name:?}")));
    }
    Ok(())
}

/// The shared flag prefix for the bundled MySQL-protocol **interactive clients**
/// (`mysql` / `mariadb`): loopback TCP, root/no password (the local-dev setup),
/// plus a 10s `--connect-timeout` so a stalled-accept server errors out instead
/// of hanging the calling command forever (B25). Connect-phase ONLY — run time
/// stays unbounded on purpose: a big import legitimately runs for minutes, and a
/// wall-clock cap here would be the B34 mistake.
///
/// NOT used by `export_to_downloads`: the dump tools (`mysqldump` 8.0/8.4,
/// `mariadb-dump` 11.4/12.3) all REJECT `--connect-timeout` ("unknown variable",
/// hard exit — verified against the bundled binaries), so the dump keeps its own
/// unbounded-connect args rather than a flag that breaks every export.
// Public (was crate-private) so `examples/db_dump_flags_check.rs` can prove
// the real bundled binaries' verdicts on the exact production argv: clients
// accept this array INCLUDING the connect bound; dump tools reject that same
// bound, which is why export_to_downloads deliberately does not use it.
pub fn client_base_args(port: u16) -> [String; 6] {
    [
        "--no-defaults".into(),
        "--protocol=TCP".into(),
        "--host=127.0.0.1".into(),
        format!("--port={port}"),
        "--user=root".into(),
        "--connect-timeout=10".into(),
    ]
}

/// Run one SQL statement via a **bundled** MySQL-protocol client (TCP to the
/// loopback server, root/no password — the local-dev setup). `client` is the
/// client BINARY (`bin/mysql` from the MySQL tree, or `bin/mariadb` from the
/// mariadb bundle — same protocol, same flags); `what` labels the error.
pub(crate) fn mysql_exec(client: &SqlClient, port: u16, sql: &str, what: &str) -> Result<()> {
    let out = crate::platform::command(client.path())
        .args(client_base_args(port))
        .args(["-e", sql])
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{what} failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Export database `name` into the user's Downloads folder as
/// `<domain>-db.sql` (numbered on collision — same convention as the
/// debug-log download), via the bundled `mysqldump` from the extracted MySQL
/// tree (or `mariadb-dump` from the bundle). WP-CLI's `wp db export` shells out
/// to a PATH `mysqldump` — absent in a Finder-launched app (same rationale as
/// [`create_database`]). Returns the destination path. Requires the server to
/// be running. `dump` is the dump BINARY.
///
/// `pub(crate)`: the ONE way in from outside this crate is
/// [`crate::core::db::DbEngine`]'s method of the same name, which dispatches on
/// the client's engine. Since PostgreSQL joined the site engines, a caller that
/// reaches for `database::` by name is choosing a wire protocol by hand — the
/// #329 lesson (a type sees every caller; a grep sees its pattern) applied to
/// the module boundary.
pub(crate) fn export_to_downloads(dump: &Path, port: u16, domain: &str, name: &str) -> Result<PathBuf> {
    let dest = dump_dest(domain)?;
    dump_to_file(dump, port, name, &dest)?;
    Ok(dest)
}

/// Dump database `name` into `dest` — the export's dump, to a path the caller
/// chose (`core::dbclone`'s private scratch file). The flags are the export's,
/// unchanged, so both paths copy exactly the same things: tables, views and
/// triggers; NOT stored routines or events (`--routines`/`--events` are off by
/// default in the dump tools, and WordPress uses neither). A failed dump
/// removes what it wrote.
pub(crate) fn dump_to_file(dump: &Path, port: u16, name: &str, dest: &Path) -> Result<()> {
    dump_tables_to_file(dump, port, name, &[], dest)
}

/// [`dump_to_file`] for the named `tables` only (every table when empty) —
/// `core::live_sync::push` dumps one table at a time so each lands as its own
/// chunk stream. Table names are validated like database names.
pub(crate) fn dump_tables_to_file(dump: &Path, port: u16, name: &str, tables: &[&str], dest: &Path) -> Result<()> {
    validate_db_name(name)?;
    for t in tables {
        validate_db_name(t)?;
    }
    // --result-file (not shell redirection): no shell involved, so a Downloads
    // path with spaces can't break, and mysqldump writes the file itself.
    // Deliberately NOT client_base_args, because the dump tools don't honor
    // --connect-timeout — with vendor-split semantics (proven live by
    // examples/db_dump_flags_check, 28 Jul 2026): mysqldump 8.0/8.4 hard-errors
    // ("unknown variable") and would break every export; mariadb-dump
    // 11.4/12.3 only WARNS and ignores it, dead weight spraying a warning per
    // export. So this one path keeps an unbounded connect (B25).
    // `--net-buffer-length=65536`: no extended INSERT grows past 64 KB (the default lets one
    // statement reach 1 MB). A live-sync push sends a dump in chunks split BETWEEN statements
    // and halves the chunk when the host answers 413 (#892) — a statement bigger than what the
    // host takes can never be halved (leg 19b, 16 Oct 2026: 12 × 47 KB posts in one 560 KB
    // INSERT were refused at every chunk size). Both mysqldump and mariadb-dump take it.
    let out = crate::platform::command(dump)
        .args([
            "--no-defaults",
            "--protocol=TCP",
            "--host=127.0.0.1",
            &format!("--port={port}"),
            "--user=root",
            "--net-buffer-length=65536",
            &format!("--result-file={}", dest.display()),
            name,
        ])
        .args(tables)
        .output()?;
    if !out.status.success() {
        // A failed dump can leave a partial file — never leave it for the user
        // to mistake for a good backup.
        let _ = std::fs::remove_file(dest);
        return Err(Error::Other(format!(
            "exporting database `{name}` failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// The Downloads path an export writes to: `<domain>-db.sql`, numbered on
/// collision (same convention as the debug-log download). Shared with
/// `core::postgres`'s export so the two engines cannot drift into two naming
/// conventions for the same button.
pub(crate) fn dump_dest(domain: &str) -> Result<PathBuf> {
    let downloads = crate::core::downloads::user_downloads_dir()?;
    let mut dest = downloads.join(format!("{domain}-db.sql"));
    let mut n = 1;
    while dest.exists() {
        dest = downloads.join(format!("{domain}-db-{n}.sql"));
        n += 1;
    }
    Ok(dest)
}

/// Import a `.sql` dump into database `name` via the bundled `mysql` client,
/// feeding the file over **stdin** (no shell, no quoting problems — same
/// rationale as `--result-file` in [`export_to_downloads`]). DESTRUCTIVE: the
/// dump executes as-is, so tables it contains overwrite existing ones; the
/// caller owns the confirm/backup UX. Requires the MySQL server to be running.
///
/// `pub(crate)`: the ONE way in from outside this crate is
/// [`crate::core::db::DbEngine`]'s method of the same name, which dispatches on
/// the client's engine. Since PostgreSQL joined the site engines, a caller that
/// reaches for `database::` by name is choosing a wire protocol by hand — the
/// #329 lesson (a type sees every caller; a grep sees its pattern) applied to
/// the module boundary.
pub(crate) fn import_from_file(client: &SqlClient, port: u16, name: &str, file: &Path) -> Result<()> {
    validate_db_name(name)?;
    let f = std::fs::File::open(file)
        .map_err(|e| Error::Other(format!("open {}: {e}", file.display())))?;
    if f.metadata()?.len() == 0 {
        return Err(Error::Other(format!(
            "{} is empty — not importing",
            file.display()
        )));
    }
    let out = crate::platform::command(client.path())
        .args(client_base_args(port))
        .arg(name)
        .stdin(std::process::Stdio::from(f))
        .output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "importing into `{name}` failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Import `file` into `name` as a THROWAWAY user whose only grant is `name`.*
/// (ledger #837, the security review's critical finding). The stream a live
/// site's plugin answers is data from a machine rexenv does not control; piped
/// into `mysql` as root it could reach every other local database, the `mysql`
/// schema, `CREATE USER`, `GRANT` — anything. Under this user every such
/// statement fails at the server with "access denied", whatever the text says;
/// `--local-infile=0` closes `LOAD DATA LOCAL`. The user is created for the one
/// import and dropped after it, success or not; its password travels in the
/// environment, never on the argv (`ps` would show it).
///
/// Not an allow-list of statements: parsing SQL to decide what it touches is
/// how allow-lists get bypassed. The wall is the server's own privilege check.
pub(crate) fn import_from_file_scoped(client: &SqlClient, port: u16, name: &str, file: &Path) -> Result<()> {
    validate_db_name(name)?;
    let user = format!("rexpull_{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let pass = uuid::Uuid::new_v4().simple().to_string();
    let host = "127.0.0.1"; // what the server sees of a TCP loopback client
    mysql_exec(
        client,
        port,
        &format!("CREATE USER '{user}'@'{host}' IDENTIFIED BY '{pass}'; GRANT ALL PRIVILEGES ON `{name}`.* TO '{user}'@'{host}'; FLUSH PRIVILEGES;"),
        "creating the import user",
    )?;
    let result = (|| {
        let f = std::fs::File::open(file).map_err(|e| Error::Other(format!("open {}: {e}", file.display())))?;
        if f.metadata()?.len() == 0 {
            return Err(Error::Other(format!("{} is empty — not importing", file.display())));
        }
        let args: Vec<String> = client_base_args(port).into_iter().filter(|a| a != "--user=root").collect();
        let out = crate::platform::command(client.path())
            .args(args)
            .arg(format!("--user={user}"))
            .arg("--local-infile=0")
            .env("MYSQL_PWD", &pass)
            .arg(name)
            .stdin(std::process::Stdio::from(f))
            .output()?;
        if !out.status.success() {
            return Err(Error::Other(format!(
                "importing into `{name}` failed (exit {}): {}",
                crate::core::proc::exit_text(out.status.code()),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    })();
    let _ = mysql_exec(client, port, &format!("DROP USER IF EXISTS '{user}'@'{host}';"), "dropping the import user");
    result
}

/// Create a database if it doesn't exist, via the bundled `mysql` client.
/// WP-CLI's `wp db create` shells out to whatever `mysql` is on PATH — a
/// Finder-launched app has the bare launchd PATH (no Homebrew), so rexenv
/// must always use its own client from the extracted MySQL tree.
///
/// `pub(crate)`: the ONE way in from outside this crate is
/// [`crate::core::db::DbEngine`]'s method of the same name, which dispatches on
/// the client's engine. Since PostgreSQL joined the site engines, a caller that
/// reaches for `database::` by name is choosing a wire protocol by hand — the
/// #329 lesson (a type sees every caller; a grep sees its pattern) applied to
/// the module boundary.
pub(crate) fn create_database(client: &SqlClient, port: u16, name: &str) -> Result<()> {
    validate_db_name(name)?;
    mysql_exec(
        client,
        port,
        &format!("CREATE DATABASE IF NOT EXISTS `{name}`"),
        &format!("creating database `{name}`"),
    )
}

/// Drop a site's database if it exists (site teardown). Same strict name rule
/// as [`create_database`] — the caller passes only a name derived from the
/// site's validated domain, so an arbitrary/other database can't be named.
///
/// `pub(crate)`: the ONE way in from outside this crate is
/// [`crate::core::db::DbEngine`]'s method of the same name, which dispatches on
/// the client's engine. Since PostgreSQL joined the site engines, a caller that
/// reaches for `database::` by name is choosing a wire protocol by hand — the
/// #329 lesson (a type sees every caller; a grep sees its pattern) applied to
/// the module boundary.
pub(crate) fn drop_database(client: &SqlClient, port: u16, name: &str) -> Result<()> {
    validate_db_name(name)?;
    mysql_exec(
        client,
        port,
        &format!("DROP DATABASE IF EXISTS `{name}`"),
        &format!("dropping database `{name}`"),
    )
}

/// One query through the bundled client, rows back as bare tab-separated
/// lines (`-N -B`).
fn mysql_query(client: &SqlClient, port: u16, sql: &str, what: &str) -> Result<String> {
    let out = crate::platform::command(client.path())
        .args(client_base_args(port))
        .args(["-N", "-B", "-e", sql])
        .output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "{what} failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Does database `name` exist? Asked of the server, never inferred from a
/// table list — an EMPTY database has no rows in `information_schema.tables`.
pub(crate) fn database_exists(client: &SqlClient, port: u16, name: &str) -> Result<bool> {
    validate_db_name(name)?;
    let out = mysql_query(
        client,
        port,
        &format!("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = '{name}'"),
        &format!("looking up database `{name}`"),
    )?;
    Ok(out.lines().any(|l| l.trim() == name))
}

/// Every table and view in database `name`, sorted.
pub(crate) fn list_tables(client: &SqlClient, port: u16, name: &str) -> Result<Vec<String>> {
    validate_db_name(name)?;
    let out = mysql_query(
        client,
        port,
        &format!(
            "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{name}' \
             ORDER BY TABLE_NAME"
        ),
        &format!("listing the tables of `{name}`"),
    )?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Every BASE TABLE of `name` with its change stamp, for the live-sync status
/// (`core::live_sync::stamps`): `CHECKSUM TABLE`'s value for each table — exact,
/// local, cheap enough for a site's own database — except the `content` tables
/// (`(table, query)`: the options tables and a network's sitemeta), whose stamp is the
/// content sum over their non-volatile rows, the same rule the companion plugin applies
/// on live. One client call per statement.
pub(crate) fn table_stamps(client: &SqlClient, port: u16, name: &str, content: &[(String, String)]) -> Result<std::collections::BTreeMap<String, String>> {
    validate_db_name(name)?;
    let out = mysql_query(
        client,
        port,
        &format!("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{name}' AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME"),
        &format!("listing the tables of `{name}`"),
    )?;
    let tables: Vec<String> = out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let mut stamps = std::collections::BTreeMap::new();
    let q = |t: &str| format!("`{}`.`{}`", name, t.replace('`', "``"));
    let plain: Vec<String> = tables.iter().filter(|t| !content.iter().any(|(c, _)| c == *t)).map(|t| q(t)).collect();
    if !plain.is_empty() {
        let out = mysql_query(client, port, &format!("CHECKSUM TABLE {}", plain.join(", ")), &format!("stamping the tables of `{name}`"))?;
        for line in out.lines() {
            // `db.table<TAB>checksum` — the table part may itself hold a dot, so split at the FIRST
            // dot (the database name is validated dot-free) and the LAST tab.
            let Some((lhs, sum)) = line.rsplit_once('\t') else { continue };
            let table = lhs.split_once('.').map(|(_, t)| t).unwrap_or(lhs);
            stamps.insert(table.to_string(), format!("ck:{}", sum.trim()));
        }
    }
    for (opt, sql) in content {
        if tables.iter().any(|t| t == opt) {
            let out = mysql_query(client, port, &format!("USE `{name}`; {sql}"), &format!("stamping `{name}`.`{opt}`"))?;
            let row = out.lines().last().unwrap_or("").trim();
            let (n, sum) = row.split_once('\t').unwrap_or((row, "0"));
            stamps.insert(opt.to_string(), format!("opt:{}:{}", n.trim(), sum.trim()));
        }
    }
    Ok(stamps)
}

/// Disk size (data + indexes) of every database, in bytes, via one
/// `information_schema` query on the bundled client — backs the per-site "DB
/// size" number on the Sites page (a REAL per-site figure even for sites that
/// share nginx + a php-fpm pool). Requires the server to be running.
///
/// `pub(crate)`: the ONE way in from outside this crate is
/// [`crate::core::db::DbEngine`]'s method of the same name, which dispatches on
/// the client's engine. Since PostgreSQL joined the site engines, a caller that
/// reaches for `database::` by name is choosing a wire protocol by hand — the
/// #329 lesson (a type sees every caller; a grep sees its pattern) applied to
/// the module boundary.
pub(crate) fn db_sizes(client: &SqlClient, port: u16) -> Result<Vec<(String, u64)>> {
    let out = crate::platform::command(client.path())
        .args(client_base_args(port))
        .args([
            "-N", // no header
            "-B", // tab-separated batch mode
            "-e",
            "SELECT table_schema, COALESCE(SUM(data_length + index_length), 0) \
             FROM information_schema.tables GROUP BY table_schema",
        ])
        .output()?;
    if !out.status.success() {
        return Err(Error::Other(format!(
            "listing database sizes failed (exit {}): {}",
            crate::core::proc::exit_text(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (name, size) = l.split_once('\t')?;
            Some((name.to_string(), size.trim().parse().ok()?))
        })
        .collect())
}

/// The server's own argv (everything but the per-OS supervision flags), pure so a
/// lib test can pin it. `--skip-log-bin` is load-bearing: MySQL 8.x turns the binary
/// log ON by default and `--no-defaults` means no my.cnf (and no `SET PERSIST`) can
/// turn it off, so every write to every site was journalled in ROW format and kept
/// for 30 days (`binlog_expire_logs_seconds` 2592000, 1 GB per file). A user measured
/// 60 GB of `binlog.*` on a Mac (14 Oct 2026); the dev machine carried 629 MB. A local
/// dev server has no replica and no point-in-time restore, so the log buys nothing.
/// Ledger #858.
pub fn server_args(basedir: &Path, datadir: &Path, port: u16, socket: &Path, log: &Path) -> Vec<String> {
    vec![
        "--no-defaults".to_string(),
        format!("--basedir={}", basedir.display()),
        format!("--datadir={}", datadir.display()),
        format!("--port={port}"),
        format!("--socket={}", socket.display()),
        "--bind-address=127.0.0.1".to_string(),
        "--mysqlx=OFF".to_string(), // skip the X protocol (avoids :33060)
        "--skip-log-bin".to_string(), // no binary log: local dev, see above
        format!("--log-error={}", log.display()),
    ]
}

/// True for the files MySQL's binary log consists of and NOTHING else: `binlog.index`
/// and `binlog.` + exactly six digits (the default `--log-bin` basename; rexenv never
/// set another). Anything else in the datadir — `binlog.000001.bak`, `binlog`, a
/// schema dir, `ib_logfile0` — is not ours to touch.
pub fn is_binlog_file(name: &str) -> bool {
    match name.strip_prefix("binlog.") {
        Some("index") => true,
        Some(rest) => rest.len() == 6 && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Delete the binary-log files a server that ran WITH the log left in `datadir`
/// (ledger #858). Called by `start` right before the spawn, while no server runs:
/// `--skip-log-bin` makes mysqld ignore `binlog.index`, so without this the files
/// from before the flag landed would sit there forever — the 60 GB stays 60 GB.
/// Deletes ONLY `is_binlog_file` names, one level deep, never a directory; a file
/// that will not delete is skipped and logged, never fatal (a start must not fail
/// over housekeeping). Returns `(files, bytes)` removed. Rollback of transactions
/// that were PREPARED in the old log and never committed is the accepted cost.
pub fn remove_binlogs(datadir: &Path) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(datadir) else {
        return (0, 0);
    };
    let (mut files, mut bytes) = (0usize, 0u64);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_binlog_file(name) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.is_file() {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                files += 1;
                bytes += meta.len();
            }
            Err(e) => log::warn!("mysql: could not remove old binary log {}: {e}", path.display()),
        }
    }
    (files, bytes)
}

/// Start the shared MySQL server (foreground) via `ProcessSupervisor`.
///
/// The platform's `mysqld_supervision_args` go last: on Windows `--no-monitor`, so the
/// pid rexenv holds is the server itself and a stop cannot orphan it (ledger #600).
pub fn start(
    platform: &dyn Platform,
    basedir: &Path,
    datadir: &Path,
    port: u16,
    socket: &Path,
) -> Result<Child> {
    let log = platform.paths().log_dir()?.join("mysql-error.log");
    let mut args = server_args(basedir, datadir, port, socket, &log);
    args.extend(platform.supervisor().mysqld_supervision_args());
    let (files, bytes) = remove_binlogs(datadir);
    if files > 0 {
        log::info!("mysql: removed {files} binary-log file(s), {} MB, left by a server that ran with log_bin ON (ledger #858)", bytes / 1_000_000);
    }
    // A lock file from a server that died unclean, whose pid another program now has,
    // makes mysqld abort ("Unable to setup unix socket lock file") on every start until
    // someone removes two files by hand — measured after a VM reboot (`core::stale_lock`).
    let lock = std::path::PathBuf::from(format!("{}.lock", socket.display()));
    if let Some(why) = crate::core::stale_lock::clear_if_stale(platform, &lock, &socket.display().to_string(), &[socket])? {
        log::warn!("mysql: removed a stale lock before starting — {why}");
    }
    let stdout_log = platform.paths().log_dir()?.join("mysql-stdout.log");
    platform
        .supervisor()
        .spawn_logged(&mysqld_bin(basedir), &args, &stdout_log)
}

/// Stop a running MySQL by pid (SIGTERM → graceful shutdown).
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// True if MySQL is accepting connections on its loopback port.
pub fn mysql_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_paths_under_basedir() {
        let base = Path::new("/opt/mysql");
        assert_eq!(mysqld_bin(base), Path::new("/opt/mysql/bin/mysqld"));
        assert_eq!(mysql_client_bin(base), Path::new("/opt/mysql/bin/mysql"));
    }

    #[test]
    fn is_initialized_checks_system_schema() {
        let dir = std::env::temp_dir().join("rexenv-mysql-init-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_initialized(&dir));
        std::fs::create_dir_all(dir.join("mysql")).unwrap();
        assert!(is_initialized(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mysql_running_false_on_closed_port() {
        assert!(!mysql_running(9));
    }

    #[test]
    fn client_base_args_keep_the_flag_order_and_bound_the_connect() {
        // The exact prefix all three interactive-client call sites pass: the
        // original five flags in their original order, plus the connect bound
        // appended LAST (B25). A reorder or a dropped flag here silently changes
        // three subprocess invocations at once — pin the whole array.
        assert_eq!(
            client_base_args(13306),
            [
                "--no-defaults",
                "--protocol=TCP",
                "--host=127.0.0.1",
                "--port=13306",
                "--user=root",
                "--connect-timeout=10",
            ]
        );
    }

    #[test]
    fn server_args_disable_the_binary_log_and_keep_no_defaults() {
        // Pin the whole argv: `--skip-log-bin` is what stops the 60 GB (#858), and
        // `--no-defaults` is why nothing else (my.cnf, SET PERSIST) could.
        let args = server_args(
            Path::new("/b"),
            Path::new("/d"),
            13306,
            Path::new("/r/mysql.sock"),
            Path::new("/l/mysql-error.log"),
        );
        assert_eq!(
            args,
            [
                "--no-defaults",
                "--basedir=/b",
                "--datadir=/d",
                "--port=13306",
                "--socket=/r/mysql.sock",
                "--bind-address=127.0.0.1",
                "--mysqlx=OFF",
                "--skip-log-bin",
                "--log-error=/l/mysql-error.log",
            ]
        );
    }

    #[test]
    fn is_binlog_file_matches_mysql_names_only() {
        for yes in ["binlog.index", "binlog.000001", "binlog.000600", "binlog.999999"] {
            assert!(is_binlog_file(yes), "{yes}");
        }
        for no in [
            "binlog", "binlog.", "binlog.00001", "binlog.0000001", "binlog.00000a",
            "binlog.000001.bak", "binlog.index.old", "mysql-bin.000001", "ib_logfile0",
            "ibdata1", "mysql", "undo_001", "auto.cnf",
        ] {
            assert!(!is_binlog_file(no), "{no}");
        }
    }

    #[test]
    fn remove_binlogs_deletes_only_the_log_files() {
        let dir = std::env::temp_dir().join(format!("rexenv-binlog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mysql")).unwrap();
        std::fs::create_dir_all(dir.join("binlog.000009")).unwrap(); // a DIR with a log name
        for (name, size) in [("binlog.index", 30), ("binlog.000001", 1000), ("binlog.000002", 500)] {
            std::fs::write(dir.join(name), vec![b'x'; size]).unwrap();
        }
        for keep in ["ibdata1", "auto.cnf", "binlog.000001.bak", "mysql-bin.000001"] {
            std::fs::write(dir.join(keep), b"keep").unwrap();
        }
        assert_eq!(remove_binlogs(&dir), (3, 1530));
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["auto.cnf", "binlog.000001.bak", "binlog.000009", "ibdata1", "mysql", "mysql-bin.000001"]
        );
        assert_eq!(remove_binlogs(&dir), (0, 0)); // idempotent
        assert_eq!(remove_binlogs(&dir.join("nope")), (0, 0)); // no datadir = no-op
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_database_rejects_unsafe_names() {
        let base = SqlClient::test_at("/nonexistent");
        for bad in ["", "wp;drop", "a`b", "a b", "a-b"] {
            assert!(create_database(&base, 13306, bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn drop_database_rejects_unsafe_names() {
        let base = SqlClient::test_at("/nonexistent");
        for bad in ["", "wp;drop", "a`b", "a b", "a-b", "*", "wp_x.y"] {
            assert!(drop_database(&base, 13306, bad).is_err(), "accepted {bad:?}");
        }
    }
}
