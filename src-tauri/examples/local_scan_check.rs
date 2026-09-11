//! Live check for the Local (WP Engine) scan. Run:
//! `cargo run --example local_scan_check`
//!
//! Runs the REAL discovery against whatever Local has on this machine and
//! checks three things (`docs/PLAN-local-import.md`, ledger #570/#571/#572):
//!
//!   1. **It is read-only.** Every file under `~/Local Sites` and Local's own
//!      registry files are fingerprinted (path, size, mtime) before and after
//!      the scan and must be identical. Local's app-data SUBDIRECTORIES (its
//!      Electron caches, `run/`) are deliberately not walked: the scan never
//!      goes there, and a running Local rewrites its caches on its own clock,
//!      which would make this check fail for a reason that isn't ours.
//!   2. **No importable row carries a domain site creation would refuse** —
//!      `.local` is re-homed before anyone sees the row.
//!   3. **Every importable row maps back to its database server** through the
//!      same registry lookup the database import uses.
//!
//! Every row is printed with its classification, so the list can be reconciled
//! by eye against Local's own sidebar. With no Local on this machine it says so
//! and exits 0 — an absent tool is not a failure.
//!
//! The default TLD is taken as `rex` rather than read from the app database:
//! this example never opens the real app state.

use rexenv_lib::core::localwp;
use rexenv_lib::core::valet::SiteStatus;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod common;

type Print = BTreeMap<String, (u64, Option<std::time::SystemTime>)>;

/// Every file under `dir` as path → (size, mtime). Reading changes atime only,
/// so a difference here means something was WRITTEN. `deep = false` records the
/// directory's own entries without descending.
fn fingerprint(dir: &Path, deep: bool) -> Print {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
            let rel = p.strip_prefix(dir).unwrap_or(&p).display().to_string();
            if md.is_dir() {
                if deep {
                    stack.push(p);
                }
                out.insert(format!("{rel}/"), (0, None));
            } else {
                out.insert(rel, (md.len(), md.modified().ok()));
            }
        }
    }
    out
}

fn main() -> std::process::ExitCode {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let app_data = localwp::local_home(&home);
    let sites_root = home.join("Local Sites");
    if !app_data.join("sites.json").is_file() {
        println!("No Local installation (no {}) — nothing to check.", app_data.join("sites.json").display());
        return std::process::ExitCode::SUCCESS;
    }
    let mut check = common::Check::new("local_scan_check");

    println!("=== fingerprinting (read-only proof) ===");
    let before_registry = fingerprint(&app_data, false);
    let before_sites = fingerprint(&sites_root, true);
    println!("  {:<60} {} entries", app_data.display(), before_registry.len());
    println!("  {:<60} {} entries", sites_root.display(), before_sites.len());

    println!("\n=== scan ===");
    let Some((source, rows)) = localwp::discover(&home, "rex") else {
        check.is("a registry on disk is discovered as a source", false, "discover returned None");
        return check.verdict();
    };
    println!("  {} {}", source.kind.label(), source.home);
    for n in &source.notes {
        println!("    note: {n}");
    }

    let after_registry = fingerprint(&app_data, false);
    let after_sites = fingerprint(&sites_root, true);
    for (label, was, now) in [
        ("Local's registry files", &before_registry, &after_registry),
        ("~/Local Sites", &before_sites, &after_sites),
    ] {
        let changed: Vec<String> = now
            .iter()
            .filter(|(k, v)| was.get(*k) != Some(v))
            .map(|(k, _)| k.clone())
            .chain(was.keys().filter(|k| !now.contains_key(*k)).cloned())
            .collect();
        check.is(
            &format!("{label} are unchanged by the scan (#570)"),
            changed.is_empty(),
            &format!("{} entries differ: {:?}", changed.len(), changed.iter().take(8).collect::<Vec<_>>()),
        );
    }

    println!("\n=== classification ({} rows) ===", rows.len());
    let mut importable = 0;
    for r in &rows {
        let php = r.php_minor.clone().unwrap_or_else(|| "-".into());
        match &r.status {
            SiteStatus::Importable => {
                importable += 1;
                println!(
                    "  ready        {:<24} php={php:<4} {}{}",
                    r.domain,
                    r.renamed_from.as_deref().map(|f| format!("(was {f}) ")).unwrap_or_default(),
                    r.path.as_deref().unwrap_or("-")
                );
                let tld = r.domain.rsplit('.').next().unwrap_or("");
                check.is(
                    &format!("{} is on a TLD rexenv accepts (#571)", r.domain),
                    rexenv_lib::core::tld::classify(tld).allowed && tld != "local",
                    &r.domain,
                );
                let db = r.path.as_deref().and_then(|p| localwp::db_source_for(&home, Path::new(p)));
                check.is(
                    &format!("{}'s docroot maps back to its Local database server (#572)", r.domain),
                    db.as_ref().is_some_and(|d| d.port > 0),
                    &format!("{db:?}"),
                );
            }
            SiteStatus::Unsupported(why) => println!("  can't import {:<24} php={php:<4} {why}", r.domain),
            other => println!("  {:<12} {:<24} {other:?}", "attention", r.domain),
        }
    }
    println!("\n{} rows · {importable} importable", rows.len());
    check.verdict()
}
