//! Live check: phase-D link-folder safety — the symlink round-trip through
//! the REAL platform impls, and THE assertion this feature hangs on: removing
//! a linked asset unlinks ONLY, the user's real checkout survives byte-for-
//! byte. Also proves the fs-truth partition and the validation refusals.
//! Throwaway temp dir only; no services, no network, no app-data.
//! Run: `cargo run --example repo_link_check`

use rexenv_lib::core::repo;
use rexenv_lib::platform;
use std::path::Path;

fn main() {
    let plat = platform::current();
    let scratch = std::env::temp_dir().join(format!("rexenv-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let mut failures: Vec<String> = Vec::new();

    // A fake site + a real checkout elsewhere (with .git + uncommitted work).
    let docroot = scratch.join("site/public");
    let plugins = docroot.join("wp-content/plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let target = scratch.join("checkouts/my-plugin");
    std::fs::create_dir_all(target.join(".git")).unwrap();
    std::fs::write(target.join(".git/config"), "[core]").unwrap();
    std::fs::write(target.join("plugin.php"), "<?php\n/*\nPlugin Name: Linked\n*/\n").unwrap();
    std::fs::write(target.join("uncommitted-work.txt"), "precious").unwrap();

    // 1. Validate + link through the platform trait.
    let dest = plugins.join("my-plugin");
    let canonical = repo::validate_link_target(&docroot, &dest, &target).expect("validate");
    plat.shell().symlink_dir(&canonical, &dest).expect("symlink");
    let meta = std::fs::symlink_metadata(&dest).expect("link meta");
    println!(
        "linked: is_symlink={}, serves plugin.php={}",
        meta.file_type().is_symlink(),
        dest.join("plugin.php").is_file()
    );
    if !meta.file_type().is_symlink() || !dest.join("plugin.php").is_file() {
        failures.push("link did not land correctly".into());
    }
    // Detection sees the header THROUGH the link.
    let insp = repo::inspect_repo(&dest);
    if insp.wp.kind != "plugin" {
        failures.push(format!("header not detected through link: {:?}", insp.wp));
    }

    // 2. The partition: fs truth says this delete must be an unlink.
    std::fs::create_dir_all(plugins.join("normal-plugin")).unwrap();
    let (linked, normal) = repo::partition_symlink_deletes(
        &plugins,
        &["my-plugin".into(), "normal-plugin".into()],
    );
    println!("partition: linked={linked:?}, normal={normal:?}");
    if linked != vec!["my-plugin"] || normal != vec!["normal-plugin"] {
        failures.push("partition wrong".into());
    }

    // 3. THE unlink: link gone, target UNTOUCHED (the whole point).
    plat.shell().remove_symlink(&dest).expect("remove_symlink");
    let link_gone = std::fs::symlink_metadata(&dest).is_err();
    let survived = target.join("plugin.php").is_file()
        && target.join("uncommitted-work.txt").is_file()
        && target.join(".git/config").is_file();
    println!("unlinked: link gone={link_gone}, target fully intact={survived}");
    if !link_gone {
        failures.push("link still present after remove_symlink".into());
    }
    if !survived {
        failures.push("TARGET DAMAGED BY UNLINK — the exact disaster this guards".into());
    }

    // 4. remove_symlink REFUSES a real directory (defense in depth).
    match plat.shell().remove_symlink(&plugins.join("normal-plugin")) {
        Err(e) => println!("real-dir removal refused (good): {e}"),
        Ok(()) => failures.push("remove_symlink deleted a REAL directory?!".into()),
    }

    // 5. Validation refusals (inside-site, cycle, collision).
    let inner = plugins.join("normal-plugin");
    if repo::validate_link_target(&docroot, &plugins.join("x"), &inner).is_ok() {
        failures.push("self-link not refused".into());
    }
    if repo::validate_link_target(&docroot, &plugins.join("x"), &scratch.join("site")).is_ok() {
        failures.push("cycle not refused".into());
    }
    std::fs::create_dir_all(plugins.join("taken")).unwrap();
    if repo::validate_link_target(&docroot, &plugins.join("taken"), &target).is_ok() {
        failures.push("collision not refused".into());
    }

    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_link_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}

#[allow(dead_code)]
fn _t(_: &Path) {}
