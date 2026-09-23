//! Shared helpers for the live-check examples.
//!
//! Not an example itself: Cargo auto-discovers `examples/*.rs` and
//! `examples/*/main.rs`, so a `mod.rs` in a subdirectory is compiled only where
//! it is declared (`mod common;`).
//!
//! # THE INVARIANT — read this before writing an example
//!
//! **Examples run against REAL app data and REAL processes.** They link the app
//! library, resolve the same binary cache, and (historically) wrote into the
//! same `~/Library/Application Support/dev.rexenv.rexenv` tree the user's
//! running stack lives in. So:
//!
//! > Anything an example WRITES, SPAWNS or DELETES must be inside
//! > fixture-owned scope — a sandbox app-data root, a temp directory the
//! > example created, a Drop-owned process guard. Never a derived path, never
//! > the shared prefix, never a path computed from the real `Paths`.
//!
//! **The actual shape of this boundary — where it is structure and where it is
//! discipline.** [`sandbox`] makes the invariant STRUCTURAL, but only for the
//! ~20 of 109 examples that call it; the rest run on the REAL `Paths` and rest
//! on per-example review (a [`Reaped`] guard for spawns, a self-created temp dir
//! for writes), not on a type that redirects the paths. And even under
//! `sandbox` the shared binary cache is a DELIBERATE real, mutable exception:
//! examples add to it, and `download_progress_check` deliberately DELETES one
//! content-addressed entry to test re-download. So the honest boundary is not
//! "an example writes nothing real" — it is "an example writes nothing real
//! EXCEPT the content-addressed binary cache, and only the ~20 sandbox callers
//! have even that guaranteed by structure rather than by review."
//!
//! This has now bitten three times, each fixed per-instance until this note:
//!
//! 1. an example `rm -rf`'d `docroot.parent()` and took the user's whole Sites
//!    folder with it;
//! 2. examples SIGKILLed php-fpm masters and leaked title-rewritten workers
//!    that squatted ports for days (fixed by [`Reaped`], below);
//! 3. examples started nginx with the REAL `cfg.nginx_prefix`, so their nginx
//!    wrote — and on exit cleared — the running stack's `nginx.pid`. The real
//!    master stayed alive but became undiscoverable, and the user's very next
//!    site import failed with `nginx -s reload` → `invalid PID number ""`.
//!
//! [`sandbox`] is the structural answer to (3): it hands back a `Platform`
//! whose paths are all temporary, so an example never HOLDS the real config
//! dir, prefix or pid path and cannot pass one by accident. Use it for anything
//! that generates configs or spawns services. The deliberate exceptions are
//! documented on [`sandbox`] itself.
//!
//! ## …and a gap the "mutable exception" note missed: the app DELETES from that cache
//!
//! The shared binary cache is called a deliberate mutable exception above —
//! examples add to it. What that missed is the other direction: the app REMOVES
//! from it. `gc_outdated_php_caches` sweeps every PHP tree the registry does not
//! select, at every launch. So an example that resolves `binaries::pins().php`
//! (the PIN) on a machine whose registry selects a newer patch — an ordinary
//! in-app PHP update — has downloaded a tree the app is entitled to delete, and
//! may delete between two phases of the same run. On 10 Sep 2026 that killed
//! `laravel_postgres_check` mid-`composer create-project`: `No such file or
//! directory`, exit 127, an error naming nothing. The app was right and the
//! check was the odd one out. An example in that position must verify the binary
//! is still there and NAME the sweep, rather than failing inside somebody else's
//! subprocess.
//!
//! ## …and the gap `sandbox` does NOT close: PORTS
//!
//! **`sandbox` covers paths and says nothing about ports**, and every
//! service-or-network-tier example inherits that gap. It bites in the direction
//! that looks like success: with the user's stack up, an example that brings up
//! its own services binds — or worse, TALKS TO — theirs. `mcp_mail_check` found
//! it with Mailpit (its legs would have planted test mail in a real inbox and
//! proven the filter against the user's own mail). `tunnel_exposure_check` found
//! it again with MySQL on the shared port, where `install_for_site` would have
//! created its fixture database inside the user's RUNNING engine — same accident,
//! bigger blast radius, and both would have read as the example working.
//!
//! So the refusal lives HERE, in [`require_ports_free`], rather than in each
//! example: a correct implementation with a warning beside it did not stop the
//! same mistake being made twice in this repo (see `core::copy_scan`), and the
//! next network-tier example should not have to rediscover it.
//!
//! ## …and the SECOND gap: the sites dir
//!
//! `sites::provision` puts a docroot under the `sites_dir` SETTING, which falls
//! back to `~/rexenv/Sites` — a path computed from the home directory, **not
//! from `Paths`**. A sandboxed `Platform` cannot redirect it. So an example that
//! sandboxes its paths and provisions a site writes into the USER'S real Sites
//! folder, and nothing says so.
//!
//! That is not hypothetical either: it happened on 13 Aug 2026, hours after the
//! ports gap above was written down. Documenting a gap did not prevent the next
//! instance of it — so this one is closed by SHAPE, twice over:
//!
//! - [`sandbox_db`] is the one-liner that opens an example's database, and it
//!   pins `sites_dir` into the sandbox as it does. The easy path is the safe
//!   one; reaching the unsafe path now means deliberately calling
//!   `db::open_for_platform` yourself.
//! - [`SandboxGuard`] snapshots the real Sites folder when the sandbox is
//!   created and SHOUTS on drop if anything new appeared there — including on a
//!   panic. Prevention for the ordinary path, a loud backstop for the rest.
//!
//! ## Cleanup discipline
//!
//! An example that spawns a service must reap it from a DROP GUARD, never from
//! a statement at the end of `main`: everything in between is an `assert!` /
//! `.expect()` that can unwind straight past that statement.
//!
//! And a FORKING service (php-fpm, httpd, nginx) has to be stopped gracefully.
//! SIGKILL to the master leaves its workers alive, reparented to pid 1, still
//! holding the listen socket — the failure mode `Proc::alive`'s doc comment
//! describes. Production solves this with [`Proc::terminate`] (SIGTERM → poll →
//! SIGKILL as a last resort); examples get the same behavior here rather than a
//! second implementation.
//!
//! One caveat worth stating, because it is what made the leak invisible: php-fpm
//! workers REWRITE their process title to `php-fpm: pool www`, which contains
//! neither the binary path nor the config path. A sweep keyed on the executable
//! would miss precisely the processes that leak, so [`Reaped`] matches on the
//! program NAME (`php-fpm`, `httpd`, `nginx`).
//!
//! ## The verdict contract: exit 0 means PROVEN
//!
//! `scripts/live-checks.sh` takes an example's verdict from its EXIT STATUS and
//! nothing else, and `docs/TESTING.md` states the rule as "exit 0 = proven,
//! non-zero = not". So a precondition that fails must reach the exit code, and
//! the idiom that quietly broke this in 13 examples is:
//!
//! ```ignore
//! if let Err(e) = mgr.start_all(..).await { eprintln!("start_all failed: {e}"); return; }
//! ```
//!
//! A bare `return` from `async fn main() -> ()` **exits 0**. The tier prints
//! `all green` for a run in which the stack never came up, no assertion
//! executed, and every readiness gate below was jumped over — and the operator
//! sees the `start_all failed:` line only if they read the log of a run that
//! passed. Fixed 21 Aug 2026 by giving those `main`s a return type:
//! `async fn main() -> std::process::ExitCode`, `ExitCode::FAILURE` on the
//! precondition path, `ExitCode::SUCCESS` at the end.
//!
//! **`ExitCode::FAILURE`, not `process::exit(1)`** — the two are not equivalent
//! here. `exit` runs no destructors, so it skips the [`Reaped`] and
//! [`OwnedService`] guards and the `ServiceManager`'s own `Drop`, turning a
//! failed run into a leaked service holding a production port. Returning the
//! code unwinds the stack normally first. (The same reasoning is why
//! [`await_ready`] panics rather than exits.)

#![allow(dead_code)] // each example uses a subset

// ---------------------------------------------------------------------------
// Fixture ports
// ---------------------------------------------------------------------------
//
// An example must NEVER bind a production port (docs/PORTS.md) — a sandboxed
// example on :18088 still collides with the user's running stack, and the
// runner may execute examples while the stack is up. Claim a port here (one
// line per example, unique, in the 18100+/9790+/13390+ bands) and reference
// the const; a hardcoded port literal in an example is a review smell.
// Already-claimed fixture ports live in their examples today: 18097
// (retry_recovery_check), 18099 (linked_site_check, valet_import_check),
// 13396 + 13395 (starter_seed_check: its sandbox mysqld and its sandbox
// PostgreSQL — the starter has a dialect per engine, so the check runs its legs
// twice), 13397-13399 (config_rewrite/db_restore/db_dump), 9799 (apache_site_check),
// 9998/19003 (xdebug_pool_check), 18131/9791 (nginx_php_serve), 9792
// (php_fpm_serve), 18132/9793/9794/9795 (dotfile_guard_check), 9796/9797 (fpm_candidate_check), 18134-18136
// (edge_wire_check: a squatting listener, a deliberately EMPTY port, and a
// plaintext responder — the three shapes `proxy::edge_wire` must tell apart).

/// Open an example's database inside the sandbox, with `sites_dir` PINNED.
///
/// Use this instead of `db::open_for_platform` in anything that provisions a
/// site. It exists because the pin is invisible when it is missing: the site is
/// created, the example works, and the folders turn up in the user's own Sites
/// directory. See the module doc for what that cost.
pub fn sandbox_db(platform: &dyn Platform) -> rusqlite::Connection {
    let conn = rexenv_lib::state::db::open_for_platform(platform.paths())
        .expect("open the sandbox database");
    pin_sites_dir(&conn, platform);
    conn
}

/// Pin `sites_dir` into the sandbox on a connection the example opened ITSELF.
///
/// [`sandbox_db`] is the preferred door because it cannot be used without the
/// pin. This exists for the examples that open their database somewhere of their
/// own choosing (a named file under the sandbox root, a fixture path a later
/// assertion refers to) and would otherwise have to give that up to be safe.
///
/// Called by 11 examples found on 14 Aug 2026 by grepping for `sandbox()` callers
/// that provision without pinning. Two had been found before that — one on
/// 13 Aug, one this morning — and BOTH were caught by [`SandboxGuard`]'s alarm
/// after the fact rather than by review. The alarm is the backstop working; the
/// eleven are what it had not happened to catch yet, because catching them
/// required someone to run them.
pub fn pin_sites_dir(conn: &rusqlite::Connection, platform: &dyn Platform) {
    refuse_on_the_real_app_db(conn, "pin_sites_dir");
    let sites = platform
        .paths()
        .app_data_dir()
        .expect("sandbox app data")
        .join("Sites");
    std::fs::create_dir_all(&sites).expect("sandbox sites dir");
    rexenv_lib::state::store::set_setting(conn, "sites_dir", &sites.to_string_lossy())
        .expect("pin the sandbox sites dir");
}

/// Leave the DATABASE ENGINES the way this run found them.
///
/// For examples that drive the app's own provisioning: creating a site STARTS
/// the engine through production code, the example then `adopt_dbs`-es it, and
/// adoption is deliberately not ownership — an adopted pid is nobody's child, so
/// no `Drop` reaches it and services outlive the app on purpose. Correct for the
/// product; a leak in a fixture.
///
/// **Both instances were found the same way, on 21 Aug 2026, and neither example
/// failed.** `git_site_provision_check` and `site_provision_check` each printed
/// ALL PASS and exited leaving `mysqld` on :13306 against the REAL datadir; the
/// twelve later examples that then could not start their own MySQL failed naming
/// the PORT, not the cause. A green run that reddens the rest of the tier is the
/// worst shape a check can have, because the bisect starts at the wrong file.
///
/// Restores a TRANSITION, never a state: an engine the developer already had
/// running is left alone, because stopping that would be the identical defect
/// pointing the other way.
pub struct EnginesAsFound {
    platform: Box<dyn Platform>,
    were_running: Vec<(rexenv_lib::core::db::DbEngine, bool)>,
}

/// Record which engines are up NOW; stop the ones this run starts, on every exit
/// path that runs destructors.
pub fn engines_as_found() -> EnginesAsFound {
    use rexenv_lib::core::db::DbEngine;
    let platform = rexenv_lib::platform::current();
    let were_running =
        DbEngine::ALL.into_iter().map(|e| (e, rexenv_lib::core::ports::is_listening(e.port()))).collect();
    EnginesAsFound { platform, were_running }
}

impl Drop for EnginesAsFound {
    fn drop(&mut self) {
        let marker = match self.platform.paths().app_data_dir() {
            Ok(p) => p.display().to_string(),
            Err(_) => return,
        };
        for (engine, was_running) in &self.were_running {
            if *was_running || !rexenv_lib::core::ports::is_listening(engine.port()) {
                continue;
            }
            // Ownership before signalling, always: `owned_master` matches our
            // app-data marker on the cmdline, so a developer's own MySQL on the
            // same port is never a candidate.
            let Some(pid) = self.platform.supervisor().owned_master(engine.port(), &marker) else {
                eprintln!(
                    "NOTE: {:?} is up on :{} and this run started it, but no rexenv-owned \
                     master was found to stop — leaving it",
                    engine,
                    engine.port()
                );
                continue;
            };
            // No version to hand it: this guard stops by PID what this run
            // started (PostgreSQL's `pg_ctl` path wants one — #698).
            match engine.stop(&*self.platform, pid, None) {
                Ok(()) => println!("stopped the {:?} this run started (it was down before)", engine),
                Err(e) => eprintln!("could not stop the {:?} this run started: {e}", engine),
            }
        }
    }
}

/// A file this example created, removed when the guard drops.
///
/// For artifacts that must live in a REAL directory — a probe inside the Adminer
/// docroot, say, because the point is to be served by the same vhost and pool
/// Adminer uses. A trailing `let _ = std::fs::remove_file(..)` at the end of
/// `main` does not survive the six `assert!`s above it: one unwind and the file
/// stays. In the Adminer docroot that matters more than it sounds — every
/// non-dotfile `.php` there is directly executable through the console's own
/// vhost, so a failed run leaves a live endpoint behind the wrapper's controls.
pub struct FixtureFile {
    path: PathBuf,
}

impl FixtureFile {
    /// Write `contents` to `path` and own the removal.
    pub fn write(path: PathBuf, contents: &str) -> Self {
        std::fs::write(&path, contents).expect("write the fixture file");
        FixtureFile { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A fixture-owned sites directory, pinned into `conn` and removed on drop.
///
/// [`pin_sites_dir`] pins into the SANDBOX's app-data root and is the right
/// answer for an example that sandboxes its platform. This is for the ones that
/// deliberately run on the REAL `Paths` — they talk to the real Adminer docroot,
/// the real certs, the real stack — and would otherwise provision into the
/// user's own `~/rexenv/Sites`, because `sites::provision` reads the `sites_dir`
/// SETTING and that setting falls back to a path computed from the HOME
/// directory rather than from `Paths`.
///
/// Writing there was known ("snapshot the folder before a bulk run"). What made
/// it a defect rather than a nuisance is that `adminer_deeplink_check` then
/// called `remove_dir_all` on the docroot it had provisioned — a delete inside
/// the user's real Sites folder, which is the incident this repo has already
/// paid for once (an example `rm -rf`'d `docroot.parent()` and took the whole
/// folder with it). A habit is not a control; this is.
pub struct FixtureSitesDir {
    path: PathBuf,
}

impl FixtureSitesDir {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureSitesDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Pin `sites_dir` at a fresh fixture directory for the life of the guard.
///
/// `/private/tmp` for the same reason [`sandbox`] uses it — it keeps derived
/// paths short — and pid-scoped so two runs never share a docroot.
/// Refuse to write a setting into the REAL app database.
///
/// Added 21 Aug 2026, one command before making the mistake it prevents. Pinning
/// `sites_dir` is exactly right on a fixture database and catastrophic on the
/// real one: it REPOINTS THE USER'S SITES FOLDER, so every site they own reads
/// as missing and the next launch's sweep looks at an empty directory. The
/// difference between the two calls is one earlier line choosing
/// `db::open(temp)` over `db::open_for_platform(real)`, which is not a
/// difference review reliably sees.
///
/// `Connection::path()` knows which file it opened, so the check is a fact
/// rather than a convention.
fn refuse_on_the_real_app_db(conn: &rusqlite::Connection, what: &str) {
    let Some(open_path) = conn.path() else { return };
    let real = rexenv_lib::platform::current()
        .paths()
        .app_data_dir()
        .map(|d| d.join(rexenv_lib::state::db::DB_FILE));
    let Ok(real) = real else { return };
    if std::path::Path::new(open_path) == real {
        eprintln!(
            "\n✗ REFUSING — {what} was handed the REAL app database.\n  {}\n  \
             Pinning a setting there rewrites the USER'S configuration: `sites_dir` \
             would repoint their Sites folder and every site they own would read as \
             missing.\n  Open a fixture database instead — `common::fixture_db`, \
             `common::sandbox_db`, or a temp path.\n",
            real.display()
        );
        // Same refusal shape as `require_ports_free`: there is nothing sensible to
        // do with this failure, and a caller that could ignore it is exactly what
        // this exists to prevent.
        std::process::exit(1);
    }
}

/// Where every fixture root lives: `/private/tmp` on macOS (the 103-byte socket
/// ceiling, explained at [`sandbox`]), the user's temp dir on Windows — there the
/// edge's admin endpoint is a named pipe with no such ceiling, and `/private/tmp`
/// is a DRIVE-RELATIVE path: Rust's `fs` resolves it to `C:\private\tmp`, but a
/// program handed the string as spelled (Explorer, in `windows_job_guard_check`)
/// cannot open it — measured 19 Sep 2026, the leg failed "None" for the path, not
/// the claim.
pub fn fixture_base() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::temp_dir()
    } else {
        PathBuf::from("/private/tmp")
    }
}

pub fn pin_fixture_sites_dir(conn: &rusqlite::Connection, tag: &str) -> FixtureSitesDir {
    refuse_on_the_real_app_db(conn, "pin_fixture_sites_dir");
    let path = fixture_base().join(format!("rexenv-sites-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("fixture sites dir");
    rexenv_lib::state::store::set_setting(conn, "sites_dir", &path.to_string_lossy())
        .expect("pin the fixture sites dir");
    FixtureSitesDir { path }
}

/// Every entry in the REAL sites folder, for the guard's before/after.
fn real_sites_snapshot() -> Vec<String> {
    let Ok(dir) = rexenv_lib::core::sites::default_sites_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> =
        entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    out.sort();
    out
}

/// Refuse to run while anything is listening on a port this example needs.
///
/// Call it BEFORE creating or starting anything. Each entry is
/// `(port, what it is and what borrowing it would do)` — the second half is the
/// message a reader gets, so say what would happen, not just which port.
///
/// Exits the process rather than returning an error: there is nothing sensible
/// to do with the failure, and a caller that could ignore it is the shape this
/// exists to prevent.
/// Install WordPress into a fixture site the ordinary way: a `wp_<slug>` database
/// on rexenv's MySQL, default admin, default language.
///
/// **What this removes.** `wordpress::install_for_site` takes nine arguments and
/// twelve examples passed the same eight every time — only the site title
/// differed. `wp_plugins_check` and `wp_themes_check` were byte-identical bar
/// that string. Worse, the repetition DRIFTS: `db_name_for` gained a type
/// parameter on 13 Aug 2026 and every one of those sites had to be edited. The
/// db name and the engine address are derived here now, so the next change to
/// either is one edit.
///
/// **Who should NOT use this, and why that is not a gap.** Four examples pass
/// something genuinely different, and the difference IS their subject:
/// `mariadb_site_check` (a MariaDB host and a captured `Result`, because the
/// install failing is a thing it reports), `wp_create_serve` (custom
/// `InstallOptions` — it checks the admin user rexenv creates),
/// `adminer_deeplink_check` (a RECORDED db name, not one derived from the
/// domain), `tunnel_exposure_check` (a `fail()` path rather than `expect`).
/// They call `install_for_site` directly and should keep doing so.
///
/// So there is deliberately **no guard forbidding a direct call** — measuring
/// first is what stopped one being written, and a guard that forced these four
/// through a helper would erase the thing each of them checks.
pub fn install_wp(
    php: &Path,
    wp: &Path,
    docroot: &Path,
    domain: &str,
    title: &str,
    db_client: &rexenv_lib::core::db::SqlClient,
) {
    rexenv_lib::core::wordpress::install_for_site(
        php,
        wp,
        docroot,
        domain,
        title,
        &rexenv_lib::core::wordpress::db_name_for(
            rexenv_lib::state::models::SiteType::Wordpress,
            domain,
        ),
        &format!("127.0.0.1:{}", rexenv_lib::core::database::MYSQL_PORT),
        db_client,
        &Default::default(),
    )
    .unwrap_or_else(|e| panic!("install wordpress into {domain}: {e}"));
}

/// rexenv's OWN fixed service ports, DERIVED from the constants that decide
/// them — never a literal list.
///
/// The tier runner (`scripts/live-checks.sh`) probes the same set before it
/// starts, and a second hand-written copy of these numbers is how the two would
/// drift apart. A test asserts the shell list and this one agree.
///
/// `:443` is deliberately absent: the edge is designed to outlive the app,
/// other tools shadow-bind it (Herd does), and "some Caddy is up" is not the
/// claim "rexenv's stack is running". Ports belonging to somebody else are
/// [`require_ports_free`]'s job.
pub fn rexenv_service_ports() -> Vec<(u16, &'static str)> {
    let mut ports = vec![
        (rexenv_lib::core::services::NGINX_HTTP_PORT, "rexenv's shared nginx"),
        (rexenv_lib::core::mail::MAILPIT_HTTP_PORT, "rexenv's Mailpit"),
        (rexenv_lib::core::database::MYSQL_PORT, "rexenv's MySQL"),
        (rexenv_lib::core::db::MARIADB_PORT, "rexenv's MariaDB"),
    ];
    for full in rexenv_lib::core::binaries::pins().php_versions {
        let minor = full.rsplit_once('.').map(|(m, _)| m).unwrap_or(full);
        if let Some(p) = rexenv_lib::core::php::fpm_port(minor) {
            ports.push((p, "a rexenv php-fpm pool"));
        }
    }
    ports
}

/// Refuse to run if rexenv's stack is up — the precondition EVERY service-tier
/// example has and almost none used to state.
///
/// **Why a blanket check rather than each example naming its ports.** The tier
/// runner already refuses, so this is for the example someone runs BY HAND —
/// and the failure it prevents is not a bind collision but the opposite: a
/// readiness gate that CONNECTS is satisfied by the user's server, so the
/// example joins the live stack instead of colliding with it and then reports
/// on services it does not own. `delete_site_serve` printed two false `HTTP 200`
/// preconditions that way before anything failed.
///
/// Per-example port lists were tried first and 20 of 24 examples simply never
/// got one — including two written the same day the rule was agreed. A guard
/// that has to be remembered per file is a guard that will be missing from the
/// next file, so this needs no arguments and the same call fits everywhere.
///
/// Safe to `process::exit` for the reason [`require_ports_free`] is: it runs
/// BEFORE anything is spawned. After the first `OwnedService` exists, exiting
/// leaks it — a panic unwinds and reaps, a tidy exit does not.
pub fn require_stack_stopped() {
    let busy: Vec<(u16, &str)> = rexenv_service_ports()
        .into_iter()
        .filter(|(p, _)| rexenv_lib::core::ports::is_listening(*p))
        .collect();
    if busy.is_empty() {
        return;
    }
    eprintln!("\n✗ REFUSING TO RUN — rexenv's stack is RUNNING.");
    for (p, what) in &busy {
        eprintln!("  127.0.0.1:{p} is answering ({what}).");
    }
    eprintln!(
        "\n  This example brings up its OWN services on these exact ports. Beside a live\n  \
         stack they do not collide, they JOIN: a readiness gate that connects is satisfied\n  \
         by YOUR server, and the example then reports on a stack it does not own.\n\n  \
         Fix: `rex stop` (or quit rexenv and stop its services), then re-run.\n"
    );
    std::process::exit(1);
}

pub fn require_ports_free(ports: &[(u16, &str)]) {
    for (port, what) in ports {
        if rexenv_lib::core::ports::is_listening(*port) {
            eprintln!(
                "\n✗ REFUSING TO RUN — something is already on a port this needs\n  \
                 127.0.0.1:{port} is answering ({what}).\n  \
                 This example brings up its OWN services: `common::sandbox` makes the PATHS \
                 temporary and does nothing about PORTS, so running beside a live stack means \
                 using the user's.\n  Stop the stack (in rexenv, or `rex stop`), then run this \
                 again.\n"
            );
            std::process::exit(1);
        }
    }
}

/// Block until `port` is accepting connections, or die naming the service.
///
/// # The defect this exists to end
///
/// Three examples spawned a service and then USED it with nothing in between:
/// `apache_site_check` (no wait at all), `dotfile_guard_check` (a flat
/// `sleep(800ms)`, which is a timing assumption wearing a wait's clothes), and
/// the shape recurs wherever a pool is spawned before a server that fronts it.
/// The server binds in milliseconds and satisfies its own readiness loop at
/// once; a cold php-fpm under CPU contention has not bound yet, so the first
/// request reaches the front-end with nothing behind it.
///
/// What made it expensive was not the flake — it was the MISDIAGNOSIS. The
/// failure surfaced downstream as `php-via-fpm=false` (Apache), `502` (nginx),
/// `503` (httpd): all of which read as "the web server cannot execute PHP" and
/// send the reader at the wrong subject. `apache_site_check`'s instance sat in
/// `docs/archive/SHIPPED-2026-08.md` as an unexplained transient from 3 Aug 2026
/// until the second capture, on 20 Aug, showed the two PHP legs failing while
/// the two static legs passed — which is the pool's fingerprint, not the server's.
///
/// So: wait HERE, and when the wait fails, fail HERE — with the port and the
/// service named, before any downstream check can offer a plausible wrong
/// answer. Exits the process for [`require_ports_free`]'s reason: there is
/// nothing sensible for a caller to do with it, and a caller that could ignore
/// it is the shape this exists to prevent.
///
/// `log` is an optional file to spill on failure (php-fpm's error_log is the
/// one that says WHY, and every one of these examples was discarding it).
pub fn await_listening(port: u16, what: &str, log: Option<&Path>) {
    await_ready(&format!("{what} (127.0.0.1:{port})"), log, || {
        rexenv_lib::core::ports::is_listening(port)
    });
}

/// [`await_listening`] for readiness that is not a TCP port.
///
/// The edge's admin surface is a UNIX SOCKET by design — a TCP admin on a root
/// Caddy is arbitrary file r/w as root (docs/ARCHITECTURE.md) — so the examples
/// that wait for it cannot poll a port at all, and a helper that only knew
/// about ports would have sent them back to flat sleeps. Same contract: poll,
/// then fail HERE with the subject named and its log spilled, so a missing
/// precondition never masquerades as a broken subject downstream.
pub fn await_ready(what: &str, log: Option<&Path>, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let said = log
        .map(|path| match std::fs::read_to_string(path) {
            Ok(text) if !text.trim().is_empty() => {
                format!("\n  its own log ({}):\n{text}", path.display())
            }
            Ok(_) => format!("\n  its own log ({}) is empty — it never got far enough to write", path.display()),
            Err(e) => format!("\n  its own log ({}) is unreadable: {e}", path.display()),
        })
        .unwrap_or_default();
    // PANIC, never `process::exit`. This is the difference between reporting a
    // leak and CAUSING one: `exit` skips every Drop, so the `Reaped` and
    // `OwnedService` guards that own the just-spawned children never run and
    // those children keep their ports. Measured on 20 Aug 2026, the first day
    // this helper existed — one `exit` here left a php-fpm on :9783 and took
    // out EIGHT later examples in the same tier run, every one of them
    // reporting "already in use by rexenv (php-fpm, pid 11694)". A readiness
    // guard that poisons the rest of the suite is worse than the flake it
    // replaced.
    //
    // `require_ports_free` may still `exit`, and the distinction is the point:
    // it runs BEFORE anything is spawned, so it has nothing to leak. This runs
    // after.
    panic!(
        "{what} was not ready within 20s.\n  \
         Every check below that needs it would fail as though the SERVICE were \
         broken — this says otherwise, here, before they run.{said}"
    );
}

use rexenv_lib::error::Result as RexResult;
use rexenv_lib::platform::traits::{
    AutostartManager, BinaryProvider, CertTrustManager, DnsAgentManager, DnsManager,
    EdgeSupervisor, Paths, Platform, PermissionManager, PrivilegeManager, ProcessSupervisor,
    ShellRunner,
};
use rexenv_lib::core::proc::Proc;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Sandboxed platform
// ---------------------------------------------------------------------------

/// `Paths` rooted in a throwaway directory.
///
/// Every service path the app computes — the config dir, the log dir, the nginx
/// PREFIX and therefore `nginx.pid`, the `run/` sockets, the SQLite file, certs
/// — is derived from these, so redirecting `Paths` redirects all of them at
/// once. That is what makes the sandbox structural rather than a convention.
struct SandboxPaths {
    root: PathBuf,
    /// The REAL binary cache: the one deliberate exception. It is
    /// content-addressed, checksum-verified and atomically published, and not
    /// sharing it would mean re-downloading ~600 MB of MySQL per example run.
    /// Deliberately MUTABLE, not add-only (that was overstated): examples add to
    /// it, and `download_progress_check` takes one content-addressed entry out of
    /// the way to exercise re-download.
    ///
    /// **That last argument used to end "safe only because every entry is
    /// re-fetchable by checksum", and that is true only while the network is**
    /// (24 Aug 2026). Offline, or during a registry outage — a GitHub 504 was hit
    /// on this machine on 21 Aug — a delete leaves the user without their wp-cli
    /// phar. The example RENAMES the entry now and restores it in `Drop` unless
    /// the run replaced it, so the shared cache survives a failed download and a
    /// panic alike. The exception this comment describes is still an exception;
    /// it is no longer one that can cost the user something.
    bin: PathBuf,
    hosts: PathBuf,
}

fn ensure(p: PathBuf) -> RexResult<PathBuf> {
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

impl Paths for SandboxPaths {
    fn app_data_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.clone())
    }
    fn config_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.join("config"))
    }
    fn log_dir(&self) -> RexResult<PathBuf> {
        ensure(self.root.join("logs"))
    }
    fn bin_dir(&self) -> RexResult<PathBuf> {
        Ok(self.bin.clone())
    }
    fn hosts_file(&self) -> PathBuf {
        self.hosts.clone()
    }
    fn cli_symlink_path(&self) -> RexResult<PathBuf> {
        // An example must never install or remove the user's `rex` symlink.
        Err(rexenv_lib::error::Error::Unsupported("CLI PATH install (sandboxed example)"))
    }
}

/// The real platform with its paths swapped for [`SandboxPaths`]. Everything
/// else (process supervision, privileges, DNS command builders) delegates
/// unchanged, because those are what the example is usually there to exercise.
struct SandboxPlatform {
    inner: Box<dyn Platform>,
    paths: SandboxPaths,
}

impl Platform for SandboxPlatform {
    fn paths(&self) -> &dyn Paths {
        &self.paths
    }
    fn dns(&self) -> &dyn DnsManager {
        self.inner.dns()
    }
    fn cert_trust(&self) -> &dyn CertTrustManager {
        self.inner.cert_trust()
    }
    fn privileges(&self) -> &dyn PrivilegeManager {
        self.inner.privileges()
    }
    fn supervisor(&self) -> &dyn ProcessSupervisor {
        self.inner.supervisor()
    }
    fn autostart(&self) -> &dyn AutostartManager {
        self.inner.autostart()
    }
    fn permissions(&self) -> &dyn PermissionManager {
        self.inner.permissions()
    }
    fn shell(&self) -> &dyn ShellRunner {
        self.inner.shell()
    }
    fn binaries(&self) -> &dyn BinaryProvider {
        self.inner.binaries()
    }
    fn edge(&self) -> &dyn EdgeSupervisor {
        self.inner.edge()
    }
    fn dns_agent(&self) -> &dyn DnsAgentManager {
        self.inner.dns_agent()
    }
    /// Delegated unchanged, and safely: `AppBundle` only ever touches the paths
    /// it is HANDED, so a sandboxed example that passes fixture paths stages and
    /// swaps fixture bundles. There is nothing to redirect — and redirecting it
    /// would mean the example exercised a different implementation from the one
    /// that ships, which is the whole thing L1 exists to avoid.
    fn app_bundle(&self) -> &dyn rexenv_lib::platform::traits::AppBundle {
        self.inner.app_bundle()
    }
}

/// Removes the sandbox tree when the example ends, however it ends.
pub struct SandboxGuard {
    root: PathBuf,
    /// The real Sites folder as it was when the sandbox opened. Compared on
    /// drop — see the module doc's second gap.
    real_sites_before: Vec<String>,
}

impl SandboxGuard {
    /// Where the sandbox lives — for printing, or for planting fixtures.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        // The backstop for the sites-dir gap. Runs on a panic too, which is
        // exactly when it is needed: on 13 Aug 2026 an example provisioned two
        // sites into the user's REAL Sites folder, then died, and nothing said
        // so — the folders were found by hand afterwards. This does NOT delete
        // anything: a cleanup that guessed at what was a fixture is a worse
        // failure than the litter, and one this project has already had.
        let after = real_sites_snapshot();
        let new: Vec<&String> =
            after.iter().filter(|d| !self.real_sites_before.contains(d)).collect();
        if !new.is_empty() {
            let dir = rexenv_lib::core::sites::default_sites_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            eprintln!(
                "\n!! THIS EXAMPLE WROTE INTO THE USER'S REAL SITES FOLDER !!\n   \
                 {dir}\n   new: {new:?}\n   \
                 `common::sandbox` sandboxes PATHS; the docroot comes from the `sites_dir` \
                 SETTING, which falls back to the home directory. Open the example's database \
                 with `common::sandbox_db`, which pins it.\n   \
                 Nothing has been deleted — check these are yours before removing them.\n"
            );
        }
    }
}

/// A sandboxed `Platform` rooted at `root`, with NO guard — for a check whose phases are
/// separate processes, where the root must outlive the first one (`sandbox` removes its
/// tree when its process ends). The caller owns `root` and removes it. Same exceptions as
/// [`sandbox`]: the real binary cache, the real hosts file.
pub fn sandbox_platform_at(root: PathBuf) -> Box<dyn Platform> {
    let real = rexenv_lib::platform::current();
    let bin = real.paths().bin_dir().expect("real binary cache");
    let hosts = real.paths().hosts_file();
    Box::new(SandboxPlatform { inner: real, paths: SandboxPaths { root, bin, hosts } })
}

/// A `Platform` whose app data is a throwaway directory.
///
/// Use this in ANY example that generates configs or spawns services. The
/// example then never holds the real config dir, nginx prefix or pid path, so
/// it cannot pass one by accident — the failure mode that broke a user's stack
/// (see the module note). Keep the guard alive for the whole run; dropping it
/// deletes the sandbox.
///
/// ```ignore
/// let (plat, _sandbox) = common::sandbox("my_check");
/// let cfg = sites::rebuild_configs(&conn, &*plat, &ca, PORT, 8081, 8444)?; // writes into the sandbox
/// ```
///
/// Two deliberate exceptions, stated on the methods above: the binary cache is
/// shared (re-downloading 600 MB per run is worse) and MUTABLE — examples add to
/// it and one deletes a content-addressed entry to test re-download (safe:
/// re-fetchable by checksum); the hosts file is the real one (only ever read).
/// Privileged operations
/// and the DNS command builders also delegate to the real platform — an example
/// that calls those is asking for a real system change and should say so.
pub fn sandbox(tag: &str) -> (Box<dyn Platform>, SandboxGuard) {
    let real = rexenv_lib::platform::current();
    let bin = real.paths().bin_dir().expect("real binary cache");
    let hosts = real.paths().hosts_file();
    // NOT `std::env::temp_dir()`, and the difference is 37 bytes that decide
    // whether the edge can start at all. macOS's per-user TMPDIR is 49
    // characters (`/var/folders/51/kd63p2nj6sz67msbn4pvzc3h0000gq/T/`), which
    // put every sandboxed `<root>/config/caddy-admin.sock` at 110-115 bytes
    // against a 103-byte ceiling — so caddy died on `bind: invalid argument`
    // before it ever listened, in EVERY sandboxed example that enables the
    // admin socket. The note below blames the TAG, and that reading is what hid
    // this: shortening `create_site_serve` to `createsrv` (8 characters off)
    // still failed, because the base was always the larger half. `/private/tmp`
    // is 12 characters and puts the same paths at 75-79.
    //
    // The tradeoff, stated: `/private/tmp` is world-writable where TMPDIR is
    // per-user 0700. The root is still pid-scoped and still removed by
    // `SandboxGuard`, and this is fixture scaffolding on a developer's machine
    // — but it IS a weaker directory, and that is the price of an edge that
    // starts.
    let root = fixture_base().join(format!("rexenv-sandbox-{tag}-{}", std::process::id()));

    // A sandbox root that is too LONG breaks the edge, and the failure names
    // nothing. Caddy's admin unix socket lives at `<root>/config/caddy-admin.sock`
    // and macOS binds at most 103 bytes of socket path, so whether a sandboxed
    // example can start the edge depends on how it was NAMED: `wp_create_serve`
    // came to 108 bytes and caddy said only `bind: invalid argument` (14 Aug
    // 2026). Refuse here, where the length is chosen, with the arithmetic — an
    // example that fails on its own tag should say so in one line.
    //
    // This is the FIXTURE half. The same ceiling exists in production
    // (`core::proxy::check_unix_socket_len`), where the driver is the user's home
    // directory rather than a tag.
    let sock_len = root.join("config").join("caddy-admin.sock").as_os_str().len();
    if sock_len > rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH {
        let over = sock_len - rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH;
        // A WARNING, not a refusal — and the first version got this wrong.
        // Whether the length matters depends on whether the example's edge asks
        // for an ADMIN SOCKET, and `sandbox` cannot know that: the sandboxed edge
        // in `tunnel_exposure_check` runs with admin off, so the path is never
        // bound and the example is fine. Refusing here blocked a working check —
        // found by migrating that example onto the shared guard in the same
        // session, which is the argument for migrating in the same session.
        // The hard failure belongs at the point of USE, where the answer is
        // known: `core::proxy::admin_socket_path` refuses there with the same
        // arithmetic. This line exists so a future example that DOES ask for the
        // socket learns why before it reads "bind: invalid argument".
        eprintln!(
            "\n⚠ sandbox tag {tag:?} produces a caddy admin socket path of {sock_len} bytes; \
             macOS binds at most {} (over by {over}).\n  \
             Harmless unless this example's edge enables the ADMIN socket — if it does, it \
             will fail with \"bind: invalid argument\". Shorten the tag by {over}.\n",
            rexenv_lib::core::proxy::MAX_UNIX_SOCKET_PATH
        );
    }

    let _ = std::fs::remove_dir_all(&root);
    // Sweep this tag's LEFTOVERS from earlier runs. `SandboxGuard`'s Drop
    // removes the root, but an example that ends with `process::exit` — which
    // every `fail()` in this tree does — runs no destructors, so a failing
    // run leaves its root behind. Two failed `tunnel_exposure_check` runs left
    // 280 MB each (13 Aug 2026). Self-healing beats remembering: the next run
    // of the SAME example clears them, and nothing outside this tag is touched.
    // The SAME directory the roots live in — `/private/tmp`, not
    // `std::env::temp_dir()`. When the root moved here (bd9748b, the 103-byte
    // socket limit) this sweep was left reading macOS's per-user TMPDIR, so from
    // that day it swept a directory that could no longer contain a single
    // leftover: self-healing that healed nothing, and silently, because an empty
    // read_dir looks exactly like a clean machine.
    if let Ok(entries) = std::fs::read_dir(root.parent().expect("sandbox root has a parent")) {
        let prefix = format!("rexenv-sandbox-{tag}-");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&prefix) && entry.path() != root {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    std::fs::create_dir_all(&root).expect("create the sandbox root");
    let platform = SandboxPlatform {
        inner: real,
        paths: SandboxPaths { root: root.clone(), bin, hosts },
    };
    // Belt: prove the sandbox really is somewhere else before handing it out.
    // If a future change ever makes these coincide, an example would start
    // writing the user's stack again — and that failure is silent until their
    // next reload breaks, which is exactly how this got shipped once already.
    let real_data = rexenv_lib::platform::current()
        .paths()
        .app_data_dir()
        .expect("real app data dir");
    let sandboxed = platform.paths().app_data_dir().expect("sandbox app data dir");
    assert!(
        sandboxed != real_data && !sandboxed.starts_with(&real_data),
        "sandbox root {} is inside the REAL app data {} — an example must never write there",
        sandboxed.display(),
        real_data.display()
    );
    (Box::new(platform), SandboxGuard { root, real_sites_before: real_sites_snapshot() })
}

// ---------------------------------------------------------------------------
// Fixture SQLite
// ---------------------------------------------------------------------------

/// Deletes the fixture database file when the example ends, however it ends.
pub struct FixtureDb {
    path: PathBuf,
}

impl FixtureDb {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A migrated, throwaway app database in the temp dir — the replacement for
/// the hand-rolled `temp_dir().join("rexenv-<task>.db")` idiom, whose
/// hardcoded names collided across examples (`rexenv-6_1.db` was two different
/// examples' database; concurrent runs corrupted each other). The pid suffix
/// makes concurrent runs safe; the guard cleans up what the old idiom left
/// behind forever. Keep the guard alive for the whole run.
pub fn fixture_db(tag: &str) -> (rusqlite::Connection, FixtureDb) {
    let path = std::env::temp_dir()
        .join(format!("rexenv-fixture-{tag}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn = rexenv_lib::state::db::open(&path).expect("open fixture SQLite");
    (conn, FixtureDb { path })
}

// ---------------------------------------------------------------------------
// Plain HTTP probe
// ---------------------------------------------------------------------------

/// The edge's HTTPS status for `host`, or `"000"` when it answered nothing.
///
/// `curl` through our own CA with an explicit `--resolve`, because that is what
/// every example that talks to the edge already does and a second mechanism
/// here would be a second set of TLS behaviours to reason about.
pub fn https_status(host: &str, port: u16, ca_pem: &Path) -> String {
    let out = std::process::Command::new("curl").args(["--max-time", "60"])
        .args([
            "-s",
            "--resolve",
            &format!("{host}:{port}:127.0.0.1"),
            "--cacert",
            &ca_pem.display().to_string(),
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!("https://{host}:{port}/"),
        ])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "000".to_string(),
    }
}

/// Wait for the edge to ANSWER, which is not what [`await_listening`] proves.
///
/// `await_listening` proves the socket ACCEPTS. Caddy binds its listener before
/// it has finished loading certificates and routes, so a request made in that
/// window comes back `000` — curl's "no HTTP response at all" — and the example
/// reads it as the SITE being broken. Measured 20 Aug 2026 in
/// `frankenphp_edge_serve`: the edge logged `enabling HTTP/3 listener addr
/// :8443` and the run was over 245ms later with both sites at 000.
///
/// The flat sleeps this replaces hid the window by being generous, which is the
/// honest reason a sleep sometimes "works": it is not a check, but it is a long
/// one. The answer is not a longer sleep — it is polling the fact the assertions
/// depend on.
///
/// **Not circular.** This waits for ANY status; the assertions afterwards demand
/// 200. A 502 ends the wait immediately and fails on its own merits, which is
/// the difference between a readiness gate and a retry loop that hides a bug.
pub fn await_answering(host: &str, port: u16, ca_pem: &Path, what: &str) {
    await_ready(what, None, || https_status(host, port, ca_pem) != "000");
}

/// One loopback HTTP/1.1 GET with an explicit Host header (how the shared
/// nginx routes vhosts). Returns the raw response (headers + body), or the
/// error as a string — callers assert on content either way. Deliberately
/// std-only: a probe with its own connection pool would hide first-connection
/// failures.
pub fn http_get(port: u16, host: &str, path: &str) -> String {
    http_get_timeout(port, host, path, Duration::from_secs(5))
}

/// [`http_get`] with an explicit client read timeout — for probes that are
/// deliberately SLOWER than a server-side default under test (nginx's own
/// 60s `fastcgi_read_timeout`, say), where the 5s default would time out the
/// prober instead of the thing being proven.
pub fn http_get_timeout(port: u16, host: &str, path: &str, timeout: Duration) -> String {
    use std::io::{Read, Write};
    let run = || -> std::io::Result<String> {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port))?;
        s.set_read_timeout(Some(timeout))?;
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")?;
        let mut out = String::new();
        s.read_to_string(&mut out)?;
        Ok(out)
    };
    run().unwrap_or_else(|e| format!("<probe error: {e}>"))
}

// ---------------------------------------------------------------------------
// Uniform verdict reporting
// ---------------------------------------------------------------------------

/// The one pass/fail contract every example should speak: ✓/✗ lines while
/// running, one `<name>: PASS`/`FAIL` line at the end, exit code 0/1 — so a
/// runner (scripts/live-checks.sh) can trust any example's exit status.
///
/// End `main` with `return checks.verdict();` (signature
/// `fn main() -> std::process::ExitCode`) rather than `std::process::exit` —
/// exit skips destructors, and the whole point of [`Reaped`]/[`SandboxGuard`]/
/// [`FixtureDb`] is that they run.
pub struct Check {
    name: &'static str,
    failed: u32,
}

impl Check {
    pub fn new(name: &'static str) -> Self {
        Self { name, failed: 0 }
    }

    /// Record one named assertion. `detail` prints only on failure.
    pub fn is(&mut self, label: &str, pass: bool, detail: &str) {
        if pass {
            println!("  ✓ {label}");
        } else {
            println!("  ✗ {label} — {detail}");
            self.failed += 1;
        }
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Print the one green/red line and return the exit code for `main`.
    pub fn verdict(self) -> std::process::ExitCode {
        if self.failed == 0 {
            println!("{}: PASS", self.name);
            std::process::ExitCode::SUCCESS
        } else {
            println!("{}: FAIL ({} check(s))", self.name, self.failed);
            std::process::ExitCode::FAILURE
        }
    }
}

/// A spawned check process that is reaped when it goes out of scope — including
/// when an assertion panics and unwinds.
///
/// Owns the [`Child`] exclusively: nothing else may `wait`/`kill` it, so the pid
/// can never be signalled after it has been reaped (and possibly reused).
///
/// `std::process::exit` does NOT run destructors, so an example that exits
/// non-zero must call [`Reaped::reap`] explicitly on that path — it is
/// idempotent, and the guard stays as the panic backstop.
pub struct Reaped {
    proc: Option<Proc>,
    pid: u32,
    /// Dedicated fixture port this service listens on. Swept after the master
    /// dies, since a forking service's workers can outlive it.
    port: u16,
    /// Program name as it appears in `ps` output — see the module note on
    /// title-rewritten workers.
    marker: String,
}

impl Reaped {
    /// Take exclusive ownership of `child`, listening on the FIXTURE `port`.
    ///
    /// `marker` must be the program name (`php-fpm`, `httpd`), not a path.
    /// `port` must be a throwaway port the example dedicates to this check —
    /// the sweep is reachable only through this type precisely so it can never
    /// be pointed at a port the running stack owns.
    pub fn new(child: Child, port: u16, marker: impl Into<String>) -> Self {
        let pid = child.id();
        Self {
            proc: Some(Proc::Child(child, Instant::now())),
            pid,
            port,
            marker: marker.into(),
        }
    }

    /// Take ownership of a handle a core start function already produced.
    ///
    /// `DbEngine::start` returns a `Proc`, not a `Child`: PostgreSQL is launched
    /// through `pg_ctl` where a plain spawn could carry an admin token, and that
    /// detaches (ledger #698). The sweep works either way — it stops by pid and
    /// then frees the port.
    pub fn from_proc(proc: Proc, port: u16, marker: impl Into<String>) -> Self {
        let pid = proc.id();
        Self { proc: Some(proc), pid, port, marker: marker.into() }
    }

    /// The spawned master's pid — valid for printing after reaping too.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Stop the service and free its port. Idempotent.
    pub fn reap(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            proc.terminate();
            sweep_port(self.port, &self.marker);
        }
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        self.reap();
    }
}

/// Own a service that listens on a SHARED (production) port and stop it on drop
/// — including when the example panics.
///
/// **Why not [`Reaped`].** That type also sweeps its port for title-rewritten
/// workers, and the sweep decides ownership by PROGRAM NAME. On a fixture port
/// that is unambiguous. On 9783 or 18088 the user's own php-fpm and nginx match
/// the same names, so pointing the sweep there could kill the running stack —
/// which is exactly what `Reaped`'s contract forbids. Ownership here is the
/// `Child` handle: a process THIS example spawned, never ambiguous.
///
/// **What it does not cover, stated because the gap is the interesting part.**
/// [`Proc::terminate`] sends SIGTERM and only escalates to SIGKILL after a
/// grace period, and a master that shuts down on SIGTERM takes its workers with
/// it. A master that ignores SIGTERM gets killed and CAN leave workers holding
/// the port — the case `Reaped`'s sweep exists for. Pair this with
/// [`require_ports_free`] so that residue surfaces in the NEXT run as a refusal
/// naming the port, rather than as a service quietly borrowed from a corpse.
///
/// Written 14 Aug 2026 after `wp_create_serve` panicked before its teardown
/// lines and left mysqld, nginx and php-fpm running — while its sandbox datadir
/// was removed on drop, leaving a MySQL answering on a directory that no longer
/// existed. Three later examples connected to it and failed with
/// `ERROR 3680: Failed to create schema directory (errno 2)`, which names
/// nothing, and the wrong three examples got blamed.
pub struct OwnedService {
    proc: Option<Proc>,
    pid: u32,
    what: &'static str,
}

impl OwnedService {
    /// Take exclusive ownership of a service `child`. `what` is for messages.
    pub fn new(child: Child, what: &'static str) -> Self {
        let pid = child.id();
        Self { proc: Some(Proc::Child(child, Instant::now())), pid, what }
    }

    /// The spawned pid — valid for printing after stopping too.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Stop the service. Idempotent; runs from `Drop` on the panic path too.
    pub fn stop(&mut self) {
        if let Some(mut proc) = self.proc.take() {
            proc.terminate();
        }
    }

    /// Name, for a teardown line that says what it stopped.
    pub fn what(&self) -> &'static str {
        self.what
    }
}

impl Drop for OwnedService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Kill anything still LISTENING on `port` whose command line contains
/// `marker` — the orphaned workers a dead master leaves behind.
///
/// Deliberately private: reachable only via [`Reaped`], so a sweep always
/// belongs to a process this example spawned on a port it chose.
fn sweep_port(port: u16, marker: &str) {
    let ours = |pid: u32| command_of(pid).contains(marker);

    for pid in listeners_on(port).into_iter().filter(|p| ours(*p)) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    for _ in 0..20 {
        if listeners_on(port).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for pid in listeners_on(port).into_iter().filter(|p| ours(*p)) {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

/// Pids listening on `port` (empty when the port is free or `lsof` is absent).
fn listeners_on(port: u16) -> Vec<u32> {
    let out = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// A pid's full command line, or "" if it is gone.
fn command_of(pid: u32) -> String {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Public tunnels
// ---------------------------------------------------------------------------

/// The live tunnel's pid, reachable from every teardown path (a static, because
/// `fail()` and a panic hook cannot be handed a value).
/// Every adopted tunnel, not just the last one.
///
/// **This was a single `AtomicU32`, and the second tunnel silently orphaned the
/// first** (24 Aug 2026). `tunnel_exposure_check` gained a leg that starts a
/// SECOND quick tunnel — for an override site's own backend port — and the new
/// adoption overwrote the slot. The teardown then reaped only the newer pid; the
/// first tunnel survived the run with a LIVE PUBLIC URL, and the leg that checks
/// "stopping the share really unpublishes it" failed pointing at its own URL,
/// which is the guard reporting the exact damage it had just done.
///
/// A guard whose whole promise is "no tunnel outlives this run" cannot hold that
/// promise with room for one. Both are tracked now, and both are reaped.
static TUNNELS: std::sync::Mutex<Vec<(u32, String)>> = std::sync::Mutex::new(Vec::new());
static HOOK_INSTALLED: std::sync::Once = std::sync::Once::new();

/// Guard for a tunnel that is PUBLIC while it runs.
///
/// A leaked cloudflared is the one outcome an example must not produce, and the
/// three paths that end a run today all skip something:
///
/// - normal scope exit → `Drop` runs;
/// - `fail()` → `process::exit` runs NO destructors (this has bitten twice —
///   a Mailpit guard, then this file's own tunnel), so `fail` must reap first;
/// - a panic → `Drop` runs while unwinding, but only if the guard is still in
///   scope, so a hook is installed as well.
///
/// [`tunnel_guard_check`](../tunnel_guard_check.rs) proves all three by running
/// this example's own binary in each mode and asserting the child is dead
/// afterwards — arranged, not assumed.
///
/// # The guard's own failure mode
///
/// `tunnels::stop` used to be called as `let _ = …`: best-effort, silent. If the
/// tunnel did not die, nothing said so and a public URL stayed up. So this reaps
/// in three escalating steps and PROVES the outcome — stop, then SIGTERM, then
/// SIGKILL, polling for the process to actually go. If it is still alive after
/// all three, the example SHOUTS the pid and the URL, because at that point the
/// only remaining remedy is a human with a terminal.
pub struct PublicTunnel(());

/// Register a live tunnel and install the panic hook. Hold the returned guard
/// for the rest of the run.
pub fn adopt_public_tunnel(pid: u32, public_url: &str) -> PublicTunnel {
    if let Ok(mut t) = TUNNELS.lock() {
        t.push((pid, public_url.to_string()));
    }
    HOOK_INSTALLED.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            reap_public_tunnel();
            prev(info);
        }));
    });
    eprintln!("public tunnel adopted: pid {pid} at {public_url}");
    PublicTunnel(())
}

/// Record the public URL once it is known. A quick tunnel's URL does not exist
/// until cloudflared prints it, so the guard is adopted BEFORE the URL — the pid
/// is the thing that must be reachable from the teardown paths, and a tunnel
/// with an unknown URL still has to die.
pub fn note_public_tunnel_url(url: &str) {
    // Fills in the MOST RECENTLY adopted tunnel's URL — the one whose URL was
    // unknown when it was adopted. With more than one in flight, naming which is
    // the difference between a teardown message that identifies the leak and one
    // that names somebody else's URL.
    if let Ok(mut t) = TUNNELS.lock() {
        if let Some(last) = t.last_mut() {
            last.1 = url.to_string();
        }
    }
}

/// Alive means RUNNING — a zombie counts as dead, exactly as production's
/// `process_running` counts it. This cannot be `kill -0`: that succeeds on a
/// zombie, and the tunnel pid here is this example's OWN child, which nothing
/// ever `wait()`s — so from the moment cloudflared exits until the example
/// does, `kill -0` answers "alive" about a corpse. Found 15 Aug 2026 when the
/// guard's evidence file recorded `tunnel_guard_check`'s sleep stand-in as
/// surviving SIGKILL (it was a zombie throughout); the 14 Aug "still alive 3s
/// after tunnels::stop" sighting has the same measurement under it.
fn pid_alive(pid: u32) -> bool {
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()
    {
        Ok(o) => {
            let state = String::from_utf8_lossy(&o.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        }
        Err(_) => false,
    }
}

fn gone_within(pid: u32, ms: u64) -> bool {
    wait_dead(pid, ms).is_some()
}

/// Poll until `pid` is gone; `Some(elapsed ms)` if it died within `ms`.
fn wait_dead(pid: u32, ms: u64) -> Option<u64> {
    let start = Instant::now();
    loop {
        if !pid_alive(pid) {
            return Some(start.elapsed().as_millis() as u64);
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Append one reap observation to the persistent evidence file for the
/// "does `tunnels::stop` routinely need SIGTERM?" question (docs/TODO.md,
/// filed 14 Aug 2026 on ONE sighting).
///
/// This file exists because the original plan — "watch the guard's output over
/// the next few tunnel runs" — could not work: a successful `live-checks.sh`
/// run DELETES its log directory, so the only sightings that persist are ones
/// a human happened to be watching. Every reap now leaves a line here, fast
/// path included — the base rate ("N runs, all dead under a second") is itself
/// evidence, and it is exactly what a one-sighting question needs.
///
/// Lives in `.evidence/` at the repo root: machine-local, gitignored, and — the
/// part that matters — OUTSIDE `target/`.
///
/// **It used to live under `target/`, which `cargo clean` deletes.** The whole
/// value of this file is accumulation: a single sighting nobody can compare is
/// what the question started as, and 14 lines of `outcome=stop` are what
/// answered it. Putting the only record of a rare event in the directory a
/// developer wipes routinely means the next recurrence is a single observation
/// again — the exact state the recording exists to prevent, restored by a
/// housekeeping command nobody would think to mention. Found 26 Aug 2026 while
/// closing the row this file feeds.
fn record_tunnel_stop_evidence(pid: u32, outcome: &str, elapsed_ms: u64) {
    use std::io::Write;
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.evidence"));
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("tunnel-stop-evidence.log");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let example = std::env::args()
        .next()
        .map(|a| {
            Path::new(&a).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(a)
        })
        .unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "epoch={epoch} example={example} pid={pid} outcome={outcome} elapsed_ms={elapsed_ms}");
    }
}

/// Stop the registered tunnel and PROVE it stopped. Idempotent; safe from
/// `Drop`, from `fail()`, and from the panic hook.
pub fn reap_public_tunnel() {
    // DRAIN: every adopted tunnel, newest first, so a run that started two does
    // not leave one public. Draining under the lock means a panic hook and a
    // Drop racing to tear down cannot both take the same pid.
    let adopted: Vec<(u32, String)> = match TUNNELS.lock() {
        Ok(mut t) => t.drain(..).rev().collect(),
        Err(_) => return,
    };
    for (pid, url) in adopted {
        reap_one_public_tunnel(pid, &url);
    }
}

fn reap_one_public_tunnel(pid: u32, url: &str) {
    let url = url.to_string();
    let plat = rexenv_lib::platform::current();

    let start = Instant::now();
    let _ = rexenv_lib::core::tunnels::stop(&*plat, pid);
    if let Some(ms) = wait_dead(pid, 3000) {
        record_tunnel_stop_evidence(pid, "stop", ms);
        eprintln!("public tunnel reaped: pid {pid} stopped in {ms}ms (verified dead)");
        return;
    }
    // The production stop path did not take within 3s. Observed once on the
    // first real run of this guard (14 Aug 2026), and whether that was a
    // defect in `tunnels::stop` or a graceful shutdown slower than the window
    // could not be told apart — signalling immediately DESTROYS the evidence,
    // because a death right after SIGTERM is indistinguishable from the
    // earlier stop still finishing. So the anomalous path now watches a
    // further 7s before escalating: a death in the 3–10s window with no signal
    // sent is "graceful but slow", recorded as such; survival past 10s is the
    // stop path genuinely not taking. The cost is bounded (≤7s more of a
    // teardown that is already anomalous) and only ever paid on the case the
    // TODO question is about. The previous `let _ = tunnels::stop(..)` would
    // have returned quietly with the process still up and the URL still public.
    eprintln!(
        "public tunnel pid {pid} still alive 3s after tunnels::stop — \
         watching to 10s before escalating (slow-graceful vs stop-defect)"
    );
    if wait_dead(pid, 7000).is_some() {
        let total = start.elapsed().as_millis() as u64;
        record_tunnel_stop_evidence(pid, "stop_slow_no_signal", total);
        eprintln!(
            "public tunnel reaped: pid {pid} stopped in {total}ms with NO signal sent — \
             tunnels::stop worked, just slower than 3s (recorded)"
        );
        return;
    }
    record_tunnel_stop_evidence(pid, "stop_no_effect_10s", start.elapsed().as_millis() as u64);
    eprintln!("public tunnel pid {pid} still alive 10s after tunnels::stop — escalating to SIGTERM");
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    if gone_within(pid, 3000) {
        record_tunnel_stop_evidence(pid, "sigterm", start.elapsed().as_millis() as u64);
        eprintln!("public tunnel reaped: pid {pid} stopped after SIGTERM (verified dead)");
        return;
    }
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    if gone_within(pid, 3000) {
        record_tunnel_stop_evidence(pid, "sigkill", start.elapsed().as_millis() as u64);
        eprintln!("public tunnel reaped: pid {pid} stopped after SIGKILL (verified dead)");
        return;
    }
    record_tunnel_stop_evidence(pid, "survived_all", start.elapsed().as_millis() as u64);
    eprintln!(
        "\n!! A PUBLIC TUNNEL IS STILL RUNNING AND THIS EXAMPLE COULD NOT STOP IT !!\n  \
         pid {pid} survived tunnels::stop, SIGTERM and SIGKILL.\n  \
         public URL: {url}\n  \
         It is reachable from the internet until someone kills it by hand:\n    \
         kill -9 {pid}\n"
    );
}

impl Drop for PublicTunnel {
    fn drop(&mut self) {
        reap_public_tunnel();
    }
}

// ── Unix file-mode and symlink fixtures ─────────────────────────────────────
// Examples are macOS live checks, but they compile wherever the lib does, and
// the Windows port's compile check builds them (docs/PLAN-windows-port.md W1).
// These keep the unix-only std extensions in one place. Off unix they are inert
// — a mode of 0, a no-op chmod, an Unsupported symlink — which is honest for a
// check nobody runs there yet.

/// `path`'s permission bits (`mode & 0o777`), or 0 when they cannot be read —
/// and always 0 off unix.
pub fn mode_bits(path: impl AsRef<std::path::Path>) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

/// chmod `path` to `mode`. A no-op off unix.
pub fn set_mode(path: impl AsRef<std::path::Path>, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// A symlink `dst` → `src`. Unsupported off unix.
pub fn symlink(src: impl AsRef<std::path::Path>, dst: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dst);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlink fixtures are unix-only"))
    }
}
