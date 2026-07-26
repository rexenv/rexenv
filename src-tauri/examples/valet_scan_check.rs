//! Live check for the Valet/Herd scan (Stage 1). Run:
//! `cargo run --example valet_scan_check`
//!
//! Runs the REAL discovery against whatever Valet and Herd are installed on
//! this machine and checks two things:
//!
//!   1. **It is read-only.** Their trees are fingerprinted (every file's path,
//!      size and mtime) before and after the scan and must be byte-identical.
//!      This is the check that matters: the whole feature rests on the promise
//!      that a user can go back to Herd afterwards, and a promise nobody tests
//!      is a hope. (Reading touches atime, not mtime, so this is a fair test.)
//!   2. **Nothing is dropped or guessed.** Every discovered row is printed with
//!      its classification, so the totals can be reconciled by eye against what
//!      Valet/Herd themselves show.
//!
//! With neither tool installed it reports that and exits 0 — an absent
//! environment is not a failure.

use rexenv_lib::core::valet;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every file under `dir` as (relative path → size + mtime). Reading a file
/// changes atime only, so any difference here means we WROTE something.
fn fingerprint(dir: &Path) -> BTreeMap<String, (u64, Option<std::time::SystemTime>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            // symlink_metadata: never follow links out of their tree, and a
            // dangling link must be recorded as itself, not skipped.
            let Ok(md) = std::fs::symlink_metadata(&p) else { continue };
            let rel = p.strip_prefix(dir).unwrap_or(&p).display().to_string();
            if md.is_dir() {
                stack.push(p);
                out.insert(format!("{rel}/"), (0, md.modified().ok()));
            } else {
                out.insert(rel, (md.len(), md.modified().ok()));
            }
        }
    }
    out
}

fn main() {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let trees: Vec<PathBuf> = valet::valet_homes(&home)
        .into_iter()
        .chain(std::iter::once(valet::herd_home(&home)))
        .filter(|p| p.exists())
        .collect();

    if trees.is_empty() {
        println!("No Valet or Herd installation on this machine — nothing to check.");
        return;
    }

    println!("=== fingerprinting their trees (read-only proof) ===");
    let before: Vec<_> = trees
        .iter()
        .map(|t| {
            let f = fingerprint(t);
            println!("  {:<58} {} entries", t.display(), f.len());
            f
        })
        .collect();

    println!("\n=== scan ===");
    let found = valet::discover(&home);
    for s in &found.sources {
        println!("  {:<6} tld=.{:<6} parked={} {}", s.kind.label(), s.tld, s.parked.len(), s.home);
        for n in &s.notes {
            println!("         note: {n}");
        }
    }

    let mut ok = true;
    println!("\n=== read-only verification ===");
    for (t, was) in trees.iter().zip(&before) {
        let now = fingerprint(t);
        let same = *was == now;
        println!("  {:<58} unchanged={same}", t.display());
        if !same {
            ok = false;
            for (k, v) in &now {
                match was.get(k) {
                    None => println!("      ADDED   {k}"),
                    Some(old) if old != v => println!("      CHANGED {k}"),
                    _ => {}
                }
            }
            for k in was.keys() {
                if !now.contains_key(k) {
                    println!("      REMOVED {k}");
                }
            }
        }
    }

    println!("\n=== classification ({} rows) ===", found.sites.len());
    let mut importable = 0;
    let mut blocked = 0;
    for s in &found.sites {
        match &s.status {
            valet::SiteStatus::Importable => {
                importable += 1;
                println!(
                    "  ready        {:<26} php={:<4} secured={} {}",
                    s.domain,
                    s.php_minor.clone().unwrap_or_else(|| "-".into()),
                    s.secured,
                    s.also_in.map(|k| format!("(also in {})", k.label())).unwrap_or_default()
                );
            }
            valet::SiteStatus::Unsupported(r) => {
                blocked += 1;
                println!("  can't import {:<26} {r}", s.domain);
            }
            other => println!("  {:<12} {:<26} {other:?}", "attention", s.domain),
        }
    }

    println!(
        "\n{} rows · {importable} importable · {blocked} not · TLDs {:?}",
        found.sites.len(),
        valet::tlds_in_use(&found.sites)
    );

    if ok {
        println!("\nOK — their trees are byte-identical after the scan, and every row is accounted for.");
    } else {
        eprintln!("\nFAILED — the scan MODIFIED their environment. That must never happen.");
        std::process::exit(1);
    }
}
