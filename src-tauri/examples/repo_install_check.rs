//! Live check: detect → composer/npm install → build against REAL local
//! fixture repos, through the exact pipeline the add-from-Git feature uses
//! (clone from a local repo, streamed steps, pinned composer.phar on the
//! bundled PHP). Networked (packagist + npm registry + the composer.phar
//! download on first run); writes only a throwaway temp dir + the shared
//! binary cache (by design). No services touched.
//! Run: `cargo run --example repo_install_check`

use rexenv_lib::core::{binaries, devtools, php, repo};
use rexenv_lib::platform;
use std::path::Path;

fn git_fixture(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.email=fixtures@rexenv.test",
            "-c",
            "user.name=rexenv fixtures",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        let st = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(&args)
            .status()
            .expect("git");
        assert!(st.success(), "git {args:?} failed in {}", dir.display());
    }
}

const PLUGIN_PHP: &str = "<?php\n/*\nPlugin Name: Repo Fixture\n*/\n";

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let scratch =
        std::env::temp_dir().join(format!("rexenv-repo-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let mut failures: Vec<String> = Vec::new();

    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let sup = plat.supervisor();

    // Tools: user's node/npm; OUR php (site PHP stand-in: pinned 8.3) + the
    // NEW pinned composer.phar (downloads into the real cache on first run).
    let git = devtools::resolve_git(&*plat, &env).expect("git").path;
    let node = devtools::resolve_node(&env).expect("node");
    let npm = devtools::resolve_package_manager(&env, "npm").expect("npm").path;
    let patch = php::patch_for_minor("8.3").expect("8.3 pinned");
    let php_bin = binaries::resolve(&*plat, "php", patch).await.expect("bundled php");
    let composer = binaries::resolve_file(&*plat, "composer", binaries::COMPOSER_VERSION)
        .await
        .expect("pinned composer.phar");
    println!("node = {} ({:?})", node.path.display(), node.version);
    println!("php  = {}", php_bin.display());
    println!("composer.phar = {}\n", composer.display());

    // ---- Fixture A: composer-only plugin, cloned through clone_repo ----
    let src_a = scratch.join("src-composer");
    git_fixture(
        &src_a,
        &[
            ("plugin.php", PLUGIN_PHP),
            ("composer.json", r#"{"require":{"psr/log":"^3.0"}}"#),
        ],
    );
    let a = scratch.join("checkout-composer");
    let cancel = repo::CancelToken::new();
    let mut lines = 0u32;
    print!("A: clone local fixture … ");
    match repo::clone_repo(
        sup,
        &git,
        &env,
        &src_a.to_string_lossy(),
        None,
        &a,
        &cancel,
        &mut |_| lines += 1,
    ) {
        Ok(()) => println!("ok ({lines} lines)"),
        Err(e) => failures.push(format!("A clone failed: {e}")),
    }
    let insp = repo::inspect_repo(&a);
    println!(
        "A: inspect → composer={} node={:?} wp={}/{:?}",
        insp.composer,
        insp.node.as_ref().map(|n| &n.manager),
        insp.wp.kind,
        insp.wp.name
    );
    if !insp.composer || insp.node.is_some() || insp.wp.kind != "plugin" {
        failures.push("A inspection wrong".into());
    }
    print!("A: composer install … ");
    let mut clines = 0u32;
    match repo::composer_install(sup, &php_bin, &composer, &a, &env, &cancel, &mut |_| {
        clines += 1
    }) {
        Ok(()) => {
            let vendor = a.join("vendor/psr/log").is_dir() && a.join("vendor/autoload.php").is_file();
            println!("ok ({clines} lines), vendor/psr/log + autoload.php present = {vendor}");
            if !vendor {
                failures.push("A vendor/ not produced".into());
            }
        }
        Err(e) => failures.push(format!("A composer install failed: {e}")),
    }

    // ---- Fixture B: npm plugin with a build script ----
    let b = scratch.join("checkout-node");
    git_fixture(
        &b,
        &[
            ("plugin.php", PLUGIN_PHP),
            (
                "package.json",
                r#"{"name":"repo-fixture","version":"1.0.0","dependencies":{"is-odd":"^3.0.1"},"scripts":{"build":"node build.js"}}"#,
            ),
            (
                "build.js",
                "require('is-odd');const fs=require('fs');fs.mkdirSync('build',{recursive:true});fs.writeFileSync('build/out.js','ok');",
            ),
        ],
    );
    let insp = repo::inspect_repo(&b);
    let plan = insp.node.clone().expect("node plan");
    println!(
        "\nB: inspect → manager={} (by {}), has_build={}",
        plan.manager, plan.pinned_by, plan.has_build
    );
    if plan.manager != "npm" || !plan.has_build {
        failures.push("B inspection wrong".into());
    }
    print!("B: npm install … ");
    let mut nlines = 0u32;
    match repo::node_install(sup, &npm, &b, &env, &cancel, &mut |_| nlines += 1) {
        Ok(()) => {
            let dep = b.join("node_modules/is-odd").is_dir();
            println!("ok ({nlines} lines), node_modules/is-odd present = {dep}");
            if !dep {
                failures.push("B node_modules not produced".into());
            }
        }
        Err(e) => failures.push(format!("B npm install failed: {e}")),
    }
    print!("B: npm run build … ");
    match repo::node_build(sup, &npm, &b, &env, &cancel, &mut |_| {}) {
        Ok(()) => {
            let out = std::fs::read_to_string(b.join("build/out.js")).unwrap_or_default();
            println!("ok, build/out.js = {out:?}");
            if out != "ok" {
                failures.push("B build artifact wrong".into());
            }
        }
        Err(e) => failures.push(format!("B build failed: {e}")),
    }

    // ---- Fixture C: both managers + .nvmrc mismatch warning ----
    let c = scratch.join("checkout-both");
    git_fixture(
        &c,
        &[
            ("plugin.php", PLUGIN_PHP),
            ("composer.json", r#"{"require":{}}"#),
            ("package.json", r#"{"scripts":{"build":"true"}}"#),
            (".nvmrc", "18\n"),
        ],
    );
    let insp = repo::inspect_repo(&c);
    let have = node.version.clone().unwrap_or_default();
    let warn = insp.node_want.as_deref().and_then(|w| repo::node_version_warning(w, &have));
    println!("\nC: inspect → composer={} node={:?} want={:?}", insp.composer, insp.node.is_some(), insp.node_want);
    match &warn {
        Some(w) => println!("C: node warning fires (good): {w}"),
        None => failures.push(format!("C: no node warning for .nvmrc=18 vs {have}")),
    }

    // ---- Failure mapping, live ----
    let f = scratch.join("checkout-fail");
    git_fixture(
        &f,
        &[
            ("plugin.php", PLUGIN_PHP),
            ("composer.json", r#"{"require":{"php":">=9.0"}}"#),
        ],
    );
    print!("\nF: composer install (requires php >=9.0) … ");
    match repo::composer_install(sup, &php_bin, &composer, &f, &env, &cancel, &mut |_| {}) {
        Ok(()) => failures.push("F: impossible php requirement installed Ok?!".into()),
        Err(e) => {
            let msg = e.to_string();
            println!("errored (good):\n  {}", msg.lines().next().unwrap_or(""));
            if !msg.contains("Switch the site's PHP version") {
                failures.push(format!("F: composer error not mapped: {msg}"));
            }
        }
    }
    // Missing tool → honest mapped error (bun assumed absent; skip if present).
    if devtools::find_optional(&env, "bun").is_none() {
        let err = devtools::resolve_package_manager(&env, "bun").unwrap_err().to_string();
        println!("F: missing bun maps to: {}", err.lines().last().unwrap_or(""));
        if !err.contains("brew install") {
            failures.push(format!("F: bun error unmapped: {err}"));
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
    println!();
    if failures.is_empty() {
        println!("repo_install_check: ALL PASS");
    } else {
        for f in &failures {
            println!("FAIL: {f}");
        }
        std::process::exit(1);
    }
}
