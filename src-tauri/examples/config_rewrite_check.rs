//! Live check for Stage 3 — the connection rewrite end to end (plan §8 step 8).
//! Run: `cargo run --example config_rewrite_check`
//!
//! One sandbox mysqld on a fixture port plays "rexenv's engine"; the site is a
//! fixture wp-config in a sandbox temp dir. Nothing here touches the user's
//! real stack, app data, Valet, Herd, or DBngin (see `common::sandbox`).
//!
//! Proves, in order:
//!   1. the ROOT case end to end with real credentials: dedicated user
//!      created holding THEIR password, rewrite applied byte-exactly (the
//!      password line untouched, and absent from the diff), and the sign-in
//!      verification minting its proof against the REWRITTEN FILE AS RE-READ —
//!      signing in as the dedicated user with their password;
//!   2. a failed write leaves their file byte-untouched (temp+rename makes a
//!      half-written config unrepresentable) — the Stage 1 recovery lesson;
//!   3. revert restores the BYTE-IDENTICAL original, mode included;
//!   4. the digest classifier refuses an edited-since file without force;
//!   5. a REVERTED config can no longer mint a proof (WrongTarget — it points
//!      at the old server again);
//!   6. re-scanning the REWRITTEN config lands `SelfImport::ThisSite` — the
//!      §7 self-source guard closes over the rewrite's own output;
//!   7. a COLLISION-RENAMED copy (the config says `local`, the copy is the
//!      fixture database) connects when the plan also moves the database name
//!      — and, the control, the same rewrite without the name cannot mint a
//!      proof (ledger #574; before 12 Sep 2026 this case was tell-only).

use rexenv_lib::core::confedit::{self, RewritePlan};
use rexenv_lib::core::confrewrite::{self, FileEditedReason, RevertCheck};
use rexenv_lib::core::confverify::{self, VerifyFail};
use rexenv_lib::core::dbdump::{self, SelfImport};
use rexenv_lib::core::dbmirror;
use rexenv_lib::core::{binaries, database, dbimport};
use rexenv_lib::state::db;
use std::time::Duration;

mod common;
use common::Reaped;

const PORT: u16 = 13397;
const DB: &str = "rwcheck_ea";
const DOMAIN: &str = "rwcheck.test";
const THEIR_PASSWORD: &str = "hunter2";

fn check(ok: &mut bool, name: &str, pass: bool, detail: &str) {
    println!("{} {name}{}", if pass { "✓" } else { "✗" }, if pass { String::new() } else { format!(" — {detail}") });
    *ok &= pass;
}

#[tokio::main]
async fn main() {
    let (plat, sandbox) = common::sandbox("config_rewrite_check");
    let mut ok = true;

    // ── sandbox mysqld = "rexenv's engine" on a fixture port ────────────────
    let basedir = binaries::resolve_dir(&*plat, "mysql", binaries::pins().mysql)
        .await
        .expect("mysql tree (cached)");
    let datadir = sandbox.root().join("mysql-data");
    database::initialize(&*plat, &basedir, &datadir).expect("init datadir");
    let socket = sandbox.root().join("mysql.sock");
    let mut mysqld = Reaped::new(
        database::start(&*plat, &basedir, &datadir, PORT, &socket).expect("start mysqld"),
        PORT,
        "mysqld",
    );
    for _ in 0..150 {
        if database::mysql_running(PORT) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(database::mysql_running(PORT), "sandbox mysqld is up");
    let (client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");
    rexenv_lib::core::db::DbEngine::Mysql.create_database(&client, PORT, DB).expect("create the imported copy");

    // ── the fixture site: a root-case wp-config pointing at "their" server ──
    let project = sandbox.root().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let config = project.join("wp-config.php");
    let original = format!(
        "<?php\n// The user's own file, conventions and all.\r\n\
         define( 'DB_NAME', '{DB}' );\n\
         define( 'DB_USER', 'root' );\n\
         define( 'DB_PASSWORD', '{THEIR_PASSWORD}' );\n\
         define( 'DB_HOST', '127.0.0.1:3306' );\n\
         $table_prefix = 'wp_';\n"
    );
    std::fs::write(&config, &original).unwrap();
    common::set_mode(&config, 0o600).unwrap();

    // ── 1a. plan + rewrite: the diff IS the write, and holds no secret ──────
    let dedicated = dbmirror::dedicated_user_name(DOMAIN);
    let plan =
        RewritePlan::wp(&format!("127.0.0.1:{PORT}"), Some(&dedicated)).expect("plan");
    let rewrite = confedit::rewrite(&original, &plan).expect("rewrite plans cleanly");
    check(
        &mut ok,
        "the diff carries no secret and both changed lines",
        rewrite.diff.iter().all(|d| !d.text.contains(THEIR_PASSWORD))
            && rewrite.diff.iter().any(|d| d.text.contains(&dedicated))
            && rewrite.diff.iter().any(|d| d.text.contains(&format!("127.0.0.1:{PORT}"))),
        &format!("{:?}", rewrite.diff),
    );
    check(
        &mut ok,
        "the password line survives byte-identical in the new content",
        rewrite.new_content.contains(&format!("define( 'DB_PASSWORD', '{THEIR_PASSWORD}' );")),
        "password line was touched",
    );

    // ── 1b. backup (0600, first wins), mirror, write, verify ────────────────
    let backup = confrewrite::backup_path(&*plat, "site-fixture", &config).expect("backup path");
    confrewrite::write_backup(&*plat, &backup, &original).expect("backup written");
    let mode = common::mode_bits(&backup);
    check(&mut ok, "the backup is born 0600", mode == 0o600, &format!("mode {mode:o}"));

    dbmirror::mirror_dedicated(&client, PORT, DB, DOMAIN, THEIR_PASSWORD)
        .expect("dedicated user created");
    confrewrite::atomic_write_preserving_mode(&*plat, &config, &rewrite.new_content)
        .expect("atomic write");
    let mode = common::mode_bits(&config);
    check(&mut ok, "their chmod 600 survives the rewrite", mode == 0o600, &format!("mode {mode:o}"));

    let scratch = sandbox.root().join("scratch");
    let proof = confverify::verify_signin(&*plat, &client, &project, &scratch, PORT, DB)
        .expect("verify runs");
    check(
        &mut ok,
        "the proof mints only from a real sign-in as the dedicated user",
        matches!(&proof, Ok(p) if !p.http_confirmed()),
        &format!("{proof:?}"),
    );

    // ── 2. a failed write leaves their file byte-untouched ──────────────────
    let after_write = std::fs::read_to_string(&config).unwrap();
    common::set_mode(&project, 0o555).unwrap();
    let denied = confrewrite::atomic_write_preserving_mode(&*plat, &config, "sabotage");
    common::set_mode(&project, 0o755).unwrap();
    check(
        &mut ok,
        "a failed write errors AND the file still holds the previous bytes",
        denied.is_err() && std::fs::read_to_string(&config).unwrap() == after_write,
        "the write half-landed",
    );

    // ── 6 (while rewritten). re-scan lands ThisSite ─────────────────────────
    let conn_now = dbimport::read_connection(&project).expect("re-read the rewritten config");
    let identity = match rexenv_lib::core::dbsource::probe(&conn_now.host, conn_now.port) {
        rexenv_lib::core::dbsource::Probe::Listening(id) => id,
        other => panic!("fixture engine not listening: {other:?}"),
    };
    let ours = [dbdump::OurEngine { port: PORT, version: binaries::pins().mysql.into() }];
    let is_ours = dbdump::server_is_ours(&conn_now.host, conn_now.port, &identity, &ours);
    let state = db::open(&sandbox.root().join("app.db")).expect("sandbox state db");
    state
        .execute(
            "INSERT INTO sites (id, name, domain, type, php_version, path, db_name)
             VALUES ('s1','Rw', ?1, 'wordpress', '8.3', ?2, ?3)",
            rusqlite::params![DOMAIN, project.display().to_string(), DB],
        )
        .unwrap();
    let class = dbdump::classify_self_import(&state, "s1", &conn_now.database).unwrap();
    check(
        &mut ok,
        "re-scanning the rewritten config lands ThisSite on our own engine",
        is_ours && class == SelfImport::ThisSite,
        &format!("is_ours={is_ours} class={class:?}"),
    );

    // ── 4. edited-since refuses without force ───────────────────────────────
    let digest = confrewrite::sha256_hex(rewrite.new_content.as_bytes());
    let mut edited = std::fs::read_to_string(&config).unwrap();
    edited.push_str("// their later edit\n");
    std::fs::write(&config, &edited).unwrap();
    let verdict = confrewrite::classify_revert(Some(&edited), Some(&original), Some(&digest));
    check(
        &mut ok,
        "an edited-since file classifies as FileEdited, never a silent restore",
        verdict == RevertCheck::FileEdited { reason: FileEditedReason::EditedSinceRewrite },
        &format!("{verdict:?}"),
    );

    // ── 3. revert: byte-identical original, mode kept ───────────────────────
    std::fs::write(&config, &rewrite.new_content).unwrap(); // back to un-edited
    let current = std::fs::read_to_string(&config).unwrap();
    let verdict = confrewrite::classify_revert(Some(&current), Some(&original), Some(&digest));
    check(&mut ok, "an untouched rewrite classifies CleanRestore", verdict == RevertCheck::CleanRestore, &format!("{verdict:?}"));
    confrewrite::atomic_write_preserving_mode(&*plat, &config, &original).expect("restore");
    let restored = std::fs::read_to_string(&config).unwrap();
    let mode = common::mode_bits(&config);
    check(
        &mut ok,
        "revert restores the byte-identical original (CRLF quirk included), mode kept",
        restored == original && mode == 0o600,
        "restore drifted",
    );

    // ── 5. a reverted config can no longer mint a proof ─────────────────────
    let after = confverify::verify_signin(&*plat, &client, &project, &scratch, PORT, DB)
        .expect("verify runs");
    check(
        &mut ok,
        "the reverted file fails WrongTarget — no proof without the rewrite",
        matches!(after, Err(VerifyFail::WrongTarget { port: 3306, .. })),
        &format!("{after:?}"),
    );

    // ── 7. a collision-renamed copy connects by renaming the config too ─────
    // The Local shape: every site's config names `local`, so the second import
    // restores under another name. Here the copy is DB and the config says
    // `local`, signing in as root with a password, over Local's socket host.
    const RENAMED_DOMAIN: &str = "rwcheck-local.test";
    let renamed = sandbox.root().join("project-renamed");
    std::fs::create_dir_all(&renamed).unwrap();
    let renamed_config = renamed.join("wp-config.php");
    let renamed_original = format!(
        "<?php\n\
         define( 'DB_NAME', 'local' );\n\
         define( 'DB_USER', 'root' );\n\
         define( 'DB_PASSWORD', '{THEIR_PASSWORD}' );\n\
         define( 'DB_HOST', 'localhost' );\n\
         $table_prefix = 'wp_';\n"
    );
    let renamed_user = dbmirror::dedicated_user_name(RENAMED_DOMAIN);
    dbmirror::mirror_dedicated(&client, PORT, DB, RENAMED_DOMAIN, THEIR_PASSWORD)
        .expect("dedicated user for the renamed site");

    // Control first: host + user only leaves `local` in the file — no proof.
    let no_name = RewritePlan::wp(&format!("127.0.0.1:{PORT}"), Some(&renamed_user)).expect("plan");
    let without = confedit::rewrite(&renamed_original, &no_name).expect("rewrite");
    std::fs::write(&renamed_config, &without.new_content).unwrap();
    let control = confverify::verify_signin(&*plat, &client, &renamed, &scratch, PORT, DB)
        .expect("verify runs");
    check(
        &mut ok,
        "control: without the name the rewritten config still says `local` and mints no proof",
        control.is_err(),
        &format!("{control:?}"),
    );

    let with_name = RewritePlan::wp(&format!("127.0.0.1:{PORT}"), Some(&renamed_user))
        .expect("plan")
        .with_database(DB)
        .expect("a valid database name");
    let renamed_rewrite = confedit::rewrite(&renamed_original, &with_name).expect("rewrite");
    check(
        &mut ok,
        "the renamed diff moves DB_NAME to the copy and still carries no secret",
        renamed_rewrite.diff.iter().any(|d| d.text.contains(&format!("'DB_NAME', '{DB}'")))
            && renamed_rewrite.diff.iter().all(|d| !d.text.contains(THEIR_PASSWORD)),
        &format!("{:?}", renamed_rewrite.diff),
    );
    std::fs::write(&renamed_config, &renamed_original).unwrap();
    confrewrite::atomic_write_preserving_mode(&*plat, &renamed_config, &renamed_rewrite.new_content)
        .expect("atomic write");
    let renamed_proof = confverify::verify_signin(&*plat, &client, &renamed, &scratch, PORT, DB)
        .expect("verify runs");
    check(
        &mut ok,
        "a collision-renamed copy connects: the proof mints against the renamed file",
        matches!(&renamed_proof, Ok(p) if !p.http_confirmed()),
        &format!("{renamed_proof:?}"),
    );

    mysqld.reap();
    if ok {
        println!("\nconfig_rewrite_check: ALL CHECKS PASSED");
    } else {
        println!("\nconfig_rewrite_check: FAILURES ABOVE");
        std::process::exit(1);
    }
}
