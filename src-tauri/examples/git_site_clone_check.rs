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
//!   7. The front-end asset phase's inputs are read correctly off a real
//!      checkout: the repo's `packageManager` field beats its lockfile, the
//!      build script is seen, and `node_modules` is NOT in the clone.
//!  10. Both WordPress layouts are placed correctly: a stock repo serves its
//!      root with `wp-content`, a Bedrock one serves `web/` with `app` — and
//!      only the Bedrock one turns off core_download + `wp config create`. Its
//!      `.env` is wired over the repository's own example with eight salts,
//!      one line per key.
//!   9. Any PHP repository works as a Blank-PHP site: a Symfony checkout is
//!      named, served from its own `public/`, and arrives without `vendor/` —
//!      which is what the deps phase exists for.
//!   8. The Repository panel's reads and writes work at the PROJECT root —
//!      status/branch, the loss warning that gates a branch switch, fetch and
//!      checkout — and `.git` is above the folder the web server serves.
//!
//! What it does NOT prove: the asset build itself. `<manager> install` /
//! `run build` are the developer's own toolchain and are covered against real
//! binaries by `repo_run_all_check` (network tier); the non-fatal outcome the
//! provisioning phase wraps them in is a `docs/SMOKE-TEST.md` item.
//!
//! Everything written lives under this example's own temp root or the sandbox
//! app-data root; nothing is derived from the real `Paths` (examples/common).

use rexenv_lib::core::{devtools, dotenv, laravel, mail, repo, sites, ssl};
use rexenv_lib::state::db;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

/// Run one git command in `dir`, pinning the identity a commit needs so this
/// works whatever the developer's global config says.
fn git_in(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "rexenv checks")
        .env("GIT_AUTHOR_EMAIL", "checks@rexenv.invalid")
        .env("GIT_COMMITTER_NAME", "rexenv checks")
        .env("GIT_COMMITTER_EMAIL", "checks@rexenv.invalid")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

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
    git_in(&dir, &["init", "--quiet", "--initial-branch=main"]);
    git_in(&dir, &["add", "-A"]);
    git_in(&dir, &["commit", "--quiet", "-m", "fixture"]);
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
    // A second branch on the shop remote — the Repository panel's checkout is
    // only meaningful against a remote that has somewhere to go.
    git_in(&laravel_remote, &["branch", "develop"]);
    let plain_remote =
        make_repo(&remotes, "tools", &[("index.php", "<?php echo 'tools';\n")]);
    // A Laravel repo with a Vite front end, in the shape one really ships:
    // `packageManager` pinned, a lockfile that disagrees with it (the field is
    // authoritative — a fixture where both agree proves nothing), and a build
    // script. `node_modules` is gitignored, which is the whole reason the
    // assets phase exists.
    let vite_remote = make_repo(
        &remotes,
        "storefront",
        &[
            ("artisan", "#!/usr/bin/env php\n<?php // fixture\n"),
            ("public/index.php", "<?php echo 'storefront';\n"),
            ("composer.json", "{\"name\":\"acme/storefront\"}\n"),
            (".env.example", ENV_EXAMPLE),
            (
                "package.json",
                "{\"packageManager\":\"pnpm@9.1.0\",\
                  \"scripts\":{\"build\":\"vite build\",\"dev\":\"vite\"}}\n",
            ),
            ("package-lock.json", "{\"lockfileVersion\":3}\n"),
            (".gitignore", ".env\n/vendor\n/node_modules\n/public/build\n"),
        ],
    );

    // A Symfony repo — the "any PHP repository" case. Its front controller is
    // `public/index.php` like Laravel's, but there is no `artisan`, so only
    // `detect_project` can tell them apart. `vendor/` is gitignored, which is
    // the whole reason a cloned Blank-PHP site needs a deps phase.
    let symfony_remote = make_repo(
        &remotes,
        "invoices",
        &[
            ("bin/console", "#!/usr/bin/env php\n<?php // fixture\n"),
            ("public/index.php", "<?php echo 'invoices';\n"),
            ("composer.json", "{\"name\":\"acme/invoices\"}\n"),
            (".gitignore", "/vendor\n"),
        ],
    );

    // Two WordPress shapes. A STOCK repo that gitignores core and wp-config
    // (the common one — the code is a theme plus a few plugins), and a BEDROCK
    // one, where Composer owns core and `.env` owns the configuration.
    let stock_wp_remote = make_repo(
        &remotes,
        "blog",
        &[
            ("wp-config-sample.php", "<?php // fixture\n"),
            ("wp-content/themes/acme/style.css", "/* Theme Name: Acme */\n"),
            (".gitignore", "/wp-config.php\n/wp-admin\n/wp-includes\n"),
        ],
    );
    let bedrock_remote = make_repo(
        &remotes,
        "roots",
        &[
            ("web/wp-config.php", "<?php require_once dirname(__DIR__).'/config/application.php';\n"),
            ("config/application.php", "<?php // fixture\n"),
            ("web/app/themes/acme/style.css", "/* Theme Name: Acme */\n"),
            ("composer.json", "{\"name\":\"acme/roots\"}\n"),
            (".env.example", "DB_NAME=\nDB_USER=\nDB_PASSWORD=\n# DB_HOST=localhost\nWP_HOME=http://example.com\nWP_SITEURL=${WP_HOME}/wp\nAUTH_KEY=\nSECURE_AUTH_KEY=\nLOGGED_IN_KEY=\nNONCE_KEY=\nAUTH_SALT=\nSECURE_AUTH_SALT=\nLOGGED_IN_SALT=\nNONCE_SALT=\n"),
            (".gitignore", ".env\n/vendor\n/web/wp\n"),
        ],
    );

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
        Ok(dotenv::EnvOrigin::Example) => {}
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
        true,
    );
    std::fs::write(laravel::env_path(&docroot), &wired).expect("write .env");
    // `MAIL_MAILER=log` used to be asserted here as "the example's own keys
    // survive". That reading was the bug (ledger #504): a cloned app whose
    // `.env` says `log` — or names a real SMTP provider — must still land its
    // mail in Mailpit, and the file half is what makes the catch survive
    // `php artisan config:cache`.
    let mut want: Vec<String> = vec![
        "DB_CONNECTION=mysql".into(),
        "DB_DATABASE=lv_shop_rex".into(),
        "APP_URL=https://shop.rex".into(),
    ];
    want.extend(mail::laravel_env().into_iter().map(|(k, v)| format!("{k}={v}")));
    for w in &want {
        if !wired.contains(w.as_str()) {
            ok = fail(&format!(".env is missing {w}"));
        }
    }
    for unwanted in ["sqlite", "# DB_", "http://localhost", "MAIL_MAILER=log"] {
        if wired.contains(unwanted) {
            ok = fail(&format!(".env still contains {unwanted:?} — a second answer for a key"));
        }
    }
    println!("   .env wired, and its mail points at Mailpit whatever the repo shipped");

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
    // Pin `sites_dir` into the sandbox: `sites::provision` reads the SETTING,
    // which falls back to the home directory, so without this the docroots land
    // in the user's real ~/rexenv/Sites (14 Aug 2026 sweep).
    common::pin_sites_dir(&conn, &*plat);
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
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
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

    // ── 7. What the assets phase reads off a real checkout ──────────────
    println!("\n=== 7. front-end assets detected from the cloned tree ===");
    let storefront = sites_dir.join("storefront.rex");
    std::fs::create_dir_all(&storefront).expect("docroot");
    let vite = sites::GitSource {
        url: vite_remote.to_string_lossy().into_owned(),
        git_ref: Some("main".into()),
    };
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &vite,
        &storefront,
        SiteType::Laravel,
        &cancel,
        &mut sink,
    ) {
        Ok(_) => {
            // The phase runs THIS inspection against the real checkout — the
            // manager is the repo's own answer, never a rexenv preference.
            let inspection = repo::inspect_repo(&storefront);
            match &inspection.node {
                None => ok = fail("no package.json seen in a repo that ships one"),
                Some(node) => {
                    println!(
                        "   {} (pinned by {}) · build script: {}",
                        node.manager, node.pinned_by, node.has_build
                    );
                    if node.manager != "pnpm" || node.pinned_by != "packageManager" {
                        ok = fail("the repo's `packageManager` field must beat its lockfile");
                    }
                    if !node.has_build {
                        ok = fail("the build script was not detected");
                    }
                }
            }
            if !inspection.composer {
                ok = fail("composer.json not seen — the deps phase would skip silently");
            }
            // node_modules is gitignored: the clone lands WITHOUT it, which is
            // exactly why the phase is needed rather than optional polish.
            if storefront.join("node_modules").exists() {
                ok = fail("node_modules arrived in the clone?! the fixture is unrealistic");
            }
        }
        Err(e) => ok = fail(&format!("clone failed: {e}")),
    }

    // ── 8. The Repository panel's reads and writes, at the PROJECT root ──
    println!("\n=== 8. git ops against the site's own checkout ===");
    {
        let sup = plat.supervisor();
        let status = repo::read_git_status(sup, &git, &env, &docroot);
        match &status {
            Ok(st) => {
                println!("   branch={:?} dirty={} untracked={}", st.branch, st.changed, st.untracked);
                if st.branch.as_deref() != Some("main") {
                    ok = fail("the panel would show the wrong branch");
                }
                // §2 wrote `.env` into this checkout and the fixture gitignores
                // it — a clean tree here is what makes the panel's "safe to
                // switch branch" answer true.
                if repo::loss_warning(st).is_some() {
                    ok = fail("a gitignored .env must not read as uncommitted work");
                }
            }
            Err(e) => ok = fail(&format!("read_git_status at the project root: {e}")),
        }

        // The status the CHECKOUT/delete confirmations are built on: real
        // uncommitted work must be counted, by name, before anything moves.
        std::fs::write(docroot.join("artisan"), "#!/usr/bin/env php\n<?php // edited\n")
            .expect("dirty the tree");
        match repo::read_git_status(sup, &git, &env, &docroot) {
            Ok(st) => match repo::loss_warning(&st) {
                Some(w) => println!("   dirty tree warns: {w}"),
                None => ok = fail("an edited tracked file must produce a loss warning"),
            },
            Err(e) => ok = fail(&format!("status on a dirty tree: {e}")),
        }
        git_in(&docroot, &["checkout", "--quiet", "--", "artisan"]);

        // Fetch + checkout, the two panel buttons that touch the remote and the
        // working tree. Both run at the PROJECT root — one level above what the
        // web server serves, which is where the clone put `.git`.
        if let Err(e) = repo::git_fetch(sup, &git, &env, &docroot, &cancel, &mut sink) {
            ok = fail(&format!("git fetch at the project root: {e}"));
        }
        if let Err(e) = repo::git_checkout(sup, &git, &env, &docroot, "develop", &cancel, &mut sink)
        {
            ok = fail(&format!("git checkout develop: {e}"));
        }
        match repo::read_git_status(sup, &git, &env, &docroot) {
            Ok(st) if st.branch.as_deref() == Some("develop") => {
                println!("   switched to develop, and the panel reads it live")
            }
            Ok(st) => ok = fail(&format!("still on {:?} after checkout", st.branch)),
            Err(e) => ok = fail(&format!("status after checkout: {e}")),
        }
        // `.git` is at the project root, NOT under what nginx serves — the
        // dotfile guard is the other half of that, and this is the half that
        // says the panel is pointed at the right directory.
        if !docroot.join(".git").is_dir() || docroot.join("public/.git").exists() {
            ok = fail("the checkout must live at the project root, above the served folder");
        }
    }

    // ── 9. Any PHP repository, served from its own front controller ─────
    println!("\n=== 9. a Symfony repo cloned as a Blank-PHP site ===");
    let invoices = sites_dir.join("invoices.rex");
    std::fs::create_dir_all(&invoices).expect("docroot");
    let symfony = sites::GitSource {
        url: symfony_remote.to_string_lossy().into_owned(),
        git_ref: Some("main".into()),
    };
    match sites::clone_into_docroot(
        plat.supervisor(),
        &git,
        &env,
        &symfony,
        &invoices,
        // Blank PHP accepts whatever landed — the document root is DETECTED,
        // not assumed, which is what makes one site type cover every framework
        // rexenv does not special-case.
        SiteType::Php,
        &cancel,
        &mut sink,
    ) {
        Ok(detected) => {
            println!("   detected {} · serving {:?}", detected.label, detected.docroot_rel);
            if detected.label != "Symfony" || detected.docroot_rel != "public" {
                ok = fail("a Symfony checkout must be served from public/, and named");
            }
            // The `deps` phase's own test: composer.json present, vendor/ not.
            // Serving this without installing would be a 500, which is why the
            // phase exists rather than being an offer.
            if !invoices.join("composer.json").is_file() {
                ok = fail("composer.json missing — the deps phase would skip and the site would 500");
            }
            if invoices.join("vendor").exists() {
                ok = fail("vendor/ arrived in the clone?! the fixture is unrealistic");
            }
            // And it is NOT mistaken for the framework next door.
            if detected.site_type != SiteType::Php {
                ok = fail("a Symfony repo is not a Laravel site");
            }
        }
        Err(e) => ok = fail(&format!("clone failed: {e}")),
    }

    // ── 10. WordPress, both layouts ─────────────────────────────────────
    println!("\n=== 10. WordPress repositories ===");
    for (name, remote, want_label, want_rel, want_content, composer_core) in [
        ("blog.rex", &stock_wp_remote, "WordPress", "", "wp-content", false),
        ("roots.rex", &bedrock_remote, sites::LABEL_BEDROCK, "web", "app", true),
    ] {
        let dest = sites_dir.join(name);
        std::fs::create_dir_all(&dest).expect("docroot");
        let src = sites::GitSource {
            url: remote.to_string_lossy().into_owned(),
            git_ref: Some("main".into()),
        };
        match sites::clone_into_docroot(
            plat.supervisor(),
            &git,
            &env,
            &src,
            &dest,
            SiteType::Wordpress,
            &cancel,
            &mut sink,
        ) {
            Ok(detected) => {
                println!("   {name}: {} · serving {:?}", detected.label, detected.docroot_rel);
                if detected.label != want_label || detected.docroot_rel != want_rel {
                    ok = fail(&format!("{name}: wrong layout — {detected:?}"));
                }
                // The two phases a composer-managed layout must stand down for.
                if sites::wordpress_core_from_composer(&detected) != composer_core {
                    ok = fail(&format!("{name}: core_download/wp-config decision is wrong"));
                }
                // The content dir, read from the SERVED root — recorded at
                // create from an EMPTY folder, so it would have said
                // `wp-content` for Bedrock and every mu-plugin writer would
                // have landed in a directory the site does not load.
                let served =
                    if want_rel.is_empty() { dest.clone() } else { dest.join(want_rel) };
                let content = sites::detect_content_dir_rel(&served);
                if content != want_content {
                    ok = fail(&format!("{name}: content dir is {content}, expected {want_content}"));
                }
                println!("   {name}: content dir {content}");
            }
            Err(e) => ok = fail(&format!("{name}: clone failed: {e}")),
        }
    }

    // The Bedrock `.env` this site would actually get, written over the
    // repository's own example.
    {
        let project = sites_dir.join("roots.rex");
        match dotenv::ensure_file(&project, rexenv_lib::core::wordpress::BEDROCK_ENV_SEED) {
            Ok(dotenv::EnvOrigin::Example) => {}
            Ok(other) => ok = fail(&format!(".env came from {other:?}, expected the example")),
            Err(e) => ok = fail(&format!("ensure_file: {e}")),
        }
        let original = std::fs::read_to_string(project.join(".env")).expect("read .env");
        let wired = rexenv_lib::core::wordpress::wire_bedrock_env(
            &original,
            "https://roots.rex",
            &rexenv_lib::core::wordpress::BedrockDb {
                name: "wp_roots_rex".into(),
                user: "root".into(),
                password: String::new(),
                host: "127.0.0.1:13306".into(),
            },
        );
        for want in ["DB_NAME=wp_roots_rex", "DB_HOST=127.0.0.1:13306", "WP_HOME=https://roots.rex"] {
            if !wired.contains(want) {
                ok = fail(&format!("Bedrock .env is missing {want}"));
            }
        }
        if wired.contains("# DB_HOST") || wired.contains("http://example.com") {
            ok = fail("a commented twin or the example URL survived the wiring");
        }
        for key in rexenv_lib::core::wordpress::SALT_KEYS {
            let lines = wired.lines().filter(|l| l.starts_with(&format!("{key}="))).count();
            if lines != 1 {
                ok = fail(&format!("{key} appears {lines} times — one answer per key"));
            }
            if dotenv::is_blank(&wired, key) {
                ok = fail(&format!("{key} was left unset"));
            }
        }
        println!("   roots.rex: .env wired, all eight salts generated exactly once");
    }

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
