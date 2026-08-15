//! Live check: every flag we hand a bundled dump tool is ACCEPTED by that real
//! binary — and the one we deliberately exclude is really REJECTED. Run:
//! `cargo run --example db_dump_flags_check`
//!
//! The incident this closes: `--connect-timeout` broke every export with
//! "unknown variable" — a fact no unit test could see, because whether a tool
//! accepts a flag is the TOOL's fact, not ours. The flag lists live in
//! `core::dbdump::dump_tool_flags` / `core::database::client_base_args`; this
//! check feeds those EXACT production arrays to the real binaries, so the
//! proof can't drift from the code.
//!
//! Parse-time probes only (`--version` after option parsing): no server is
//! spawned, no port is bound, nothing outside a temp dir is written. The
//! shared binary cache is the only real thing used. Default pins always
//! resolve (cache hit on a warm machine); non-default pinned versions are
//! probed strictly offline — present in the cache or honestly skipped.

use rexenv_lib::core::database;
use rexenv_lib::core::dbdump::dump_tool_flags;
use rexenv_lib::core::dbsource::Vendor;
use rexenv_lib::core::{binaries, mariadb};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One probe: does `tool <args> --version` get past option parsing?
fn parses(tool: &Path, args: &[String]) -> (bool, String) {
    let out = Command::new(tool)
        .args(args)
        .arg("--version")
        .output()
        .unwrap_or_else(|e| panic!("running {}: {e}", tool.display()));
    (out.status.success(), String::from_utf8_lossy(&out.stderr).trim().to_string())
}

fn check(ok: &mut bool, name: &str, pass: bool, detail: &str) {
    if pass {
        println!("  ✓ {name}");
    } else {
        println!("  ✗ {name} — {detail}");
        *ok = false;
    }
}

/// Every production flag for `vendor`'s dump tool, one probe per flag plus the
/// whole set at once (a flag can be valid alone and conflict in company).
fn probe_dump_tool(ok: &mut bool, label: &str, tool: &Path, vendor: Vendor, tmp: &Path) {
    println!("{label}: {}", tool.display());

    // Modern source (no version-conditional flags) and, for MySQL tools, a
    // 5.x source which adds --column-statistics=0.
    let mut flag_sets = vec![dump_tool_flags(vendor, "8.0.27")];
    if vendor == Vendor::Mysql {
        flag_sets.push(dump_tool_flags(vendor, "5.7.44"));
    }
    for flags in &flag_sets {
        for f in flags {
            let (pass, err) = parses(tool, std::slice::from_ref(f));
            check(ok, &format!("accepts {f}"), pass, &err);
        }
        let (pass, err) = parses(tool, flags);
        check(ok, &format!("accepts the whole set ({})", flags.join(" ")), pass, &err);
    }

    // --result-file with a value, as dump() passes it.
    let result = format!("--result-file={}", tmp.join("probe.sql").display());
    let (pass, err) = parses(tool, &[result.clone()]);
    check(ok, "accepts --result-file=<path>", pass, &err);

    // A defaults file shaped like DefaultsFile::create (the [client] group,
    // quoted password) must be accepted as the FIRST argument.
    let cnf = tmp.join(format!("{label}-ok.cnf"));
    std::fs::write(&cnf, "[client]\nhost=127.0.0.1\nport=13306\nuser=root\npassword=\"p\"\n")
        .expect("write cnf");
    let (pass, err) = parses(tool, &[format!("--defaults-extra-file={}", cnf.display())]);
    check(ok, "accepts the DefaultsFile shape first", pass, &err);

    // THE NEGATIVES — why the connect bound is excluded. Vendor semantics
    // DIFFER, which this check's own first run discovered (28 Jul 2026,
    // falsifying the old "every dump tool hard-errors" comment):
    //   mysqldump     — hard error, exit != 0 ("unknown variable"): the flag
    //                   would break every export.
    //   mariadb-dump  — WARNS "unknown variable" and continues, exit 0: the
    //                   flag is dead weight that never bounds anything.
    // Either way the exclusion is right; each vendor is pinned to its actual
    // behavior so a future tool version changing it fails here.
    let probe_unknown = |args: &[String]| {
        let (pass, err) = parses(tool, args);
        let warned = err.to_lowercase().contains("unknown");
        match vendor {
            Vendor::Mysql => (!pass && warned, format!("expected a hard 'unknown' error: {err}")),
            Vendor::Mariadb => {
                (pass && warned, format!("expected warn-and-continue 'unknown': {err}"))
            }
        }
    };
    let (pass, detail) = probe_unknown(&["--connect-timeout=10".into()]);
    check(ok, "--connect-timeout not honored on argv (per-vendor semantics)", pass, &detail);
    let bad_cnf = tmp.join(format!("{label}-bad.cnf"));
    std::fs::write(&bad_cnf, "[client]\nconnect-timeout=10\n").expect("write cnf");
    let (pass, detail) =
        probe_unknown(&[format!("--defaults-extra-file={}", bad_cnf.display())]);
    check(ok, "connect-timeout not honored in the defaults group", pass, &detail);

    // Sanity for the Mariadb flag list: the MySQL-only flags must be absent.
    // mariadb-dump doesn't know --set-gtid-purged (it warns and ignores), so
    // shipping it would spray a warning per export while doing nothing.
    if vendor == Vendor::Mariadb {
        let flags = &flag_sets[0];
        check(
            ok,
            "mariadb flag set carries no MySQL-only flags",
            !flags.iter().any(|f| f.contains("gtid") || f.contains("column-statistics")),
            &flags.join(" "),
        );
        let (pass, detail) = probe_unknown(&["--set-gtid-purged=OFF".into()]);
        check(ok, "…which mariadb-dump indeed doesn't know (warns, ignores)", pass, &detail);
    }
}

/// A cached (strictly offline) dump tool for a non-default pinned version.
fn cached_dump_tool(plat: &dyn rexenv_lib::platform::traits::Platform, engine: &str, version: &str) -> Option<PathBuf> {
    let bin = plat.paths().bin_dir().ok()?;
    let tool = match engine {
        "mysql" => bin.join(format!("mysql-{version}")).join("bin/mysqldump"),
        "mariadb" => mariadb::mariadb_dump_bin(&bin.join(format!("mariadb-{version}"))),
        _ => return None,
    };
    tool.is_file().then_some(tool)
}

#[tokio::main]
async fn main() {
    let plat = rexenv_lib::platform::current();
    let tmp = std::env::temp_dir().join(format!("rexenv-dump-flags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");
    let mut ok = true;

    // Default pins — resolve (warm cache on a dev machine; the one download
    // this check may legitimately trigger on a cold one).
    let mysql = binaries::resolve_dir(&*plat, "mysql", binaries::MYSQL_VERSION)
        .await
        .expect("mysql tree");
    probe_dump_tool(&mut ok, "mysqldump-default", &mysql.join("bin/mysqldump"), Vendor::Mysql, &tmp);

    let mdb = binaries::resolve_bundle(&*plat, "mariadb", binaries::MARIADB_VERSION)
        .await
        .expect("mariadb bundle");
    probe_dump_tool(&mut ok, "mariadb-dump-default", &mariadb::mariadb_dump_bin(&mdb), Vendor::Mariadb, &tmp);

    // The client binaries: the interactive array INCLUDING the connect bound
    // is valid for clients — the contrast that makes the exclusion coherent.
    let client_args = database::client_base_args(13306).to_vec();
    // Client paths via `sql_client_bins` — the ONE constructor of `SqlClient`
    // — so this probe exercises the exact binary production would spawn.
    let (mdb_client, _) = rexenv_lib::core::db::DbEngine::Mariadb
        .sql_client_bins(&*plat, binaries::MARIADB_VERSION)
        .await
        .expect("mariadb client");
    let (mysql_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, binaries::MYSQL_VERSION)
        .await
        .expect("mysql client");
    for (label, client) in [
        ("mysql-client", mysql_client.path().to_path_buf()),
        ("mariadb-client", mdb_client.path().to_path_buf()),
    ] {
        let (pass, err) = parses(&client, &client_args);
        check(
            &mut ok,
            &format!("{label} accepts client_base_args (incl. --connect-timeout)"),
            pass,
            &err,
        );
    }

    // Non-default pinned versions: probe when cached, say so when not.
    for v in binaries::MYSQL_VERSIONS.iter().filter(|v| **v != binaries::MYSQL_VERSION) {
        match cached_dump_tool(&*plat, "mysql", v) {
            Some(tool) => probe_dump_tool(&mut ok, &format!("mysqldump-{v}"), &tool, Vendor::Mysql, &tmp),
            None => println!("  – mysql {v} not cached; skipped (offline probe only)"),
        }
    }
    for v in binaries::MARIADB_VERSIONS.iter().filter(|v| **v != binaries::MARIADB_VERSION) {
        match cached_dump_tool(&*plat, "mariadb", v) {
            Some(tool) => {
                probe_dump_tool(&mut ok, &format!("mariadb-dump-{v}"), &tool, Vendor::Mariadb, &tmp)
            }
            None => println!("  – mariadb {v} not cached; skipped (offline probe only)"),
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
    if ok {
        println!("db_dump_flags_check: PASS");
    } else {
        println!("db_dump_flags_check: FAIL");
        std::process::exit(1);
    }
}
