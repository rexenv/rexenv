//! Live check for `BinaryProvider::prepare_binary_tree` — that "in-tree" means
//! the load command RESOLVES inside the bundle, not that it is spelled with a
//! `@loader_path/` prefix.
//!
//! `cargo run --example relink_tree_check`
//!
//! ## Why this example exists
//!
//! `needs_tree_relink` used to return false for anything starting `@loader_path/`.
//! Every non-system dep of the shivammathur `php@7.4` bottle is spelled
//! `@loader_path/../../../../opt/<formula>/lib/…`, which **escapes** the bundle —
//! so both the rewrite loop and the post-relink VERIFY loop skipped them,
//! `prepare_binary_tree` reported success over 49 Mach-Os, and the published
//! binary died in dyld. Worse, it dies unrecoverably: `resolve_bundle`'s early
//! return only checks that the `member` file EXISTS, so the dead tree caches and
//! every later resolve short-circuits to it (`docs/archive/PLAN-php-74-support.md` §5.2/§5.3).
//!
//! The unit test proves the predicate. This proves the PROVIDER — real Mach-O,
//! real `install_name_tool`, real `codesign`, real `otool` — because the bug was
//! never in the arithmetic, it was in what the tool chain was asked to look at.
//!
//! ## Fixture scope (the examples/common invariant)
//!
//! Writes ONLY into a temp dir this example creates and removes. Spawns no
//! service, touches no app data, needs no network — hence the `sandbox` tier.
//! The Mach-O under test is a copy of this example's own binary, so the check
//! works on any machine with the Xcode command line tools and nothing else.

use rexenv_lib::platform;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `otool -L` deps of a Mach-O, excluding its own ID line.
fn deps(path: &Path) -> Vec<String> {
    let out = Command::new("otool").arg("-L").arg(path).output().expect("otool");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

fn change(path: &Path, from: &str, to: &str) {
    let st = Command::new("install_name_tool")
        .args(["-change", from, to])
        .arg(path)
        .output()
        .expect("install_name_tool");
    assert!(st.status.success(), "install_name_tool: {}", String::from_utf8_lossy(&st.stderr));
}

/// A bundle-shaped tree with one real Mach-O at `bin/probe`, whose first
/// `/usr/lib` dep has been rewritten to `escaping`.
fn fixture(root: &Path, escaping: &str) -> PathBuf {
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    let probe = root.join("bin/probe");
    std::fs::copy(std::env::current_exe().unwrap(), &probe).unwrap();
    let victim = deps(&probe)
        .into_iter()
        .find(|d| d.starts_with("/usr/lib/"))
        .expect("the example binary links at least one /usr/lib dylib");
    change(&probe, &victim, escaping);
    assert!(deps(&probe).iter().any(|d| d == escaping), "fixture did not take");
    probe
}

fn main() {
    let plat = platform::current();
    let root = std::env::temp_dir().join(format!("rexenv-relink-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // The exact shape read off the php@7.4 bottle: four levels up, then back
    // down into a Homebrew opt prefix that is nowhere near the bundle.
    const ESCAPING: &str = "@loader_path/../../../../opt/fixture/lib/libfixture.dylib";

    // ── Leg A: an escaping @loader_path whose dylib is NOT bundled must be a
    //    LOUD error. Before the fix this returned Ok and published a dead tree.
    let a = root.join("a");
    fixture(&a, ESCAPING);
    match plat.binaries().prepare_binary_tree(&a) {
        Ok(()) => {
            println!("✗ FAIL: an escaping @loader_path was accepted — this is the shipped-dead-tree bug");
            std::process::exit(1);
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("libfixture.dylib"),
                "the error must name the missing dylib: {msg}"
            );
            println!("✓ A. escaping @loader_path, dylib absent → refused: {msg}");
        }
    }

    // ── Leg B: the same escaping name, but the dylib IS in the bundle. The tree
    //    must be REPAIRED — rewritten to an in-tree @loader_path — not merely
    //    tolerated. This is what makes the fix a fix rather than a new refusal.
    let b = root.join("b");
    let probe_b = fixture(&b, ESCAPING);
    std::fs::copy(std::env::current_exe().unwrap(), b.join("lib/libfixture.dylib")).unwrap();
    plat.binaries()
        .prepare_binary_tree(&b)
        .expect("a bundled dep must relink, not refuse");
    let after = deps(&probe_b);
    assert!(
        after.iter().any(|d| d == "@loader_path/../lib/libfixture.dylib"),
        "the escaping name was not repaired: {after:?}"
    );
    assert!(
        !after.iter().any(|d| d.contains("/opt/fixture/")),
        "the escaping name survived: {after:?}"
    );
    println!("✓ B. escaping @loader_path, dylib bundled → rewritten to @loader_path/../lib/…");

    // ── Leg C: the control. A load command that already resolves inside the
    //    tree is left ALONE — otherwise leg A would pass for the boring reason
    //    that the predicate rejects everything.
    let c = root.join("c");
    let probe_c = fixture(&c, "@loader_path/../lib/libfixture.dylib");
    std::fs::copy(std::env::current_exe().unwrap(), c.join("lib/libfixture.dylib")).unwrap();
    plat.binaries()
        .prepare_binary_tree(&c)
        .expect("an already-in-tree dep must pass untouched");
    assert!(
        deps(&probe_c).iter().any(|d| d == "@loader_path/../lib/libfixture.dylib"),
        "an in-tree dep was disturbed"
    );
    println!("✓ C. in-tree @loader_path → untouched (so A is not rejecting everything)");

    // ── Leg D: a nested Mach-O's in-tree path climbs the right number of levels,
    //    which is the case the four-dot bottle strings look like from below.
    let d = root.join("d");
    std::fs::create_dir_all(d.join("lib/modules")).unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    let nested = d.join("lib/modules/probe.so");
    std::fs::copy(std::env::current_exe().unwrap(), &nested).unwrap();
    let victim = deps(&nested).into_iter().find(|x| x.starts_with("/usr/lib/")).unwrap();
    change(&nested, &victim, ESCAPING);
    std::fs::copy(std::env::current_exe().unwrap(), d.join("lib/libfixture.dylib")).unwrap();
    plat.binaries().prepare_binary_tree(&d).expect("nested relink");
    assert!(
        deps(&nested).iter().any(|x| x == "@loader_path/../../lib/libfixture.dylib"),
        "nested Mach-O got the wrong number of levels: {:?}",
        deps(&nested)
    );
    println!("✓ D. nested Mach-O → @loader_path/../../lib/… (levels counted from the tree, not the string)");

    let _ = std::fs::remove_dir_all(&root);
    println!("\nrelink_tree_check: all green");
}
