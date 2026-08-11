//! Live check for **creating a site from a git repository** (Stage 1 of
//! `docs/PLAN-git-site-clone.md`). Run:
//! `cargo run --example git_site_clone_check`
//!
//! Hermetic on purpose: the "remotes" are bare repositories this example builds
//! in its own temp dir, so it needs no network, no credentials, and no service.
//! What it exercises is the part unit tests cannot — real `git clone` against a
//! real remote, and the staging → `remove_dir` → `rename` move that lands the
//! checkout in the site's own docroot.
//!
//! Proves:
//!   1. A Laravel repo clones INTO the docroot, is detected as Laravel, and
//!      reports `public/` as the folder to serve.
//!   2. `.env` is created from the repo's `.env.example` and wired to this
//!      site's database — with the example file's own keys intact.
//!   3. A docroot that already holds something is REFUSED, and the refusal
//!      deletes nothing.
//!   4. A repo whose shape is not the chosen type is NAMED, the docroot is left
//!      empty, and **no staging directory survives** — the cleanup path that
//!      only runs when the clone SUCCEEDED and the verification then failed.
//!   5. A failed clone (a ref that does not exist) leaves the same clean state.
//!   6. Creating a Blank-PHP site from a repo does NOT write the phpinfo probe
//!      page — which would make prepare block its own clone phase.
//!
//! Everything written lives under this example's own temp root or the sandbox
//! app-data root; nothing is derived from the real `Paths` (examples/common).

use rexenv_lib::core::{devtools, laravel, repo, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

/// One fixture "remote": a real repository with a real commit.
fn make_repo(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("fixture repo dir");
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent");
        }
        std::fs::write(&path, body).expect("fixture file");
    }
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&dir)
            // A developer's global git config may set anything; pin the bits
            // the commit needs so this works on any machine.
            .env("GIT_AUTHOR_NAME", "rexenv checks")
            .env("GIT_AUTHOR_EMAIL", "checks@rexenv.invalid")
            .env("GIT_COMMITTER_NAME", "rexenv checks")
            .env("GIT_COMMITTER_EMAIL", "checks@rexenv.invalid")
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "fixture"]);
    dir
}

/// Leftover staging directories beside a docroot — must always be empty.
fn staging_leftovers(sites_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(sites_dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(".rexenv-clone-"))
        .collect()
}

fn entries(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<String> =
        rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect();
    v.sort();
    v
}

/// A `.env.example` in the shape a real Laravel repo ships: `DB_CONNECTION`
/// live and SQLITE, the MySQL keys commented out. The friendlier fixture (a
/// tidy live MySQL block) would not exercise the half that decides whether the
/// app talks to the database this site advertises.
const ENV_EXAMPLE: &str = "APP_NAME=Shop\n\
     APP_ENV=local\n\
     APP_KEY=\n\
     APP_URL=http://localhost\n\
     \n\
     DB_CONNECTION=sqlite\n\
     # DB_HOST=127.0.0.1\n\
     # DB_PORT=3306\n\
     # DB_DATABASE=laravel\n\
     # DB_USERNAME=root\n\
     # DB_PASSWORD=\n\
     \n\
     MAIL_MAILER=log\n";

fn main() {
    // Sandboxed: certificates, config and app data land in a throwaway root, so
    // this example cannot touch the running stack. See examples/common.
    let (plat, _sandbox) = common::sandbox("git_site_clone_check");
    let mut ok = true;

    let scratch =
        std::env::temp_dir().join(format!("rexenv-git-site-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("scratch root");
    // Every path this example removes is built from `scratch` — never from a
    // docroot's parent, and never from the real `Paths` (the incident in
    // examples/common's header).
    let _cleanup = ScratchGuard(scratch.clone());

    let env = plat.shell().login_shell_env().expect("login_shell_env");
    let git = devtools::resolve_git(&*plat, &env).expect("git resolves").path;
    println!("git = {}\n", git.display());

    let remotes = scratch.join("remotes");
    let laravel_remote = make_repo(
        &remotes,
        "shop",
        &[
            ("artisan", "#!/usr/bin/env php\n<?php // fixture\n"),
            ("public/index.php", "<?php echo 'shop';\n"),
            ("composer.json", "{\"name\":\"acme/shop\"}\n"),
            (".env.example", ENV_EXAMPLE),
            (".gitignore", ".env\n/vendor\n"),
        ],
    );
    let plain_remote =
        make_repo(&remotes, "tools", &[("index.php", "<?php echo 'tools';\n")]);

    // The sites folder these fixture docroots live in — ours, not the app's.
    let sites_dir = scratch.join("Sites");
    std::fs::create_dir_all(&sites_dir).expect("fixture sites dir");
    let cancel = repo::CancelToken::new();
    let mut sink = |line: &str| println!("   {line}");

    // ── 1. A Laravel repo lands in the docroot ──────────────────────────
    println!("=== 1. clone a Laravel repo into an empty docroot ===");
    let docroot = sites_dir.join("shop.rex");
    std::fs::create_dir_all(&docroot).expect("docroot");
    let src = sites::GitSource {
        url: laravel_remote.to_string_lossy().into_owned(),
        git_ref: Some("main".into()),
    };
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &src,
        &docroot,
        SiteType::Laravel,
        &cancel,
        &mut sink,
    ) {
        Ok(detected) => {
            println!("   detected {} · serving {:?}", detected.label, detected.docroot_rel);
            if detected.site_type != SiteType::Laravel {
                ok = fail("detected type is not Laravel");
            }
            if detected.docroot_rel != "public" {
                ok = fail(&format!("docroot_rel is {:?}, expected \"public\"", detected.docroot_rel));
            }
            if !docroot.join("artisan").is_file() || !docroot.join("public/index.php").is_file() {
                ok = fail("the checkout did not land in the docroot");
            }
            if !docroot.join(".git").exists() {
                ok = fail(".git is missing — the clone phase's skip test would never fire");
            }
        }
        Err(e) => ok = fail(&format!("clone failed: {e}")),
    }
    if !staging_leftovers(&sites_dir).is_empty() {
        ok = fail(&format!("staging survived a SUCCESSFUL clone: {:?}", staging_leftovers(&sites_dir)));
    }

    // ── 2. .env from the repo's example, wired to this site ─────────────
    println!("\n=== 2. .env from .env.example, wired to the site's database ===");
    match laravel::ensure_env_file(&docroot) {
        Ok(laravel::EnvOrigin::Example) => {}
        Ok(other) => ok = fail(&format!(".env came from {other:?}, expected the repo's example")),
        Err(e) => ok = fail(&format!("ensure_env_file: {e}")),
    }
    let original = std::fs::read_to_string(laravel::env_path(&docroot)).expect("read .env");
    let wired = laravel::wire_env(
        &original,
        "https://shop.rex",
        &laravel::DbSettings {
            connection: "mysql".into(),
            host: "127.0.0.1".into(),
            port: 13306,
            database: "lv_shop_rex".into(),
            username: "root".into(),
            password: String::new(),
        },
    );
    std::fs::write(laravel::env_path(&docroot), &wired).expect("write .env");
    for want in ["DB_CONNECTION=mysql", "DB_DATABASE=lv_shop_rex", "APP_URL=https://shop.rex", "MAIL_MAILER=log"] {
        if !wired.contains(want) {
            ok = fail(&format!(".env is missing {want}"));
        }
    }
    for unwanted in ["sqlite", "# DB_", "http://localhost"] {
        if wired.contains(unwanted) {
            ok = fail(&format!(".env still contains {unwanted:?} — a second answer for a key"));
        }
    }
    println!("   .env wired, the example's own MAIL_MAILER survived");

    // ── 3. A non-empty docroot is refused, and nothing is deleted ───────
    println!("\n=== 3. a docroot with contents is refused ===");
    let before = entries(&docroot);
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &src,
        &docroot,
        SiteType::Laravel,
        &cancel,
        &mut sink,
    ) {
        Ok(_) => ok = fail("cloning into a NON-EMPTY docroot succeeded — it must refuse"),
        Err(e) => {
            println!("   refused: {e}");
            if !e.to_string().contains("not empty") {
                ok = fail("the refusal does not say the folder is not empty");
            }
        }
    }
    if entries(&docroot) != before {
        ok = fail("the refusal changed the docroot's contents");
    }
    if !docroot.join("artisan").is_file() {
        ok = fail("the refusal deleted files — the whole point of remove_dir");
    }

    // ── 4. Wrong shape: named, cleaned up, docroot left empty ───────────
    println!("\n=== 4. a repo that is not the chosen type ===");
    let mismatch = sites_dir.join("wrong.rex");
    std::fs::create_dir_all(&mismatch).expect("docroot");
    let plain = sites::GitSource {
        url: plain_remote.to_string_lossy().into_owned(),
        git_ref: None,
    };
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &plain,
        &mismatch,
        SiteType::Laravel,
        &cancel,
        &mut sink,
    ) {
        Ok(_) => ok = fail("a plain PHP repo was accepted as a Laravel site"),
        Err(e) => {
            println!("   refused: {e}");
            // The message must name what was FOUND and what to create instead;
            // assert on that actionable half rather than on exact prose.
            let msg = e.to_string();
            if !msg.contains("PHP project") || !msg.contains("Create the site as") {
                ok = fail("the mismatch message must name the shape found and the way out");
            }
        }
    }
    if !mismatch.is_dir() || !entries(&mismatch).is_empty() {
        ok = fail(&format!("the docroot must be left empty, found {:?}", entries(&mismatch)));
    }
    let left = staging_leftovers(&sites_dir);
    if !left.is_empty() {
        ok = fail(&format!("staging survived a verified-then-rejected clone: {left:?}"));
    }
    println!("   docroot left empty, no staging directory behind");

    // ── 5. A failed clone leaves the same clean state ───────────────────
    println!("\n=== 5. a ref that does not exist ===");
    let badref = sites_dir.join("badref.rex");
    std::fs::create_dir_all(&badref).expect("docroot");
    let ghost = sites::GitSource {
        url: laravel_remote.to_string_lossy().into_owned(),
        git_ref: Some("no-such-branch".into()),
    };
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &ghost,
        &badref,
        SiteType::Laravel,
        &cancel,
        &mut sink,
    ) {
        Ok(_) => ok = fail("a missing branch cloned successfully?!"),
        Err(e) => println!("   refused: {e}"),
    }
    if !badref.is_dir() || !entries(&badref).is_empty() {
        ok = fail("a failed clone left the docroot dirty");
    }
    if !staging_leftovers(&sites_dir).is_empty() {
        ok = fail("staging survived a FAILED clone");
    }

    // ── 6. The probe page never blocks its own clone ────────────────────
    println!("\n=== 6. a Blank-PHP site from a repo gets no phpinfo page ===");
    let db_path = scratch.join("git-site-check.db");
    let conn = db::open(&db_path).expect("db");
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("ca");
    let site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "Tools".into(),
            domain: "gitcheck-tools.test".into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            // A URL the parser accepts — `provision` validates and creates the
            // folder; it never clones, which is the clone phase's job.
            git_url: "acme/tools".into(),
            git_ref: None,
        },
    )
    .expect("provision a cloning php site");
    let created = PathBuf::from(&site.path);
    if !entries(&created).is_empty() {
        ok = fail(&format!(
            "a cloning site's docroot must be EMPTY, found {:?} — the clone would refuse it",
            entries(&created)
        ));
    }
    if site.git_url.as_deref() != Some("https://github.com/acme/tools.git") {
        ok = fail(&format!("the row must record the normalized repo, got {:?}", site.git_url));
    }
    println!("   docroot empty, repo recorded as {:?}", site.git_url);

    println!("\n{}", if ok { "git_site_clone_check: PASS" } else { "git_site_clone_check: FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

fn fail(msg: &str) -> bool {
    println!("   ✗ {msg}");
    false
}

/// Removes ONLY the directory this example created, from a Drop so an
/// `expect` between here and the end cannot skip it.
struct ScratchGuard(PathBuf);
impl Drop for ScratchGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
