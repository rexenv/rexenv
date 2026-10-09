//! core::sites — site lifecycle domain logic (PLATFORM-AGNOSTIC).
//!
//! Phase 1 task 1.2: create / list / get / delete persisted via `state::store`.
//! Later tasks extend `create` to also generate the docroot, cert, vhost, and
//! edge-router route (§7); this module stays the single entry point for site
//! operations so commands/ remain thin.

use crate::core::{
    adminer, apache, binaries, frankenphp, php, proxy, repo, services, ssl, stopped_page, tld,
    tunnels,
};
use crate::error::{Error, Result};
use crate::platform::traits::{Platform, ProcessSupervisor};
use crate::state::models::{
    MultisiteMode, NewSite, ServiceStatus, Site, SiteDbEngine, SiteOrigin, SiteType, WebServer,
};
use crate::state::store;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Strict validation for a site domain (defense-in-depth — M7). The domain becomes a
/// filesystem path (docroot), nginx/Caddy config tokens (`server_name`, host), a cert
/// SAN, and a database name — so reject anything that could traverse a path, inject a
/// config directive, or isn't a plain lowercase hostname. The TLD is POLICY-DRIVEN
/// (`core::tld`): hard-blocked TLDs (`.local`, `.dev`, 2-letter, popular gTLDs) are
/// refused here — the trust boundary for BOTH create and change-domain — so a blocked
/// TLD can't get through even via a direct IPC invoke. The UI already slugs input to
/// this shape; this is the backstop for every other caller.
pub(crate) fn validate_domain(domain: &str) -> Result<()> {
    let reject = |why: &str| Error::Other(format!("invalid domain '{domain}': {why}"));
    // DNS caps a name at 253 chars; stay well under any fs/DB-identifier limit too.
    if domain.is_empty() || domain.len() > 253 {
        return Err(reject("must be 1–253 characters"));
    }
    let labels: Vec<&str> = domain.split('.').collect();
    // At least one label before the TLD, and a TLD the policy allows.
    if labels.len() < 2 {
        return Err(reject("must have a label before the TLD (e.g. mysite.rex)"));
    }
    tld::ensure_allowed(labels.last().expect("len >= 2"))?;
    for label in &labels {
        if label.is_empty() {
            return Err(reject("has an empty label"));
        }
        if label.len() > 63 {
            return Err(reject("has a label over 63 characters"));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(reject("has a label starting or ending with '-'"));
        }
        // a–z, 0–9, hyphen only: blocks path separators, spaces, wildcards, quotes,
        // ';'/'{'/newlines (config injection), and uppercase (case-dup + fs/DB drift).
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(reject("labels may contain only a–z, 0–9, and '-'"));
        }
    }
    Ok(())
}

/// The web servers a site can actually be created with on this build, in the order the New
/// Site dialog offers them.
///
/// Derived from the same gate that REFUSES (`ensure_server_available_on`), so the picker cannot
/// offer what create would reject: the dialog held its own hardcoded list, which on Windows
/// meant Apache was offered and then refused at create time — honest, but late, and the same
/// "the frontend keeps a second copy" shape the PHP rows already avoid (W10, ledger #643).
pub fn offered_web_servers() -> Vec<WebServer> {
    offered_web_servers_on(std::env::consts::OS)
}

/// [`offered_web_servers`] for a NAMED os — a parameter for the same reason every other gate
/// in W10 takes one: the bar only `cargo check`s for Windows, so a list built from
/// `std::env::consts::OS` alone would have a half that cannot fail on the machine running the
/// test, and a plant against it would read PASSED rather than proving anything.
pub fn offered_web_servers_on(os: &str) -> Vec<WebServer> {
    [WebServer::Nginx, WebServer::Frankenphp, WebServer::Apache, WebServer::Openlitespeed]
        .into_iter()
        .filter(|s| ensure_server_available_on(*s, os).is_ok())
        .collect()
}

/// Servers with a real backend on a NAMED os — refused here in CORE otherwise, so no IPC path
/// can create a site the stack would silently serve through nginx while claiming another
/// server (M7). OpenLiteSpeed was refused everywhere until its own build landed
/// (`rexenv/runtimes` `openlitespeed-1.9.3-1`, 4 Oct 2026); it is now pinned for macOS and
/// Linux and still refused on Windows, where no OpenLiteSpeed build exists at all (ledger #774).
///
/// The os is a parameter so BOTH answers can be measured from either host. The bar only
/// `cargo check`s for Windows, so a gate that read `std::env::consts::OS` directly would
/// have an untestable half until W12's Windows runner, and an untestable refusal is one
/// nobody has seen refuse (W10, ledger #642). There is no host-reading wrapper beside it:
/// every caller either takes the os already (`refuse_unbuildable_on`, `set_web_server_on`,
/// `offered_web_servers_on`) or is a test naming the os it means — the wrapper that used
/// to sit here became unreachable the moment the create path took the parameter (W12).
fn ensure_server_available_on(server: WebServer, os: &str) -> Result<()> {
    // Nginx is the one server that ships everywhere. The other two are asked of the pins
    // rather than listed here: FrankenPHP and Apache each have a macOS artifact and no
    // Windows one — Apache on Windows would mean Apache Lounge, a third-party trust decision
    // D4 left out of v1 — so both refuse there, and both appear the day a pin lands, without
    // this function being edited.
    let shipped = match server {
        WebServer::Nginx => true,
        WebServer::Frankenphp => {
            binaries::ships_on("frankenphp", binaries::pins().frankenphp, os)
        }
        WebServer::Apache => binaries::ships_on("httpd", binaries::pins().httpd, os),
        WebServer::Openlitespeed => {
            binaries::ships_on("openlitespeed", binaries::pins().openlitespeed, os)
        }
    };
    if shipped {
        Ok(())
    } else if server == WebServer::Openlitespeed {
        // Not "yet": OpenLiteSpeed is a POSIX server with no Windows port anywhere. The
        // sentence is the platform's, so it can say what is true of THIS OS.
        Err(Error::Other(crate::platform::words::for_os(os).openlitespeed_unavailable.to_string()))
    } else {
        Err(Error::Other(format!(
            "web server {} is not available on this platform yet",
            server.as_db()
        )))
    }
}

/// A FrankenPHP site is served by the PHP compiled INTO FrankenPHP, not by the
/// site's php-fpm pool — so `php_version` is a promise FrankenPHP cannot keep.
///
/// Refused when the MAJOR versions differ, which is the line where "ignored"
/// becomes "broken": running an 8.1 codebase on 8.5 is a version skew, running a
/// **7.4** codebase on 8.5 is a different language — the removals PHP 8.0 made
/// are exactly why a site is still pinned to 7.4. The rule compares data rather
/// than naming versions, so it stays true when either pin moves.
///
/// Scope, stated: the same-major mismatch (an 8.1 site served by 8.5) is NOT
/// refused here. Refusing it would break FrankenPHP sites that work today, so
/// it was ANSWERED by disclosure instead, on 15 Aug 2026 (ledger #333): the
/// SiteDetail Environment card shows the SERVED version, disables the picker,
/// and says "Fixed by FrankenPHP. Switch the web server to Nginx or Apache to
/// choose a version." This comment said "tracked in docs/TODO.md" for six days
/// after that shipped, pointing at a row that no longer existed — a dangling
/// pointer describing an open gap that was closed.
fn ensure_server_runs_php(server: WebServer, php_version: &str) -> Result<()> {
    if server != WebServer::Frankenphp {
        return Ok(());
    }
    let major = |v: &str| v.split('.').next().unwrap_or_default().to_string();
    let embedded = binaries::pins().frankenphp_embedded_php;
    if major(php_version) == major(embedded) {
        return Ok(());
    }
    Err(Error::Other(format!(
        "FrankenPHP embeds its own PHP {} and cannot run PHP {}. A site on {} would be \
         served by {} instead — silently. Use Nginx or Apache for this site.",
        php::minor_of(embedded),
        php::minor_of(php_version),
        php::minor_of(php_version),
        php::minor_of(embedded),
    )))
}

/// WordPress may never be backed by PostgreSQL — refused HERE, at the one
/// function every site insert passes through, not in the New-site dialog.
///
/// `wpdb` speaks mysqli / PDO-MySQL and nothing else: a WordPress site on
/// PostgreSQL is not a limited site, it is a site whose first query fails, with
/// a database already created and a docroot already written. The UI simply not
/// offering the pair is not the guard — `rex site create --db postgres` and the
/// MCP `create_site` tool both parse the engine out of a string and reach this
/// function with whatever the caller sent.
///
/// Laravel and Blank PHP have no such constraint from the FRAMEWORK: `pgsql` is
/// a first-class Laravel driver and a PDO DSN prefix. They are refused only on a
/// PHP MINOR whose build cannot reach PostgreSQL through PDO
/// ([`crate::core::php::pdo_pgsql_supported`] — 7.4 and 8.0 today). **Two
/// refusals, two lifetimes and two subjects**: WordPress's is permanent and
/// about `wpdb`; this one is about the runtime the site happens to run, and the
/// same site on 8.1 is fine (docs/archive/PLAN-postgres-sites.md §b).
fn ensure_engine_supports(
    site_type: SiteType,
    engine: SiteDbEngine,
    php_patch: &str,
) -> Result<()> {
    if engine != SiteDbEngine::Postgres {
        return Ok(());
    }
    if site_type == SiteType::Wordpress {
        return Err(Error::Other(
            "WordPress cannot run on PostgreSQL — core's database layer speaks MySQL only. \
             Use MySQL or MariaDB for a WordPress site; PostgreSQL is available for Laravel \
             and Blank PHP sites."
                .into(),
        ));
    }
    // …and for the two types that COULD use it, the remaining blocker is the
    // RUNTIME rather than the framework: Laravel's `pgsql` driver and the
    // Blank-PHP starter's `db.php` both go through PDO, and the builds for 7.4
    // and 8.0 have no working `pdo_pgsql` (measured — see
    // `php::pdo_pgsql_supported`). Refusing at create time is the honest shape:
    // the alternative is a site that provisions cleanly, gets a database and an
    // `.env`, and then hangs on its first query with an error naming neither PHP
    // nor rexenv. The refusal names the WAY OUT, because unlike the WordPress
    // one there is one: choose a newer PHP.
    //
    // The PATCH decides it, not the minor: rexenv runs patches it did not build
    // (the update manifest offers upstream's newer ones), and asking per minor
    // said "8.3 is fine" about a machine running 8.3.32 — the site was created
    // and `artisan migrate` then spun at 100% CPU for minutes (ledger #550).
    if !crate::core::php::pdo_pgsql_supported(php_patch) {
        return Err(Error::Other(format!(
            "PHP {php_patch} cannot reach PostgreSQL — a PostgreSQL site talks to its \
             database through PDO, and that build has no working `pdo_pgsql` (it advertises \
             the driver and then hangs; measured against PostgreSQL 16, 17 and 18). Use PHP \
             {} or newer for this site, or MySQL/MariaDB on this one.",
            crate::core::php::oldest_pdo_pgsql_minor()
        )));
    }
    Ok(())
}

/// The per-site override backend port for `server`, or `None` for nginx (which
/// has no per-site port — it vhosts by `server_name` on the shared stack).
/// FrankenPHP/Apache hash the domain into a small loopback range, so two domains
/// of the same server type can land on the same port.
fn override_port(domain: &str, server: WebServer) -> Option<u16> {
    match server {
        WebServer::Frankenphp => Some(super::frankenphp::site_port(domain)),
        WebServer::Apache => Some(super::apache::site_port(domain)),
        WebServer::Openlitespeed => Some(super::openlitespeed::site_port(domain)),
        WebServer::Nginx => None,
    }
}


/// Clear, actionable error for an override-port collision (B20).
fn override_port_collision_error(domain: &str, other: &str, port: u16) -> Error {
    Error::Other(format!(
        "can't use this web server for \"{domain}\": its backend port ({port}) collides with \
         \"{other}\", so the two sites would share one server. Rename this site or give it a \
         different web server."
    ))
}

/// The override backend port range for `server` (`base`, `count`), or `None` for
/// nginx (no per-site port). FrankenPHP and Apache ranges are DISJOINT so the two
/// types can never collide.
fn override_range(server: WebServer) -> Option<(u16, u16)> {
    match server {
        WebServer::Frankenphp => Some((super::frankenphp::FRANKENPHP_BASE_PORT, 100)),
        WebServer::Apache => Some((super::apache::APACHE_BASE_PORT, 100)),
        WebServer::Openlitespeed => Some((super::openlitespeed::OPENLITESPEED_BASE_PORT, 100)),
        WebServer::Nginx => None,
    }
}

/// The AUTHORITATIVE override backend port for a site: the RECORDED port once
/// allocated (B20 §4 — never re-derived, so a domain change can't orphan the
/// running backend), falling back to the derived `site_port(domain)` ONLY in the
/// transitional window before the one-time backfill records it. For a
/// non-colliding site the fallback value EQUALS the recorded value, so consumers
/// see no change across the backfill. `None` for nginx. Every port consumer reads
/// this — never `site_port` directly — so post-backfill the recorded port is
/// authoritative and a later domain change cannot orphan the backend.
pub fn recorded_override_port(site: &Site) -> Option<u16> {
    site.override_port.or_else(|| override_port(&site.domain, site.web_server))
}

/// Allocate a collision-free override backend port for `server` given the other
/// sites (their recorded/derived ports are the taken set) — the LOWEST free port
/// in the range. `Ok(None)` for nginx; `Err` only if all 100 slots are in use.
fn allocate_override_port(others: &[Site], server: WebServer) -> Result<Option<u16>> {
    let Some((base, count)) = override_range(server) else {
        return Ok(None); // nginx has no per-site port
    };
    let taken: std::collections::HashSet<u16> =
        others.iter().filter_map(recorded_override_port).collect();
    match (base..base + count).find(|p| !taken.contains(p)) {
        Some(p) => Ok(Some(p)),
        None => Err(Error::Other(format!(
            "no free {} backend port — all {count} slots from {base} are in use",
            server.as_db()
        ))),
    }
}

/// B20-A guard, KEPT and reworked to the recorded model: the domain of any site
/// OTHER than `self_id` that already records `port`, or `None`. A belt on the
/// allocator (which picks a free port, so this never fires) — the permanent
/// safety net against two override sites sharing one backend.
fn recorded_port_conflict(others: &[Site], self_id: &str, port: u16) -> Option<String> {
    others
        .iter()
        .filter(|s| s.id != self_id)
        .find(|s| recorded_override_port(s) == Some(port))
        .map(|s| s.domain.clone())
}

/// One-time idempotent backfill (B20 §4, Phase B): record each override site's
/// port. A non-colliding site gets its EXACT current derived port — zero
/// disruption, since the running backend + edge route already use it. A
/// pre-existing collision (two domains hashing to the same slot — already the B20
/// bug, one bleeding into the other) is resolved by giving the SECOND site (by
/// `created_at, id`) a free port while the FIRST keeps its derived port; the
/// second was already broken, so its own backend on the next start is a FIX. Only
/// touches rows with a NULL `override_port` and an override server, in ONE
/// transaction (crash → rollback → clean re-run), run at startup before any
/// backend spawns. No UNIQUE constraint (B21) — uniqueness is enforced HERE,
/// collisions resolved not rejected, so it can't brick on existing data.
pub fn backfill_override_ports(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let mut sites = store::list_sites(&tx)?;
    // Deterministic: oldest first keeps the derived port on a collision.
    sites.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    // Seed with already-recorded ports so a re-run (idempotent) can't reassign
    // one that's live, and earlier rows in THIS pass reserve theirs.
    let mut taken: std::collections::HashSet<u16> =
        sites.iter().filter_map(|s| s.override_port).collect();
    for s in &sites {
        if s.override_port.is_some() {
            continue; // already recorded — idempotent skip
        }
        let Some((base, count)) = override_range(s.web_server) else {
            continue; // nginx — no port to record
        };
        let derived =
            override_port(&s.domain, s.web_server).expect("override server has a derived port");
        let port = if !taken.contains(&derived) {
            derived // non-colliding: preserve the EXACT current port (zero disruption)
        } else {
            (base..base + count).find(|p| !taken.contains(p)).ok_or_else(|| {
                Error::Other(format!("no free {} backend port during backfill", s.web_server.as_db()))
            })? // pre-existing collision: the later site moves to a free port
        };
        taken.insert(port);
        store::set_site_override_port(&tx, &s.id, Some(port))?;
    }
    tx.commit()?;
    Ok(())
}

/// The LEGACY lexical ownership test: is `path` under a managed sites root?
///
/// Factored out so the v17 backfill and the pre-backfill fallback can't drift
/// apart. This is no longer a delete-time guard — reading it at delete time is
/// exactly the bug v17 fixes, since `configured` comes from a setting the user
/// can change after the site was created.
fn docroot_under_managed_root(
    path: &str,
    configured: &Path,
    default_dir: &Path,
    legacy_dir: &Path,
) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && (p.starts_with(configured) || p.starts_with(default_dir) || p.starts_with(legacy_dir))
}

/// Record, ONCE, whether rexenv owns each existing site's docroot (v17 Phase B).
///
/// Evaluates the legacy lexical test — is the docroot under the configured
/// sites dir, the `~/rexenv/Sites` default, or the legacy app-data one — and
/// FREEZES its answer on the row, so deletion stops depending on a setting the
/// user can change afterwards. Every pre-v17 row therefore keeps exactly
/// today's behavior, including docroots moved outside the sites folder, which
/// stay preserved as the move dialog promises.
///
/// Only touches NULL rows, in ONE transaction (crash → rollback → clean
/// re-run), run at startup before any site is served. Idempotent: a recorded
/// row is never revisited, so a later sites-dir change can't flip an answer.
pub fn backfill_docroot_managed(conn: &Connection, platform: &dyn Platform) -> Result<()> {
    let configured = sites_dir(conn, platform)?;
    let default_dir = default_sites_dir()?;
    let legacy_dir = legacy_sites_dir(platform)?;
    let tx = conn.unchecked_transaction()?;
    for s in store::list_sites(&tx)? {
        if s.docroot_managed.is_some() {
            continue; // already recorded — idempotent skip
        }
        let managed =
            docroot_under_managed_root(&s.path, &configured, &default_dir, &legacy_dir);
        store::set_site_docroot_managed(&tx, &s.id, managed)?;
    }
    tx.commit()?;
    Ok(())
}

/// One-time v24 backfill: record each existing WordPress site's content dir
/// from the same fs markers detection uses (`detect_content_dir_rel`) — a
/// linked Bedrock/Radicle site created before v24 must not keep writing
/// mu-plugins into a dead `wp-content/`. Idempotent (NULL rows only); the
/// probe runs here ONCE and the answer is recorded, mirroring the v17
/// docroot-ownership backfill. Non-WP rows are skipped (they never consult
/// it; NULL stays honest).
pub fn backfill_content_dir(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for s in store::list_sites(&tx)? {
        if s.content_dir.is_some() || s.site_type != SiteType::Wordpress {
            continue; // already recorded / never consulted — idempotent skip
        }
        // Unreachable docroot (unmounted volume, temporarily missing linked
        // folder): every marker probes false and the record is SET-ONCE — a
        // wrong "wp-content" stamped here would condemn a Bedrock site to
        // the pre-v24 bug permanently (audit A3). Leave NULL; a later launch
        // with the volume mounted records the truth, and NULL already reads
        // as the safe default meanwhile.
        if !Path::new(&s.path).exists() {
            continue;
        }
        let rel = detect_content_dir_rel(Path::new(&s.path));
        store::set_site_content_dir(&tx, &s.id, rel)?;
    }
    tx.commit()?;
    Ok(())
}

/// Remove rexenv's mu-plugin artifacts from a site's docroot at the moments
/// the site ENDS or changes identity (step 6, 28 Jul 2026) — the linked-repo
/// lens: a preserved docroot must not keep files we wrote. Removes the tunnel
/// file (dead origin), the scratch-mail stamp (M2b), and the login file
/// (domain baked in; its owner rule:
/// lives while rexenv manages the site, rewritten by every issue, gone at
/// delete/rename) across EVERY known layout. `remove_dir` (delete only, not
/// rename — a still-managed site will likely recreate it): also remove the
/// `mu-plugins/` dir itself when RECORDED as ours (v25 `mu_dir_created`) and
/// empty again — never inferred from emptiness, a user's own empty dir is not
/// ours. Best-effort by design: every failure is logged, none blocks the
/// caller's teardown.
pub fn cleanup_muplugin_artifacts(site: &Site, remove_dir: bool) {
    let docroot = Path::new(&site.path);
    if let Err(e) = crate::core::wp_tunnel::disable(docroot) {
        log::warn!("sites: could not remove the tunnel mu-plugin for {}: {e}", site.domain);
    }
    if let Err(e) = crate::core::wp_login::remove(docroot) {
        log::warn!("sites: could not remove the login mu-plugin for {}: {e}", site.domain);
    }
    // The scratch-mail stamp (M2b). Not automatic: this sweep is a
    // hand-maintained list of the files rexenv owns, while its NAME claims all
    // of them — so a third owned mu-plugin had to be added here by hand, and
    // `every_owned_mu_plugin_is_swept_by_the_cleanup_that_claims_them_all`
    // now fails the build if a fourth is not.
    if let Err(e) = crate::core::wp_mailtag::disable(docroot) {
        log::warn!("sites: could not remove the scratch-mail mu-plugin for {}: {e}", site.domain);
    }
    // The loopback-DNS file. Domain-agnostic, so a RENAME does not need it
    // gone — but this sweep is also the site's exit, and a preserved docroot
    // must not keep files we wrote. The rename path re-installs it right after
    // calling this (`commands::sites::change_site_domain`).
    if let Err(e) = crate::core::wp_dns::remove(docroot) {
        log::warn!("sites: could not remove the loopback-DNS mu-plugin for {}: {e}", site.domain);
    }
    // The mail catcher. Domain-agnostic like the DNS file, and swept for the
    // same reason: a preserved docroot must not keep files we wrote.
    if let Err(e) = crate::core::wp_mail_catch::remove(docroot) {
        log::warn!("sites: could not remove the mail mu-plugin for {}: {e}", site.domain);
    }
    if remove_dir && site.mu_dir_created == Some(true) {
        remove_if_effectively_empty(&docroot.join(site.content_dir_rel()).join("mu-plugins"));
    }
}

/// Remove `dir` when it holds nothing but benign OS noise (`.DS_Store`,
/// `._*` — the established benign-basename set). Any real entry keeps the dir
/// untouched; a missing dir is a no-op.
fn remove_if_effectively_empty(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut noise = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".DS_Store" || name.starts_with("._") {
            noise.push(e.path());
        } else {
            return; // real content — not ours to judge
        }
    }
    for p in noise {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_dir(dir);
}

/// A UNIQUE, ≤64-char database name for a NEW site of THIS type (the prefix is
/// per type: `wp_`/`lv_`/`php_` — `wordpress::db_name_prefix`). Prefers the clean
/// [`wordpress::db_name_for`] base (what existing sites already store); falls
/// back to a hash-suffixed form when that base would COLLIDE with an existing
/// site or exceed MySQL's 64-char identifier limit. Without this, `db_name_for`
/// (not injective — `a-b.test` and `a.b.test` both reduce to `wp_a_b_test`)
/// would let two distinct domains silently share ONE database (data bleed, and
/// deleting either drops both — finding B21). The check-then-use is atomic under
/// the app-wide db lock that already serializes create (same as `domain_exists`).
fn unique_db_name(conn: &Connection, site_type: SiteType, domain: &str) -> Result<String> {
    let base = super::wordpress::db_name_for(site_type, domain);
    if base.len() <= super::wordpress::DB_NAME_MAX && !store::db_name_exists(conn, &base)? {
        return Ok(base);
    }
    // Base collides or overflows 64 chars — disambiguate with a hash of the full
    // domain. A remaining collision here needs two distinct domains to share
    // both the truncated slug AND the 32-bit hash (~1 in 4 billion): refuse
    // rather than risk a silent shared database.
    let disambiguated = super::wordpress::db_name_disambiguated(site_type, domain);
    if store::db_name_exists(conn, &disambiguated)? {
        return Err(Error::Other(format!(
            "could not derive a unique database name for '{domain}' — rename the site slightly"
        )));
    }
    Ok(disambiguated)
}

/// Create a site whose docroot rexenv OWNS — the folder we just created under
/// the sites dir, which teardown may remove.
///
/// Linking an existing folder goes through [`provision`], which records the
/// ownership explicitly; the flag is never taken from IPC input, so no caller
/// can claim ownership of a path it doesn't own and get it deleted later.
pub fn create(conn: &Connection, new: NewSite) -> Result<Site> {
    create_on(conn, new, std::env::consts::OS)
}

/// [`create`] as the user of a NAMED os would run it. Only the availability gates read
/// the os (see [`refuse_unbuildable_on`]); everything else about a site is the same
/// everywhere. Tests about FrankenPHP or Apache name `"macos"`, because those servers
/// have no Windows pin and `create` there refuses before the behaviour under test runs.
pub fn create_on(conn: &Connection, new: NewSite, os: &str) -> Result<Site> {
    let git = validate_git_source(&new, &Ownership::User)?;
    create_recording_ownership_on(conn, new, true, Ownership::User, git, os)
}

/// [`create`], with the docroot-ownership answer supplied by the caller that
/// KNOWS it (see [`Site::docroot_managed`](crate::state::models::Site)), and
/// the repository source already validated.
///
/// `git` is a PARAMETER rather than something re-derived here, and that is not
/// a style choice: by the time provisioning reaches this function it has
/// overwritten `new.path` with the docroot it resolved, so `path` no longer
/// means "the caller asked to link a folder" — re-running the rule here read
/// every cloned site as also-linked and refused it. The rule belongs where its
/// inputs still mean what they say.
/// The refusals that depend only on WHAT is asked for — server, PHP, engine —
/// never on the disk. Pure, so `provision_with` can run them before it creates
/// anything and the insert chokepoint can run them again for every other path.
/// Refuse a site whose DATABASE ENGINE does not ship on this OS.
///
/// W10 (#642) gated every surface that STARTS an engine — the Databases page, Services' rows,
/// the reserved-port list, adoption, the download plan, `start_database` and `spawn_db` — but a
/// site carries its engine in its ROW, and creation never asked. **Measured on the Dell, 16 Sep
/// 2026:** `rex site create --db mariadb` created the site and exited 0 on Windows, where
/// MariaDB has no pin (D4); it would then have failed at `spawn_db`, which is offered-then-
/// refused — the shape #643 removed for web servers. The os is a parameter for the same reason
/// every other W10 gate takes one: the bar only `cargo check`s for Windows, so a gate reading
/// `std::env::consts::OS` alone has a half that cannot fail on the machine running the test
/// (ledger #647).
fn ensure_engine_available_on(engine: SiteDbEngine, os: &str) -> Result<()> {
    // ONE gate with ONE pair of sentences (`DbEngine::ensure_available_on`): the
    // host's macOS when that is the reason, else the platform's — so `rex`, the
    // MCP server, the Databases page and this create path cannot disagree.
    crate::core::db::DbEngine::from_site(engine).ensure_available_on(os)
}

/// The database engines a site can actually be created with on this build, in the order the New
/// Site dialog offers them.
///
/// Derived from the same gate that REFUSES ([`ensure_engine_available_on`]), for the reason
/// [`offered_web_servers`] exists: the dialog listed `mysql` and `mariadb` unconditionally, so on
/// Windows it offered MariaDB and create then refused it — offered-then-refused, the shape #643
/// removed for web servers and #647 left standing for engines (ledger #648).
pub fn offered_db_engines() -> Vec<SiteDbEngine> {
    offered_db_engines_on(std::env::consts::OS)
}

/// [`offered_db_engines`] for a NAMED os, so both answers are measurable from either host.
pub fn offered_db_engines_on(os: &str) -> Vec<SiteDbEngine> {
    [SiteDbEngine::Mysql, SiteDbEngine::Mariadb, SiteDbEngine::Postgres]
        .into_iter()
        .filter(|e| ensure_engine_available_on(*e, os).is_ok())
        .collect()
}

fn refuse_unbuildable(conn: &Connection, new: &NewSite) -> Result<()> {
    refuse_unbuildable_on(conn, new, std::env::consts::OS)
}

/// [`refuse_unbuildable`] for a NAMED os — the same reason `available_on`,
/// `ensure_server_available_on` and `offered_db_engines_on` take one (W10, #642): the
/// gates refuse FrankenPHP and Apache where they have no pin, so a test about what
/// those servers DO can only run on the os that ships them. Without this the ten
/// `sites` tests that name an override server passed on macOS and failed on Windows
/// inside the gate, never reaching the behaviour they are named for (W12).
fn refuse_unbuildable_on(conn: &Connection, new: &NewSite, os: &str) -> Result<()> {
    ensure_server_available_on(new.web_server, os)?;
    ensure_engine_available_on(new.db_engine, os)?;
    ensure_server_runs_php(new.web_server, &new.php_version)?;
    // The patch this site will really run — the user's selection floored by the
    // pin — because the PostgreSQL rule is a fact about the ARTIFACT, and the
    // row stores only the minor.
    let php_patch = crate::core::php::effective_patch(conn, &new.php_version)?
        .unwrap_or_else(|| new.php_version.clone());
    ensure_engine_supports(new.site_type, new.db_engine, &php_patch)
}

fn create_recording_ownership(
    conn: &Connection,
    new: NewSite,
    docroot_managed: bool,
    ownership: Ownership,
    git: Option<GitSource>,
) -> Result<Site> {
    create_recording_ownership_on(conn, new, docroot_managed, ownership, git, std::env::consts::OS)
}

/// [`create_recording_ownership`] for a NAMED os — see [`create_on`].
#[allow(clippy::too_many_arguments)]
fn create_recording_ownership_on(
    conn: &Connection,
    new: NewSite,
    docroot_managed: bool,
    ownership: Ownership,
    git: Option<GitSource>,
    os: &str,
) -> Result<Site> {
    validate_domain(&new.domain)?;
    validate_docroot_path(&new.path)?;
    refuse_unbuildable_on(conn, &new, os)?;
    if let Some(owner) = domain_taken_by(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "{} already reaches the site \"{owner}\" — one hostname can only reach one site",
            new.domain
        )));
    }
    // Allocate a COLLISION-FREE override backend port for the site's server
    // (None for nginx) — recorded once and never re-derived (B20 §4).
    let others = list(conn)?;
    let override_port = allocate_override_port(&others, new.web_server)?;
    // B20-A guard, kept as a belt on the allocator: the allocated port must not
    // already be recorded by another site (the allocator guarantees this, so it
    // never fires — permanent defense-in-depth against a shared backend).
    if let Some(port) = override_port {
        if let Some(other) = recorded_port_conflict(&others, "", port) {
            return Err(override_port_collision_error(&new.domain, &other, port));
        }
    }
    // Derived from the domain ONCE, here — every later operation reads the
    // stored value, so a domain change never re-points the database. Unique per
    // site: a slug collision or >64-char overflow falls back to a hash suffix
    // (finding B21) so two domains can never share one database.
    let db_name = unique_db_name(conn, new.site_type, &new.domain)?;
    // Recorded ONCE from the stored path's own markers (v24) — Bedrock/
    // Radicle linked docroots keep mu-plugins out of a dead `wp-content/`.
    // Non-WP sites never consult it.
    let content_dir = (new.site_type == SiteType::Wordpress)
        .then(|| detect_content_dir_rel(Path::new(&new.path)).to_string());
    // What the web server roots at, decided ONCE here (v32). A Laravel project
    // WE create keeps `.env` at `path` and serves `path/public`; a LINKED one
    // was detected by reading the disk and its stored path ALREADY points at
    // the folder to serve (`DetectedProject::docroot_rel` was applied at link
    // time), so appending `public` again would serve a directory that does not
    // exist. Ownership of the docroot is exactly that distinction.
    let docroot_subdir = if new.site_type == SiteType::Laravel && docroot_managed {
        LARAVEL_DOCROOT_SUBDIR.to_string()
    } else {
        String::new()
    };
    let site = Site {
        id: Uuid::new_v4().to_string(),
        name: new.name,
        domain: new.domain,
        site_type: new.site_type,
        status: ServiceStatus::Stopped,
        php_version: new.php_version,
        web_server: new.web_server,
        ssl: true,
        path: new.path,
        created_at: store::db_now(conn)?,
        multisite: MultisiteMode::None,
        db_name,
        db_engine: new.db_engine,
        xdebug: false,
        override_port,
        // Inserted PROVISIONED: every non-job caller (tests, examples, future
        // import paths) gets yesterday's semantics. The provision job — the
        // only flow that can die half-done — flips this to 0 itself right
        // after insert and back to 1 when it settles ok.
        provisioned: true,
        docroot_managed: Some(docroot_managed),
        // No database exists yet at insert time. NULL keeps legacy semantics
        // (whatever the provision job creates is ours); the import job records
        // the real answer before it creates anything.
        db_created: None,
        content_dir,
        // No mu-plugins dir has been created by us at insert time (v25).
        mu_dir_created: None,
        // v27, from the ONE ownership value — recorded at the insert, never
        // derived later from the domain or the path.
        origin: match ownership {
            Ownership::User | Ownership::UserByAgent { .. } => SiteOrigin::User,
            Ownership::Agent { .. } => SiteOrigin::Agent,
        },
        // `UserByAgent` records NO client on the row: `agent_client` is the
        // scratch badge's field, and a user's own site must never wear it.
        // Who created it is the feed's fact.
        agent_client: match &ownership {
            Ownership::User | Ownership::UserByAgent { .. } => None,
            Ownership::Agent { client, .. } => Some(client.clone()),
        },
        // The clock starts here, at the insert, so a site that fails LATER in
        // provisioning still expires and still gets reaped — a half-built
        // scratch site is exactly the kind that would otherwise linger forever.
        expires_at: match &ownership {
            Ownership::User | Ownership::UserByAgent { .. } => None,
            Ownership::Agent { ttl_hours, .. } => Some(store::db_time_from_now(conn, *ttl_hours)?),
        },
        docroot_subdir,
        // Recorded at the INSERT, before the clone has run, because RETRY is
        // this design's recovery path: a job that dies mid-clone leaves
        // `provisioned = 0`, and the Retry button — possibly after an app
        // restart, with the job registry long gone — has nowhere else to learn
        // which repository to fetch. So the row states the site's SOURCE, and
        // `provisioned` states whether the code actually arrived; reading the
        // first as "the code is here" is the second field's job to correct.
        git_url: git.as_ref().map(|g| g.url.clone()),
        git_ref: git.as_ref().and_then(|g| g.git_ref.clone()),
        // Recorded only for a CLONED site: on every other row NULL is the
        // honest "nobody was asked", which `Site::runs_migrations` reads as the
        // unconditional yes those paths have always had.
        git_migrate: git.as_ref().map(|_| new.git_migrate),
        git_build_assets: git.as_ref().map(|_| new.git_build_assets),
        // Recorded only where the question was ASKED (v41): a Blank-PHP site
        // whose docroot rexenv creates and does not clone into. Everywhere else
        // NULL is the honest "nobody was asked" — a linked folder is the user's
        // to fill and a clone brings its own code, so neither is ever seeded.
        starter_db: (new.site_type == SiteType::Php && docroot_managed && git.is_none())
            .then_some(new.starter_db),
        enabled: true,
    };
    store::insert_site(conn, &site)?;
    Ok(site)
}

/// Why a starter database cannot be created for this shape of site, or `None`.
///
/// [`create`] records `starter_db` only where the question was ASKED — Blank
/// PHP, in a docroot rexenv makes, not cloned into — using a `.then_some` that
/// DROPS the field everywhere else. Silently: ask for it on a WordPress site
/// and you get a normal WordPress site, no error, no seeded table, and nothing
/// that says why. The dialog cannot ask wrongly (it only offers the field for
/// Blank PHP), but `rex site create --starter-db` can, so the reason lives here
/// next to the rule it explains rather than in the caller that happened to need
/// it first.
pub fn starter_db_refusal(site_type: SiteType, linked_path: &str, cloning: bool) -> Option<&'static str> {
    if site_type != SiteType::Php {
        return Some(
            "--starter-db needs --type php: a WordPress site brings its own database and \
             schema, so there is nothing to seed",
        );
    }
    if !linked_path.is_empty() {
        return Some(
            "--starter-db cannot be combined with --path: a linked folder is yours to fill, \
             and rexenv never writes into one",
        );
    }
    if cloning {
        return Some(
            "--starter-db cannot be combined with a git clone: the repository brings its own \
             code, and seeding beside it would be rexenv writing into your checkout",
        );
    }
    None
}

/// All sites, newest first.
/// Every site's domain except `id`'s — what [`crate::core::logs::is_run_log_of`]
/// needs to tell this site's Git job logs from a neighbour's whose domain
/// extends its name.
pub fn other_domains(conn: &Connection, id: &str) -> Result<Vec<String>> {
    Ok(list(conn)?.into_iter().filter(|s| s.id != id).map(|s| s.domain).collect())
}

pub fn list(conn: &Connection) -> Result<Vec<Site>> {
    store::list_sites(conn)
}

/// One site by id, or `None`.
pub fn get(conn: &Connection, id: &str) -> Result<Option<Site>> {
    store::get_site(conn, id)
}

/// Delete a site by id; returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    store::delete_site(conn, id)
}

/// Set a site's status, returning the updated site (or `None` if it doesn't exist).
pub fn set_status(conn: &Connection, id: &str, status: ServiceStatus) -> Result<Option<Site>> {
    store::set_site_status(conn, id, status)?;
    get(conn, id)
}

/// The display name `rex site create <domain>` gives a site when none is passed: the
/// domain without its last label (`s1.rex` → `s1`, `shop.acme.test` → `shop.acme`). The
/// dialog pairs a name with `<slug(name)>.<tld>`; this is the same pairing read backwards.
/// Until 29 Sep 2026 the CLI stored the whole domain as the name (`docs/TODO.md`'s
/// "Smaller, same run").
pub fn name_from_domain(domain: &str) -> String {
    let d = domain.trim().trim_end_matches('.');
    match d.rsplit_once('.') {
        Some((base, _tld)) if !base.is_empty() => base.to_string(),
        _ => d.to_string(),
    }
}

/// Rename a site's DISPLAY name only (the domain, docroot, DB and certs are keyed
/// off the domain, so they're untouched). Returns the updated site, or `None` if
/// it doesn't exist. Errors on a blank name.
pub fn rename(conn: &Connection, id: &str, name: &str) -> Result<Option<Site>> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::error::Error::Other("site name cannot be empty".into()));
    }
    store::set_site_name(conn, id, name)?;
    get(conn, id)
}

/// Preflight for a domain change — every rule that must hold BEFORE the
/// orchestrator runs any destructive step (backup / search-replace), and again
/// inside [`set_domain`] as defense-in-depth:
/// - the new domain must validate (same rules as create) and be unused
/// - it must differ from the current domain
/// - multisite is REFUSED: a network stores the domain in wp-config
///   (`DOMAIN_CURRENT_SITE`) and per-subsite rows (`wp_blogs`/`wp_site`), so a
///   single-site-style change would half-break it. Honest refusal for v1.
pub fn check_domain_change(conn: &Connection, site: &Site, new_domain: &str) -> Result<()> {
    if !matches!(site.multisite, MultisiteMode::None) {
        return Err(Error::Other(
            "domain change isn't supported on a multisite network yet — the network stores \
             the domain in wp-config and per-subsite tables, and changing it here would \
             break the sub-sites"
                .into(),
        ));
    }
    if site.domain == new_domain {
        return Err(Error::Other(format!("site already uses {new_domain}")));
    }
    validate_domain(new_domain)?;
    // BOTH tables. `domain_exists` alone answers half the question, and half an
    // answer means renaming a site onto another site's EXTRA domain (v42) —
    // two server blocks for one hostname, served by whichever nginx matched
    // first, with both sites looking correct on screen.
    if let Some(owner) = domain_taken_by(conn, new_domain)? {
        return Err(Error::Other(format!(
            "{new_domain} already reaches the site \"{owner}\" — one hostname can only reach \
             one site"
        )));
    }
    Ok(())
}

/// Is `domain` taken by ANY site — as its own domain or as an extra domain
/// (v42)? Returns the owning site's name.
///
/// One function because there are three callers that must agree: create, the
/// domain change, and the alias validator. `store::domain_exists` answers only
/// half the question, and half an answer here means two server blocks for one
/// hostname — nginx serves whichever it matched first while both sites look
/// correct in the UI.
pub fn domain_taken_by(conn: &Connection, domain: &str) -> Result<Option<String>> {
    if let Some(site) = store::site_by_domain(conn, domain)? {
        return Ok(Some(site.name));
    }
    match store::site_id_for_alias(conn, domain)? {
        Some(owner) => Ok(Some(get(conn, &owner)?.map(|s| s.name).unwrap_or(owner))),
        None => Ok(None),
    }
}

/// Change ONLY the `domain` column (after re-running [`check_domain_change`]).
/// The docroot folder and `db_name` are deliberately untouched: renaming the
/// folder risks breaking absolute paths inside the site, and MySQL has no
/// `RENAME DATABASE` — both stay keyed to the creation-time domain, which is
/// purely cosmetic. Returns the updated site (`None` if the id doesn't exist).
/// The caller (the change-domain orchestrator) owns certs, configs/reload, and
/// the WordPress URL migration.
pub fn set_domain(conn: &Connection, id: &str, new_domain: &str) -> Result<Option<Site>> {
    let Some(site) = get(conn, id)? else { return Ok(None) };
    check_domain_change(conn, &site, new_domain)?;
    // The recorded override port is DELIBERATELY untouched here (B20 §4): it was
    // allocated once and is authoritative, so a domain change no longer re-derives
    // it — which is exactly what used to orphan the running backend and break the
    // edge route. No collision check is needed (the port doesn't change, so it
    // can't newly collide); the no-shared-port guarantee is held by the allocator
    // + the create/set_web_server belts. Same "derived once, never re-derived"
    // shape as `db_name`.
    store::set_site_domain(conn, id, new_domain)?;
    get(conn, id)
}

/// Preflight for moving a site's docroot into `dest_parent` (the user-picked
/// PARENT directory — the folder keeps its current name). Every rejection
/// happens here, BEFORE any file is touched. Returns the resolved target path
/// `<dest_parent>/<folder name>`.
pub fn check_docroot_move(site: &Site, dest_parent: &Path) -> Result<PathBuf> {
    // A folder we don't own is not ours to relocate — and a cross-volume move
    // COPIES then deletes the source, so this would silently rewrite the user's
    // own project layout. Refused in core, so no IPC path can reach it.
    if site.docroot_managed == Some(false) {
        return Err(Error::Other(format!(
            "{} is your own folder — rexenv doesn't move it. Move it yourself, then link \
             the site to its new location.",
            site.path
        )));
    }
    let src = Path::new(&site.path);
    if site.path.is_empty() || !src.is_dir() {
        return Err(Error::Other(format!(
            "the site folder is missing on disk ({}) — can't move it. If the files live \
             elsewhere, this site's record is stale.",
            site.path
        )));
    }
    let name = src
        .file_name()
        .ok_or_else(|| Error::Other(format!("bad site path: {}", site.path)))?;
    if !dest_parent.is_absolute() {
        return Err(Error::Other("destination must be an absolute path".into()));
    }
    if dest_parent.starts_with(src) {
        return Err(Error::Other(
            "the destination is inside the site folder itself — pick a folder outside it".into(),
        ));
    }
    let target = dest_parent.join(name);
    if target == src {
        return Err(Error::Other("the site already lives in that folder".into()));
    }
    if target.exists() {
        return Err(Error::Other(format!(
            "{} already exists — move it away or pick another destination (never merged/overwritten)",
            target.display()
        )));
    }
    Ok(target)
}

/// Preflight for RE-POINTING a site at a folder the user moved themselves —
/// `dest` is the docroot ITSELF (not a parent), and rexenv touches no file:
/// it only records where the files now live. The complement of
/// [`check_docroot_move`], which is refused for a folder we don't own; here
/// the user did the moving, so ownership is not the question — only that the
/// destination is a real, servable directory.
pub fn check_docroot_relink(site: &Site, dest: &Path) -> Result<()> {
    validate_docroot_path(&dest.display().to_string())?;
    if !dest.is_absolute() {
        return Err(Error::Other("destination must be an absolute path".into()));
    }
    if !dest.is_dir() {
        return Err(Error::Other(format!(
            "{} isn't a folder on disk — pick the folder that now holds the site's files",
            dest.display()
        )));
    }
    if dest == Path::new(&site.path) {
        return Err(Error::Other("the site already points at that folder".into()));
    }
    Ok(())
}

/// Move a docroot to `target` (which must not exist — see [`check_docroot_move`]).
/// Same volume: one `fs::rename`. Anything else (cross-volume rename fails):
/// recursive copy → VERIFY (every file present with matching size) → the caller
/// deletes the old tree only after configs are reloaded. A failed/partial copy
/// removes the partial target and returns the error — the old path is untouched.
/// Returns `true` when the copy fallback ran (old dir still present).
pub fn move_dir(src: &Path, target: &Path) -> Result<bool> {
    match std::fs::rename(src, target) {
        Ok(()) => Ok(false),
        // Cross-device moves fail with EXDEV; other failures (permissions,
        // missing parent) fail the copy below too, with the accurate cause.
        Err(_) => {
            if let Err(e) = copy_dir_recursive(src, target).and_then(|()| verify_tree(src, target))
            {
                let _ = std::fs::remove_dir_all(target);
                return Err(Error::Other(format!(
                    "couldn't move {} to {}: {e} (nothing changed — the site still lives at \
                     the old path)",
                    src.display(),
                    target.display()
                )));
            }
            Ok(true)
        }
    }
}

/// Recursive dir copy. Symlinks are materialized via `fs::copy` (a symlink to a
/// directory errors out, aborting the move cleanly — rare in a docroot).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Verify a copied tree: every entry under `src` exists under `dst`, files with
/// equal sizes. Runs BEFORE the old tree may be deleted.
fn verify_tree(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if !to.is_dir() {
                return Err(Error::Other(format!("copy verify failed: missing dir {}", to.display())));
            }
            verify_tree(&entry.path(), &to)?;
        } else {
            let (a, b) = (entry.metadata()?.len(), to.metadata().map(|m| m.len()));
            if b.ok() != Some(a) {
                return Err(Error::Other(format!(
                    "copy verify failed: {} missing or size mismatch",
                    to.display()
                )));
            }
        }
    }
    Ok(())
}

/// Persist a moved docroot's new path (files must already exist there) and
/// return the updated site.
pub fn set_path(
    conn: &Connection,
    platform: &dyn Platform,
    id: &str,
    path: &Path,
) -> Result<Option<Site>> {
    let path_s = path.display().to_string();
    validate_docroot_path(&path_s)?;
    if !store::set_site_path(conn, id, &path_s)? {
        return Ok(None);
    }
    // Moving a docroot OUT of the sites folder gives up our claim on it — the
    // move dialog promises such a folder is "kept, not deleted". Recorded HERE,
    // the one choke point every moved path passes through, at the moment the
    // fact changes; deletion later just reads it. Monotonic: this can only ever
    // clear the flag, never re-claim a folder.
    let managed = docroot_under_managed_root(
        &path_s,
        &sites_dir(conn, platform)?,
        &default_sites_dir()?,
        &legacy_sites_dir(platform)?,
    );
    if !managed {
        store::set_site_docroot_managed(conn, id, false)?;
    }
    get(conn, id)
}

/// Reject a docroot path containing a char that can't be safely emitted into the
/// generated nginx / Caddy / Apache configs — `"` breaks the quoted string, `$`
/// interpolates in nginx, `{`/`}` are Caddy placeholders, `\` escapes, and
/// control chars (newline, …) break the directive. Escaping can't neutralize all
/// of these across the three formats (nginx has no literal-`$` escape), so the
/// path is validated at INPUT instead. Space is allowed — paths are quoted, so
/// spaces (the common case) work. Enforced only where `site.path` is PERSISTED
/// (`create`/`set_path`), never at emit, so existing sites are grandfathered and
/// a stored path is never re-rejected on regenerate (B26).
/// What an existing project folder looks like, decided by reading the
/// filesystem ONLY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProject {
    /// The site type to create it as.
    pub site_type: SiteType,
    /// The folder to serve, relative to the linked root (`""` = the root
    /// itself). Laravel/Symfony serve `public/`, Bedrock serves `web/`, and so
    /// on — a docroot is not always the project root.
    pub docroot_rel: String,
    /// Human-readable framework name for the UI ("WordPress", "Laravel", …).
    pub label: &'static str,
    /// True when the folder already holds an installed app we must adopt
    /// as-is rather than provision into.
    pub existing_install: bool,
}

/// Every content-dir layout rexenv has ever written under (stock WP, Bedrock,
/// Radicle). REMOVAL of rexenv-owned mu-plugins sweeps all of them — a file a
/// pre-v24 bug wrote into a Bedrock repo's dead `wp-content/` must still get
/// cleaned up, and removing an exact filename from a dir that never had it is
/// a no-op. Lives HERE, next to layout detection, so the write-side fact and
/// the removal sweep can't drift apart (the one-fact-two-computations class).
pub(crate) const CONTENT_DIR_LAYOUTS: [&str; 3] = ["wp-content", "app", "content"];

/// The WP content dir RELATIVE to a docroot, from the same fs markers
/// [`detect_project`] trusts (sibling `config/application.php` = Bedrock,
/// content at `docroot/app`; sibling `bedrock/application.php` = Radicle,
/// content at `docroot/content`; else WP's stock `wp-content`). Called ONCE
/// per site — at creation and by the v24 backfill — and recorded; writers
/// read the record. Never call this at write time: a `web/wp-content/` this
/// bug itself once littered into a Bedrock repo would poison a use-time
/// probe, which is exactly why the layout checks run before any `wp-content`
/// fallback.
pub fn detect_content_dir_rel(docroot: &Path) -> &'static str {
    let sibling = |rel: &str| docroot.parent().is_some_and(|p| p.join(rel).exists());
    if sibling("config/application.php") && docroot.join("app").exists() {
        return "app";
    }
    // UNVERIFIED against a real project: `public/content` is Roots' documented
    // Radicle layout, but no Radicle site has been linked on a live install
    // yet — whoever first links one should confirm mu-plugins actually load
    // from here before trusting features that write into it.
    if sibling("bedrock/application.php") && docroot.join("content").exists() {
        return "content";
    }
    "wp-content"
}

/// Roots' Bedrock: WordPress core installed by Composer into `web/wp`, config in
/// `.env` + `config/application.php`.
pub const LABEL_BEDROCK: &str = "WordPress (Bedrock)";
/// Roots' Radicle: the same shape one folder over.
pub const LABEL_RADICLE: &str = "WordPress (Radicle)";

/// Does this layout get its WordPress CORE from Composer rather than from a
/// core download?
///
/// The one question the provisioning driver asks about a cloned WordPress
/// checkout, and it decides two phases: `core_download` must not run (Composer
/// puts core in `web/wp`, and a download would litter `web/` with a second
/// copy), and `wp config create` must not run (the repository ships the
/// `wp-config.php` stub, and its real configuration is `.env`).
///
/// Keyed on the LABEL, which is why those are constants: the markers that
/// decide it live in `detect_project` and must not be re-sniffed here — a
/// second copy of the sniff is a second answer waiting to drift.
pub fn wordpress_core_from_composer(detected: &DetectedProject) -> bool {
    detected.label == LABEL_BEDROCK || detected.label == LABEL_RADICLE
}

/// Classify an existing project folder by probing the filesystem — **never by
/// executing anything in it**.
///
/// This deliberately does NOT interpret Valet's PHP "drivers": running a
/// `LocalValetDriver.php` to learn a docroot would mean executing the user's
/// code during a scan. The codebase's standing rule is detection = pure fs,
/// execution = an explicit user action (`core::repo`'s clone/detect split), and
/// the same rule holds here. Folders whose shape we can't place come back as
/// `Php` serving the root, which the caller surfaces for confirmation rather
/// than guessing silently.
///
/// Order matters: the most specific marker wins, mirroring what the shipped
/// Valet drivers actually resolve to.
pub fn detect_project(root: &Path) -> DetectedProject {
    let has = |rel: &str| root.join(rel).exists();
    let php = |rel: &str, label, existing| DetectedProject {
        site_type: SiteType::Php,
        docroot_rel: rel.to_string(),
        label,
        existing_install: existing,
    };

    // Bedrock/Radicle: WordPress, but wp-config.php is NOT at the served root —
    // the naive "wp-config.php means serve here" probe gets these wrong.
    if has("web/wp-config.php") && has("config/application.php") {
        return DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: "web".into(),
            label: LABEL_BEDROCK,
            existing_install: true,
        };
    }
    if has("public/wp-config.php") && has("bedrock/application.php") {
        return DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: "public".into(),
            label: LABEL_RADICLE,
            existing_install: true,
        };
    }
    // Plain WordPress. `wp-config-sample.php` counts: it's a downloaded core
    // that hasn't been configured yet, and Valet's own driver accepts it.
    if has("wp-config.php") || has("wp-config-sample.php") || has("wp-load.php") {
        return DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: String::new(),
            label: "WordPress",
            existing_install: true,
        };
    }
    if has("artisan") && has("public/index.php") {
        return DetectedProject {
            site_type: SiteType::Laravel,
            docroot_rel: LARAVEL_DOCROOT_SUBDIR.into(),
            label: "Laravel",
            existing_install: true,
        };
    }
    if has("craft") && has("web/index.php") {
        return php("web", "Craft CMS", true);
    }
    if has("please") && has("public/index.php") {
        return php("public", "Statamic", true);
    }
    if has("bin/console") && has("public/index.php") {
        return php("public", "Symfony", true);
    }
    if has("pub/index.php") && has("app/etc/env.php") {
        return php("pub", "Magento", true);
    }
    // Generic front-controller layouts.
    for dir in ["public", "web", "www"] {
        if has(&format!("{dir}/index.php")) || has(&format!("{dir}/index.html")) {
            return php(dir, "PHP project", true);
        }
    }
    if has("index.php") {
        return php("", "PHP project", true);
    }
    if has("index.html") {
        return php("", "Static site", true);
    }
    // Nothing recognisable — serve the root and let the user confirm. Empty
    // folders land here too, which the caller reports honestly.
    php("", "Unknown", root.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false))
}

/// Whether a linked folder carries a marker meaning our own docroot detection
/// could disagree with what Valet/Herd actually served it as — a per-project or
/// machine-wide custom driver. The caller surfaces this instead of guessing.
pub fn has_custom_valet_driver(root: &Path) -> bool {
    root.join("LocalValetDriver.php").exists()
}

/// A canonical path in the form it is STORED and rendered: Windows' `canonicalize` answers the
/// extended-length form (`\\?\C:\Users\x\site`, `\\?\UNC\server\share\p`), which the generated
/// nginx config would carry as `//?/C:/…` (`services::nginx_path`) and every "where is my site"
/// sentence would show with four punctuation marks the user never typed. The prefix is dropped
/// here, ONCE, at the point a linked docroot is stored (ledger #770); nothing else is changed —
/// a Unix path, or a path without the prefix, comes back as it went in. Comparisons must keep
/// using the raw canonical path (both sides verbatim, #642); this is for storage only.
pub fn plain_path(p: PathBuf) -> PathBuf {
    let text = p.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p
}


/// Preflight for LINKING an existing folder as a site's docroot (Stage 0).
///
/// The folder is the USER'S — we never created it and will never delete it — so
/// every rejection happens here, before anything is created. Returns the
/// CANONICAL path, which is what gets stored and what every consumer (vhost,
/// php-fpm routing, git assets, terminal cwd) then uses, so a later symlink
/// swap can't redirect what we serve.
///
/// Blast-radius refusals are deliberate: a docroot can be published with one
/// click by the tunnel feature, and serving a home directory would leave
/// `~/.ssh` one dotfile-guard bug from the internet. Only the directories
/// THEMSELVES are refused — `~/Desktop/myproject` is a perfectly normal place
/// to keep a site, and Valet users really do keep them there.
///
/// **Every comparison here puts a canonical path on BOTH sides.** `canonicalize` returns a
/// verbatim path on Windows and the crates supplying the other side do not, so a plain
/// comparison is false exactly where the guard is needed (W12, 17 Sep 2026 — measured on
/// the Dell). Three guards had it: the blast radius, our own app data, and the managed
/// sites folder. The per-site overlap loop below does NOT, because the paths it compares
/// were stored from this function's own canonical return.
pub fn validate_linked_docroot(
    conn: &Connection,
    platform: &dyn Platform,
    path: &str,
) -> Result<PathBuf> {
    validate_linked_docroot_on(conn, platform, path, std::env::consts::OS)
}

/// [`validate_linked_docroot`] for a NAMED os — see [`create_on`]. Only the blast radius
/// differs: `/` and `/Users` are the unix roots, `C:\` and `C:\Users` the Windows ones.
pub fn validate_linked_docroot_on(
    conn: &Connection,
    platform: &dyn Platform,
    path: &str,
    os: &str,
) -> Result<PathBuf> {
    let raw = Path::new(path.trim());
    if raw.as_os_str().is_empty() || !raw.is_absolute() {
        return Err(Error::Other("pick a folder using an absolute path".into()));
    }
    if !raw.exists() {
        return Err(Error::Other(format!("{} doesn't exist", raw.display())));
    }
    if !raw.is_dir() {
        return Err(Error::Other(format!("{} isn't a folder", raw.display())));
    }
    // Resolve symlinks + `..` ONCE, here: everything downstream stores and
    // serves this exact path.
    let canon = raw
        .canonicalize()
        .map_err(|e| Error::Other(format!("could not resolve {}: {e}", raw.display())))?;
    // The stored path is emitted into the generated server configs (B26).
    validate_docroot_path(&canon.display().to_string())?;

    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    // The unix roots only: on Windows `/` and `/Users` name nothing, and the drive root
    // plus the profile parent (`C:\`, `C:\Users`) are that host's same class. Named by os
    // rather than read from the host so both answers stay measurable from either machine
    // (W10/#642) — the bar only `cargo check`s for Windows.
    let mut blast_radius: Vec<PathBuf> = if os == "windows" {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(root) = canon.ancestors().last() {
            roots.push(root.to_path_buf());
        }
        if let Some(parent) = home.as_ref().and_then(|h| h.parent()) {
            roots.push(parent.to_path_buf());
        }
        roots
    } else {
        vec![PathBuf::from("/"), PathBuf::from("/Users")]
    };
    if let Some(home) = &home {
        blast_radius.push(home.clone());
        for dir in ["Desktop", "Documents", "Downloads"] {
            blast_radius.push(home.join(dir));
        }
    }
    // A volume root (`/Volumes/<name>`) is the same class as `/`.
    if canon.parent() == Some(Path::new("/Volumes")) {
        blast_radius.push(canon.clone());
    }
    // **Compared CANONICAL against CANONICAL, never against the path a directory crate
    // handed us.** `canonicalize` on Windows returns a VERBATIM path (`\\?\C:\Users\DELL`)
    // while `BaseDirs::home_dir()` returns `C:\Users\DELL`, so the plain `contains`/
    // `starts_with` below answered FALSE for the home folder itself — the refusal this
    // function's doc promises simply did not fire on that host. Measured on the Dell,
    // 17 Sep 2026: `canon(home) == plain home` is false, `canon(dir).starts_with(plain
    // home)` is false, and `starts_with(canon home)` is true. An entry that does not exist
    // (`/Volumes` off macOS) keeps its literal form, which is what the comparison wants.
    let resolve = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let blast_radius: Vec<PathBuf> = blast_radius.iter().map(|p| resolve(p)).collect();
    if blast_radius.contains(&canon) {
        return Err(Error::Other(format!(
            "{} is too broad to serve — a site's folder can be shared publicly with one \
             click, which would expose everything inside it. Pick the project folder itself.",
            canon.display()
        )));
    }

    // Our own app data holds the CA key, every site certificate and the app
    // database — never serve it.
    // Canonical on both sides, for the reason written at the blast radius above: this one
    // is the guard that matters most, and on Windows it was answering false.
    if let Ok(app_data) = platform.paths().app_data_dir().map(|p| resolve(&p)) {
        if canon == app_data || canon.starts_with(&app_data) {
            return Err(Error::Other(
                "that folder is rexenv's own application data — pick your project folder".into(),
            ));
        }
    }

    // Inside the managed sites folder there is nothing to link: that's a normal
    // site, and linking would only opt its docroot out of cleanup.
    // `sites_dir` is a SETTING or a `BaseDirs` join — never canonical either.
    let managed = resolve(&sites_dir(conn, platform)?);
    if canon.starts_with(&managed) {
        return Err(Error::Other(format!(
            "{} is inside your rexenv sites folder — create a site normally instead of \
             linking it",
            canon.display()
        )));
    }

    // No overlap with an existing site, in EITHER direction (the same rule
    // `repo::validate_link_target` applies to linked plugin/theme folders):
    // nested docroots would serve one site's files under another's domain.
    //
    // **`list(conn)` is read HERE, per call, and must stay that way.** It looks
    // like an obvious hoist — the Valet import's `enrich` already holds a
    // hoisted `existing` slice two lines from its call — and `docs/TODO.md`
    // carried "hoist if imports grow" as a row until 23 Aug 2026. It is a trap.
    // The import APPLY loop creates sites one at a time, each through `create`,
    // which calls this; so importing two Valet projects where one nests inside
    // the other is refused only because the second validation sees the first
    // site's freshly-written row. A snapshot taken before the batch cannot
    // contain it, and both would be created — the one-time-check-on-a-mutable-
    // fact family, which has bitten this tree repeatedly.
    //
    // The cost it buys is not the one the row worried about: this is one SELECT
    // per candidate, and `detect_project` runs immediately before it in the same
    // function doing strictly more filesystem I/O. The neighbour dominates.
    for other in list(conn)? {
        if other.path.is_empty() {
            continue;
        }
        let theirs = Path::new(&other.path);
        if canon == theirs {
            return Err(Error::Other(format!(
                "{} is already served by {}",
                canon.display(),
                other.domain
            )));
        }
        if canon.starts_with(theirs) || theirs.starts_with(&canon) {
            return Err(Error::Other(format!(
                "{} overlaps {}'s folder ({}) — pick a folder that doesn't contain, and \
                 isn't inside, another site",
                canon.display(),
                other.domain,
                theirs.display()
            )));
        }
    }
    // The STORED form: canonical, without the verbatim prefix Windows' `canonicalize` adds
    // (`\\?\C:\…`, #770) — every comparison above ran on the raw canonical path.
    Ok(plain_path(canon))
}

/// The first character a generated server config cannot carry, judged inside each folder NAME
/// rather than across the raw string (ledger #303). A `\` inside a name ends or escapes a quoted
/// config string and is found wherever it sits; a `\` that is Windows' path SEPARATOR is in no
/// name, and every config writer renders such a path with forward slashes
/// (`services::nginx_path`, ledger #602). Measured on the Dell, 14 Sep 2026: the raw-string check
/// refused every Windows site folder, since every Windows path has a backslash.
fn config_breaking_char(path: &str, breaks: impl Fn(char) -> bool) -> Option<char> {
    Path::new(path).components().find_map(|part| match part {
        std::path::Component::Normal(name) => name.to_string_lossy().chars().find(|&c| breaks(c)),
        _ => None,
    })
}

fn validate_docroot_path(path: &str) -> Result<()> {
    if let Some(c) =
        config_breaking_char(path, |c| matches!(c, '"' | '$' | '{' | '}' | '\\') || c.is_control())
    {
        return Err(Error::Other(format!(
            "the site folder path contains {c:?}, which can't be used in the web-server \
             config — pick a folder without any of \" $ {{ }} \\ or control characters"
        )));
    }
    Ok(())
}

/// Every refusal a web-server switch can meet, asked BEFORE anything is fetched for it — the
/// switch command calls this ahead of its binary prefetch, and [`set_web_server_on`] again
/// before it writes. Until 4 Oct 2026 the command prefetched first, so switching a Windows site
/// to OpenLiteSpeed answered "1 of 1 downloads failed — fix the connection … no binary manifest
/// for openlitespeed 1.9.3 on windows" instead of the refusal sentence (measured on the Dell);
/// a server with no pin for the OS has nothing to download, and the user was sent to fix a
/// connection that was never broken. (ledger #786)
pub fn check_switch_on(conn: &Connection, id: &str, server: WebServer, os: &str) -> Result<()> {
    ensure_server_available_on(server, os)?;
    let Some(site) = get(conn, id)? else { return Ok(()) };
    // The site keeps its PHP version across a server switch, so the pair has to
    // be checked here too — a 7.4 site switched to FrankenPHP is the same lie
    // as one created that way.
    ensure_server_runs_php(server, &site.php_version)?;
    // The site keeps its env vars too, and OpenLiteSpeed cannot carry every value the
    // others can (a value with both quote characters) — refused here, before the switch,
    // rather than discovered by a backend that cannot write its config (ledger #775).
    if server == WebServer::Openlitespeed {
        super::openlitespeed::check_env(&store::get_site_env(conn, id)?)?;
    }
    Ok(())
}

/// Switch a site's web server (Phase 2 §4.1): update ONLY the `web_server` column
/// — no docroot/cert/DB rebuild — and return the updated site. FrankenPHP, Apache and
/// OpenLiteSpeed have backends of their own; nginx is the shared one. The caller brings the new
/// backend up / old down and reloads the edge.
pub fn set_web_server(conn: &Connection, id: &str, server: WebServer) -> Result<Option<Site>> {
    set_web_server_on(conn, id, server, std::env::consts::OS)
}

/// [`set_web_server`] for a NAMED os — see [`create_on`]. Switching TO FrankenPHP is a
/// macOS-only possibility today, and the tests about what the switch does say so.
pub fn set_web_server_on(
    conn: &Connection,
    id: &str,
    server: WebServer,
    os: &str,
) -> Result<Option<Site>> {
    check_switch_on(conn, id, server, os)?;
    if get(conn, id)?.is_none() {
        return Ok(None);
    }
    // Switching servers reallocates the recorded override port: a free port in
    // the new server's range, or None when switching to nginx (B20 §4).
    let others: Vec<Site> = list(conn)?.into_iter().filter(|s| s.id != id).collect();
    let new_port = allocate_override_port(&others, server)?;
    // B20-A guard, kept as a belt on the allocator.
    if let Some(port) = new_port {
        if let Some(other) = recorded_port_conflict(&others, id, port) {
            return Err(override_port_collision_error(&get(conn, id)?.unwrap().domain, &other, port));
        }
    }
    if !store::set_site_web_server(conn, id, server.as_db())? {
        return Ok(None);
    }
    store::set_site_override_port(conn, id, new_port)?;
    get(conn, id)
}

/// Convert a WordPress site to a multisite network (§10.1): run `wp core
/// multisite-convert` (subdomain or subdirectory) to write the network constants,
/// then persist the mode so `rebuild_configs` regenerates nginx with the matching
/// rewrite template. The caller reloads the edge afterward (no docroot/cert/DB
/// rebuild). Returns the updated site, or `None` if the id doesn't exist.
pub fn convert_multisite(
    conn: &Connection,
    php_bin: &Path,
    wp_phar: &Path,
    docroot: &Path,
    id: &str,
    mode: MultisiteMode,
) -> Result<Option<Site>> {
    if matches!(mode, MultisiteMode::None) {
        return Err(Error::Other(
            "multisite mode must be 'subdomain' or 'subdirectory'".into(),
        ));
    }
    // Already a network ON DISK — a Valet/Herd network that imported as a single
    // site before 13 Sep 2026, or one converted outside rexenv. Convert on a live
    // network rewrites its wp-config and tables; record what the FILE declares
    // instead, whatever mode was asked for.
    if let Some(existing) = network_mode_on_disk(docroot) {
        return adopt_multisite(conn, id, existing);
    }
    crate::core::wordpress::multisite_convert(
        php_bin,
        wp_phar,
        docroot,
        matches!(mode, MultisiteMode::Subdomain),
    )?;
    if !store::set_site_multisite(conn, id, mode.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// The network mode a WordPress install's OWN wp-config declares — `MULTISITE`
/// true, `SUBDOMAIN_INSTALL` choosing between the two — or `None` for a single
/// site (`WP_ALLOW_MULTISITE` only permits one). A read of the file's text
/// through `phpconf` (comments skipped, no PHP runs), the same kind of read the
/// database import makes of it. Pure.
pub fn network_mode_in_wp_config(text: &str) -> Option<MultisiteMode> {
    let on = |name: &str| {
        crate::core::phpconf::wp_define_str(text, name).is_ok_and(|v| v == "true" || v == "1")
    };
    on("MULTISITE").then(|| {
        if on("SUBDOMAIN_INSTALL") {
            MultisiteMode::Subdomain
        } else {
            MultisiteMode::Subdirectory
        }
    })
}

/// [`network_mode_in_wp_config`] for the wp-config WordPress would load from
/// `docroot` (there, or one level up). `None` when there is none or it can't be
/// read. Valet and Herd record no network anywhere else, so this is how their
/// imports learn one (13 Sep 2026 — they landed as single sites before).
pub fn network_mode_on_disk(docroot: &Path) -> Option<MultisiteMode> {
    let path = crate::core::phpconf::wp_config_path(docroot)?;
    network_mode_in_wp_config(&std::fs::read_to_string(path).ok()?)
}

/// Record that a site already IS a network — the Local import's adopt path
/// (`docs/PLAN-local-multisite.md`). Unlike [`convert_multisite`], this runs
/// nothing: the network exists in their wp-config and in the copied database,
/// and `multisite-convert` on a live network rewrites both. Every other path to
/// a network converts first; the import must never (an adopted site left at
/// `none` is served as a single site AND offered "Convert to multisite", which
/// would do exactly that). `None` is refused — un-networking a site is
/// [`clear_multisite`]'s, after a reset. Returns the updated site, `None` if the
/// id doesn't exist; the caller reloads the web tier.
pub fn adopt_multisite(conn: &Connection, id: &str, mode: MultisiteMode) -> Result<Option<Site>> {
    if matches!(mode, MultisiteMode::None) {
        return Err(Error::Other("adopting a network needs 'subdomain' or 'subdirectory'".into()));
    }
    if !store::set_site_multisite(conn, id, mode.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// Flip a site back to single-site after a reset — the fresh database has no
/// network, and the reset cleared the multisite constants from wp-config.
/// Returns the updated site (`None` if it doesn't exist).
pub fn clear_multisite(conn: &Connection, id: &str) -> Result<Option<Site>> {
    if !store::set_site_multisite(conn, id, MultisiteMode::None.as_db())? {
        return Ok(None);
    }
    get(conn, id)
}

/// Switch a site's PHP version (Phase 2 §1.4): update ONLY the `php_version`
/// column — no docroot, cert, or DB rebuild — and return the updated site (or
/// `None` if it doesn't exist). The version must have a pinned build and is
/// normalized to its minor series (`8.3.31` → `8.3`). The caller ensures the
/// target pool is running and reloads nginx so the change takes effect.
pub fn set_php_version(conn: &Connection, id: &str, version: &str) -> Result<Option<Site>> {
    let minor = php::minor_of(version);
    if php::patch_for_minor(&minor).is_none() {
        // A minor this macOS cannot run says so, before the "no build" sentence
        // below would call a shipped version unknown (§6.3 of the macOS-13 plan).
        if let Some(major) = crate::core::binaries::php_minor_needs_macos(&minor) {
            return Err(Error::Other(format!(
                "PHP {minor}: {}",
                crate::core::binaries::needs_macos_sentence(major)
            )));
        }
        // NAME what is available; never substitute a neighbouring minor. The
        // import path settled this rule (`phpTarget: null` when theirs isn't one
        // we ship, so the user chooses) and it holds harder for an agent: a
        // silent bump to the nearest shipped minor would have it report a
        // compatibility result for a version it never tested — and the nearer
        // the substitute, the more convincing the wrong answer. The set is
        // derived, not listed
        // again, so this sentence cannot outlive the versions it names.
        return Err(Error::Other(format!(
            "rexenv has no PHP {minor} build. Available: {}.",
            php::available_minors().join(", ")
        )));
    }
    // Third door into the same lie: a FrankenPHP site whose PHP is switched to a
    // major FrankenPHP cannot run. Checked against the site's CURRENT server, so
    // all three ways to form the pair (create, switch server, switch PHP) are
    // covered rather than the two that were obvious.
    if let Some(site) = get(conn, id)? {
        ensure_server_runs_php(site.web_server, &minor)?;
    }
    if !store::set_site_php_version(conn, id, &minor)? {
        return Ok(None);
    }
    get(conn, id)
}

/// Toggle a site's Xdebug (§8.2). Validation lives in CORE (M7 — no IPC path
/// can enable it where it can't work): FrankenPHP sites are refused (their
/// embedded PHP never touches the fpm pools), as are minors without a pinned
/// Xdebug bottle (8.0: the static build can't dlopen any .so). Returns the
/// updated site, or `None` if the id doesn't exist.
pub fn set_xdebug(conn: &Connection, id: &str, enabled: bool) -> Result<Option<Site>> {
    set_xdebug_on(conn, id, enabled, std::env::consts::OS)
}

/// [`set_xdebug`] for a NAMED os — see [`create_on`]. Xdebug has no Windows pins (D4), so
/// there every enable is refused by `xdebug_unavailable_reason_on` before the flag is
/// touched; a test about what the TOGGLE does has to be able to say which os it means.
pub fn set_xdebug_on(conn: &Connection, id: &str, enabled: bool, os: &str) -> Result<Option<Site>> {
    if enabled {
        let Some(site) = get(conn, id)? else {
            return Ok(None);
        };
        if matches!(site.web_server, WebServer::Frankenphp) {
            return Err(Error::Other(
                "Xdebug isn't available on FrankenPHP sites — FrankenPHP embeds its own \
                 PHP and never uses the shared pools. Switch the site to Nginx or Apache \
                 first."
                    .into(),
            ));
        }
        let minor = php::minor_of(&site.php_version);
        // The sentence comes from `core::binaries`, not from here: it depends on
        // WHY the minor has none, and this call site is in no position to know.
        // It used to say "its static build can't load extensions" for every
        // absence — true of 7.4 and 8.0, and a confident falsehood the day a
        // minor ships before its Xdebug bottle does.
        if let Some(why) = crate::core::binaries::xdebug_unavailable_reason_on(&minor, os) {
            return Err(Error::Other(why));
        }
    }
    if !store::set_site_xdebug(conn, id, enabled)? {
        return Ok(None);
    }
    get(conn, id)
}

/// Serve this site, or stop serving it (v44) — the core half: the recorded
/// switch, with the refusals that belong to the DECISION rather than to any
/// caller. Returns the updated site, or `None` when the id is not a site.
///
/// The one refusal here is a site whose provisioning never finished: it has no
/// serving surface to take away, and offering Start/Stop beside "setup
/// incomplete" would invite a user to fix a half-built site with the wrong
/// verb. Retry is the verb that half-built sites have.
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> Result<Option<Site>> {
    let Some(site) = get(conn, id)? else {
        return Ok(None);
    };
    if !site.provisioned {
        return Err(Error::Other(format!(
            "{} did not finish setting up, so there is nothing to start or stop yet. \
             Use Retry to finish it (or delete it).",
            site.domain
        )));
    }
    if !store::set_site_enabled(conn, id, enabled)? {
        return Ok(None);
    }
    get(conn, id)
}

/// What a [`teardown`] actually did. The docroot half is REPORTED rather than
/// silent: "your folder is still there" and "your folder is gone" are not
/// details a delete may leave ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Teardown {
    /// Whether the site existed at all.
    pub existed: bool,
    /// Whether the docroot was removed. Always false for a folder rexenv does
    /// not own — a linked one, or one moved outside the sites folder.
    pub docroot_removed: bool,
}

/// Full teardown of a site: remove its DB row, cert material, per-site
/// config/log artifacts, and — only if rexenv owns it — its docroot.
///
/// Does NOT rewrite the shared configs — call [`rebuild_configs`] + reload after
/// so the site stops being served — and does NOT drop the site's database (that
/// needs a running server + resolved binaries; `commands::delete_site` does it
/// before calling here).
///
/// **Docroot ownership is READ, never inferred from the path** (v17): a folder
/// we didn't create is never removed, wherever it lives and whatever the
/// sites-dir setting happens to say now. Only a pre-v17 row the startup
/// backfill hasn't reached yet falls back to the legacy lexical test — which
/// yields exactly the answer the backfill would have recorded.
pub fn teardown(conn: &Connection, platform: &dyn Platform, id: &str) -> Result<Teardown> {
    let site = match get(conn, id)? {
        Some(s) => s,
        None => return Ok(Teardown { existed: false, docroot_removed: false }),
    };

    store::delete_site(conn, id)?;

    // The settled import fact goes with the site (its mirrored user, if any,
    // was dropped by the delete command BEFORE teardown, reading this row —
    // recorded, not derived). Also closes a Stage 2 gap: without this, the
    // row outlived its site as an orphan.
    store::delete_db_import(conn, id)?;

    // The cloned plugins/themes an agent added (v29). Same gap as the import
    // row above and the same fix: no foreign key, so nothing removes these on
    // their own. The read path (`all_scratch_packages`) ALSO joins to `sites`,
    // and that join STAYS — two defences on one fact, the shape used elsewhere
    // here. They fail differently: this delete keeps the table from growing
    // orphans, the join keeps a row that somehow survives from ever being shown
    // as a package of a site that is gone. Removing either because the other
    // exists is how a fact ends up with none.
    store::delete_scratch_packages(conn, id)?;

    // D2's plain-delete leg: the rewrite records and OUR backups go with the
    // site. Their config file itself is never touched here — reverting first
    // is a choice the delete confirm offers (the default button), never a
    // silent side effect.
    for r in store::config_rewrites_for_site(conn, id)? {
        let _ = std::fs::remove_file(&r.backup_path);
        store::delete_config_rewrite(conn, id, &r.file)?;
    }

    // Remove the per-site cert dir (best-effort).
    let cert_dir = ssl::site_cert_dir(platform.paths(), &site.domain)?;
    let _ = std::fs::remove_dir_all(&cert_dir);

    // Remove per-site config/log artifacts (best-effort): the FrankenPHP and
    // Apache override configs + logs (if the site ever ran an override server)
    // and the tunnel log (if it was ever shared). Paths come from the owning
    // modules so the names can't drift.
    //
    // Apache was MISSING here until 13 Aug 2026 — this list is hand-maintained
    // while the function's name promises every per-site artifact, which is the
    // narrower-than-its-claim family. `every_per_site_artifact_is_swept_by_both
    // _sweeps` now fails the build when a module grows one of these paths and
    // this list does not learn about it.
    if let Ok(conf) = frankenphp::config_path(platform, &site.domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = frankenphp::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(conf) = apache::config_path(platform, &site.domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = apache::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = apache::error_log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    // OpenLiteSpeed keeps a whole server root per site (config, run dir, LSCache storage):
    // the config's directory's parent IS that root, removed whole; its logs live beside the
    // other servers' and come from the module's one list.
    if let Ok(conf) = super::openlitespeed::config_path(platform, &site.domain) {
        let _ = std::fs::remove_file(conf);
    }
    super::openlitespeed::remove_server_root(platform, &site.domain);
    if let Ok(log) = super::openlitespeed::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    for log in super::openlitespeed::log_paths(platform, &site.domain).unwrap_or_default() {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = tunnels::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    // The job logs, one per run (provisioning, WordPress install, database
    // import, Git jobs). The row is already gone, so `list` is every OTHER site
    // — a neighbour whose domain extends this one keeps its files. No list, no
    // sweep: guessing without the neighbours could take theirs.
    if let Ok(others) = list(conn) {
        let others: Vec<String> = others.into_iter().map(|s| s.domain).collect();
        crate::core::logs::remove_run_logs(platform, &site.domain, &others);
    }

    // Remove the docroot, but only if it's ours — and never while a git
    // worktree whose repository lives elsewhere sits inside it (ledger #814).
    // `delete_preflight` refuses that delete before anything destructive runs;
    // this is the second defence, for any caller that reached teardown without
    // it, and it fails toward KEEPING the folder: the row is already gone, so
    // erroring here would strand nothing, while deleting would destroy work no
    // repository has a copy of.
    let owned = docroot_owned(conn, platform, &site)?;
    let foreign = if owned && !site.path.is_empty() {
        super::worktree::foreign_checkout_under(Path::new(&site.path))
    } else {
        None
    };
    if let Some(dir) = &foreign {
        log::warn!(
            "sites: kept {} — it holds a git worktree ({}) whose repository lives elsewhere",
            site.path,
            dir.display()
        );
    }
    let docroot_removed = owned
        && foreign.is_none()
        && !site.path.is_empty()
        && std::fs::remove_dir_all(&site.path).is_ok();

    Ok(Teardown { existed: true, docroot_removed })
}

/// Does rexenv own this site's docroot — may teardown remove it? The recorded
/// answer (v17); only a pre-v17 row the startup backfill hasn't reached yet
/// falls back to the legacy test: under a managed sites dir — the configured
/// one, the current default, OR the legacy app-data default (so neither
/// changing the setting nor the default-change to ~/rexenv/Sites strands
/// teardown of pre-existing sites).
fn docroot_owned(conn: &Connection, platform: &dyn Platform, site: &Site) -> Result<bool> {
    Ok(match site.docroot_managed {
        Some(owned) => owned,
        None => docroot_under_managed_root(
            &site.path,
            &sites_dir(conn, platform)?,
            &default_sites_dir()?,
            &legacy_sites_dir(platform)?,
        ),
    })
}

/// Every rule that must hold before a site delete takes its FIRST destructive
/// step (the tunnel stop, the database drop — `commands::sites::
/// delete_site_owned` runs those before [`teardown`]), so a refusal leaves the
/// site exactly as it was:
///
/// - **A parent with worktree children is refused** (ledger #815) — a worktree
///   whose main repository is gone is a broken checkout. The database refuses
///   the row delete too (v45's `parent_id` has no `ON DELETE`); this is the
///   check that says so in a sentence, and says it before the database drop.
/// - **A docroot we own that holds a git worktree is refused** (ledger #814) —
///   `remove_dir_all` would destroy uncommitted work in a checkout whose
///   repository lives elsewhere and will not notice. `git worktree remove`
///   refuses a dirty worktree on its own, so the fix line sends the user there.
pub fn delete_preflight(conn: &Connection, platform: &dyn Platform, site: &Site) -> Result<()> {
    let children = store::worktree_children(conn, &site.id)?;
    if !children.is_empty() {
        let mut names = Vec::new();
        for w in &children {
            names.push(match store::get_site(conn, &w.site_id)? {
                Some(c) => c.domain,
                None => w.site_id.clone(),
            });
        }
        return Err(Error::Other(format!(
            "{} has {} worktree site{} ({}) — delete {} first: a worktree whose main \
             repository is gone is a broken checkout.",
            site.domain,
            names.len(),
            if names.len() == 1 { "" } else { "s" },
            names.join(", "),
            if names.len() == 1 { "it" } else { "them" },
        )));
    }
    if docroot_owned(conn, platform, site)? && !site.path.is_empty() {
        if let Some(dir) = super::worktree::foreign_checkout_under(Path::new(&site.path)) {
            return Err(Error::Other(format!(
                "{} is a git worktree — its repository lives outside this site's folder, \
                 so it may hold work that exists nowhere else, and rexenv never deletes \
                 one itself. Commit or stash what you want to keep, then remove it with git \
                 (it refuses if anything is uncommitted) and delete the site again:\n  \
                 $ git worktree remove \"{}\"",
                dir.display(),
                dir.display()
            )));
        }
    }
    Ok(())
}

/// Whether a site type needs a database provisioned (Blank PHP: none;
/// WordPress/Laravel: yes). Called by the provision job to decide the `db`
/// phase and by teardown's drop decision — ONE answer, so a type can never be
/// given a database name it never gets a database for (which is exactly what
/// Laravel sites had while this function sat with no callers at all).
pub fn needs_database(site_type: SiteType) -> bool {
    matches!(site_type, SiteType::Wordpress | SiteType::Laravel)
}

/// The folder a Laravel project serves from — its front controller lives in
/// `public/index.php` and `.env` deliberately does NOT. One constant, used by
/// both detection (linked projects) and creation (`docroot_subdir`).
pub const LARAVEL_DOCROOT_SUBDIR: &str = "public";

/// A validated intent to fill a new site's docroot from a repository (v33).
///
/// `url` is the NORMALIZED form `repo::parse_source` produced — the same value
/// that reaches `git` argv, never the raw paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    pub url: String,
    /// The branch/tag picked, already through `repo::validate_ref`. `None` =
    /// whatever the remote calls default.
    pub git_ref: Option<String>,
}

/// Read a [`NewSite`]'s repository intent, or refuse it. `Ok(None)` = this site
/// is not being cloned.
///
/// THE one implementation of the rule, called from both sides on purpose:
/// [`provision_with`] calls it as the guard every caller passes through (app,
/// `rex` CLI, MCP), and the provision job calls it to get the value it will
/// clone. Two calls, one rule — the alternative was a guard in core and a
/// second, drift-prone parse in the command layer.
///
/// Three refusals, each for a different reason:
///
/// - **`git_url` + `path` together.** Linking adopts a folder rexenv promises
///   never to write into; cloning fills a folder rexenv just made. Ranking one
///   over the other would mean picking, for the user, which of two promises to
///   break. (Cloning INTO an existing folder is a real feature request and a
///   genuinely dangerous one — it belongs behind its own consent, not behind a
///   precedence rule nobody reads.)
/// - **An agent asked.** `Ownership::Agent` is the MCP scratch-site tier, and
///   a clone downloads code a MODEL chose and then executes it (`composer
///   install` runs the project's own scripts). No click stands between the
///   two. The tier grants disposable sites, not arbitrary code execution.
/// - **The URL or ref doesn't parse.** Everything that reaches `git` argv is
///   validated here, before a site row exists — the module invariant
///   `core::repo` already holds for plugin/theme clones.
pub fn validate_git_source(new: &NewSite, ownership: &Ownership) -> Result<Option<GitSource>> {
    if new.git_url.trim().is_empty() {
        return Ok(None);
    }
    if !new.path.trim().is_empty() {
        return Err(Error::Other(
            "a site is either CLONED into a folder rexenv creates or LINKED to a folder you \
             already have — not both. Clear one of them."
                .into(),
        ));
    }
    // `UserByAgent` passes here: cloning under a scope grant is the `run`
    // scope's business, decided in the tool layer BEFORE this is reached
    // (`site_create` never sets `git_url`; a future git tool claims `run`).
    // Only the scratch path is refused outright — a disposable site is not a
    // reason to run a stranger's install scripts.
    if matches!(ownership, Ownership::Agent { .. }) {
        return Err(Error::Other(
            "creating a site from a git repository is a user action: it downloads code and then \
             runs the project's own install scripts. Create the site empty and let the user \
             clone into it."
                .into(),
        ));
    }
    let src = repo::parse_source(&new.git_url)?;
    let git_ref = new
        .git_ref
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(repo::validate_ref)
        .transpose()?;
    Ok(Some(GitSource { url: src.url, git_ref }))
}

/// The repository a STORED site records as its source, re-validated.
///
/// The row is ours, so this is defence in depth rather than distrust — but the
/// module invariant is that everything reaching `git` argv passes the parser,
/// and "except when it came from our own database" is exactly the exemption
/// that stops being true the day a column is written from somewhere new.
/// The clone phase and Retry both read the source through here.
pub fn git_source_of(site: &Site) -> Result<Option<GitSource>> {
    let Some(url) = site.git_url.as_deref().filter(|u| !u.trim().is_empty()) else {
        return Ok(None);
    };
    let src = repo::parse_source(url)?;
    let git_ref = site
        .git_ref
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(repo::validate_ref)
        .transpose()?;
    Ok(Some(GitSource { url: src.url, git_ref }))
}

/// Prefix of the temporary directory a clone lands in before it is moved into
/// place. Dot-leading so it stays out of Finder and out of the sites-folder
/// listing, and distinctive enough that a leftover one is obviously ours.
const CLONE_STAGING_PREFIX: &str = ".rexenv-clone-";

/// Where a clone is written before it becomes the docroot.
///
/// A SIBLING of the docroot, never app-data or `/tmp`: the Sites folder is
/// user-configurable and may sit on another volume, where the `rename` below
/// would fail with `EXDEV` after a multi-minute download. Same parent ⇒ same
/// filesystem ⇒ the move is atomic and instant.
fn staging_dir(docroot: &Path, token: &str) -> Result<PathBuf> {
    let parent = docroot
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent directory", docroot.display())))?;
    let name = docroot
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Other(format!("{} has no folder name", docroot.display())))?;
    Ok(parent.join(format!("{CLONE_STAGING_PREFIX}{name}-{token}")))
}

/// Refuse a docroot that already holds anything.
///
/// The site's own folder, created empty during prepare — so "not empty" means
/// either a checkout already landed (Retry's job to notice, not this one's) or
/// the user put files there. Either way this is not the moment to decide whose
/// files they are.
fn ensure_empty_docroot(docroot: &Path) -> Result<()> {
    let mut entries = std::fs::read_dir(docroot).map_err(|e| {
        Error::Other(format!("can't read {} to clone into it: {e}", docroot.display()))
    })?;
    if entries.next().is_some() {
        return Err(Error::Other(format!(
            "{} is not empty — rexenv only clones into a folder it just created. Delete this \
             site and create it again, or link the existing folder instead.",
            docroot.display()
        )));
    }
    Ok(())
}

/// Does what landed match the kind of site the user asked to create?
///
/// The type is chosen BEFORE the clone (it fixes the phase list and the binary
/// plan — including whether ~600 MB of database engine is downloaded), and
/// `ls-remote` cannot see files, so the check has to happen afterwards. The
/// honest failure names what was actually found: re-typing the site here would
/// change the job's plan after the card had already described it.
///
/// `Php` accepts anything — a document root full of files IS a PHP site, and
/// [`detect_project`] falls back to serving the root for shapes it can't place.
fn verify_cloned_shape(detected: &DetectedProject, expect: SiteType) -> Result<()> {
    if expect == SiteType::Php || detected.site_type == expect {
        return Ok(());
    }
    Err(Error::Other(format!(
        "this repository looks like {} — not {}. Create the site as {} instead (the code is \
         cloned, nothing else has been set up).",
        detected.label,
        expect.as_db(),
        detected.site_type.as_db()
    )))
}

/// Fill a site's own (empty) docroot from a repository, and report what landed.
///
/// The move is the interesting part. `repo::clone_repo` refuses a `dest` that
/// exists — deliberately, so it may remove a partial checkout on failure
/// without ever deleting a directory it did not create, and that guard is not
/// worth weakening for this. So the clone goes to a staging sibling and is
/// moved in:
///
/// ```text
/// clone → <sites>/.rexenv-clone-<domain>-<token>
/// remove_dir(docroot)     ← the OS itself refuses a NON-EMPTY directory
/// rename(staging, docroot)
/// ```
///
/// `remove_dir`, never `remove_dir_all`: the guarantee that this cannot destroy
/// a developer's files is the operating system's, not a check of ours that a
/// later edit could get wrong. (`ensure_empty_docroot` above runs first so the
/// refusal is a readable message rather than an `ENOTEMPTY`; it is the message,
/// not the guarantee.)
///
/// Every exit path removes the staging directory — a path this function built
/// and `clone_repo` created, so `remove_dir_all` there is scoped to our own
/// work. On a failed rename the (empty) docroot is put back, since the rest of
/// provisioning and a later Retry both expect it to exist.
#[allow(clippy::too_many_arguments)] // flat mirror of the step's inputs
pub fn clone_into_docroot(
    supervisor: &dyn ProcessSupervisor,
    git: &Path,
    env: &[(String, String)],
    src: &GitSource,
    docroot: &Path,
    expect: SiteType,
    cancel: &repo::CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<DetectedProject> {
    ensure_empty_docroot(docroot)?;
    let token = Uuid::new_v4().to_string();
    let staging = staging_dir(docroot, &token[..8])?;

    repo::clone_repo(
        supervisor,
        git,
        env,
        &src.url,
        src.git_ref.as_deref(),
        &staging,
        cancel,
        on_line,
    )?;

    // From here on, anything that goes wrong must not leave the staging dir
    // behind — a half-cloned project sitting beside the sites folder is both
    // confusing and, for a private repo, a checkout nobody knows they have.
    let finish = (|| -> Result<DetectedProject> {
        let detected = detect_project(&staging);
        verify_cloned_shape(&detected, expect)?;
        std::fs::remove_dir(docroot).map_err(|e| {
            Error::Other(format!(
                "the clone finished but {} could not be replaced ({e}) — nothing was deleted.",
                docroot.display()
            ))
        })?;
        if let Err(e) = std::fs::rename(&staging, docroot) {
            // The docroot was empty; put it back so the rest of provisioning
            // (and Retry) still finds the folder it expects.
            let _ = std::fs::create_dir_all(docroot);
            return Err(Error::Other(format!(
                "moving the clone into {} failed: {e}",
                docroot.display()
            )));
        }
        Ok(detected)
    })();
    if finish.is_err() && staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    finish
}

/// The validated TLD of `domain` — its last label, returned only after the
/// full domain validation (charset, labels, TLD policy) passes. Callers use it
/// to key per-TLD side effects (the OS resolver file) off an already-vetted
/// value, never off raw input.
pub fn domain_tld(domain: &str) -> Result<String> {
    validate_domain(domain)?;
    Ok(domain
        .rsplit('.')
        .next()
        .expect("validate_domain guarantees a TLD")
        .to_string())
}

// ── Extra domains a site answers on (v42) ─────────────────────────────────────

/// Every hostname a site answers on: its own domain FIRST, then its aliases in
/// stored order.
///
/// One function, because every consumer needs the same list and each of them
/// getting it right separately is how a site ends up with a certificate for two
/// names, an nginx `server_name` for three and an edge route for one.
pub fn all_domains(conn: &Connection, site: &Site) -> Result<Vec<String>> {
    let mut out = vec![site.domain.clone()];
    out.extend(store::get_site_aliases(conn, &site.id)?);
    Ok(out)
}

/// Validate an alias against the WHOLE hostname space, not just the alias
/// table.
///
/// The schema can only enforce half of this: `site_domains.domain` is UNIQUE,
/// so two sites cannot share an alias, but nothing in SQL stops an alias equal
/// to some site's PRIMARY domain — and that collision is the dangerous one.
/// Two server blocks answering one hostname means nginx serves whichever it
/// matched first while both sites look fine in the UI, and the user's own
/// mental model ("this name belongs to that project") is what breaks.
///
/// Returns the normalized value to store.
pub fn validate_alias(conn: &Connection, site_id: &str, domain: &str) -> Result<String> {
    let domain = normalize_hostname(domain);
    validate_domain(&domain)?;
    if let Some(other) = store::site_by_domain(conn, &domain)? {
        return Err(Error::Other(if other.id == site_id {
            format!("\"{domain}\" is already this site's own domain")
        } else {
            format!(
                "\"{domain}\" is already the domain of the site \"{}\" — one hostname can only \
                 reach one site, or nginx answers with whichever server block it matched first",
                other.name
            )
        }));
    }
    if let Some(owner) = store::site_id_for_alias(conn, &domain)? {
        return Err(Error::Other(if owner == site_id {
            format!("this site already answers on \"{domain}\"")
        } else {
            let name = get(conn, &owner)?.map(|s| s.name).unwrap_or_else(|| owner.clone());
            format!("\"{domain}\" is already an extra domain of the site \"{name}\"")
        }));
    }
    Ok(domain)
}

/// Add an alias to a site (validated). Returns the stored value.
pub fn add_alias(conn: &Connection, site_id: &str, domain: &str) -> Result<String> {
    let site = get(conn, site_id)?
        .ok_or_else(|| Error::Other(format!("site not found: {site_id}")))?;
    let domain = validate_alias(conn, &site.id, domain)?;
    store::add_site_alias(conn, &site.id, &domain)?;
    Ok(domain)
}

/// Remove an alias. `false` when the site did not answer on that name — never
/// an error, so a retry after a partial failure is safe.
///
/// Normalised exactly as [`validate_alias`] normalises on the way IN — the
/// first version forgot the trailing dot, so `--add shop.rex.` stored
/// `shop.rex` and `--remove shop.rex.` could not find it.
pub fn remove_alias(conn: &Connection, site_id: &str, domain: &str) -> Result<bool> {
    store::remove_site_alias(conn, site_id, &normalize_hostname(domain))
}

/// The ONE spelling a hostname has inside rexenv: trimmed, no trailing dot,
/// lower-case. Every boundary that accepts a hostname from a person (the
/// Domains card, `rex site domains`, an import) normalises through here BEFORE
/// it validates or compares — `validate_domain` rejects upper-case outright, so
/// a caller that validated the raw string refused `Shop.rex` while the core
/// beneath it would have stored `shop.rex`.
pub fn normalize_hostname(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Settings key for the configurable sites root.
pub const SITES_DIR_KEY: &str = "sites_dir";

/// How long a scratch (agent-created) site lives without being used, in hours
/// (MCP M2a, PLAN §4.3). ONE definition, so the clock that creation starts and
/// the clock activity extends can never drift apart. A user-configurable
/// setting lands with the create tool; until then this is the value.
pub const SCRATCH_TTL_HOURS: i64 = 24;

/// Settings key for the default TLD new sites are created under (v1 of the
/// configurable-TLD feature: default-for-NEW-sites only — existing sites keep
/// their domain; re-point one via Change domain if wanted).
pub const DEFAULT_TLD_KEY: &str = "default_tld";

/// The default TLD for new sites: the `default_tld` setting if it holds an
/// allowed TLD, else the `.rex` backbone. A stored value that no longer
/// passes policy (edited DB, tightened blocklist) falls back rather than
/// resurfacing a blocked TLD in the UI.
pub fn default_tld(conn: &Connection) -> Result<String> {
    match store::get_setting(conn, DEFAULT_TLD_KEY)? {
        Some(t) if tld::ensure_allowed(&t).is_ok() => Ok(t),
        _ => Ok(tld::BACKBONE_TLD.to_string()),
    }
}

/// Set the default TLD for new sites. POLICY-GATED in the backend: a blocked
/// TLD (.local, .dev, 2-letter, popular gTLDs …) is refused with the policy's
/// reason — even via a direct IPC invoke. `.test` need not be the value; it
/// stays active regardless (backbone).
pub fn set_default_tld(conn: &Connection, new_tld: &str) -> Result<String> {
    let new_tld = new_tld.trim().trim_start_matches('.');
    tld::ensure_allowed(new_tld)?;
    store::set_setting(conn, DEFAULT_TLD_KEY, new_tld)?;
    Ok(new_tld.to_string())
}

/// Validate + store the sites folder. The gated setter for [`SITES_DIR_KEY`],
/// the same shape as [`set_default_tld`] — the generic KV command routes here so
/// no IPC path can smuggle a value past this.
///
/// # Why this refuses rather than sanitises
///
/// The value becomes a provision docroot (`<sites_dir>/<domain>`) and is written
/// into generated Caddy and nginx configs. Those are QUOTED (app-data paths
/// contain spaces), so a quote or a backslash in the value ends the quoted
/// string early and the rest of the path becomes config — the B26 concern.
/// Silently stripping the character would hand back a folder the user did not
/// pick and did not agree to, and their sites would be created somewhere they
/// never chose. A refusal is the only answer that cannot be wrong.
///
/// Relative paths are refused for the same reason: a docroot resolved against
/// whatever the app's working directory happens to be is not a location anyone
/// chose. Everything else a real folder can contain — spaces, unicode, `'` —
/// is accepted, because the configs quote and nothing here goes near a shell.
///
/// # An existing value that would not pass
///
/// Is left alone, deliberately. Validation is on the WRITE path only: the read
/// ([`sites_dir`]) is unchanged, so a value stored before this existed keeps
/// resolving exactly as it did. Refusing at read time would silently relocate a
/// user's sites folder to the default and make every site they own look missing
/// — a far worse outcome than the litter it would prevent, and on a released
/// product it would arrive as "rexenv lost my sites". The next time they change
/// the folder, the new value is validated.
pub fn set_sites_dir(conn: &Connection, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::Other(
            "The sites folder can't be empty. Pick a folder, or leave the setting alone to keep \
             using the default."
                .into(),
        ));
    }
    if !Path::new(trimmed).is_absolute() {
        return Err(Error::Other(format!(
            "The sites folder has to be a full path (starting at `/`, or at a drive such as `C:\\`) \
             — `{trimmed}` is relative, so where your sites landed would depend on where rexenv \
             was started from."
        )));
    }
    // The characters that cannot survive a quoted path in a generated config,
    // plus NUL which cannot survive a filesystem call. `"` and `\` end or escape
    // the quoted string; a newline ends the directive. Judged per folder NAME, so
    // Windows' `\` separator is not one of them (`config_breaking_char`).
    if let Some(bad) = config_breaking_char(trimmed, |c| matches!(c, '"' | '\\' | '\n' | '\r' | '\0')) {
        let shown = match bad {
            '\n' => "a line break".to_string(),
            '\r' => "a carriage return".to_string(),
            '\0' => "a null byte".to_string(),
            c => format!("`{c}`"),
        };
        return Err(Error::Other(format!(
            "The sites folder can't contain {shown}: the path is written into rexenv's web-server \
             configs, and that character would end the line early. Pick a folder without it."
        )));
    }
    store::set_setting(conn, SITES_DIR_KEY, trimmed)?;
    Ok(trimmed.to_string())
}

/// Default site docroot root: `~/rexenv/Sites` — user-visible and Finder
/// friendly (`directories` resolves the home dir correctly per OS). Only a
/// DEFAULT: an explicitly configured `sites_dir` setting always wins, and
/// existing site rows hold absolute paths, so changing the default never
/// orphans previously created sites.
///
/// `pub` so the live-check harness can name the exact directory production
/// would use, rather than recomputing `~/rexenv/Sites` beside it and drifting.
/// It is the path an example that forgets to pin `sites_dir` writes into — the
/// USER'S real Sites folder — which happened on 13 Aug 2026.
pub fn default_sites_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| Error::Other("could not resolve the home directory".into()))?;
    Ok(base.home_dir().join("rexenv").join("Sites"))
}

/// The pre-`~/rexenv/Sites` default under app-data. Kept ONLY so teardown can
/// still delete the docroots of sites created under the old default — without
/// it, deleting such a site would silently strand its files on disk.
fn legacy_sites_dir(platform: &dyn Platform) -> Result<PathBuf> {
    Ok(platform.paths().app_data_dir()?.join("sites"))
}

/// Root directory for site docroots: the `sites_dir` setting if set, else the
/// `~/rexenv/Sites` default.
/// Refuse to create docroots in the USER'S real Sites folder from a sandboxed
/// platform.
///
/// **The fixture cannot forget to write this one.** `sites::provision` reads the
/// `sites_dir` SETTING, which falls back to a HOME-derived default — a path no
/// sandboxed `Paths` can redirect — so an example that sandboxes its paths and
/// forgets `common::pin_fixture_sites_dir` provisions into `~/rexenv/Sites` and
/// leaves docroots there. Measured 24 Aug 2026 on the machine this was written
/// on: **19 orphaned directories, 437 MB**, five of them whole WordPress
/// installs, left by examples that ran before the pinning sweep.
///
/// The signal is unambiguous and cannot fire on a real install: a sandbox's
/// `app_data_dir` lives under `/private/tmp`, while a real one is always inside
/// the user's home (`~/Library/Application Support/…` on macOS). So "app-data is
/// OUTSIDE the home directory, and `sites_dir` is still the home-derived
/// default" means exactly one thing — a fixture that pinned its paths and not
/// its sites folder.
///
/// A refusal rather than a redirect: silently relocating would make the example
/// pass while proving something about a directory nobody chose, and the whole
/// lesson of this class is that a fixture writing where it did not intend is the
/// defect, not the cleanup.
fn refuse_unpinned_sandbox_sites_dir(dir: &Path, platform: &dyn Platform) -> Result<()> {
    let Ok(app_data) = platform.paths().app_data_dir() else {
        return Ok(()); // cannot tell; production behaviour unchanged
    };
    refuse_unpinned_sandbox_sites_dir_for(dir, &app_data)
}

/// The rule, with the platform's app-data path as a PARAMETER.
///
/// Split out so a test can drive both sides. Through the live function the
/// sandboxed case is unreachable — a unit test runs on the real platform — and a
/// guard whose failing branch no test can reach is the vacuous shape this repo
/// keeps finding.
fn refuse_unpinned_sandbox_sites_dir_for(dir: &Path, app_data: &Path) -> Result<()> {
    let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) else {
        return Ok(());
    };
    let sandboxed = !app_data.starts_with(&home);
    let is_default = default_sites_dir().is_ok_and(|d| d == dir);
    if sandboxed && is_default {
        return Err(Error::Other(format!(
            "refusing to provision into {} from a sandboxed platform (app data is {}). \
             This is a fixture that sandboxed its PATHS and not its SITES FOLDER: \
             `sites_dir` is a setting with a home-derived default, which no sandboxed \
             `Paths` can redirect. Call `common::pin_fixture_sites_dir(&conn, \"tag\")` \
             (or open the database with `common::sandbox_db`, which pins for you) before \
             provisioning.",
            dir.display(),
            app_data.display()
        )));
    }
    Ok(())
}

pub fn sites_dir(conn: &Connection, _platform: &dyn Platform) -> Result<PathBuf> {
    match store::get_setting(conn, SITES_DIR_KEY)? {
        Some(p) if !p.trim().is_empty() => Ok(PathBuf::from(p)),
        _ => default_sites_dir(),
    }
}

/// Ensure the resolved sites root exists — first run creates `~/rexenv/Sites`
/// (`create_dir_all`: fine when `~/rexenv` already exists without `Sites`,
/// idempotent when both do). Also recreates a user-configured folder that
/// vanished. The caller logs a failure (unwritable home, permissions) without
/// aborting startup — provision surfaces its own clear error later.
pub fn ensure_sites_dir(conn: &Connection, platform: &dyn Platform) -> Result<PathBuf> {
    let dir = sites_dir(conn, platform)?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Provision a new site end-to-end (filesystem + cert + DB row).
///
/// Two shapes, decided by whether the caller supplied a path:
/// - **empty path — we create the site**: docroot is `<sites_dir>/<domain>`, and
///   for Blank PHP the generated starter `index.php` is dropped in. Ours to
///   delete.
/// - **non-empty path — LINK an existing folder**: it is validated
///   ([`validate_linked_docroot`]) and served in place. We never create it,
///   never write into it here, and record that we don't own it, so deleting the
///   site can never remove it.
///
/// The DB-provisioning step branches on [`needs_database`] (a hook for 9.2).
/// Does NOT (re)write the shared server configs — call [`rebuild_configs`] +
/// reload after, so one apply covers any number of changes.
/// Who a new site will belong to — and, because the two must never disagree,
/// the SAME value decides whether its creation may raise a privileged prompt.
///
/// Threading one value rather than two (an `origin` plus a `never_prompt` bool)
/// is deliberate: "an agent-created site never prompts" is then true by
/// construction instead of true as long as every caller remembers to set both.
/// A bool pair is exactly the shape that drifts.
#[derive(Debug, Clone)]
pub enum Ownership {
    /// The user asked for this site, in the app or the CLI.
    User,
    /// An agent created it through the MCP server: recorded `origin='agent'`,
    /// stamped with the client name and a TTL, and never allowed to prompt.
    Agent {
        /// The MCP client's self-reported name — display-only, capped at write.
        client: String,
        /// Hours from now until it expires.
        ttl_hours: i64,
    },
    /// The USER's site, created on their behalf by an agent under a scope grant
    /// (MCP parity, `docs/archive/PLAN-mcp-parity.md` §4.1). Recorded exactly as `User`
    /// — `origin='user'`, no client badge, no clock, never reaped — because the
    /// user asked for it (the grant is the asking). What differs is the one
    /// thing a grant is NOT: an administrator password. So it never prompts,
    /// and a site whose TLD has no resolver fails with the setup message
    /// instead of raising the macOS dialog on an agent's behalf (#210's rule,
    /// third variant).
    UserByAgent {
        /// The MCP client's self-reported name — for the FEED row, not the
        /// site row (the Sites page reads `origin` alone, #219).
        client: String,
    },
}

impl Ownership {
    /// May creating this site raise a privileged password prompt? Derived, never
    /// passed alongside — see the type doc.
    pub fn resolver_prompt(&self) -> crate::core::dns::ResolverPrompt {
        match self {
            Ownership::User => crate::core::dns::ResolverPrompt::Allow,
            Ownership::Agent { .. } | Ownership::UserByAgent { .. } => {
                crate::core::dns::ResolverPrompt::Never
            }
        }
    }
}

pub fn provision(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    new: NewSite,
) -> Result<Site> {
    provision_with(conn, platform, ca, new, Ownership::User)
}

/// [`provision`], with ownership recorded at the insert (v27). The agent
/// variant is a distinct VALUE rather than a flag on the human path, so no
/// caller can pass the wrong one by accident and nothing downstream has to ask
/// which mode it is in.
pub fn provision_with(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    mut new: NewSite,
    ownership: Ownership,
) -> Result<Site> {
    validate_domain(&new.domain)?; // before any docroot/cert/DB use of the domain
    if let Some(owner) = domain_taken_by(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "{} already reaches the site \"{owner}\" — one hostname can only reach one site",
            new.domain
        )));
    }
    // Evaluated HERE, while `new.path` still means "the caller asked to link
    // this folder" — below, it becomes the resolved docroot. Refused before the
    // docroot and the certificate exist, so a bad repo URL, or an agent asking
    // for one, leaves nothing to clean up.
    let git = validate_git_source(&new, &ownership)?;
    // The site-SHAPE refusals, here for the same reason. They also run inside
    // `create_recording_ownership` — the insert chokepoint, which every path
    // reaches — but that is AFTER this function has made the folder, written a
    // Blank-PHP starter page into it and issued a certificate. Found by the site
    // matrix on 11 Sep 2026: every refused WordPress/PostgreSQL create left an
    // empty folder in the user's Sites directory, and every refused Blank-PHP
    // one left an `index.php` in it.
    refuse_unbuildable(conn, &new)?;

    // A caller-supplied path means LINK: adopt the folder as-is. Nothing is
    // created and nothing is written into it — not even the Blank-PHP probe
    // file, which would land in the user's own project.
    let linked = !new.path.trim().is_empty();
    let docroot = if linked {
        validate_linked_docroot(conn, platform, &new.path)?
    } else {
        // The one place a docroot is CREATED, so the one place the sandbox
        // check belongs — a linked site (the other branch) writes nothing.
        let root = sites_dir(conn, platform)?;
        refuse_unpinned_sandbox_sites_dir(&root, platform)?;
        let docroot = root.join(&new.domain);
        std::fs::create_dir_all(&docroot)?;
        // The Blank-PHP starter page, but NOT when a clone is about to fill this
        // folder: `clone_into_docroot` requires an empty docroot, so writing a
        // placeholder here would make the site's own prepare phase the thing
        // that blocks its clone phase.
        //
        // The PAGE only. Its `db.php` is written by the provision job's
        // `configure` phase, next to the `CREATE DATABASE` it describes — the
        // database name is allocated below (`unique_db_name`, which needs the
        // connection), and a connection file naming a database that does not
        // exist yet is a file that lies for the length of a provision.
        if matches!(new.site_type, SiteType::Php) && new.git_url.trim().is_empty() {
            super::starter::write_files(&docroot, None)?;
        }
        docroot
    };

    // Issue the per-site cert (wildcard SAN) signed by our CA.
    ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, &new.domain)?;

    // Database branch (pluggable): Blank PHP needs none. For WordPress the DB is
    // created downstream by `wordpress::install_wordpress` (bundled-mysql
    // `create_database` before `wp core install`), not here.
    if needs_database(new.site_type) {
        // Intentionally empty — marks where a site type needing a DB at
        // provision time (rather than at install time) would plug in.
    }

    new.path = docroot.display().to_string();
    create_recording_ownership(conn, new, !linked, ownership, git)
}

// ---------------------------------------------------------------------------
// Streamed-provision progress (the create-site card). PURE — phases are OUR
// step boundaries (deterministic Rust code), zero subprocess-output parsing.
// ---------------------------------------------------------------------------

/// Coarse fixed weights per provision phase — NOT time estimates. Equal slices
/// lie in feel (the bar races through the instant phases then parks for
/// minutes on the two network ones); these constants put the visual budget
/// where wall-time actually lives. The bar still only moves on real phase
/// completions and, within `fetch`, real downloaded bytes.
pub const PROVISION_PHASE_WEIGHTS: &[(&str, u32)] = &[
    ("prepare", 3),       // resolver + docroot + cert + row (ours, instant)
    ("fetch", 27),        // binary prefetch — REAL byte progress folds in
    ("clone", 20),        // git clone — a whole history over the network
    ("db", 5),            // spawn engine + readiness probe
    ("core_download", 35), // wp core download ~25MB — no byte signal (B25)
    ("configure", 5),     // wp-config + CREATE DATABASE
    ("core_install", 10), // wp core install
    ("deps", 30),         // composer install — the long pole of a cloned site
    ("finalize", 5),      // artisan key:generate + migrate
    ("assets", 25),       // <manager> install + run build — node_modules is not small
    ("blueprint", 5),     // blueprint plugins/themes (when requested)
    ("serve", 10),        // pool + edge reload + await_ready
];

fn provision_phase_weight(key: &str) -> u32 {
    PROVISION_PHASE_WEIGHTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, w)| *w)
        .unwrap_or(5)
}

/// Phase-weighted determinate progress for the streamed site-create job.
///
/// Same honesty contract as [`crate::core::wordpress::InstallProgress`] (see its doc for
/// the B25 "no invented percentage" distinction — this too is OBSERVED
/// progress, never an estimate): the job's applicable phases are fixed at
/// start (weights renormalize, so a phase that can't happen — no blueprint,
/// non-WordPress — is never a slice that can't fill), the percentage is
/// MONOTONIC, capped at 99 until the job settles ok ([`Self::finish`]), and
/// simply stops moving on failure/cancel — the caller never rolls it back or
/// snaps it forward. Within one phase, `fraction` carries the only real
/// sub-signal we have: the download Hub's byte fraction during `fetch`
/// (genuine bytes, not a guess); every other phase contributes 0 until done.
pub struct ProvisionProgress {
    weights: Vec<u32>,
    total: u32,
    pct: u8,
}

impl ProvisionProgress {
    /// `keys` = the job's applicable phases, in execution order.
    pub fn new(keys: &[&str]) -> Self {
        let weights: Vec<u32> = keys.iter().map(|k| provision_phase_weight(k)).collect();
        let total = weights.iter().sum::<u32>().max(1);
        Self { weights, total, pct: 0 }
    }

    pub fn pct(&self) -> u8 {
        self.pct
    }

    /// Phases `0..completed` are done; `fraction` (clamped 0..=1) is the
    /// current phase's real sub-progress. Returns the monotonic overall pct.
    pub fn advance(&mut self, completed: usize, fraction: f64) -> u8 {
        let done: u32 = self.weights.iter().take(completed).sum();
        let current = self
            .weights
            .get(completed)
            .map(|w| f64::from(*w) * fraction.clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let raw = ((f64::from(done) + current) / f64::from(self.total) * 100.0) as u8;
        self.pct = self.pct.max(raw.min(99));
        self.pct
    }

    /// The job settled ok — the ONLY way to 100.
    pub fn finish(&mut self) -> u8 {
        self.pct = 100;
        self.pct
    }
}

/// Whether a site is served by the shared nginx. Override servers (FrankenPHP,
/// Apache, OpenLiteSpeed) run their own backend process and are excluded — asked
/// POSITIVELY: until 4 Oct 2026 this was "not FrankenPHP or Apache", which would have
/// put an OpenLiteSpeed site into the shared nginx config and the tunnel allowlist the
/// day the server was offered (ledger #777). Also the single
/// source of truth for tunnel eligibility (`tunnels::ensure_tunnelable`):
/// tunnels originate from the shared nginx, so "in the nginx config" and
/// "safe to tunnel" must be the same predicate — a drift between them is the
/// wrong-vhost exposure all over again.
pub(crate) fn is_nginx_served(s: &Site) -> bool {
    s.web_server == WebServer::Nginx
}

/// Does this site get a SERVING block in the SHARED nginx config?
///
/// Two independent reasons not to, and they mean different things: an override
/// site (FrankenPHP/Apache) is served by its own backend, and a site the user
/// STOPPED (v44) is served by nothing at all. A stopped site still gets a block
/// of its own — a STOPPED one (`services::NginxStopped`, ledger #511): nginx
/// answers a name it has no block for from its default server, i.e. another
/// site, and a tunnel reaches nginx without passing the edge. This predicate
/// decides only the serving half; `rebuild_configs_for` pairs it with the
/// stopped list.
pub(crate) fn gets_nginx_block(s: &Site) -> bool {
    s.enabled && is_nginx_served(s)
}

/// The edge (Caddy) upstream for a site: an override site (FrankenPHP/Apache)
/// points at its own backend port; every other site goes to the shared nginx.
fn site_upstream(s: &Site, nginx_http_port: u16) -> String {
    // Read the RECORDED override port (B20 §4), never re-derive — so the edge
    // route always agrees with the backend's actual port even after a domain
    // change. `None` ⇒ shared nginx.
    match recorded_override_port(s) {
        Some(port) => format!("127.0.0.1:{port}"),
        None => format!("127.0.0.1:{nginx_http_port}"),
    }
}

/// Map a site to its shared-nginx server block, routing `.php` to the FastCGI
/// pool of the site's PHP version (Phase 2 §1.3). An unrecognized version falls
/// back to the default pool so a site is never left pointing at a dead port.
/// `body_limits` (minor → bytes, from `php::nginx_body_limits`) sets the block's
/// `client_max_body_size` so nginx accepts what the version's PHP settings allow.
fn nginx_site_for(
    s: &Site,
    body_limits: &std::collections::HashMap<String, u64>,
    site_env: &std::collections::HashMap<String, Vec<(String, String)>>,
    aliases: &std::collections::HashMap<String, Vec<String>>,
) -> services::NginxSite {
    services::NginxSite {
        domain: s.domain.clone(),
        aliases: aliases.get(&s.id).cloned().unwrap_or_default(),
        // v32: what we SERVE, which is the project root for most sites and
        // `public/` for a Laravel project we created — never `s.path` directly.
        docroot: s.served_root(),
        php_fpm_port: pool_port_for_site(s),
        rewrite: rewrite_mode_for(s.multisite),
        body_limit: body_limits.get(&php::minor_of(&s.php_version)).copied(),
        // Sites run on their pool's own settings — only the Adminer vhost (§5.2)
        // overrides ini per request.
        read_timeout: None,
        php_value: None,
        // Valet's `/storage/*` mapping, for Laravel projects that have the
        // directory. Emitted whether or not `public/storage` exists: when the
        // symlink IS there both paths resolve to the same files, and when it is
        // not (every project developed under Valet, where the driver made the
        // symlink unnecessary) this is the difference between working uploads
        // and a site that 404s its own images after being imported.
        storage_root: laravel_storage_root(s),
        env: site_env.get(&s.id).cloned().unwrap_or_default(),
    }
}

/// The `storage/app/public` directory of a Laravel project, when it exists.
///
/// Laravel only, and existence-checked: emitting an `alias` for a directory
/// that is not there would turn every `/storage/…` request into a 404 from a
/// location block instead of falling through to the app, which for a
/// non-Laravel site would swallow a perfectly ordinary route named `/storage`.
fn laravel_storage_root(s: &Site) -> Option<std::path::PathBuf> {
    if s.site_type != SiteType::Laravel {
        return None;
    }
    let dir = Path::new(&s.path).join("storage/app/public");
    dir.is_dir().then_some(dir)
}

/// The php-fpm pool port a site's PHP `version` routes to. Only versions with a
/// pinned build run a pool; an unrecognized version falls back to the default pool
/// rather than pointing at a dead port. Shared by the nginx config builder and the
/// per-site serving check (`service_manager::site_serving`), so both agree on which
/// upstream a site actually uses.
pub(crate) fn pool_port_for(version: &str) -> u16 {
    let minor = php::minor_of(version);
    match php::patch_for_minor(&minor) {
        Some(_) => php::fpm_port(&minor).unwrap_or(services::PHP_FPM_PORT),
        None => services::PHP_FPM_PORT,
    }
}

/// [`pool_port_for`] with the site's Xdebug toggle applied: a toggled site
/// routes to the minor's DEBUG pool port instead. Falls back to the normal
/// pool when the minor has no Xdebug support (a stale flag on 8.0 can't point
/// at a port that will never listen). The single seam nginx, the override
/// backends, and `site_serving` all route through.
pub(crate) fn pool_port_for_site(s: &Site) -> u16 {
    if s.xdebug {
        if let Some(port) = php::debug_fpm_port(&php::minor_of(&s.php_version)) {
            return port;
        }
    }
    pool_port_for(&s.php_version)
}

/// Map a site's multisite mode to its rewrite template (Phase 1 §6.2). Shared by
/// the nginx config builder and the FrankenPHP override backend (`service_manager`).
pub fn rewrite_mode_for(mode: MultisiteMode) -> services::RewriteMode {
    match mode {
        MultisiteMode::None => services::RewriteMode::Single,
        MultisiteMode::Subdomain => services::RewriteMode::SubdomainMultisite,
        MultisiteMode::Subdirectory => services::RewriteMode::SubdirectoryMultisite,
    }
}

/// Paths of the regenerated shared configs (nginx + Caddy).
#[derive(Debug, Clone)]
pub struct RebuiltConfigs {
    pub nginx_conf: PathBuf,
    pub nginx_prefix: PathBuf,
    pub caddyfile: PathBuf,
}

/// Regenerate the shared nginx + Caddy configs from ALL sites in the DB. The
/// configs are derived state: one shared nginx (server block per site, by
/// `server_name`, FastCGI → php-fpm) and Caddy routes (site host → nginx,
/// TLS with each site's cert). Caller reloads the services afterwards.
pub fn rebuild_configs(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    nginx_http_port: u16,
    caddy_http_port: u16,
    caddy_https_port: u16,
) -> Result<RebuiltConfigs> {
    let sites = list(conn)?;
    let body_limits = php::nginx_body_limits(&store::all_php_settings(conn)?);
    let site_env = store::all_site_env(conn)?;
    let aliases = store::all_site_aliases(conn)?;
    rebuild_configs_for(
        &sites,
        platform,
        ca,
        nginx_http_port,
        caddy_http_port,
        caddy_https_port,
        &body_limits,
        &site_env,
        &aliases,
    )
}

/// Like [`rebuild_configs`] but from an explicit site list + precomputed nginx
/// body limits and per-site env vars (so callers holding an async lock don't
/// keep the DB connection borrowed across `.await`). `body_limits` comes from
/// `php::nginx_body_limits`; `site_env` from `store::all_site_env`, keyed by
/// site id.
// The three ports + two per-site maps are a config-inputs cluster that wants
// a struct — a deliberate deferral until this assembly is next touched for
// real work, not a threshold raise (clippy-zero bar, 28 Jul 2026).
#[allow(clippy::too_many_arguments)]
pub fn rebuild_configs_for(
    sites: &[Site],
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    nginx_http_port: u16,
    caddy_http_port: u16,
    caddy_https_port: u16,
    body_limits: &std::collections::HashMap<String, u64>,
    site_env: &std::collections::HashMap<String, Vec<(String, String)>>,
    aliases: &std::collections::HashMap<String, Vec<String>>,
) -> Result<RebuiltConfigs> {
    // Only nginx-served sites get a server block; override servers (§2/§3) have
    // their own backend process.
    let mut nginx_sites: Vec<services::NginxSite> = sites
        .iter()
        // A site the user STOPPED (v44) gets no SERVING block here — it gets a
        // STOPPED block instead, built below (`stopped_sites`), because a name
        // with no block at all is answered by nginx's default server, which is
        // another site (ledger #511).
        .filter(|s| gets_nginx_block(s))
        .map(|s| nginx_site_for(s, body_limits, site_env, aliases))
        .collect();
    // Internal Adminer vhost (§5.2): served by the default php-fpm pool, rooted at
    // its isolated docroot. Not a Site → never a tunnel origin (§9).
    // Big-dump import is this vhost's job — ONE cap driving nginx and PHP
    // together, so neither 413s nor silently truncates (§5.2). Adminer runs on
    // the DEFAULT pool, so it takes the larger of the generous floor and what
    // that pool's own settings already allow: the floor can only raise a limit,
    // never hold a user who configured more down to it.
    let adminer_cap =
        adminer::import_cap(body_limits.get(&php::minor_of(binaries::pins().php)).copied());
    // The same cap as a `.user.ini` in the docroot: a php-cgi group (Windows) ignores the vhost's
    // PHP_VALUE, and every PHP reads the file (#788).
    adminer::write_user_ini(&adminer::docroot(platform)?, adminer_cap)?;
    nginx_sites.push(services::NginxSite {
        domain: adminer::ADMINER_HOST.to_string(),
        // The tooling vhost answers on ONE name, by design (#24: it has no site
        // row, so it can never be shared or aliased).
        aliases: Vec::new(),
        docroot: adminer::docroot(platform)?,
        php_fpm_port: services::PHP_FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: Some(adminer_cap),
        read_timeout: Some(adminer::IMPORT_TIMEOUT_SECS),
        php_value: Some(adminer::import_php_value(adminer_cap)),
        // The tooling vhost is not a Laravel project and never gets the mapping.
        storage_root: None,
        env: Vec::new(),
    });
    // One page per stopped site, with that site's OWN domain baked in — the
    // page names the site, and a request arriving over a tunnel carries an
    // address that is not it (the trycloudflare host). Written by this rebuild,
    // so a rename lands here for free.
    let stopped_pages = stopped_page::ensure_for(
        platform,
        &sites
            .iter()
            .filter(|s| !s.enabled)
            .map(|s| (s.id.clone(), s.domain.clone()))
            .collect::<Vec<_>>(),
    )?;
    // **Every stopped site gets a block here, whatever serves it when running.**
    // nginx has no match for a name it was given no block for, so it answers
    // from its DEFAULT server — another site — and anything that reaches nginx
    // without passing the edge (a public tunnel does exactly this, with
    // `--http-host-header`) would publish a neighbour's site at this address.
    // Found live by the owner the day after the switch shipped.
    let stopped_sites: Vec<services::NginxStopped> = sites
        .iter()
        .filter(|s| !s.enabled)
        .filter_map(|s| {
            Some(services::NginxStopped {
                domain: s.domain.clone(),
                page_dir: stopped_pages.get(&s.id)?.clone(),
                aliases: aliases.get(&s.id).cloned().unwrap_or_default(),
                wildcard: matches!(s.multisite, MultisiteMode::Subdomain),
            })
        })
        .collect();
    let (nginx_conf, nginx_prefix) =
        services::write_nginx_config(platform, nginx_http_port, nginx_sites, stopped_sites)?;

    // Every site (nginx- or override-served) gets a Caddy edge route: TLS with the
    // local CA, reverse-proxy to that site's upstream (shared nginx, or its own
    // override backend port).
    let mut routes = Vec::with_capacity(sites.len());
    for s in sites {
        let extra = aliases.get(&s.id).cloned().unwrap_or_default();
        // ONE certificate covering every name this site answers on: a browser
        // handed a cert whose SANs omit the host it asked for shows a full-page
        // interstitial, which is the loudest failure this product has — over a
        // name the user deliberately added.
        let cert = ssl::ensure_site_cert_with(
            platform.paths(),
            platform.permissions(),
            ca,
            &s.domain,
            &extra,
        )?;
        routes.push(proxy::SiteRoute {
            host: s.domain.clone(),
            aliases: extra,
            // Subdomain multisite serves every `*.mysite.rex` sub-site from the
            // one wildcard cert + backend (§10.2); other modes are single-host.
            wildcard: matches!(s.multisite, MultisiteMode::Subdomain),
            upstream: site_upstream(s, nginx_http_port),
            cert_path: cert.cert_path,
            key_path: cert.key_path,
            // Still a route, still its own certificate — it just answers 503
            // from THIS site's page. The cert is issued either way so that
            // starting the site again is a config reload and not a certificate
            // the browser has never seen.
            stopped: stopped_pages.get(&s.id).cloned(),
        });
    }
    // Edge route for the internal Adminer vhost (TLS via local CA → shared nginx).
    let adminer_cert =
        ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, adminer::ADMINER_HOST)?;
    routes.push(proxy::SiteRoute {
        host: adminer::ADMINER_HOST.to_string(),
        aliases: Vec::new(),
        wildcard: false,
        upstream: format!("127.0.0.1:{nginx_http_port}"),
        cert_path: adminer_cert.cert_path,
        key_path: adminer_cert.key_path,
        // The tooling vhost is not a Site and has no switch to stop it.
        stopped: None,
    });
    let caddyfile = proxy::write_caddyfile(
        platform,
        &proxy::CaddyConfig {
            http_port: caddy_http_port,
            https_port: caddy_https_port,
            routes,
            // Bind Caddy's admin API to our unix socket (not TCP :2019) so the root
            // edge exposes no unauthenticated local control surface (task 2.3 / H5).
            admin_socket: Some(proxy::admin_socket_path(platform)?),
            // Windows binds loopback only — no firewall prompt (owner ruling, ledger #611).
            default_bind: platform.edge().default_bind().map(str::to_string),
            // Written on EVERY rebuild, not only when missing: the page is
            // generated, and "only if absent" is how a wording fix in an update
            // never reaches a machine that already has yesterday's file.
        },
    )?;

    Ok(RebuiltConfigs {
        nginx_conf,
        nginx_prefix,
        caddyfile,
    })
}

#[cfg(test)]
mod tests {
    /// Ledger #770 — **a linked docroot is stored without Windows' verbatim prefix**: the
    /// extended-length form `canonicalize` answers there would reach the nginx config as
    /// `//?/C:/…`. Literal strings, so the rule is asserted from every host; the Windows run
    /// (what nginx does with the plain form) is the SMOKE row.
    #[test]
    fn a_stored_docroot_has_no_verbatim_prefix() {
        assert_eq!(plain_path(PathBuf::from(r"\\?\C:\Users\dell\Sites\shop")), PathBuf::from(r"C:\Users\dell\Sites\shop"));
        assert_eq!(plain_path(PathBuf::from(r"\\?\UNC\nas\web\shop")), PathBuf::from(r"\\nas\web\shop"), "a UNC path keeps its two leading slashes");
        assert_eq!(plain_path(PathBuf::from("/Users/x/Sites/shop")), PathBuf::from("/Users/x/Sites/shop"));
        assert_eq!(plain_path(PathBuf::from(r"C:\Users\dell\site")), PathBuf::from(r"C:\Users\dell\site"), "a plain Windows path is untouched");
        assert_eq!(plain_path(PathBuf::from(r"\\nas\web")), PathBuf::from(r"\\nas\web"), "a plain UNC path is untouched");
        let prod = include_str!("sites.rs").split("\n#[cfg(test)]").next().unwrap_or_default();
        let f = &prod[prod.find("pub fn validate_linked_docroot_on(").expect("the validation")..];
        let f = &f[..f.find("\n}\n").unwrap_or(f.len())];
        assert!(f.contains("Ok(plain_path(canon))"), "the linked docroot must be STORED through plain_path");
    }

    /// `rex site create s1.rex` names the site `s1`, as the dialog would pair them —
    /// not `s1.rex` (a name that repeats the domain beside it).
    #[test]
    fn the_cli_default_name_is_the_domain_without_its_tld() {
        assert_eq!(name_from_domain("s1.rex"), "s1");
        assert_eq!(name_from_domain("shop.acme.test"), "shop.acme");
        assert_eq!(name_from_domain(" S1.rex. "), "S1");
        assert_eq!(name_from_domain("localhost"), "localhost", "no label to strip");
        assert_eq!(name_from_domain(".rex"), ".rex", "an empty base is not a name");
        // TEXT: the CLI's site.create arm defaults through it, never to the domain itself.
        let cli = crate::core::copy_scan::production_source(include_str!("../cli_server.rs"));
        let arm = cli.split("\"site.create\" => {").nth(1).expect("the site.create arm");
        let arm = &arm[..arm.find("\n        \"").unwrap_or(arm.len())];
        assert!(arm.contains("name_from_domain(&a.domain)"), "rex site create names the site from the domain rule");
        assert!(!arm.contains("unwrap_or_else(|| a.domain.clone())"), "the whole domain is the name again");
    }

    use super::*;
    use crate::state::db;
    use crate::state::models::{SiteDbEngine, SiteType, WebServer};

    /// Ledger #647 — **a site cannot be created with an engine this OS does not ship.** Found by
    /// running the real CLI on the Dell, not by reading: `--db mariadb` created the site and
    /// exited 0, because creation checked the WEB SERVER and the PostgreSQL/PHP pair but never
    /// asked whether the engine itself has a pin here. Both os answers, from either host.
    #[test]
    fn a_site_cannot_be_created_with_an_engine_this_os_does_not_ship() {
        use crate::state::models::SiteDbEngine as E;
        for (engine, os) in
            [(E::Mysql, "macos"), (E::Mysql, "windows"), (E::Postgres, "macos"), (E::Postgres, "windows"), (E::Mariadb, "macos")]
        {
            assert!(ensure_engine_available_on(engine, os).is_ok(), "{engine:?} ships on {os}");
        }
        // §6.3 (ledger #710): on a macOS 13 host PostgreSQL is refused with the
        // macOS it needs — the Databases page, New Site, `rex` and the MCP server
        // all read this one gate, so this sentence is the one they all show.
        {
            use crate::core::binaries::{install_tier, BinaryTier};
            install_tier(BinaryTier::Legacy13);
            let err = ensure_engine_available_on(E::Postgres, "macos").expect_err("no build loads on 13").to_string();
            let os = crate::platform::words::current().os_name;
            assert!(err.starts_with(&format!("PostgreSQL: Needs {os} 14")), "{err}");
            // …and the engine's OWN door says the same words: `rex db versions --set
            // postgres` on the 13.6 VM answered "not available on this platform yet"
            // because that command had a second copy of the gate (23 Sep 2026).
            let own = crate::core::db::DbEngine::Postgres.ensure_available_on("macos").expect_err("refused").to_string();
            assert_eq!(own, err, "two doors, one sentence");
            assert!(!offered_db_engines_on("macos").contains(&E::Postgres));
            assert!(ensure_engine_available_on(E::Mysql, "macos").is_ok());
            install_tier(BinaryTier::Legacy14);
            assert!(ensure_engine_available_on(E::Postgres, "macos").is_ok(), "16.4.0 loads on 14");
            install_tier(BinaryTier::Standard);
        }
        // D4: MariaDB has no Windows pin — MySQL 8.4 and 8.0 both ship there, so it is out of v1.
        let e = ensure_engine_available_on(E::Mariadb, "windows").expect_err("no Windows pin");
        let msg = e.to_string();
        assert!(msg.contains("MariaDB"), "the refusal must name the engine: {msg}");
        assert!(msg.contains("not available on this platform yet"), "{msg}");
    }

    /// Ledger #648 — **the engine picker offers exactly what create would accept**, and the dialog
    /// keeps no list of its own. The twin of the web-server claim (#643); engines were the half left
    /// standing, which is how a MariaDB site could be offered on Windows and refused on create.
    #[test]
    fn the_new_site_dialog_offers_the_engines_core_allows() {
        use crate::state::models::SiteDbEngine as E;
        for os in ["macos", "windows"] {
            for e in offered_db_engines_on(os) {
                assert!(ensure_engine_available_on(e, os).is_ok(), "{e:?} offered on {os} but refused there");
            }
            assert!(offered_db_engines_on(os).contains(&E::Mysql), "MySQL ships on {os}");
        }
        // The half only Windows shows: D4 leaves MariaDB out of v1 there.
        assert!(!offered_db_engines_on("windows").contains(&E::Mariadb), "MariaDB has no Windows pin (D4)");
        assert_eq!(offered_db_engines_on("macos").len(), 3, "all three ship on macOS");

        let dialog = crate::core::copy_scan::strip_ts_comments(include_str!(
            "../../../src/components/sites/NewSiteDialog.tsx"
        ));
        assert!(dialog.contains("Database"), "the stripper ate the source");
        // The GUARD, not the literal: the option tag survives being wrapped in a condition, so
        // asserting its absence would pass on the very shape this is about (the same near-miss the
        // web-server twin had).
        assert!(
            dialog.contains(r#"offeredEngines.includes("mariadb")"#),
            "the engine picker renders MariaDB unconditionally — on Windows that is offered-then-refused"
        );
        assert!(dialog.contains("offeredEngines"), "the dialog does not read the offered engines from the backend");
    }

    /// Ledger #647 — **and creation actually ASKS.** The test above proves the gate's logic; on its
    /// own that is only half the claim, because the gate could exist and no path could call it —
    /// which is exactly what the Dell caught. Read from the module's own production source, so a
    /// call deleted by hand is seen.
    #[test]
    fn creation_asks_whether_the_engine_ships_here() {
        let src = crate::core::copy_scan::production_source(include_str!("sites.rs"));
        // TWO halves, because the gate now takes the os as a parameter (W12): the work is
        // in `_on`, and the host-reading wrapper is what every production caller reaches.
        // Checking only the first would pass with nothing calling it; only the second
        // would pass with the gate deleted from the body.
        let from = src.find("fn refuse_unbuildable_on").expect("the stripper ate refuse_unbuildable_on");
        let to = src[from..].find("\n}").expect("refuse_unbuildable_on ends");
        let body = &src[from..from + to];
        assert!(
            body.contains("ensure_engine_available_on(new.db_engine, os)?;"),
            "creation does not ask whether the engine ships here — a site could be created with an \
             engine that has no pin on this OS, and would fail later at spawn_db:\n{body}"
        );
        let wrapper_at =
            src.find("fn refuse_unbuildable(").expect("the stripper ate refuse_unbuildable");
        let wrapper_end = src[wrapper_at..].find("\n}").expect("refuse_unbuildable ends");
        let wrapper = &src[wrapper_at..wrapper_at + wrapper_end];
        assert!(
            wrapper.contains("refuse_unbuildable_on(conn, new, std::env::consts::OS)"),
            "the host-reading wrapper no longer reaches the gate — creation would ask nothing \
             on the machine it runs on:\n{wrapper}"
        );
    }

    /// Ledger #643 — **the picker offers exactly what create would accept**, and the dialog
    /// keeps no list of its own. Read from the dialog's own source, so a hardcoded array put
    /// back by hand is seen.
    #[test]
    fn the_new_site_dialog_offers_the_servers_core_allows() {
        // Every offered server passes the gate that would refuse it — asked for BOTH os
        // values, so neither half depends on which machine runs the test.
        for os in ["macos", "windows", "linux"] {
            for s in offered_web_servers_on(os) {
                assert!(
                    ensure_server_available_on(s, os).is_ok(),
                    "{s:?} is offered on {os} but create refuses it there"
                );
            }
            assert!(offered_web_servers_on(os).contains(&WebServer::Nginx), "nginx ships on {os}");
        }
        // The half that only Windows shows: D4 leaves both of these out of v1, so the picker
        // must not list them there — offered-then-refused is what this replaced.
        let windows = offered_web_servers_on("windows");
        for s in [WebServer::Apache, WebServer::Frankenphp, WebServer::Openlitespeed] {
            assert!(!windows.contains(&s), "{s:?} has no Windows pin and must not be offered (D4)");
        }
        assert_eq!(offered_web_servers_on("macos").len(), 4, "all four ship on macOS");

        let dialog = crate::core::copy_scan::strip_ts_comments(include_str!(
            "../../../src/components/sites/NewSiteDialog.tsx"
        ));
        assert!(dialog.contains("SERVERS"), "the stripper ate the source");
        assert!(
            !dialog.contains("SERVERS.map("),
            "the picker renders its hardcoded list unfiltered — on Windows that offers Apache, \
             which create then refuses"
        );
        assert!(
            dialog.contains("offeredServers"),
            "the dialog does not read the offered set from the backend"
        );
    }

    /// Ledger #642 — **the web-server gate reads the pins, per OS.** Nginx ships everywhere;
    /// FrankenPHP and Apache have a macOS artifact and no Windows one (D4 — Apache on Windows
    /// would mean Apache Lounge, a third-party trust decision left out of v1).
    ///
    /// Asked for BOTH os values from either host. The bar only `cargo check`s for Windows, so
    /// a gate reading `std::env::consts::OS` alone would have a half that cannot fail on the
    /// machine running the test — which is not a guard, it is a comment.
    #[test]
    fn the_web_server_gate_reads_the_pins_per_os() {
        for server in [WebServer::Nginx, WebServer::Frankenphp, WebServer::Apache] {
            assert!(ensure_server_available_on(server, "macos").is_ok(), "{server:?} on macOS");
        }
        assert!(ensure_server_available_on(WebServer::Nginx, "windows").is_ok());
        for server in [WebServer::Frankenphp, WebServer::Apache] {
            let e = ensure_server_available_on(server, "windows")
                .expect_err("not in Windows v1 (D4)")
                .to_string();
            assert!(e.contains(server.as_db()), "the refusal must name the server: {e}");
            assert!(e.contains("not available on this platform yet"), "{e}");
        }
        // OpenLiteSpeed (ledger #774): rexenv's own build for macOS and Linux, and NO Windows
        // build anywhere — so the Windows refusal is the permanent sentence, never "yet".
        for os in ["macos", "linux"] {
            assert!(ensure_server_available_on(WebServer::Openlitespeed, os).is_ok(), "OLS on {os}");
        }
        let e = ensure_server_available_on(WebServer::Openlitespeed, "windows")
            .expect_err("OpenLiteSpeed has no Windows build")
            .to_string();
        assert_eq!(e, crate::platform::words::WINDOWS.openlitespeed_unavailable);
        assert!(!e.contains("yet"), "{e}");
        assert!(offered_web_servers_on("linux").contains(&WebServer::Openlitespeed));
        assert!(!offered_web_servers_on("windows").contains(&WebServer::Openlitespeed));
    }

    /// **A create refused for its shape leaves nothing behind — no folder, no
    /// starter page.** The refusal used to live only at the insert chokepoint,
    /// which `provision_with` reaches AFTER making the docroot, writing the
    /// Blank-PHP page and issuing the certificate; the site matrix of 11 Sep 2026
    /// found eleven such folders in its Sites directory. The CA is a literal on
    /// purpose: a green run returns before the certificate step, and a run with
    /// the early refusal removed fails there on the empty CA instead of signing.
    /// **Planting costs one directory**, measured: `ensure_site_cert` makes
    /// `certs/refused-wordpress.rex/` in the REAL app data before it reads the
    /// CA — remove it after a plant.
    #[test]
    fn a_site_refused_for_its_shape_leaves_no_folder_behind() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let root = std::env::temp_dir().join(format!("rexenv-refused-shape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        store::set_setting(&conn, SITES_DIR_KEY, &root.display().to_string()).unwrap();
        let ca = ssl::LocalCa {
            cert_pem: String::new(),
            key_pem: String::new(),
            cert_path: std::path::PathBuf::from("/dev/null"),
            key_path: std::path::PathBuf::from("/dev/null"),
        };
        for (site_type, php) in [(SiteType::Wordpress, "8.4"), (SiteType::Php, "7.4"), (SiteType::Laravel, "7.4")] {
            let domain = format!("refused-{}.rex", site_type.as_db());
            let new = NewSite {
                name: domain.clone(),
                domain: domain.clone(),
                site_type,
                php_version: php.into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Postgres,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: site_type == SiteType::Php,
            };
            let err = provision_with(&conn, &*platform, &ca, new, Ownership::User).unwrap_err().to_string();
            assert!(err.contains("PostgreSQL"), "refused for the shape, not something later: {err}");
            assert!(!root.join(&domain).exists(), "{site_type:?} on {php}: the refused create left {domain}/ behind");
        }
        assert!(list(&conn).unwrap().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn postgres_is_refused_at_the_insert_chokepoint_for_two_different_reasons() {
        // Not in the dialog: `rex site create --db postgres` and the MCP
        // create_site tool both parse the engine out of a string and land here.
        let err = ensure_engine_supports(SiteType::Wordpress, SiteDbEngine::Postgres, "8.4.23")
            .expect_err("WordPress on PostgreSQL must be refused");
        let msg = err.to_string();
        assert!(msg.contains("PostgreSQL"), "{msg}");
        assert!(msg.contains("MariaDB"), "the refusal must name what to use instead: {msg}");
        assert!(
            !msg.contains("pdo_pgsql") && !msg.contains("stalls"),
            "WordPress is refused for its OWN reason, not the runtime's: {msg}"
        );
        // …and it holds on a PHP that CAN reach PostgreSQL — the WordPress
        // refusal must not be quietly resting on the runtime one.
        assert!(crate::core::php::pdo_pgsql_supported("8.4.23"));

        // The second refusal has a different subject: the PHP MINOR, not the
        // site type. Both types are allowed on a runtime with the driver and
        // refused on one without, and the two messages must not be confusable —
        // only one of them names a way forward, because only one has one.
        for t in [SiteType::Laravel, SiteType::Php] {
            for good in ["8.1.34", "8.5.8"] {
                assert!(
                    ensure_engine_supports(t, SiteDbEngine::Postgres, good).is_ok(),
                    "{t:?} on PostgreSQL must be allowed on PHP {good}"
                );
            }
            // A patch NEWER than our pin for a minor that otherwise has the
            // driver — the shape that hung a real provision (#550), and the one
            // an in-app update recreates the day upstream ships 8.3.33.
            for bad in ["7.4.33", "8.0.30", "8.3.33"] {
                let m = ensure_engine_supports(t, SiteDbEngine::Postgres, bad)
                    .expect_err("PostgreSQL needs PDO, which this build lacks")
                    .to_string();
                assert!(m.contains("PDO"), "the refusal must name the reason: {m}");
                assert!(
                    m.contains(&crate::core::php::oldest_pdo_pgsql_minor()),
                    "and the version that works: {m}"
                );
                assert!(m.contains(bad), "and the build the site is on: {m}");
            }
        }

        // Nothing else changed: every MySQL-protocol pairing stays allowed, on
        // every runtime — including the two with no PostgreSQL driver, since
        // that is exactly what they are still good for.
        for (t, e) in [
            (SiteType::Wordpress, SiteDbEngine::Mysql),
            (SiteType::Wordpress, SiteDbEngine::Mariadb),
            (SiteType::Laravel, SiteDbEngine::Mysql),
            (SiteType::Php, SiteDbEngine::Mariadb),
        ] {
            for v in ["7.4.33", "8.0.30", "8.3.33", "8.4.23"] {
                assert!(ensure_engine_supports(t, e, v).is_ok(), "{t:?} + {e:?} on {v}");
            }
        }
    }

    const WP_PHASES: &[&str] =
        &["prepare", "fetch", "db", "core_download", "configure", "core_install", "serve"];

    #[test]
    fn provision_progress_weights_land_where_wall_time_lives() {
        // WP create without blueprint: total 95. Completing each phase in
        // order gives the coarse-weighted staircase — the two network phases
        // own the bulk of the bar.
        let mut p = ProvisionProgress::new(WP_PHASES);
        assert_eq!(p.advance(1, 0.0), 3); // prepare done → 3/95
        assert_eq!(p.advance(2, 0.0), 31); // + fetch 27
        assert_eq!(p.advance(3, 0.0), 36); // + db 5
        assert_eq!(p.advance(4, 0.0), 73); // + core_download 35
        assert_eq!(p.advance(5, 0.0), 78); // + configure 5
        assert_eq!(p.advance(6, 0.0), 89); // + core_install 10
        assert_eq!(p.advance(7, 0.0), 99); // all done — STILL capped: not settled
        assert_eq!(p.finish(), 100);
    }

    #[test]
    fn provision_progress_renormalizes_over_applicable_phases() {
        // Non-WP: prepare/fetch/serve only (3+27+10 = 40). No reserved slice
        // that can never fill — the applicable set IS the denominator.
        let mut p = ProvisionProgress::new(&["prepare", "fetch", "serve"]);
        assert_eq!(p.advance(1, 0.0), 7); // 3/40
        assert_eq!(p.advance(2, 0.0), 75); // 30/40
        assert_eq!(p.advance(3, 0.0), 99); // capped until finish
        assert_eq!(p.finish(), 100);
    }

    #[test]
    fn provision_progress_folds_real_bytes_into_the_fetch_slice() {
        // During fetch (phase index 1), fraction = the Hub's Σbytes/Σtotal —
        // genuine byte progress, scaled into fetch's 27-weight slice.
        let mut p = ProvisionProgress::new(WP_PHASES);
        p.advance(1, 0.0);
        assert_eq!(p.advance(1, 0.5), 17); // (3 + 13.5)/95
        assert_eq!(p.advance(1, 1.0), 31); // fetch bytes complete
        // Phase completion agrees with bytes-done — no jump, no regression.
        assert_eq!(p.advance(2, 0.0), 31);
    }

    #[test]
    fn provision_progress_is_monotonic_even_if_a_fraction_regresses() {
        // A Hub retry can reset an item's bytes to 0 (give-up path deletes
        // the partial — honest). The OVERALL bar must never move backwards.
        let mut p = ProvisionProgress::new(WP_PHASES);
        p.advance(1, 0.9);
        let high = p.pct();
        assert_eq!(p.advance(1, 0.1), high);
        // Out-of-range fractions clamp, never panic or overshoot.
        assert_eq!(p.advance(1, 7.0), p.advance(1, 1.0));
        assert!(p.advance(1, -3.0) >= high);
    }

    #[test]
    fn provision_progress_freezes_where_it_stopped() {
        // Failure/cancel = the caller simply stops advancing: the value
        // holds; only finish() (settle ok) can produce 100.
        let mut p = ProvisionProgress::new(WP_PHASES);
        p.advance(3, 0.0); // died in core_download
        let frozen = p.pct();
        assert_eq!(frozen, 36);
        assert_eq!(p.pct(), frozen);
        assert!(frozen < 100);
    }

    fn sample(name: &str, domain: &str) -> NewSite {
        NewSite {
            name: name.into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: format!("~/Sites/{name}"),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            starter_db: false,
        }
    }

    /// A site being CLONED: no `path` (rexenv makes the folder), a repo URL in
    /// the shape a developer actually pastes.
    fn cloning(url: &str, git_ref: Option<&str>) -> NewSite {
        NewSite {
            path: String::new(),
            site_type: SiteType::Laravel,
            git_url: url.into(),
            git_ref: git_ref.map(str::to_string),
            ..sample("shop", "shop.rex")
        }
    }

    #[test]
    fn a_clone_normalizes_the_paste_and_keeps_the_ref_the_user_picked() {
        // The value that reaches git argv is the PARSED one, never the paste —
        // `owner/repo` is the GitHub shorthand and expands to a clone URL.
        let src = validate_git_source(&cloning("acme/shop", None), &Ownership::User)
            .unwrap()
            .expect("a repo url means a clone");
        assert_eq!(src.url, "https://github.com/acme/shop.git");
        assert_eq!(src.git_ref, None, "no ref = whatever the remote calls default");

        let src = validate_git_source(
            &cloning("https://github.com/acme/shop/tree/develop", Some("develop")),
            &Ownership::User,
        )
        .unwrap()
        .unwrap();
        // A pasted web URL keeps its own spelling minus the `/tree/…` route —
        // git clones it either way, and rewriting a URL the user can read is
        // how a self-hosted forge gets a URL it never served.
        assert_eq!(src.url, "https://github.com/acme/shop");
        assert_eq!(src.git_ref.as_deref(), Some("develop"));

        // An empty ref field is "they picked nothing", not a ref named "".
        let src = validate_git_source(&cloning("acme/shop", Some("  ")), &Ownership::User)
            .unwrap()
            .unwrap();
        assert_eq!(src.git_ref, None);

        // No URL at all: this site simply isn't a clone.
        assert_eq!(validate_git_source(&sample("blog", "blog.rex"), &Ownership::User).unwrap(), None);
    }

    #[test]
    fn staging_sits_beside_the_docroot_so_the_move_can_never_cross_a_filesystem() {
        // The Sites folder is user-configurable and may be on another volume.
        // Staging in app-data or /tmp would turn the final move into an EXDEV
        // failure AFTER a multi-minute download — the worst possible moment.
        let docroot = Path::new("/Volumes/Work/Sites/shop.rex");
        let staging = staging_dir(docroot, "a1b2c3d4").unwrap();
        assert_eq!(staging.parent(), docroot.parent(), "same parent ⇒ same filesystem");
        assert_ne!(staging, docroot);
        let name = staging.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(".rexenv-clone-"), "hidden and obviously ours: {name}");
        assert!(name.contains("shop.rex") && name.contains("a1b2c3d4"));
        // Two clones of the same site can't collide on the staging path.
        assert_ne!(staging_dir(docroot, "aaaaaaaa").unwrap(), staging_dir(docroot, "bbbbbbbb").unwrap());
    }

    #[test]
    fn a_docroot_with_anything_in_it_is_refused_before_a_byte_is_fetched() {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-clone-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ensure_empty_docroot(&dir).is_ok(), "prepare leaves the docroot empty");

        // A dotfile counts: `read_dir` sees it, and so would a clone's collision.
        std::fs::write(dir.join(".DS_Store"), "").unwrap();
        let err = ensure_empty_docroot(&dir).unwrap_err().to_string();
        assert!(err.contains("not empty"), "{err}");
        assert!(std::fs::read_dir(&dir).unwrap().count() == 1, "a refusal deletes nothing");

        // A path that isn't a directory at all fails readably rather than
        // panicking somewhere downstream.
        assert!(ensure_empty_docroot(&dir.join("nope")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_clone_that_is_not_the_chosen_kind_of_site_is_named_not_re_typed() {
        // Re-typing here would change the phase list and the binary plan AFTER
        // the card described them — the card would be narrating a job it isn't
        // running. Naming what was found is the honest half.
        let wp = DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: "web".into(),
            label: "WordPress (Bedrock)",
            existing_install: true,
        };
        let err = verify_cloned_shape(&wp, SiteType::Laravel).unwrap_err().to_string();
        assert!(err.contains("WordPress (Bedrock)"), "says what it found: {err}");
        assert!(err.contains("wordpress"), "and what to create instead: {err}");
        assert!(err.contains("nothing else has been set up"), "{err}");

        let laravel = DetectedProject {
            site_type: SiteType::Laravel,
            docroot_rel: LARAVEL_DOCROOT_SUBDIR.into(),
            label: "Laravel",
            existing_install: true,
        };
        assert!(verify_cloned_shape(&laravel, SiteType::Laravel).is_ok());
        // Blank PHP accepts whatever landed: a folder of files IS a PHP site,
        // and detection already falls back to serving the root.
        assert!(verify_cloned_shape(&wp, SiteType::Php).is_ok());
        let unknown = DetectedProject {
            site_type: SiteType::Php,
            docroot_rel: String::new(),
            label: "Unknown",
            existing_install: false,
        };
        assert!(verify_cloned_shape(&unknown, SiteType::Php).is_ok());
        assert!(
            verify_cloned_shape(&unknown, SiteType::Laravel).is_err(),
            "an empty or unrecognisable repo is not a Laravel app"
        );
    }

    #[test]
    fn cloning_into_a_folder_the_user_linked_is_refused_rather_than_ranked() {
        // Two promises that cannot both be kept: a linked folder is never
        // written into, a cloned docroot is filled. Picking one silently would
        // break the other on a site the user thought they were linking.
        let both = NewSite { path: "/Users/x/code/shop".into(), ..cloning("acme/shop", None) };
        let err = validate_git_source(&both, &Ownership::User).unwrap_err().to_string();
        assert!(err.contains("CLONED") && err.contains("LINKED"), "{err}");
    }

    #[test]
    fn an_agent_can_never_create_a_site_from_a_repository() {
        // A clone downloads code a MODEL chose and `composer install` then runs
        // that project's own scripts — with no click in between. The scratch
        // tier grants disposable sites, not arbitrary code execution.
        let agent = Ownership::Agent { client: "Claude Code".into(), ttl_hours: 24 };
        let err = validate_git_source(&cloning("acme/shop", None), &agent).unwrap_err().to_string();
        assert!(err.contains("user action"), "{err}");
        // The refusal is about the CLONE, not about agents creating sites: an
        // ordinary scratch site is still fine.
        assert!(validate_git_source(&sample("scratch", "s.scratch.rex"), &agent).is_ok());
    }

    #[test]
    fn a_repo_reference_that_cannot_be_parsed_never_reaches_git() {
        for bad in [
            "git://github.com/acme/shop.git",       // unencrypted, dropped by forges
            "https://github.com/acme/shop/x.zip",   // an archive, not a repository
            "not a url at all",                     // whitespace
            "ftp://example.com/shop",               // unsupported scheme
        ] {
            assert!(
                validate_git_source(&cloning(bad, None), &Ownership::User).is_err(),
                "{bad} must be refused before a site row exists"
            );
        }
        // A ref is argv too — same gate.
        assert!(validate_git_source(&cloning("acme/shop", Some("--upload-pack=x")), &Ownership::User)
            .is_err());
    }

    /// Every site type can be cloned. WordPress was refused through Stages 1–3
    /// (a checkout without its database is not a site) and is admitted in Stage
    /// 4 with that fact STATED rather than designed around: the repository
    /// supplies the code, provisioning supplies a fresh empty database, and the
    /// dialog says so before Create.
    #[test]
    fn every_site_type_can_be_cloned() {
        for ty in [SiteType::Laravel, SiteType::Php, SiteType::Wordpress] {
            let new = NewSite { site_type: ty, ..cloning("acme/site", None) };
            assert!(validate_git_source(&new, &Ownership::User).is_ok(), "{ty:?}");
        }
    }

    /// The layouts whose CORE comes from Composer — the one question the
    /// provisioning driver asks about a cloned WordPress checkout, because it
    /// turns off both `core_download` and `wp config create`.
    #[test]
    fn only_the_roots_layouts_get_their_wordpress_core_from_composer() {
        let of = |label, rel: &str| DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: rel.into(),
            label,
            existing_install: true,
        };
        assert!(wordpress_core_from_composer(&of(LABEL_BEDROCK, "web")));
        assert!(wordpress_core_from_composer(&of(LABEL_RADICLE, "public")));
        assert!(!wordpress_core_from_composer(&of("WordPress", "")));
        assert!(!wordpress_core_from_composer(&DetectedProject {
            site_type: SiteType::Laravel,
            docroot_rel: "public".into(),
            label: "Laravel",
            existing_install: true,
        }));

        // The labels are the SAME strings detection produces — a constant that
        // drifted from the detector would silently turn the two skips off.
        let dir = std::env::temp_dir().join(format!("rexenv-bedrock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("web/wp-config.php"), "<?php").unwrap();
        std::fs::write(dir.join("config/application.php"), "<?php").unwrap();
        let detected = detect_project(&dir);
        assert_eq!(detected.label, LABEL_BEDROCK);
        assert!(wordpress_core_from_composer(&detected));
        assert_eq!(detected.docroot_rel, "web");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn validate_docroot_path_allows_spaces_but_rejects_config_breaking_chars() {
        // Spaces (quoted in the config) and ordinary paths are fine — these are
        // the real cases, so existing sites keep working.
        for ok in ["/Sites/acme/public", "/Users/me/My Sites/blog", "~/Sites/a-b_1.test"] {
            assert!(validate_docroot_path(ok).is_ok(), "{ok} must be allowed");
        }
        // The unescapable-across-nginx/Caddy/Apache set + control chars → rejected.
        for bad in ["/a\"b", "/a$b", "/a{b", "/a}b", "/a\nb", "/a\tb"] {
            assert!(validate_docroot_path(bad).is_err(), "{bad:?} must be rejected");
        }
        // The backslash is judged per folder NAME, deliberately (`config_breaking_char`,
        // and `set_sites_dir` says so in as many words): on Windows it is the separator,
        // so `/a\b` is the two folders `a` and `b` and nothing in either name is
        // config-breaking. On unix it is an ordinary character inside one name, and
        // refused. Asserting the unix answer everywhere made this fail on the Dell for a
        // behaviour that is correct there (W12).
        #[cfg(unix)]
        assert!(validate_docroot_path("/a\\b").is_err(), "a backslash inside a NAME is rejected");
        #[cfg(windows)]
        assert!(
            validate_docroot_path("/a\\b").is_ok(),
            "on Windows `\\` separates folders — neither name is config-breaking"
        );
        // Either way a backslash INSIDE a folder name is refused, which is the claim
        // that has to hold on both: Windows cannot put one in a name at all, so the
        // quoted-path hazard this guards is unreachable there.
        assert!(validate_docroot_path("/a/b\"c").is_err(), "a quote inside a name is rejected");
    }

    #[test]
    fn create_and_set_path_reject_a_config_breaking_docroot() {
        // The two choke points that PERSIST site.path (== the emitted docroot).
        let conn = db::open_in_memory().unwrap();
        let mut bad = sample("Bad", "bad.test");
        bad.path = "/Sites/ev$il".into();
        assert!(create(&conn, bad).is_err(), "create must reject a config-breaking path");

        // A clean site persists; moving it to a bad path is refused (set_path).
        let ok = create(&conn, sample("Ok", "ok.test")).unwrap();
        assert!(
            set_path(&conn, &*crate::platform::current(), &ok.id, Path::new("/Sites/ok\"x"))
                .is_err(),
            "set_path must reject"
        );
        // …and the stored path is untouched (rejected before the UPDATE).
        assert_eq!(get(&conn, &ok.id).unwrap().unwrap().path, ok.path);
    }

    #[test]
    fn set_xdebug_validates_in_core_and_flips_the_flag() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("A", "a.test")).unwrap(); // nginx, PHP 8.3
        assert!(!site.xdebug);

        // Happy path: on, then off. `_on("macos")` throughout, for the same reason the
        // FrankenPHP leg below already names it: Xdebug has no Windows pins, so on the
        // Dell every enable is refused before the flag this test is about is reached (W12).
        let on = set_xdebug_on(&conn, &site.id, true, "macos").unwrap().unwrap();
        assert!(on.xdebug);
        let off = set_xdebug_on(&conn, &site.id, false, "macos").unwrap().unwrap();
        assert!(!off.xdebug);

        // FrankenPHP refused (embedded PHP — the pools never serve it).
        // `_on("macos")`: FrankenPHP has no Windows pin, so the switch is refused there
        // before the Xdebug rule under test is reached (W12).
        set_web_server_on(&conn, &site.id, WebServer::Frankenphp, "macos").unwrap();
        assert!(set_xdebug_on(&conn, &site.id, true, "macos").is_err());
        set_web_server_on(&conn, &site.id, WebServer::Nginx, "macos").unwrap();

        // PHP 8.0 refused (static build can't dlopen any .so).
        set_php_version(&conn, &site.id, "8.0").unwrap();
        assert!(set_xdebug_on(&conn, &site.id, true, "macos").is_err());
        set_php_version(&conn, &site.id, "8.4").unwrap();
        assert!(set_xdebug_on(&conn, &site.id, true, "macos").unwrap().unwrap().xdebug);

        // Disabling never validates (a stale flag must always be clearable).
        set_php_version(&conn, &site.id, "8.0").unwrap();
        assert!(!set_xdebug_on(&conn, &site.id, false, "macos").unwrap().unwrap().xdebug);

        // Unknown id → None, not an error.
        assert!(set_xdebug_on(&conn, "nope", true, "macos").unwrap().is_none());
        assert!(set_xdebug_on(&conn, "nope", false, "macos").unwrap().is_none());

        // And the os is a real parameter, not decoration: the same enable that succeeds
        // on macOS is refused on Windows, with the sentence that says why (D4).
        //
        // On a minor whose reason is the OS and nothing else — the site is left on 8.0 by
        // the leg above, and 8.0's refusal is `CannotLoadExtensions` on every host, so
        // asking there would have proven nothing about the os parameter. That is what my
        // first version of this leg did, and the Mac caught it.
        set_php_version(&conn, &site.id, "8.4").unwrap();
        assert!(set_xdebug_on(&conn, &site.id, true, "macos").unwrap().unwrap().xdebug);
        let win = set_xdebug_on(&conn, &site.id, true, "windows").unwrap_err().to_string();
        assert!(win.contains("Windows"), "the refusal names the os: {win}");
    }

    #[test]
    fn pool_port_for_site_routes_xdebug_sites_to_the_debug_pool() {
        let conn = db::open_in_memory().unwrap();
        let mut site = create(&conn, sample("A", "a.test")).unwrap();
        site.php_version = "8.4".into();
        // Toggle off → the shared pool, exactly pool_port_for.
        assert_eq!(pool_port_for_site(&site), pool_port_for("8.4"));
        // Toggle on → the minor's debug port WHERE ONE EXISTS. `debug_fpm_port` is gated
        // on `binaries::xdebug_supported`, which has no Windows pins, so there the answer
        // is the fallback below — and that fallback IS the rule this test is about: a
        // toggled site never routes to a port nothing will ever listen on (W12).
        site.xdebug = true;
        match php::debug_fpm_port("8.4") {
            Some(debug) => assert_eq!(pool_port_for_site(&site), debug),
            None => assert_eq!(
                pool_port_for_site(&site),
                pool_port_for("8.4"),
                "no Xdebug pin on this OS: a toggled site falls back to the normal pool"
            ),
        }
        // A stale flag on an unsupported minor falls back to the normal pool —
        // never a port nothing will ever listen on.
        site.php_version = "8.0".into();
        assert_eq!(pool_port_for_site(&site), pool_port_for("8.0"));
    }

    /// The collision the v42 table opened up, from the OTHER two directions:
    /// creating a site on a name that is already an extra domain, and RENAMING
    /// one onto it.
    ///
    /// `validate_alias` checked both tables from the start; `create` and
    /// `check_domain_change` did not — they asked `domain_exists`, which reads
    /// `sites.domain` alone. So the day extra domains shipped, `shop.test`
    /// could be an alias of A and the primary of a brand-new B at the same
    /// time: two nginx server blocks for one hostname, served by whichever
    /// matched first, with both sites looking correct on screen. Found the next
    /// morning by asking what else answers this question.
    #[test]
    fn a_name_that_is_already_an_extra_domain_cannot_be_created_or_renamed_onto() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        add_alias(&conn, &a.id, "shop.test").unwrap();

        // CREATE on the alias.
        let err = create(&conn, sample("B", "shop.test")).unwrap_err().to_string();
        assert!(
            err.contains("\"A\""),
            "creating a site on another site's extra domain must refuse, naming the owner: {err}"
        );

        // RENAME onto the alias.
        let b = create(&conn, sample("B", "b.test")).unwrap();
        let err = set_domain(&conn, &b.id, "shop.test").unwrap_err().to_string();
        assert!(
            err.contains("\"A\""),
            "renaming a site onto another site's extra domain must refuse, naming the owner: {err}"
        );

        // …and the ordinary cases still work: a free name renames, and the
        // freed one becomes available.
        assert!(set_domain(&conn, &b.id, "b2.test").unwrap().is_some());
        assert!(remove_alias(&conn, &a.id, "shop.test").unwrap());
        assert!(set_domain(&conn, &b.id, "shop.test").unwrap().is_some());
    }

    /// An extra domain must be free across the WHOLE hostname space — both
    /// tables — and the schema can only see one of them.
    ///
    /// `site_domains.domain` is UNIQUE, so SQL stops two sites sharing an
    /// alias. What SQL cannot see is an alias equal to some site's PRIMARY
    /// domain, and that is the collision that actually hurts: two server blocks
    /// answer one hostname, nginx serves whichever it matched first, and both
    /// sites look correct in the UI while the user's own model of which name
    /// belongs to which project is the thing that broke.
    #[test]
    fn an_extra_domain_must_be_free_in_both_tables() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        let b = create(&conn, sample("B", "b.test")).unwrap();

        assert_eq!(add_alias(&conn, &a.id, "shop.test").unwrap(), "shop.test");
        assert_eq!(all_domains(&conn, &a).unwrap(), vec!["a.test", "shop.test"]);
        // The primary is FIRST and is not in the alias table — every consumer
        // reads one list, so this order is the one they all get.
        assert_eq!(store::get_site_aliases(&conn, &a.id).unwrap(), vec!["shop.test"]);

        // The collision SQL cannot see: another site's own domain.
        let err = add_alias(&conn, &a.id, "b.test").unwrap_err().to_string();
        assert!(err.contains("\"B\""), "the refusal must NAME the site it collides with: {err}");
        // …and this site's own domain, which is a no-op dressed as a request.
        assert!(add_alias(&conn, &a.id, "a.test").unwrap_err().to_string().contains("own domain"));
        // The collision SQL can see, refused with the owner's name rather than
        // a constraint error.
        let err = add_alias(&conn, &b.id, "shop.test").unwrap_err().to_string();
        assert!(err.contains("\"A\""), "an alias of another site must name that site: {err}");
        // Re-adding the same site's own alias says so instead of failing raw.
        assert!(add_alias(&conn, &a.id, "shop.test").unwrap_err().to_string().contains("already"));

        // Normalised on the way in: trailing dot, case and padding are the same
        // hostname, and storing two spellings would defeat every uniqueness
        // check above.
        assert_eq!(add_alias(&conn, &a.id, "  SHOP2.TEST. ").unwrap(), "shop2.test");
        assert!(add_alias(&conn, &b.id, "Shop2.Test").unwrap_err().to_string().contains("\"A\""));

        // Invalid hostnames are refused by the SAME validator sites use — an
        // alias is a hostname nginx and a certificate have to accept.
        for bad in ["", "no-tld", "bad_underscore.test", "-lead.test", "a..test"] {
            assert!(add_alias(&conn, &a.id, bad).is_err(), "`{bad}` was accepted as an alias");
        }

        // Removal normalises the SAME way as the add: the spelling `dig`
        // prints (trailing dot) removes the name the add stored without it.
        assert!(remove_alias(&conn, &a.id, " SHOP2.TEST. ").unwrap());
        assert_eq!(all_domains(&conn, &a).unwrap(), vec!["a.test", "shop.test"]);
        // Removal is idempotent — a retry after a partial failure is safe.
        assert!(remove_alias(&conn, &a.id, "shop.test").unwrap());
        assert!(!remove_alias(&conn, &a.id, "shop.test").unwrap());
        assert_eq!(all_domains(&conn, &a).unwrap(), vec!["a.test"]);
        // Freed for the other site now that it is gone.
        assert_eq!(add_alias(&conn, &b.id, "shop.test").unwrap(), "shop.test");
    }

    /// Deleting a site takes its extra domains with it — via the FK cascade,
    /// not a second delete somebody has to remember. An orphaned alias is a
    /// hostname reserved against every future site, with no site to explain it.
    #[test]
    fn extra_domains_die_with_the_site() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        add_alias(&conn, &a.id, "shop.test").unwrap();
        assert!(store::delete_site(&conn, &a.id).unwrap());
        assert_eq!(store::site_id_for_alias(&conn, "shop.test").unwrap(), None);
        // …and the name is immediately available to another site.
        let b = create(&conn, sample("B", "b.test")).unwrap();
        assert_eq!(add_alias(&conn, &b.id, "shop.test").unwrap(), "shop.test");
    }

    #[test]
    fn validate_domain_accepts_allowed_tld_hostnames() {
        for d in [
            // the safe set…
            "acme.test", "my-site.test", "a.test", "sub.mysite.test", "wp123.test",
            "acme.localhost", "acme.example", "acme.invalid",
            // …and warn-tier custom TLDs (allowed; UI shows the shadow notice).
            // "foo.test.evil" is a hostname under .evil — warn-tier, no longer
            // special-cased just because "test" appears mid-name.
            "acme.rex", "shop.internal", "foo.test.evil",
        ] {
            assert!(validate_domain(d).is_ok(), "should accept {d}");
        }
    }

    #[test]
    fn validate_domain_rejects_unsafe_or_blocked() {
        for d in [
            "",              // empty
            "acme",          // no TLD
            ".test",         // no label before the TLD
            "../etc.test",   // path traversal
            "a/b.test",      // path separator
            "a b.test",      // space
            "a;b.test",      // config-injection char
            "a{b.test",      // config-injection char
            "Acme.test",     // uppercase
            "*.mysite.test", // wildcard
            "-bad.test",     // leading hyphen
            "bad-.test",     // trailing hyphen
            "a..test",       // empty inner label
            "acme.t3st",     // TLD with a digit
        ] {
            assert!(validate_domain(d).is_err(), "should reject {d:?}");
        }
    }

    /// The trust boundary: a hard-blocked TLD is refused by the CORE validate —
    /// i.e. even a direct `create`/`set_domain` call (bypassing the UI) fails.
    #[test]
    fn validate_domain_refuses_blocked_tlds_in_core() {
        for d in [
            "acme.local", // Bonjour/mDNS
            "acme.dev",   // real gTLD
            "acme.app", "acme.page", "acme.home", "acme.corp", "acme.mail",
            "acme.com", "acme.net", "acme.org", "acme.cloud", "acme.site", "acme.online",
            "acme.io", "acme.co", "acme.uk", // 2-letter rule
        ] {
            assert!(validate_domain(d).is_err(), "should refuse blocked TLD {d:?}");
        }

        // …and through the real entry points, not just the helper:
        let conn = db::open_in_memory().unwrap();
        assert!(create(&conn, sample("Blocked", "acme.local")).is_err());
        assert!(list(&conn).unwrap().is_empty(), "nothing persisted");
        let a = create(&conn, sample("A", "a.test")).unwrap();
        assert!(set_domain(&conn, &a.id, "a.dev").is_err());
        assert_eq!(get(&conn, &a.id).unwrap().unwrap().domain, "a.test");
    }

    #[test]
    fn default_tld_setting_round_trips_and_is_policy_gated() {
        let conn = db::open_in_memory().unwrap();
        // Fresh DB (v8 seed + v9 flip) → the .rex backbone.
        assert_eq!(default_tld(&conn).unwrap(), "rex");

        // .test is an ordinary user choice now ('.test' form is normalized).
        assert_eq!(set_default_tld(&conn, ".test").unwrap(), "test");
        assert_eq!(default_tld(&conn).unwrap(), "test");

        // Blocked TLDs are refused AT THE BACKEND — direct calls included —
        // and the stored value is untouched.
        for t in ["local", "dev", "io", "com"] {
            assert!(set_default_tld(&conn, t).is_err(), "must refuse {t}");
        }
        assert_eq!(default_tld(&conn).unwrap(), "test");

        // A blocked value smuggled into the settings table (bypassing the
        // setter) falls back to the backbone instead of surfacing.
        store::set_setting(&conn, DEFAULT_TLD_KEY, "com").unwrap();
        assert_eq!(default_tld(&conn).unwrap(), "rex");
    }

    #[test]
    fn domain_tld_returns_validated_tld_only() {
        assert_eq!(domain_tld("acme.test").unwrap(), "test");
        assert_eq!(domain_tld("sub.mysite.rex").unwrap(), "rex");
        // Invalid or blocked domains never yield a TLD to act on.
        assert!(domain_tld("acme.local").is_err());
        assert!(domain_tld("../evil.test").is_err());
        assert!(domain_tld("acme").is_err());
    }

    #[test]
    fn create_rejects_an_invalid_domain() {
        let conn = db::open_in_memory().unwrap();
        assert!(create(&conn, sample("Bad", "../evil.test")).is_err());
        // A rejected domain persists nothing.
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn create_then_list_round_trips() {
        let conn = db::open_in_memory().unwrap();
        let created = create(&conn, sample("Acme", "acme.test")).unwrap();

        assert!(!created.id.is_empty());
        assert!(matches!(created.status, ServiceStatus::Stopped));
        assert!(created.ssl);
        assert!(!created.created_at.is_empty());

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!(s.id, created.id);
        assert_eq!(s.name, "Acme");
        assert_eq!(s.domain, "acme.test");
        assert!(matches!(s.site_type, SiteType::Wordpress));
        assert!(matches!(s.web_server, WebServer::Nginx));
        assert_eq!(s.php_version, "8.3");
        assert_eq!(s.db_name, "wp_acme_test");
    }

    #[test]
    fn create_stores_the_derived_db_name() {
        let conn = db::open_in_memory().unwrap();
        let created = create(&conn, sample("Shop", "my-shop.test")).unwrap();
        // Derived once at creation and persisted — reads must return the
        // stored value, not a fresh derivation from the current domain.
        assert_eq!(created.db_name, "wp_my_shop_test");
        assert_eq!(get(&conn, &created.id).unwrap().unwrap().db_name, "wp_my_shop_test");
    }

    #[test]
    fn create_stores_the_prefix_of_the_sites_own_type_not_wordpresss() {
        // The bug: create derived the name from the domain ALONE, so a Laravel
        // app was stored as `wp_myapp_test` — the name a developer then reads in
        // Adminer, on a database WordPress never touches. Two same-slug sites of
        // different types are also DISTINCT databases now, with no hash suffix.
        let conn = db::open_in_memory().unwrap();
        let mut lara = sample("App", "myapp.test");
        lara.site_type = SiteType::Laravel;
        let lara = create(&conn, lara).unwrap();
        assert_eq!(lara.db_name, "lv_myapp_test");
        assert_eq!(get(&conn, &lara.id).unwrap().unwrap().db_name, "lv_myapp_test");

        let mut plain = sample("Plain", "plain.test");
        plain.site_type = SiteType::Php;
        assert_eq!(create(&conn, plain).unwrap().db_name, "php_plain_test");
    }

    #[test]
    fn create_gives_slug_colliding_domains_distinct_databases() {
        // `db_name_for` reduces both domains to `wp_my_shop_test`. Before B21 the
        // second site would silently bind the FIRST site's database. Now the
        // colliding one is disambiguated, so the two never share a database.
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "my-shop.test")).unwrap();
        let b = create(&conn, sample("B", "my.shop.test")).unwrap();
        assert_eq!(a.db_name, "wp_my_shop_test", "the first keeps the clean name");
        assert_ne!(b.db_name, a.db_name, "the colliding second must NOT share the DB");
        assert!(
            b.db_name.starts_with("wp_my_shop_test_"),
            "disambiguated by hash suffix: {}",
            b.db_name
        );
        assert!(b.db_name.len() <= crate::core::wordpress::DB_NAME_MAX);
    }

    /// Two distinct domains that hash to the SAME FrankenPHP backend slot — the
    /// 100-slot space guarantees a collision within 101 domains (pigeonhole).
    fn colliding_frankenphp_domains() -> (String, String) {
        let mut seen: std::collections::HashMap<u16, String> = std::collections::HashMap::new();
        for i in 0..1000u32 {
            let d = format!("collide{i}.test");
            let p = crate::core::frankenphp::site_port(&d);
            if let Some(prev) = seen.get(&p) {
                return (prev.clone(), d);
            }
            seen.insert(p, d);
        }
        panic!("expected a FrankenPHP port collision within 1000 domains");
    }

    fn fp_site(domain: &str) -> Site {
        Site {
            id: format!("id-{domain}"),
            name: domain.into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Frankenphp,
            ssl: true,
            path: format!("~/Sites/{domain}"),
            created_at: "t".into(),
            multisite: MultisiteMode::None,
            db_name: format!("wp_{domain}"),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
            enabled: true,
        }
    }

    #[test]
    fn allocate_override_port_picks_lowest_free_and_respects_the_range() {
        // FrankenPHP: lowest free from 8200; a site already recording 8200 pushes
        // the next to 8201; a DIFFERENT-range (Apache) recorded port never blocks.
        let base = super::frankenphp::FRANKENPHP_BASE_PORT;
        let mut s0 = fp_site("x.rex");
        s0.override_port = Some(base);
        assert_eq!(allocate_override_port(&[], WebServer::Frankenphp).unwrap(), Some(base));
        assert_eq!(
            allocate_override_port(&[s0.clone()], WebServer::Frankenphp).unwrap(),
            Some(base + 1)
        );
        // Apache allocates from its own (disjoint) range, ignoring the frankenphp port.
        assert_eq!(
            allocate_override_port(&[s0], WebServer::Apache).unwrap(),
            Some(crate::core::apache::APACHE_BASE_PORT)
        );
        // Nginx has no per-site port.
        assert_eq!(allocate_override_port(&[], WebServer::Nginx).unwrap(), None);
    }

    #[test]
    fn recorded_port_conflict_is_the_kept_belt_on_the_allocator() {
        // The B20-A guard, reworked to the recorded model (B20 §4): another site
        // recording the same port is flagged, naming it; self and free ports are
        // not. fp_site has no recorded port → recorded_override_port derives it,
        // and the two colliding domains derive the SAME value.
        let (da, db_) = colliding_frankenphp_domains();
        let a = fp_site(&da);
        let b = fp_site(&db_);
        let port = recorded_override_port(&a).unwrap();
        let others = [a.clone()];
        // b (a different site) on a's port → flagged, naming a's domain.
        assert_eq!(recorded_port_conflict(&others, &b.id, port), Some(da.clone()));
        // Never flagged against itself.
        assert!(recorded_port_conflict(&others, &a.id, port).is_none());
        // A free port → no conflict.
        assert!(recorded_port_conflict(&others, &b.id, port + 1).is_none());
    }

    #[test]
    fn create_allocates_distinct_ports_for_would_be_colliding_domains() {
        // The behavioral shift from B20-A → B20-B: two FrankenPHP sites whose
        // domains hash to the SAME derived slot are no longer REFUSED — the
        // allocator gives the second a DISTINCT free port, so BOTH create and
        // neither shares a backend (the collision is designed out).
        let conn = db::open_in_memory().unwrap();
        let (da, db_) = colliding_frankenphp_domains();

        let mut a = sample("A", &da);
        a.web_server = WebServer::Frankenphp;
        let sa = create_on(&conn, a, "macos").unwrap();

        let mut b = sample("B", &db_);
        b.web_server = WebServer::Frankenphp;
        let sb =
            create_on(&conn, b, "macos").expect("second colliding site is NOT refused anymore");

        // Distinct, both in the FrankenPHP range — the allocator picks the lowest
        // free (base, base+1), so they can't share a backend.
        assert_eq!(sa.override_port, Some(super::frankenphp::FRANKENPHP_BASE_PORT));
        assert_eq!(sb.override_port, Some(super::frankenphp::FRANKENPHP_BASE_PORT + 1));
        assert_ne!(sa.override_port, sb.override_port);

        // An nginx site records NO port (no per-site backend).
        let mut c = sample("C", "c.rex");
        c.web_server = WebServer::Nginx;
        assert_eq!(create(&conn, c).unwrap().override_port, None);
    }

    #[test]
    fn backfill_preserves_the_first_sites_port_and_resolves_a_collision() {
        // THE load-bearing migration test (B20 §4 Phase B + the B21 safety proof).
        // Two FrankenPHP sites whose domains hash to the SAME derived slot, both
        // with a NULL override_port (the pre-migration state), inserted directly.
        let conn = db::open_in_memory().unwrap();
        let (da, db_) = colliding_frankenphp_domains();
        let derived = super::frankenphp::site_port(&da);
        assert_eq!(derived, super::frankenphp::site_port(&db_), "domains must collide");

        let mut a = fp_site(&da);
        a.id = "id-a".into();
        a.created_at = "2026-01-01T00:00:00Z".into(); // OLDER → keeps the derived port
        let mut b = fp_site(&db_);
        b.id = "id-b".into();
        b.created_at = "2026-01-02T00:00:00Z".into();
        store::insert_site(&conn, &a).unwrap();
        store::insert_site(&conn, &b).unwrap();

        backfill_override_ports(&conn).unwrap();
        let ra = get(&conn, "id-a").unwrap().unwrap();
        let rb = get(&conn, "id-b").unwrap().unwrap();

        // ZERO-DISRUPTION HALF: the FIRST (oldest) site keeps its EXACT current
        // derived port — its running backend + edge route are untouched.
        assert_eq!(ra.override_port, Some(derived), "the first site's port is unchanged");
        // RESOLUTION HALF (B21): the already-broken second site gets a DISTINCT
        // free port — neither NULL, no crash, no UNIQUE-constraint brick.
        assert!(rb.override_port.is_some(), "second site recorded a port");
        assert_ne!(rb.override_port, Some(derived), "second resolved to a DIFFERENT port");
        assert!((8200..8300).contains(&rb.override_port.unwrap()), "in the FrankenPHP range");

        // IDEMPOTENT: a re-run touches nothing (both already recorded).
        backfill_override_ports(&conn).unwrap();
        assert_eq!(get(&conn, "id-a").unwrap().unwrap().override_port, Some(derived));
        assert_eq!(get(&conn, "id-b").unwrap().unwrap().override_port, rb.override_port);
    }

    #[test]
    fn backfill_freezes_todays_ownership_answer_including_moved_out_docroots() {
        // THE v17 migration proof. Two pre-v17 rows (docroot_managed NULL): one
        // under the CONFIGURED sites folder, one moved outside it — the case a
        // plain DEFAULT could only have guessed at, and whose files today's
        // lexical guard preserves ("kept — not deleted", the move dialog).
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let managed_root = std::env::temp_dir().join("rexenv-backfill-root");
        store::set_setting(&conn, SITES_DIR_KEY, &managed_root.display().to_string()).unwrap();

        let mut inside = fp_site("inside.test");
        inside.id = "in".into();
        inside.path = managed_root.join("inside.test").display().to_string();
        inside.docroot_managed = None; // the pre-v17 shape
        let mut outside = fp_site("outside.test");
        outside.id = "out".into();
        outside.path = std::env::temp_dir().join("someones-project").display().to_string();
        outside.docroot_managed = None;
        store::insert_site(&conn, &inside).unwrap();
        store::insert_site(&conn, &outside).unwrap();

        backfill_docroot_managed(&conn, &*platform).unwrap();

        // Under the sites folder → still ours, still deletable: unchanged.
        assert_eq!(get(&conn, "in").unwrap().unwrap().docroot_managed, Some(true));
        // Moved out → frozen as NOT ours, so teardown keeps preserving it.
        assert_eq!(get(&conn, "out").unwrap().unwrap().docroot_managed, Some(false));

        // The whole point: re-pointing the Sites folder afterwards can no longer
        // re-classify a recorded row (today that silently makes ~/code deletable).
        store::set_setting(
            &conn,
            SITES_DIR_KEY,
            &std::env::temp_dir().display().to_string(),
        )
        .unwrap();
        backfill_docroot_managed(&conn, &*platform).unwrap(); // idempotent
        assert_eq!(get(&conn, "in").unwrap().unwrap().docroot_managed, Some(true));
        assert_eq!(
            get(&conn, "out").unwrap().unwrap().docroot_managed,
            Some(false),
            "a recorded answer must never be revisited"
        );
    }

    #[test]
    fn created_sites_record_that_we_own_the_docroot() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Owned", "owned.test")).unwrap();
        assert_eq!(get(&conn, &site.id).unwrap().unwrap().docroot_managed, Some(true));
    }

    #[test]
    fn set_domain_preserves_the_recorded_override_port() {
        // The orphan fix: changing a site's domain must NOT re-derive/move its
        // recorded backend port (which used to orphan the running backend).
        let conn = db::open_in_memory().unwrap();
        let mut a = sample("A", "old.rex");
        a.web_server = WebServer::Frankenphp;
        let created = create_on(&conn, a, "macos").unwrap();
        let port = created.override_port.expect("frankenphp site has a recorded port");

        let updated = set_domain(&conn, &created.id, "new.rex").unwrap().unwrap();
        assert_eq!(updated.domain, "new.rex");
        assert_eq!(
            updated.override_port,
            Some(port),
            "a domain change must not re-derive or move the recorded port"
        );
    }

    #[test]
    fn set_domain_updates_only_the_domain() {
        let conn = db::open_in_memory().unwrap();
        let created = create(&conn, sample("Shop", "myapp.test")).unwrap();

        let updated = set_domain(&conn, &created.id, "myshop.test").unwrap().unwrap();
        assert_eq!(updated.domain, "myshop.test");
        // The database name and docroot stay keyed to the creation-time domain.
        assert_eq!(updated.db_name, "wp_myapp_test");
        assert_eq!(updated.path, created.path);
        assert_eq!(updated.name, "Shop");
    }

    /// Adopting records the mode and nothing else — it takes no PHP, no wp-cli
    /// and no docroot, so it cannot run a convert — and refuses to "adopt" a
    /// single site.
    /// A network is read from the wp-config's own defines (commented, merely
    /// allowed and false ones are not networks), and Convert on a docroot that
    /// already is one records the FILE's mode and runs nothing.
    #[test]
    fn a_network_is_read_from_wp_config_and_never_converted_again() {
        let sub = "<?php\ndefine( 'MULTISITE', true );\ndefine( 'SUBDOMAIN_INSTALL', true );\n";
        assert_eq!(network_mode_in_wp_config(sub), Some(MultisiteMode::Subdomain));
        assert_eq!(
            network_mode_in_wp_config("<?php\ndefine('MULTISITE', true);\ndefine('SUBDOMAIN_INSTALL', false);\n"),
            Some(MultisiteMode::Subdirectory)
        );
        assert_eq!(network_mode_in_wp_config("<?php\ndefine('MULTISITE', 1);\n"), Some(MultisiteMode::Subdirectory));
        assert_eq!(network_mode_in_wp_config("<?php\ndefine('WP_ALLOW_MULTISITE', true);\n"), None, "allowed is not enabled");
        assert_eq!(network_mode_in_wp_config("<?php\ndefine('MULTISITE', false);\n"), None);
        assert_eq!(network_mode_in_wp_config("<?php\n// define('MULTISITE', true);\n"), None, "a commented define is not a network");

        // The php/wp-cli paths don't exist: a convert that ran would fail.
        let dir = std::env::temp_dir().join(format!("rexenv-convert-live-network-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wp-config.php"), sub).unwrap();
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        let nope = Path::new("/nonexistent/rexenv-test-bin");
        let got = convert_multisite(&conn, nope, nope, &dir, &a.id, MultisiteMode::Subdirectory);
        let _ = std::fs::remove_dir_all(&dir);
        let site = got.expect("an existing network is recorded, not converted").expect("the site exists");
        assert_eq!(site.multisite, MultisiteMode::Subdomain, "the file's mode wins over the one asked for");
    }

    #[test]
    fn adopting_a_network_records_the_mode_and_refuses_none() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        assert!(adopt_multisite(&conn, &a.id, MultisiteMode::None).is_err());
        assert_eq!(get(&conn, &a.id).unwrap().unwrap().multisite, MultisiteMode::None, "a refusal wrote");
        let s = adopt_multisite(&conn, &a.id, MultisiteMode::Subdomain).unwrap().unwrap();
        assert_eq!(s.multisite, MultisiteMode::Subdomain);
        assert!(adopt_multisite(&conn, "nope", MultisiteMode::Subdirectory).unwrap().is_none());
    }

    #[test]
    fn set_domain_rejects_invalid_duplicate_same_and_multisite() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        create(&conn, sample("B", "b.test")).unwrap();

        assert!(set_domain(&conn, &a.id, "../evil.test").is_err());
        assert!(set_domain(&conn, &a.id, "b.test").unwrap_err().to_string().contains("already reaches"));
        assert!(set_domain(&conn, &a.id, "a.test").unwrap_err().to_string().contains("already uses"));
        assert!(set_domain(&conn, "nope", "c.test").unwrap().is_none());

        store::set_site_multisite(&conn, &a.id, MultisiteMode::Subdomain.as_db()).unwrap();
        let err = set_domain(&conn, &a.id, "c.test").unwrap_err().to_string();
        assert!(err.contains("multisite"), "must refuse multisite: {err}");

        // Nothing above changed the row.
        assert_eq!(get(&conn, &a.id).unwrap().unwrap().domain, "a.test");
    }

    /// Throwaway docroot with a nested file tree; returns (root, docroot).
    fn tree(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("rexenv-move-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let doc = root.join("from").join("acme.test");
        std::fs::create_dir_all(doc.join("wp-content/uploads")).unwrap();
        std::fs::write(doc.join("index.php"), "<?php phpinfo();\n").unwrap();
        std::fs::write(doc.join("wp-content/uploads/a.jpg"), vec![7u8; 1024]).unwrap();
        (root, doc)
    }

    fn site_at(doc: &Path) -> Site {
        Site {
            id: "m1".into(),
            name: "Acme".into(),
            domain: "acme.test".into(),
            site_type: SiteType::Wordpress,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            ssl: true,
            path: doc.display().to_string(),
            created_at: "now".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_acme_test".into(),
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            xdebug: false,
            override_port: None,
            provisioned: true,
            docroot_managed: Some(true),
            db_created: None,
            content_dir: None,
            mu_dir_created: None,
            origin: SiteOrigin::User,
            agent_client: None,
            expires_at: None,
            docroot_subdir: String::new(),
            git_url: None,
            git_ref: None,
            git_migrate: None,
            git_build_assets: None,
            starter_db: None,
            enabled: true,
        }
    }

    #[test]
    fn check_docroot_move_rejects_bad_destinations() {
        let (root, doc) = tree("checks");
        let site = site_at(&doc);

        let inside = check_docroot_move(&site, &doc.join("sub")).unwrap_err().to_string();
        assert!(inside.contains("inside the site folder"), "{inside}");

        let noop = check_docroot_move(&site, doc.parent().unwrap()).unwrap_err().to_string();
        assert!(noop.contains("already lives"), "{noop}");

        let to = root.join("to");
        std::fs::create_dir_all(to.join("acme.test")).unwrap();
        let exists = check_docroot_move(&site, &to).unwrap_err().to_string();
        assert!(exists.contains("already exists"), "{exists}");

        let rel = check_docroot_move(&site, Path::new("relative/x")).unwrap_err().to_string();
        assert!(rel.contains("absolute"), "{rel}");

        let mut gone = site.clone();
        gone.path = root.join("nope").display().to_string();
        let missing = check_docroot_move(&gone, &to).unwrap_err().to_string();
        assert!(missing.contains("missing on disk"), "{missing}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn move_dir_renames_within_a_volume_and_row_flips_via_set_path() {
        let (root, doc) = tree("rename");
        let site = site_at(&doc);
        let dest = root.join("to");
        std::fs::create_dir_all(&dest).unwrap();

        let target = check_docroot_move(&site, &dest).unwrap();
        let copied = move_dir(&doc, &target).unwrap();
        assert!(!copied, "same-volume must be a rename");
        assert!(!doc.exists(), "old path gone after rename");
        assert!(target.join("wp-content/uploads/a.jpg").exists());

        let conn = db::open_in_memory().unwrap();
        let created = create(
            &conn,
            NewSite {
                name: "Acme".into(),
                domain: "acme.test".into(),
                site_type: SiteType::Wordpress,
                php_version: "8.3".into(),
                web_server: WebServer::Nginx,
                path: doc.display().to_string(),
                db_engine: crate::state::models::SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: false,
            },
        )
        .unwrap();
        let updated =
            set_path(&conn, &*crate::platform::current(), &created.id, &target).unwrap().unwrap();
        assert_eq!(updated.path, target.display().to_string());
        // Path-only: everything else untouched.
        assert_eq!(updated.domain, "acme.test");
        assert_eq!(updated.db_name, "wp_acme_test");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_fallback_verifies_and_cleans_up_a_partial_copy() {
        let (root, doc) = tree("copy");
        let dest = root.join("to").join("acme.test");

        // The copy+verify path itself (what a cross-volume move runs).
        copy_dir_recursive(&doc, &dest).unwrap();
        verify_tree(&doc, &dest).unwrap();
        assert_eq!(dest.join("wp-content/uploads/a.jpg").metadata().unwrap().len(), 1024);

        // Tamper with the copy → verify must fail.
        std::fs::write(dest.join("wp-content/uploads/a.jpg"), b"short").unwrap();
        let err = verify_tree(&doc, &dest).unwrap_err().to_string();
        assert!(err.contains("size mismatch"), "{err}");

        // move_dir into an unwritable parent: rename AND copy fail → error
        // mentions the failure, no partial target left, source intact.
        //
        // Unix only, and this one is worth stating: `test_support::set_mode` is a NO-OP
        // off unix, so on Windows the directory was never locked, `move_dir` SUCCEEDED,
        // and `unwrap_err` panicked on `Ok(false)`. A no-op helper does not fail loudly
        // the way `symlink`'s `Unsupported` did — it quietly makes the test assert
        // something that cannot happen (W12). Locking a folder on Windows means ACLs,
        // which is a different fixture, not a mode.
        // Not as root: root ignores mode bits, so a 0o555 folder is not locked for it and
        // the move succeeds — the Ubuntu check container runs as root (27 Sep 2026). The
        // release runners and every developer run as a user, where the fixture holds.
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            let locked = root.join("locked");
            std::fs::create_dir_all(&locked).unwrap();
            crate::test_support::set_mode(&locked, 0o555).unwrap();
            let denied = move_dir(&doc, &locked.join("acme.test")).unwrap_err().to_string();
            assert!(denied.contains("nothing changed"), "{denied}");
            assert!(doc.join("index.php").exists(), "source untouched on failure");
            assert!(!locked.join("acme.test").exists(), "no partial target left");
            crate::test_support::set_mode(&locked, 0o755).unwrap();
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_domain_is_rejected() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, sample("One", "dup.test")).unwrap();
        let err = create(&conn, sample("Two", "dup.test")).unwrap_err().to_string();
        // The refusal NAMES the site that already has the hostname (3 Sep 2026,
        // when the check learned about extra domains): "already in use" sends
        // the user looking through their list, "already reaches One" ends it.
        assert!(err.contains("already reaches") && err.contains("\"One\""), "{err}");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn set_status_updates_and_returns_site() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Acme", "acme.test")).unwrap();
        assert!(matches!(site.status, ServiceStatus::Stopped));

        let updated = set_status(&conn, &site.id, ServiceStatus::Running)
            .unwrap()
            .expect("exists");
        assert!(matches!(updated.status, ServiceStatus::Running));
        // persisted
        assert!(matches!(
            get(&conn, &site.id).unwrap().unwrap().status,
            ServiceStatus::Running
        ));
        // unknown id → None
        assert!(set_status(&conn, "nope", ServiceStatus::Running)
            .unwrap()
            .is_none());
    }

    #[test]
    fn get_and_delete() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Blog", "blog.test")).unwrap();

        let fetched = get(&conn, &site.id).unwrap().expect("site exists");
        assert_eq!(fetched.domain, "blog.test");

        assert!(delete(&conn, &site.id).unwrap());
        assert!(get(&conn, &site.id).unwrap().is_none());
        assert!(!delete(&conn, &site.id).unwrap(), "second delete is a no-op");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_deleted_sites_packages_go_with_it_and_the_read_still_guards_independently() {
        // TWO defences on one fact, asserted SEPARATELY — the point of keeping
        // both is that they fail differently, so a test that only checked the
        // end result ("no package is listed") would pass with either one gone
        // and could not tell you which was carrying it.
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let site = create(&conn, sample("Packaged", "packaged.test")).unwrap();
        let pkg = |site_id: &str, slug: &str| crate::state::models::ScratchPackage {
            site_id: site_id.to_string(),
            slug: slug.to_string(),
            kind: "plugin".into(),
            source_path: "/tmp/acme".into(),
            synced_at: "2026-08-04 10:00:00".into(),
            fingerprint: "1-2-3".into(),
        };
        store::upsert_scratch_package(&conn, &pkg(&site.id, "acme")).unwrap();
        assert_eq!(store::scratch_packages(&conn, &site.id).unwrap().len(), 1, "planted");

        // (1) The DELETE: teardown takes the rows with the site, so the table
        //     does not grow orphans. Checked against the RAW table, not the
        //     joined read — the joined read would hide a surviving row and
        //     report success for the wrong reason.
        teardown(&conn, &*platform, &site.id).unwrap();
        let raw: i64 = conn
            .query_row("SELECT COUNT(*) FROM scratch_packages WHERE site_id = ?1", [&site.id], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, 0, "the package rows outlived their site as orphans");

        // (2) The JOIN, still load-bearing on its own: plant a row for a site
        //     that does not exist — the shape a pre-fix database still carries,
        //     and the shape any future path that forgets step (1) would create.
        //     The read must not surface it.
        store::upsert_scratch_package(&conn, &pkg("no-such-site-id", "ghost")).unwrap();
        let ghost_raw: i64 = conn
            .query_row("SELECT COUNT(*) FROM scratch_packages WHERE slug = 'ghost'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ghost_raw, 1, "the fixture must actually plant an orphan, or (2) proves nothing");
        assert!(
            store::all_scratch_packages(&conn).unwrap().iter().all(|p| p.slug != "ghost"),
            "the read surfaced a package of a site that does not exist — the join is not guarding"
        );
    }

    #[test]
    fn every_owned_mu_plugin_is_swept_by_the_cleanup_that_claims_them_all() {
        // `cleanup_muplugin_artifacts` is named for ALL of rexenv's mu-plugin
        // artifacts, but its body is a hand-maintained list of three specific
        // removals. That gap is the narrower-surface family, and it has a real
        // consequence beyond a stray file: `remove_if_effectively_empty` refuses
        // to remove the `mu-plugins` dir while anything remains in it, so ONE
        // unswept file silently defeats the v25 dir cleanup for every site.
        //
        // Detection is by the write side: a core module that joins
        // `"mu-plugins"` owns a file there and must be swept here.
        const SWEEP: &str = include_str!("sites.rs");
        let body = SWEEP
            .split("pub fn cleanup_muplugin_artifacts(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("the cleanup fn");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core");
        let mut owners: Vec<String> = std::fs::read_dir(&dir)
            .expect("core/")
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let stem = p.file_stem()?.to_str()?.to_string();
                // `sites.rs` itself joins the path to REMOVE the dir, not to own
                // a file in it — it is the sweeper, not a sweepee.
                if stem == "sites" || p.extension()? != "rs" {
                    return None;
                }
                std::fs::read_to_string(&p).ok()?.contains(r#"join("mu-plugins")"#).then_some(stem)
            })
            .collect();
        owners.sort();
        assert!(!owners.is_empty(), "the detection found nothing — it has stopped working");
        for owner in &owners {
            assert!(
                body.contains(&format!("core::{owner}::")),
                "`core::{owner}` writes a mu-plugin but cleanup_muplugin_artifacts never removes \
                 it. One unswept file also blocks the v25 mu-plugins dir removal for EVERY site, \
                 because the dir is only removed when effectively empty. Add its removal to the \
                 sweep; the function's name already promises it."
            );
        }
    }

    /// Both sweeps are hand-maintained lists standing behind names that promise
    /// ALL of a site's per-site artifacts — and Apache's config and log were
    /// missing from both for as long as Apache has existed. That is the same
    /// family as `every_owned_mu_plugin_is_swept_by_the_cleanup_that_claims_them
    /// _all` above, so it gets the same treatment: detection by the WRITE side,
    /// not by a second list someone has to remember.
    ///
    /// A per-site artifact is a core module exposing `config_path`/`log_path`
    /// taking a `domain` — that signature IS "I own a file named after a site".
    #[test]
    fn the_sites_folder_refuses_what_a_generated_config_cannot_carry() {
        let conn = db::open_in_memory().unwrap();

        // The B26 shape: the path is written into QUOTED Caddy/nginx directives,
        // so a quote or a backslash ends the string early and the tail becomes
        // config. Refused, never stripped — silently changing the folder would
        // create the user's sites somewhere they did not pick.
        // The backslash is judged per folder NAME (`config_breaking_char`), so it is
        // config-breaking inside a name on unix and the SEPARATOR on Windows — the same
        // distinction `validate_docroot_path` draws (#661). Asserting the unix answer on
        // both made this fail on the Dell for behaviour that is correct there (W12).
        #[cfg(unix)]
        let bad_paths = [
            "/Users/dev/My \"Sites\"",
            "/Users/dev/Sites\\evil",
            "/Users/dev/Sites\nlisten 1.2.3.4:80",
            "/Users/dev/Sites\rlisten 1.2.3.4:80",
        ];
        #[cfg(windows)]
        let bad_paths = [
            "C:\\Users\\dev\\My \"Sites\"",
            "C:\\Users\\dev\\Sites\nlisten 1.2.3.4:80",
            "C:\\Users\\dev\\Sites\rlisten 1.2.3.4:80",
        ];
        for bad in bad_paths {
            let err = set_sites_dir(&conn, bad).expect_err("must refuse").to_string();
            assert!(
                err.contains("web-server configs"),
                "the refusal must say WHY, not just no: {err}"
            );
        }
        // Relative: where the sites landed would depend on the app's cwd.
        let err = set_sites_dir(&conn, "Sites").expect_err("must refuse").to_string();
        assert!(err.contains("full path"), "{err}");
        // Empty is its own message — "leave it alone to keep the default" is a
        // different instruction from "that path is malformed".
        let err = set_sites_dir(&conn, "   ").expect_err("must refuse").to_string();
        assert!(err.contains("can't be empty"), "{err}");
        // Nothing was written by any refusal.
        assert_eq!(store::get_setting(&conn, SITES_DIR_KEY).unwrap(), None);

        // What a REAL folder can contain is accepted: spaces, unicode and an
        // apostrophe all survive a quoted path, and nothing here goes near a
        // shell. Refusing them would be sanitising by another name.
        //
        // The ROOT has to be this os's root, for the reason the refusals above split: a
        // driveless path is not absolute on Windows, so `/Users/dev/Sites` was refused
        // there as RELATIVE — and these are the cases the test says must be ALLOWED, so
        // the fixture was convicting the setter of the fixture's own spelling (W12).
        #[cfg(unix)]
        let root = "/Users";
        #[cfg(windows)]
        let root = r"C:\Users";
        let good_paths = [
            format!("{root}/dev/Sites"),
            format!("{root}/dev/My Sites"),
            format!("{root}/dev/Sites/café"),
            format!("{root}/o'brien/Sites"),
        ];
        for good in &good_paths {
            assert_eq!(set_sites_dir(&conn, good).unwrap(), *good, "{good} must be allowed");
        }
        // Trimmed, not rejected, for the one case where whitespace is a paste
        // artefact rather than part of the name — including a TRAILING newline,
        // which a copied path routinely carries. The check runs on the trimmed
        // value, so what is stored has no line break in it and the refusal above
        // is about a break in the MIDDLE, which no trim can make safe.
        let plain = format!("{root}/dev/Sites");
        assert_eq!(set_sites_dir(&conn, &format!("  {plain}  ")).unwrap(), plain);
        assert_eq!(set_sites_dir(&conn, &format!("{plain}\n")).unwrap(), plain);
        assert_eq!(
            store::get_setting(&conn, SITES_DIR_KEY).unwrap().as_deref(),
            Some(plain.as_str())
        );
    }

    /// An existing value that would fail today's rule keeps working: validation
    /// is on the WRITE path only. Refusing at READ time would relocate a user's
    /// sites folder to the default and make every site they own look missing,
    /// which is a far worse failure than the litter it prevents.
    #[test]
    fn a_sites_folder_stored_before_the_rule_still_resolves() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        // Straight past the setter, the way a pre-13-Aug-2026 value got there.
        store::set_setting(&conn, SITES_DIR_KEY, "/Users/dev/My \"Sites\"").unwrap();
        assert_eq!(
            sites_dir(&conn, &*platform).unwrap(),
            std::path::PathBuf::from("/Users/dev/My \"Sites\""),
            "the read path must not have learned to refuse — that would move their sites"
        );
    }

    /// #189 — **the row is deleted BEFORE anything on disk is, so a crash can
    /// never leave a site row pointing at a path that is gone.**
    ///
    /// Teardown is not atomic: it removes a docroot, a cert dir, override
    /// configs and logs, and the machine can die at any point in that list. The
    /// two orders fail very differently. Row-last leaves a LISTED site whose
    /// files are missing — it renders, it offers Open and Start, and every one
    /// of those actions fails on a path nobody can restore; the user's only
    /// route out is deleting a site that is already deleted. Row-first leaves
    /// orphaned FILES, which nothing shows, nothing acts on, and the user (or a
    /// later delete of the same domain) can remove.
    ///
    /// So the order is the guarantee, and it is asserted as an ORDER rather
    /// than as "the delete is on line N": a reordering that moves a removal
    /// above the row delete is exactly what this catches.
    #[test]
    fn teardown_deletes_the_row_before_it_touches_the_disk() {
        let src = crate::core::copy_scan::production_source(include_str!("sites.rs"));
        let body = src
            .split("pub fn teardown(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("teardown");
        let row = body
            .find("store::delete_site(conn, id)")
            .expect("teardown no longer deletes the site row — if it moved, move this guard");
        // Every way this function touches the filesystem. A new removal added
        // ABOVE the row delete fails here; a new KIND of removal that this list
        // does not name is caught by the count check below.
        let removals = ["remove_dir_all(", "remove_file("];
        let mut found = 0;
        for kind in removals {
            let mut from = 0;
            while let Some(at) = body[from..].find(kind) {
                let at = from + at;
                assert!(
                    at > row,
                    "teardown removes something from disk (`{kind}`) BEFORE deleting the site \
                     row. A crash in between then leaves a listed site whose files are gone: it \
                     renders, it offers Open and Start, and every action fails on a path nobody \
                     can restore. Orphaned files are the survivable direction"
                );
                found += 1;
                from = at + kind.len();
            }
        }
        assert!(
            found >= 5,
            "the scan found only {found} filesystem removals in teardown — it used to find \
             seven (docroot, cert dir, two FrankenPHP paths, two Apache paths, the tunnel log). \
             Either the removals moved out of this function, or the split stopped seeing its \
             body, and a guard that scans nothing passes for the wrong reason"
        );
        // The docroot removal is the one that matters most and the one most
        // likely to be "optimised" upward — named explicitly so its message is
        // about the docroot rather than about a generic call.
        let docroot = body.find("remove_dir_all(&site.path)").expect("docroot removal");
        assert!(docroot > row, "the docroot is removed before the row is deleted");
    }

    /// The generic KV setter is a door around every validating setter, so the
    /// routing is asserted rather than trusted — and asserted over the WHOLE
    /// surface, not over a pair of names.
    ///
    /// **What this used to be, and why that was the bug.** It named
    /// `DEFAULT_TLD_KEY` and `SITES_DIR_KEY` and checked that `set_setting`
    /// mentioned both; the command itself held one `if` per key. A THIRD gated
    /// key would have been written raw by the generic door with nothing failing
    /// — a guard checking two places inside the surface it claimed (ledger #344,
    /// the guard-covers-claimed-surface family). The fix is not another name in
    /// a list: `GATED_SETTERS` is the registry, `set_setting` DISPATCHES through
    /// it, and this test holds the shape closed from both ends — no per-key
    /// branch may come back, and every key this build rules on must be either
    /// gated or explicitly excused.
    #[test]
    fn every_gated_setting_key_is_routed_through_its_validating_setter() {
        use crate::core::settings_access as sa;

        let body = crate::core::copy_scan::production_source(include_str!(
            "../commands/settings.rs"
        ));
        let body = body
            .split("pub fn set_setting(")
            .nth(1)
            .and_then(|b| b.split("\n#[tauri::command]").next())
            .expect("set_setting");

        // 1. The dispatch is the registry, and NOTHING else. A per-key branch is
        //    refused by shape rather than by review: it is the exact thing that
        //    grew a hole here, and one branch beside the registry means a key
        //    can be gated in one place and not the other.
        assert!(
            body.contains("gated_setter(&key)"),
            "`set_setting` no longer dispatches through `settings_access::gated_setter` — the \
             generic KV command is then a way around every validating setter"
        );
        assert!(
            !body.contains("if key =="),
            "`set_setting` has grown a per-key branch again. Gate the key by adding it to \
             `GATED_SETTERS`, beside its setter, so the dispatch, `cli_access` and this guard \
             all learn about it at once"
        );

        // 2. Each registered setter really VALIDATES: it refuses a value the raw
        //    store would have taken. A registry entry pointing at a setter that
        //    waves everything through would satisfy every structural check above
        //    and gate nothing, so the refusal is measured against a real DB.
        //    A gated key with no sample here fails — the sample is part of
        //    gating, not an optional extra.
        let refusals: &[(&str, &str)] = &[
            (DEFAULT_TLD_KEY, "local"),
            (SITES_DIR_KEY, "relative/not/absolute"),
            (crate::core::scratch::SCRATCH_CAP_KEY, "0"),
            (crate::core::scratch::SCRATCH_TTL_KEY, "999"),
        ];
        let conn = db::open_in_memory().unwrap();
        for (key, setter) in sa::GATED_SETTERS {
            let bad = refusals
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| {
                    panic!(
                        "`{key}` is in GATED_SETTERS with no refusal sample in this test. Add a \
                         value its setter must REJECT — otherwise the registry proves only that \
                         a function was named, not that it gates"
                    )
                })
                .1;
            // Against whatever the key holds NOW (a fresh DB already seeds
            // `default_tld`), because the claim is that a refusal changes
            // nothing — not that the key was empty to begin with.
            let before = store::get_setting(&conn, key).unwrap();
            assert!(
                setter(&conn, bad).is_err(),
                "`{key}`'s registered setter accepted `{bad}` — a validating setter that \
                 validates nothing is a gate on paper"
            );
            assert_eq!(
                store::get_setting(&conn, key).unwrap(),
                before,
                "`{key}` was WRITTEN despite its setter refusing — the refusal must happen \
                 before the write, or the gate is a message and not a control"
            );
            assert_eq!(
                sa::cli_access(key),
                sa::CliAccess::ReadWrite,
                "`{key}` is gated but `cli_access` does not call it writable — the two must \
                 read the same registry"
            );
        }

        // 3. The CLI half, over the surface rather than a sample of it. The rule:
        //    a key may be CLI-writable only if it is GATED, or named in
        //    `UNVALIDATED_BUT_SAFE` with the reason that is acceptable.
        //    The DOMAIN is derived from the policy file's own source — every
        //    key-shaped literal it rules on — so a key added there next month is
        //    checked without anyone editing this list. That is the half the old
        //    hardcoded fourteen could not give.
        let policy_src = include_str!("settings_access.rs");
        let mut domain: Vec<&str> = policy_src
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|lit| {
                !lit.is_empty()
                    && lit.len() < 40
                    && lit.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
            .collect();
        domain.extend(sa::GATED_SETTERS.iter().map(|(k, _)| *k));
        domain.extend(sa::UNVALIDATED_BUT_SAFE.iter().map(|(k, _)| *k));
        domain.sort_unstable();
        domain.dedup();
        // The derivation is itself checked: if it ever stops seeing the keys we
        // KNOW are ruled on, it has silently narrowed and every assertion below
        // it becomes vacuous — the "reports green having asserted nothing" shape.
        for known in ["default_tld", "sites_dir", "preferred_editor", "php_update_manifest_serial"]
        {
            assert!(
                domain.contains(&known),
                "the derived key domain lost `{known}` — the scan of `settings_access.rs` no \
                 longer finds the keys it rules on, so this guard is checking an empty set"
            );
        }
        let safe: Vec<&str> = sa::UNVALIDATED_BUT_SAFE.iter().map(|(k, _)| *k).collect();
        for key in domain {
            if sa::cli_access(key) == sa::CliAccess::ReadWrite {
                assert!(
                    sa::gated_setter(key).is_some() || safe.contains(&key),
                    "`{key}` is CLI-WRITABLE but is neither in `GATED_SETTERS` nor listed in \
                     `UNVALIDATED_BUT_SAFE` with a reason. `rex config set` would write it raw"
                );
            }
        }

        // 4. …and an unknown key is DENIED, which is the default the whole policy
        //    rests on. If this ever passes as writable, the match has grown a
        //    catch-all in the wrong direction.
        assert!(
            matches!(sa::cli_access("something_nobody_ruled_on"), sa::CliAccess::Denied(_)),
            "an unknown settings key must be DENIED, not writable"
        );
    }

    #[test]
    fn every_per_site_artifact_is_swept_by_both_sweeps() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core");
        let mut owners: Vec<(String, String)> = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("core/").flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let body = crate::core::copy_scan::production_source(&text);
            for kind in ["config_path", "log_path"] {
                if body.contains(&format!("pub fn {kind}(platform: &dyn Platform, domain: &str)")) {
                    owners.push((stem.to_string(), kind.to_string()));
                }
            }
        }
        owners.sort();
        assert!(
            owners.len() >= 5,
            "the detection found {} per-site path owners — it has stopped working (expected at \
             least frankenphp's pair, apache's pair and tunnels' log)",
            owners.len()
        );

        // The two places a site's name stops being used: deleted, and renamed.
        let teardown = crate::core::copy_scan::production_source(include_str!("sites.rs"));
        let teardown = teardown
            .split("pub fn teardown(")
            .nth(1)
            .and_then(|b| b.split("\npub fn ").next())
            .expect("teardown");
        let rename = crate::core::copy_scan::production_source(include_str!(
            "../commands/sites.rs"
        ));
        let rename = rename
            .split("pub async fn change_site_domain(")
            .nth(1)
            .and_then(|b| b.split("\n/// ").next())
            .expect("change_site_domain");

        for (module, kind) in &owners {
            for (what, body) in [("teardown", teardown), ("change_site_domain", rename)] {
                assert!(
                    body.contains(&format!("{module}::{kind}(")),
                    "`core::{module}::{kind}` names a file after a site, and {what} never removes \
                     it. The function's name already promises every per-site artifact — a list \
                     that has to be remembered is how Apache's config and log survived every \
                     delete and every rename until 13 Aug 2026."
                );
            }
        }

        // Run logs have no `log_path(domain)` owner — one file per site AND per
        // run — so they are detected by the name BUILDER instead: a `format!`
        // literal shaped `<family>{domain}-{run}….log`. Every family found must
        // be one `logs::RUN_LOG_FAMILIES` knows, and both sweeps must call the
        // one remover. 284 deleted sites' provision logs were still on disk
        // when this landed (11 Sep 2026).
        let mut families: Vec<String> = Vec::new();
        for sub in ["src/core", "src/commands"] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(sub);
            for entry in std::fs::read_dir(&dir).expect("source dir").flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let body = crate::core::copy_scan::production_source(&text);
                for literal in body.split("format!(\"").skip(1).filter_map(|s| s.split('"').next()) {
                    let Some(open) = literal.find('{') else { continue };
                    let (family, rest) = literal.split_at(open);
                    let run_shaped = literal.ends_with(".log")
                        && family.ends_with('-')
                        && (rest.starts_with("{}-{") || rest.starts_with("{domain}-{"));
                    if run_shaped && !families.iter().any(|f| f == family) {
                        families.push(family.to_string());
                    }
                }
            }
        }
        families.sort();
        assert!(
            families.len() >= 4,
            "the scan found run-log families {families:?} — it has stopped working (expected at \
             least site-provision, wp-install, db-import and repo)"
        );
        for family in &families {
            assert!(
                crate::core::logs::RUN_LOG_FAMILIES.contains(&family.as_str()),
                "a `format!` builds `{family}<domain>-<run>.log`, and `logs::RUN_LOG_FAMILIES` does \
                 not list it: deleting or renaming a site leaves those files behind forever"
            );
        }
        for (what, body) in [("teardown", teardown), ("change_site_domain", rename)] {
            assert!(
                body.contains("logs::remove_run_logs("),
                "{what} never removes the site's run logs (`logs::remove_run_logs`)"
            );
        }
    }

    #[test]
    fn teardown_removes_row_and_per_site_artifacts() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        // sample() path is "~/Sites/<name>" (not under the sites dir), so the
        // docroot-removal guard skips it — the test won't touch real dirs.
        let site = create(&conn, sample("Teardown", "teardown.test")).unwrap();

        // Plant the per-site config/log artifacts a site can accrue (FrankenPHP
        // override config + log, tunnel log) and assert teardown sweeps them.
        let artifacts = [
            frankenphp::config_path(&*platform, &site.domain).unwrap(),
            frankenphp::log_path(&*platform, &site.domain).unwrap(),
            // Apache's pair, missing from the sweep until 13 Aug 2026.
            apache::config_path(&*platform, &site.domain).unwrap(),
            apache::log_path(&*platform, &site.domain).unwrap(),
            // httpd's own ErrorLog, missing from both sweeps until 18 Sep 2026.
            apache::error_log_path(&*platform, &site.domain).unwrap(),
            tunnels::log_path(&*platform, &site.domain).unwrap(),
        ];
        for p in &artifacts {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        // The job logs, one per run — in neither sweep until 11 Sep 2026 — and
        // a living neighbour whose domain extends this one, whose logs stay.
        let log_dir = platform.paths().log_dir().unwrap();
        create(&conn, sample("Neighbour", "teardown.test-2.test")).unwrap();
        let run_logs = [
            "site-provision-teardown.test-0a1b2c3d.log",
            "wp-install-teardown.test-0a1b2c3d.log",
            "db-import-teardown.test-0a1b2c3d.log",
            "repo-teardown.test-my-plugin.log",
        ]
        .map(|n| log_dir.join(n));
        let neighbour_logs = [
            "site-provision-teardown.test-2.test-0a1b2c3d.log",
            "repo-teardown.test-2.test-my-plugin.log",
        ]
        .map(|n| log_dir.join(n));
        for p in run_logs.iter().chain(&neighbour_logs) {
            std::fs::write(p, "x").unwrap();
        }

        // A settled db-import fact rides the site row (its mirrored user is
        // dropped by the delete COMMAND before teardown, reading this record).
        store::upsert_db_import(
            &conn,
            &store::NewDbImport {
                site_id: site.id.clone(),
                db_name: "ea".into(),
                table_count: 1,
                size_bytes: 1,
                source_label: "src".into(),
                mirrored_user: Some("rex_teardown_test".into()),
                skipped_tables: Vec::new(),
            },
        )
        .unwrap();

        // And a rewrite record with a real backup file: both must go with the
        // site (D2's plain-delete leg — their config file is never touched).
        let backup_dir = std::env::temp_dir()
            .join(format!("rexenv-teardown-backup-{}", std::process::id()));
        std::fs::create_dir_all(&backup_dir).unwrap();
        let backup_file = backup_dir.join("wp-config.php");
        std::fs::write(&backup_file, "original").unwrap();
        store::insert_config_rewrite(
            &conn,
            &site.id,
            "/their/project/wp-config.php",
            &backup_file.display().to_string(),
        )
        .unwrap();

        assert!(teardown(&conn, &*platform, &site.id).unwrap().existed);
        assert!(get(&conn, &site.id).unwrap().is_none());
        assert!(
            store::get_db_import(&conn, &site.id).unwrap().is_none(),
            "db_imports row must not outlive its site"
        );
        assert!(
            store::config_rewrites_for_site(&conn, &site.id).unwrap().is_empty(),
            "config_rewrites rows must not outlive their site"
        );
        assert!(!backup_file.exists(), "the backup file must go with its row");
        for p in &artifacts {
            assert!(!p.exists(), "orphaned artifact left behind: {}", p.display());
        }
        for p in &run_logs {
            assert!(!p.exists(), "a run log outlived its site: {}", p.display());
        }
        for p in &neighbour_logs {
            assert!(p.exists(), "deleting a site took a living neighbour's log: {}", p.display());
            let _ = std::fs::remove_file(p);
        }
        // Deleting again is a no-op.
        assert!(!teardown(&conn, &*platform, &site.id).unwrap().existed);
        let _ = std::fs::remove_dir_all(&backup_dir);
    }

    /// A real temp directory with a file in it, plus a site row pointing at it.
    /// Scoped to THIS fixture — the tests below only ever delete what they made.
    fn docroot_fixture(tag: &str) -> (std::path::PathBuf, NewSite) {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-teardown-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "<?php // the user's file\n").unwrap();
        let mut new = sample("Fixture", &format!("{tag}.test"));
        new.path = dir.display().to_string();
        (dir, new)
    }

    /// Build a project tree from a list of files, each created with a parent.
    fn project(tag: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-detect-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        dir
    }

    #[test]
    fn muplugin_dir_removal_needs_the_record_and_tolerates_only_noise() {
        let root = std::env::temp_dir()
            .join(format!("rexenv-mudir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let docroot = root.join("site");
        let mu = docroot.join("wp-content/mu-plugins");
        std::fs::create_dir_all(&mu).unwrap();
        std::fs::write(mu.join("rexenv-tunnel.php"), "x").unwrap();
        std::fs::write(mu.join("rexenv-login.php"), "x").unwrap();
        std::fs::write(mu.join(".DS_Store"), "").unwrap();

        // RECORDED as ours → files removed, OS noise swept, dir gone.
        let mut site = site_at(&docroot);
        site.mu_dir_created = Some(true);
        cleanup_muplugin_artifacts(&site, true);
        assert!(!mu.exists(), "recorded dir must be removed once empty");

        // NOT recorded → files removed, the dir itself untouched (a user's
        // own dir is never ours to delete, however empty).
        std::fs::create_dir_all(&mu).unwrap();
        std::fs::write(mu.join("rexenv-login.php"), "x").unwrap();
        let mut site = site_at(&docroot);
        site.mu_dir_created = None;
        cleanup_muplugin_artifacts(&site, true);
        assert!(mu.exists() && std::fs::read_dir(&mu).unwrap().next().is_none());

        // Recorded but holding a REAL file → dir stays with its content.
        std::fs::write(mu.join("their-plugin.php"), "theirs").unwrap();
        let mut site = site_at(&docroot);
        site.mu_dir_created = Some(true);
        cleanup_muplugin_artifacts(&site, true);
        assert!(mu.join("their-plugin.php").exists(), "real content is never ours to judge");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn content_dir_rel_reads_layout_markers_and_resists_poison() {
        let bed = project(
            "cdir-bedrock",
            &["web/wp-config.php", "config/application.php", "web/app/mu-plugins/x.php"],
        );
        assert_eq!(detect_content_dir_rel(&bed.join("web")), "app");
        let _ = std::fs::remove_dir_all(&bed);

        let rad = project(
            "cdir-radicle",
            &["public/wp-config.php", "bedrock/application.php", "public/content/mu-plugins/x.php"],
        );
        assert_eq!(detect_content_dir_rel(&rad.join("public")), "content");
        let _ = std::fs::remove_dir_all(&rad);

        let plain = project("cdir-plain", &["wp-config.php", "wp-content/index.php"]);
        assert_eq!(detect_content_dir_rel(&plain), "wp-content");
        let _ = std::fs::remove_dir_all(&plain);

        // Poison resistance: a stray web/wp-content — the litter our own
        // pre-v24 bug wrote — must not flip a Bedrock repo's answer. This is
        // WHY the fact is recorded once instead of probed at write time.
        let poisoned = project(
            "cdir-poisoned",
            &[
                "web/wp-config.php",
                "config/application.php",
                "web/app/mu-plugins/x.php",
                "web/wp-content/mu-plugins/rexenv-login.php",
            ],
        );
        assert_eq!(detect_content_dir_rel(&poisoned.join("web")), "app");
        let _ = std::fs::remove_dir_all(&poisoned);
    }

    #[test]
    fn backfill_content_dir_records_wp_rows_and_skips_the_rest() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let bed = project(
            "cdir-backfill",
            &["web/wp-config.php", "config/application.php", "web/app/mu-plugins/x.php"],
        );
        let mut wp = site_at(&bed.join("web"));
        wp.content_dir = None; // a pre-v24 row
        store::insert_site(&conn, &wp).unwrap();
        let mut php = site_at(Path::new("/tmp/nonwp"));
        php.id = "m2".into();
        php.domain = "php.test".into();
        php.db_name = "wp_php_test".into();
        php.site_type = SiteType::Php;
        php.content_dir = None;
        store::insert_site(&conn, &php).unwrap();

        // A WP row whose docroot is UNREACHABLE (unmounted volume) must stay
        // NULL — a set-once "wp-content" here would be permanent poison (A3).
        let mut gone = site_at(Path::new("/nonexistent-volume/project/web"));
        gone.id = "m3".into();
        gone.domain = "gone.test".into();
        gone.db_name = "wp_gone_test".into();
        gone.content_dir = None;
        store::insert_site(&conn, &gone).unwrap();

        backfill_content_dir(&conn).unwrap();
        let rows = store::list_sites(&conn).unwrap();
        let by_id = |id: &str| rows.iter().find(|s| s.id == id).unwrap();
        assert_eq!(by_id("m1").content_dir.as_deref(), Some("app"));
        assert_eq!(by_id("m2").content_dir, None); // non-WP: never consulted
        assert_eq!(by_id("m3").content_dir, None); // unreachable: left for a later launch
        // Idempotent: a second run changes nothing.
        backfill_content_dir(&conn).unwrap();
        assert_eq!(by_id("m1").content_dir.as_deref(), Some("app"));
        let _ = std::fs::remove_dir_all(&bed);
    }

    #[test]
    fn detect_project_places_the_common_layouts_and_their_docroots() {
        // The docroot is NOT always the project root — getting this wrong
        // serves the framework's source instead of its front controller.
        let cases: &[(&str, &[&str], SiteType, &str, &str)] = &[
            ("wp", &["wp-config.php", "wp-load.php"], SiteType::Wordpress, "", "WordPress"),
            // Downloaded-but-unconfigured core still reads as WordPress.
            ("wpsample", &["wp-config-sample.php"], SiteType::Wordpress, "", "WordPress"),
            // Bedrock: wp-config.php exists but NOT at the served root.
            (
                "bedrock",
                &["web/wp-config.php", "config/application.php", "web/app/mu-plugins/x.php"],
                SiteType::Wordpress,
                "web",
                "WordPress (Bedrock)",
            ),
            ("laravel", &["artisan", "public/index.php"], SiteType::Laravel, "public", "Laravel"),
            ("craft", &["craft", "web/index.php"], SiteType::Php, "web", "Craft CMS"),
            ("symfony", &["bin/console", "public/index.php"], SiteType::Php, "public", "Symfony"),
            ("plain", &["index.php"], SiteType::Php, "", "PHP project"),
            ("static", &["index.html"], SiteType::Php, "", "Static site"),
            ("frontctl", &["public/index.php"], SiteType::Php, "public", "PHP project"),
        ];
        for (tag, files, site_type, docroot, label) in cases {
            let dir = project(tag, files);
            let d = detect_project(&dir);
            assert_eq!(d.site_type, *site_type, "{tag}: site type");
            assert_eq!(d.docroot_rel, *docroot, "{tag}: docroot");
            assert_eq!(d.label, *label, "{tag}: label");
            assert!(d.existing_install, "{tag}: should read as an existing install");
            let _ = std::fs::remove_dir_all(&dir);
        }

        // An empty folder is honestly "nothing to serve yet", not a guess.
        let empty = project("empty", &[]);
        let d = detect_project(&empty);
        assert_eq!(d.label, "Unknown");
        assert!(!d.existing_install, "an empty folder holds no install");
        let _ = std::fs::remove_dir_all(&empty);

        // A per-project custom Valet driver can serve a docroot our probes
        // can't predict — flagged, never silently guessed at.
        let custom = project("driver", &["LocalValetDriver.php", "index.php"]);
        assert!(has_custom_valet_driver(&custom));
        let _ = std::fs::remove_dir_all(&custom);
    }

    #[test]
    fn a_linked_site_refuses_to_be_moved_and_moving_out_gives_up_ownership() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();

        // Linked: refused outright. A cross-volume move COPIES then deletes the
        // source, so this would rewrite the user's own project layout.
        let (dir, new) = docroot_fixture("nomove");
        let linked = create_recording_ownership(&conn, new, false, Ownership::User, None).unwrap();
        let err = check_docroot_move(&linked, &std::env::temp_dir()).unwrap_err().to_string();
        assert!(err.contains("your own folder"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);

        // Owned, moved OUT of the sites folder: we give up the claim, so the
        // move dialog's "kept — not deleted" promise stays true structurally.
        let (outside, new2) = docroot_fixture("movedout");
        let mut owned = new2;
        owned.path = String::new();
        let site = create(&conn, owned).unwrap();
        assert_eq!(get(&conn, &site.id).unwrap().unwrap().docroot_managed, Some(true));
        let moved = set_path(&conn, &*platform, &site.id, &outside).unwrap().unwrap();
        assert_eq!(
            moved.docroot_managed,
            Some(false),
            "a docroot moved outside the sites folder is no longer ours to delete"
        );
        // ...and that survives a delete: the folder stays.
        let out = teardown(&conn, &*platform, &site.id).unwrap();
        assert!(!out.docroot_removed);
        assert!(outside.exists(), "the moved-out folder must survive deletion");
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn relinking_a_moved_folder_takes_only_a_real_directory() {
        let conn = db::open_in_memory().unwrap();

        let (dir, new) = docroot_fixture("relink");
        let linked = create_recording_ownership(&conn, new, false, Ownership::User, None).unwrap();

        // The whole point of the re-point path: a folder rexenv refuses to MOVE
        // is still one it will follow after the user moved it themselves.
        assert!(check_docroot_move(&linked, &std::env::temp_dir()).is_err());
        let (dest, _) = docroot_fixture("relink-dest");
        check_docroot_relink(&linked, &dest).expect("a real folder is accepted");

        // Every rejection, before anything is recorded.
        let missing = dest.join("no-such-folder");
        let err = check_docroot_relink(&linked, &missing).unwrap_err().to_string();
        assert!(err.contains("isn't a folder"), "{err}");
        let file = dest.join("index.php");
        std::fs::write(&file, "x").unwrap();
        assert!(check_docroot_relink(&linked, &file).is_err(), "a FILE is not a docroot");
        let same = check_docroot_relink(&linked, Path::new(&linked.path)).unwrap_err().to_string();
        assert!(same.contains("already points"), "{same}");
        assert!(check_docroot_relink(&linked, Path::new("relative/x")).is_err());
        // Unemittable in the generated configs — rejected at input, as on create.
        let bad = dest.join("we$ird");
        std::fs::create_dir_all(&bad).unwrap();
        assert!(check_docroot_relink(&linked, &bad).is_err(), "$ breaks the nginx config");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn validate_linked_docroot_accepts_a_project_and_refuses_the_blast_radius() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (dir, _) = docroot_fixture("validate");

        // Happy path: a real project folder, returned CANONICAL (on macOS the
        // temp dir is a symlink, so this is a real resolution, not a no-op).
        let canon = validate_linked_docroot(&conn, &*platform, &dir.display().to_string()).unwrap();
        assert_eq!(canon, dir.canonicalize().unwrap());
        assert!(canon.is_absolute());

        let refused = |p: &str| {
            validate_linked_docroot(&conn, &*platform, p)
                .expect_err(&format!("{p} must be refused"))
                .to_string()
        };
        // Shape.
        assert!(refused("relative/path").contains("absolute"));
        assert!(refused(&dir.join("nope").display().to_string()).contains("doesn't exist"));
        assert!(refused(&dir.join("index.php").display().to_string()).contains("isn't a folder"));
        // Blast radius — a docroot can be published with one click.
        let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
        for broad in [PathBuf::from("/"), home.clone(), home.join("Desktop")] {
            if broad.exists() {
                assert!(
                    refused(&broad.display().to_string()).contains("too broad"),
                    "{} must be refused as too broad",
                    broad.display()
                );
            }
        }
        // ...but a project INSIDE one of those is perfectly normal (real Valet
        // users keep sites in ~/Desktop), so only the folder itself is refused.
        assert!(validate_linked_docroot(&conn, &*platform, &dir.display().to_string()).is_ok());

        // **The spelling the caller uses must not decide whether the guard fires.** This is
        // the regression the Dell found: every right-hand side here comes from `BaseDirs` /
        // `ProjectDirs` / the sites-dir setting, none of which are canonical, while `canon`
        // is — and on Windows `canonicalize` adds a `\\?\` prefix, so the comparison was
        // false for the home folder ITSELF and the refusal never fired. macOS has the same
        // asymmetry in gentler form (`/tmp` -> `/private/tmp`), which is what this leg uses:
        // a non-canonical spelling of a blast-radius folder is still refused.
        let tmp_home = std::env::temp_dir();
        if tmp_home.canonicalize().map(|c| c != tmp_home).unwrap_or(false) {
            // `std::env::temp_dir()` is a symlinked spelling on macOS. Serving it is not a
            // blast-radius case, so assert the PROPERTY that broke instead: what comes back
            // is canonical whichever spelling went in.
            let a = validate_linked_docroot(&conn, &*platform, &dir.display().to_string()).unwrap();
            let b = validate_linked_docroot(
                &conn,
                &*platform,
                &dir.canonicalize().unwrap().display().to_string(),
            )
            .unwrap();
            assert_eq!(a, b, "the same folder, two spellings, must resolve identically");
        }

        // The os is a PARAMETER, so the Windows radius is assertable from here (#642).
        // Only what this host can honestly answer: the home folder is too broad under
        // BOTH rules, and `/Users` is a unix root that is not part of the Windows one.
        assert!(
            validate_linked_docroot_on(&conn, &*platform, &home.display().to_string(), "windows")
                .unwrap_err()
                .to_string()
                .contains("too broad"),
            "the home folder is too broad on every os"
        );
        assert!(
            validate_linked_docroot_on(&conn, &*platform, &dir.display().to_string(), "windows")
                .is_ok(),
            "a project folder stays linkable under the Windows radius"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_linked_docroot_refuses_overlap_with_an_existing_site() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (dir, _) = docroot_fixture("overlap");
        let canon = dir.canonicalize().unwrap();

        let mut existing = sample("Existing", "existing.test");
        existing.path = canon.display().to_string();
        create(&conn, existing).unwrap();

        // Exactly the same folder.
        let err = validate_linked_docroot(&conn, &*platform, &canon.display().to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("already served by existing.test"), "{err}");
        // A subfolder of it — would serve one site's files under another domain.
        let sub = canon.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let err = validate_linked_docroot(&conn, &*platform, &sub.display().to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("overlaps"), "{err}");
        // And the containing direction is refused too.
        let err = validate_linked_docroot(
            &conn,
            &*platform,
            &canon.parent().unwrap().display().to_string(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("overlaps") || err.contains("too broad"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Each validation sees the site created by the one before it** — which is
    /// what makes importing a batch safe, and what a hoisted site list would
    /// silently break.
    ///
    /// The Valet import applies its queue one site at a time, every one through
    /// `create` → `validate_linked_docroot`. Two scanned projects where one
    /// nests inside the other are refused only because the SECOND validation
    /// reads state the first one wrote. A list fetched once before the batch —
    /// which is exactly the "hoist if imports grow" optimisation `docs/TODO.md`
    /// recommended — cannot contain it, and both would be created: two sites
    /// serving one tree, one under the other's domain.
    ///
    /// `validate_linked_docroot_refuses_overlap_with_an_existing_site` proves
    /// the RULE against a site that already existed. This proves the FRESHNESS,
    /// which is a different property and the one an optimisation takes away.
    #[test]
    fn each_link_validation_sees_the_site_the_previous_one_created() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (root, _) = docroot_fixture("batch");
        let canon = root.canonicalize().unwrap();

        // Two sibling projects under one parent, the Valet layout: ~/Sites/a,
        // ~/Sites/b — neither overlaps the other, both fine.
        let a = canon.join("a");
        let b = canon.join("b");
        // …and a folder INSIDE the first, which is the pair that must be caught.
        let a_nested = a.join("nested");
        for d in [&a, &b, &a_nested] {
            std::fs::create_dir_all(d).unwrap();
        }

        // Nothing exists yet: every one of them validates clean.
        for p in [&a, &b, &a_nested] {
            assert!(
                validate_linked_docroot(&conn, &*platform, &p.display().to_string()).is_ok(),
                "{} should be linkable before anything is imported",
                p.display()
            );
        }

        // Import the first. Only now does the nested one become a conflict —
        // and that transition is the whole point: a list read before this line
        // would still say it is fine.
        let mut first = sample("A", "a.test");
        first.path = a.display().to_string();
        create(&conn, first).unwrap();

        let err = validate_linked_docroot(&conn, &*platform, &a_nested.display().to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("overlaps"), "nested under a just-created site: {err}");

        // The unrelated sibling is still fine, so the refusal above is the
        // overlap rule and not the validator having gone uniformly negative.
        let mut second = sample("B", "b.test");
        second.path = b.display().to_string();
        create(&conn, second).unwrap();

        // And the same again one level on: the SECOND site is visible to the
        // third validation. One transition could be a fluke of ordering.
        let b_nested = b.join("nested");
        std::fs::create_dir_all(&b_nested).unwrap();
        let err = validate_linked_docroot(&conn, &*platform, &b_nested.display().to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("overlaps"), "nested under the second site: {err}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn teardown_removes_a_docroot_we_own() {
        // The direction that had NO test at all: when rexenv created the folder,
        // deleting the site really does remove it.
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (dir, new) = docroot_fixture("owned");
        let site = create(&conn, new).unwrap(); // create() == we own it
        assert_eq!(get(&conn, &site.id).unwrap().unwrap().docroot_managed, Some(true));

        let out = teardown(&conn, &*platform, &site.id).unwrap();
        assert!(out.existed);
        assert!(out.docroot_removed, "an owned docroot must be removed");
        assert!(!dir.exists(), "the folder should be gone");
    }

    /// A worktree under `dir` whose repository is somewhere else entirely.
    fn plant_worktree(dir: &Path) -> PathBuf {
        let plugin = dir.join("wp-content/plugins/my-plugin");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join(".git"), "gitdir: /elsewhere/repo/.git/worktrees/my-plugin\n")
            .unwrap();
        std::fs::write(plugin.join("work.php"), "<?php // uncommitted").unwrap();
        plugin
    }

    /// **Teardown never `remove_dir_all`s a docroot that holds a git worktree,
    /// even one it owns** (ledger #814, second defence). Plant: drop
    /// `foreign.is_none() &&` from teardown's `docroot_removed` and the folder
    /// — with the worktree's uncommitted file — is gone.
    #[test]
    fn teardown_keeps_an_owned_docroot_that_holds_a_worktree() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (dir, new) = docroot_fixture("ownedwt");
        let site = create(&conn, new).unwrap();
        assert_eq!(site.docroot_managed, Some(true));
        let plugin = plant_worktree(&dir);

        let out = teardown(&conn, &*platform, &site.id).unwrap();
        assert!(out.existed);
        assert!(!out.docroot_removed, "a docroot holding a worktree must be kept");
        assert!(plugin.join("work.php").exists(), "the worktree's work must survive");
        assert!(get(&conn, &site.id).unwrap().is_none(), "the row still goes away");
        let _ = std::fs::remove_dir_all(&dir); // fixture cleanup, ours to remove
    }

    /// **A delete is refused BEFORE its first destructive step when the site is
    /// a worktree parent, or its owned docroot holds a worktree** (ledger #814,
    /// #815). Plants: comment out either `return Err` in `delete_preflight` and
    /// its assert below fails.
    #[test]
    fn delete_preflight_refuses_worktree_parents_and_docroots_holding_a_worktree() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();

        let (dir, new) = docroot_fixture("prefl");
        let site = create(&conn, new).unwrap();
        assert!(delete_preflight(&conn, &*platform, &site).is_ok(), "a plain site deletes");

        let plugin = plant_worktree(&dir);
        let err = delete_preflight(&conn, &*platform, &site).unwrap_err().to_string();
        assert!(err.contains("git worktree remove"), "names the fix: {err}");
        assert!(err.contains(&plugin.display().to_string()), "names the folder: {err}");
        std::fs::remove_dir_all(&plugin).unwrap();
        assert!(delete_preflight(&conn, &*platform, &site).is_ok());

        let (dir2, new2) = docroot_fixture("preflchild");
        let child = create(&conn, new2).unwrap();
        store::insert_site_worktree(
            &conn,
            &child.id,
            &site.id,
            crate::state::models::WorktreeShape::Site,
            &child.path,
            false,
        )
        .unwrap();
        let err = delete_preflight(&conn, &*platform, &site).unwrap_err().to_string();
        assert!(err.contains(&child.domain), "names the child: {err}");
        assert!(delete_preflight(&conn, &*platform, &child).is_ok(), "the child itself may go");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn a_linked_docroot_is_never_created_or_populated_by_us() {
        // Retry re-ensures prepare's artifacts, and for a Blank-PHP site that
        // means writing the starter index.php. For a LINKED site that folder is
        // the user's, so the retry path branches on `docroot_managed !=
        // Some(false)` — this pins the predicate it relies on.
        let conn = db::open_in_memory().unwrap();
        let (dir, new) = docroot_fixture("noretrywrite");
        let linked = create_recording_ownership(&conn, new, false, Ownership::User, None).unwrap();
        assert_eq!(linked.docroot_managed, Some(false), "linked rows must be recognisable");

        let (dir2, new2) = docroot_fixture("ourswrite");
        let mut owned = new2;
        owned.path = String::new();
        let ours = create(&conn, owned).unwrap();
        assert_eq!(ours.docroot_managed, Some(true), "our own rows stay writable");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn teardown_never_removes_a_linked_docroot() {
        // THE guard. A linked folder lives wherever the user keeps it and is
        // never deleted — and critically, this holds even when the sites-dir
        // setting is pointed straight at it, which is exactly the case the old
        // lexical prefix test got wrong.
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        let (dir, mut new) = docroot_fixture("linked");
        new.path = String::new();
        let site = create_recording_ownership(
            &conn,
            NewSite { path: dir.display().to_string(), ..new },
            false, // linked: we did not create this folder
            Ownership::User,
            None,
        )
        .unwrap();

        // The hostile setting: Sites folder now CONTAINS the linked project, so
        // the legacy prefix test would happily delete it.
        store::set_setting(&conn, SITES_DIR_KEY, &dir.display().to_string()).unwrap();

        let out = teardown(&conn, &*platform, &site.id).unwrap();
        assert!(out.existed);
        assert!(!out.docroot_removed, "a linked docroot must never be removed");
        assert!(dir.exists(), "the user's folder must survive");
        assert!(dir.join("index.php").exists(), "and so must their files");
        assert!(get(&conn, &site.id).unwrap().is_none(), "the row still goes away");

        let _ = std::fs::remove_dir_all(&dir); // fixture cleanup, ours to remove
    }

    #[test]
    fn setting_round_trips_and_upserts() {
        let conn = db::open_in_memory().unwrap();
        assert!(store::get_setting(&conn, "k").unwrap().is_none());
        store::set_setting(&conn, "k", "v").unwrap();
        assert_eq!(store::get_setting(&conn, "k").unwrap().as_deref(), Some("v"));
        store::set_setting(&conn, "k", "v2").unwrap();
        assert_eq!(store::get_setting(&conn, "k").unwrap().as_deref(), Some("v2"));
    }

    /// **A fixture that sandboxes its PATHS and not its SITES FOLDER is refused,
    /// rather than quietly filling the user's real one.**
    ///
    /// `sites_dir` is a SETTING whose fallback is derived from the home
    /// directory, so no sandboxed `Paths` can redirect it — an example that
    /// forgets `common::pin_fixture_sites_dir` provisions into `~/rexenv/Sites`
    /// and leaves docroots behind. Measured 24 Aug 2026 on the machine this was
    /// written on: 19 orphaned directories, 437 MB, five of them whole WordPress
    /// installs, from runs that predate the pinning sweep.
    ///
    /// The sweep pinned all 17 remaining provisioners, and this is the half a
    /// sweep cannot give: the eighteenth example, written next month, cannot
    /// forget. A check the fixture does not have to remember to write.
    /// Examples that provision WITHOUT pinning `sites_dir`, each with the reason
    /// it is safe. An allow-list, not a convention: the list is what a new
    /// example has to argue its way onto.
    ///
    /// Everything else must call `common::sandbox_db` (pin included, cannot be
    /// used without it) or `common::pin_sites_dir` / `pin_fixture_sites_dir`.
    const UNPINNED_PROVISIONERS: &[(&str, &str)] = &[
        (
            "sites_folder_check",
            "its SUBJECT is a custom sites_dir: it sets SITES_DIR_KEY to a temp path by              hand and asserts the docroot lands there. Pinning would remove the thing it              checks — the same exception `download_progress_check` has for the shared              binary cache.",
        ),
        (
            "seed_and_list",
            "a seeding DEMO, not a check: its stated purpose is to put real sites in the              real app DB so the desktop app's Sites screen shows them. Writing to the              user's folder is what it is for.",
        ),
    ];

    /// **Every example that provisions either PINS `sites_dir` or is on the
    /// exception list with a reason.**
    ///
    /// `sites::provision` reads the `sites_dir` SETTING, whose fallback is
    /// derived from the HOME directory — a path no sandboxed `Platform` can
    /// redirect. So an example that forgets the pin creates docroots in the
    /// user's real `~/rexenv/Sites`, and one of them used to `remove_dir_all`
    /// there. **Measured on this machine 24 Aug 2026: 19 orphaned directories,
    /// 437 MB**, five of them whole WordPress installs.
    ///
    /// `refuse_unpinned_sandbox_sites_dir_for` (#392) catches the SANDBOXED
    /// case at runtime. This catches the rest, and it catches them at build
    /// time: a sweep pins the examples that exist, and this is the half a sweep
    /// cannot give — the next example, written next month, cannot forget.
    ///
    /// The same shape as the service-tier guard (#410), and for the same reason:
    /// 20 of 24 examples there had no guard because remembering per file does
    /// not work.
    #[test]
    fn every_provisioning_example_pins_the_sites_dir_or_says_why_not() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut unpinned = Vec::new();
        let mut seen = 0usize;
        for entry in std::fs::read_dir(&dir).expect("examples dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else { continue };
            if !src.contains("sites::provision") {
                continue;
            }
            seen += 1;
            let pinned = src.contains("sandbox_db")
                || src.contains("pin_sites_dir")
                || src.contains("pin_fixture_sites_dir");
            if !pinned {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                if !UNPINNED_PROVISIONERS.iter().any(|(n, _)| *n == name) {
                    unpinned.push(name);
                }
            }
        }
        assert!(
            seen >= 30,
            "only {seen} provisioning examples found — the scan matched almost nothing and \
             would report a clean tree either way"
        );
        assert!(
            unpinned.is_empty(),
            "these examples provision without pinning `sites_dir`: {unpinned:?}\n  \
             They will create docroots in the USER's real ~/rexenv/Sites, and anything they \
             delete afterwards deletes there.\n  \
             Use `common::sandbox_db` (pin included) or `common::pin_fixture_sites_dir` — \
             or add the example to UNPINNED_PROVISIONERS with the reason it is safe."
        );
        // The exception list may only SHRINK by being right: an entry naming an
        // example that no longer provisions is a reason nobody is checking.
        for (name, _) in UNPINNED_PROVISIONERS {
            let path = dir.join(format!("{name}.rs"));
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("UNPINNED_PROVISIONERS names {name}, which is gone"));
            assert!(
                src.contains("sites::provision"),
                "UNPINNED_PROVISIONERS still excuses {name}, which no longer provisions — \
                 delete the entry rather than leaving a standing exception nobody needs"
            );
        }
    }

    #[test]
    fn a_sandboxed_platform_may_not_provision_into_the_users_real_sites_folder() {
        let real = crate::platform::current();
        let home = directories::BaseDirs::new().expect("home").home_dir().to_path_buf();
        let default = default_sites_dir().expect("default sites dir");

        // A REAL platform is never refused, whatever the sites dir — this must
        // not fire in production, which is the whole risk of adding it.
        assert!(
            real.paths().app_data_dir().is_ok_and(|d| d.starts_with(&home)),
            "a real platform's app data must live under the home directory, or the \
             signal this guard reads is the wrong one"
        );
        assert!(refuse_unpinned_sandbox_sites_dir(&default, &*real).is_ok());
        assert!(refuse_unpinned_sandbox_sites_dir(Path::new("/private/tmp/x"), &*real).is_ok());

        // A sandboxed platform's app data lives OUTSIDE the home directory.
        let sandbox = Path::new("/private/tmp/rexenv-sandbox-x-1");
        let tmp = std::env::temp_dir().join(format!("rexenv-guard-{}", std::process::id()));

        // …provisioning into the DEFAULT is the mistake, and it is named.
        let err = refuse_unpinned_sandbox_sites_dir_for(&default, sandbox)
            .expect_err("an unpinned sandbox must be refused")
            .to_string();
        assert!(err.contains("pin_fixture_sites_dir"), "no fix offered: {err}");
        assert!(err.contains("sandboxed"), "{err}");
        // …and a PINNED one is fine, which is what keeps this a guard and not a ban.
        assert!(refuse_unpinned_sandbox_sites_dir_for(&tmp, sandbox).is_ok());
    }

    #[test]
    fn sites_dir_uses_setting_or_default() {
        let conn = db::open_in_memory().unwrap();
        let platform = crate::platform::current();
        // Default is ~/rexenv/Sites (exact casing).
        assert!(sites_dir(&conn, &*platform).unwrap().ends_with("rexenv/Sites"));
        // A configured value overrides it.
        store::set_setting(&conn, SITES_DIR_KEY, "/tmp/custom-sites").unwrap();
        assert_eq!(
            sites_dir(&conn, &*platform).unwrap(),
            PathBuf::from("/tmp/custom-sites")
        );
    }

    /// **The switch refuses a half-built site, and is idempotent otherwise.**
    ///
    /// "Setup incomplete" and "stopped" look alike on a list and are not the
    /// same state: a site whose provisioning died has no serving surface to take
    /// away, and Start on it would promise something the row cannot deliver.
    /// Retry is that site's verb — so the refusal names it rather than failing
    /// with a generic error.
    #[test]
    fn set_enabled_refuses_a_site_that_never_finished_setting_up() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Half", "half.test")).unwrap();
        store::set_site_provisioned(&conn, &site.id, false).unwrap();

        let err = set_enabled(&conn, &site.id, false).unwrap_err().to_string();
        assert!(err.contains("Retry"), "the refusal must name the verb that DOES apply: {err}");
        assert!(err.contains("half.test"), "the refusal must name the site: {err}");
        assert!(
            get(&conn, &site.id).unwrap().unwrap().enabled,
            "a refused stop must not have written the switch anyway"
        );

        // A finished site stops, stays stopped when asked twice (the UI can
        // fire twice; the second must not be an error), and starts again.
        store::set_site_provisioned(&conn, &site.id, true).unwrap();
        assert!(!set_enabled(&conn, &site.id, false).unwrap().unwrap().enabled);
        assert!(!set_enabled(&conn, &site.id, false).unwrap().unwrap().enabled);
        assert!(set_enabled(&conn, &site.id, true).unwrap().unwrap().enabled);

        // An id that is not a site is `None`, not an error — the caller says
        // "no such site" once, in its own words.
        assert!(set_enabled(&conn, "no-such-site", false).unwrap().is_none());
    }

    /// **A stopped site gets no SERVING block — and is given a stopped one
    /// instead, never nothing.**
    ///
    /// This predicate answers only the first half: does this site get the
    /// ordinary `server_name` + `fastcgi_pass` block? A stopped site does not,
    /// for the same reason an override site does not — nothing here serves it.
    ///
    /// The second half is not optional, and is asserted in
    /// `services::a_stopped_site_answers_for_itself_instead_of_falling_through_to_a_neighbour`:
    /// **a name with no block at all is answered by nginx's DEFAULT server,
    /// which is another site.** For one day this code emitted nothing for a
    /// stopped site, and a public tunnel — which proxies straight to the shared
    /// nginx, bypassing the edge — published a neighbour's site at the stopped
    /// site's address. So "absent from the serving list" must always be paired
    /// with "present in the stopped list".
    #[test]
    fn a_stopped_site_gets_no_serving_block_whichever_server_it_uses() {
        let conn = db::open_in_memory().unwrap();
        let mut new_ng = sample("NG", "ng.test");
        new_ng.web_server = WebServer::Nginx;
        let mut new_fp = sample("FP", "fp.test");
        new_fp.web_server = WebServer::Frankenphp;
        // Both `_on("macos")`: the pair is the point, and one of them is FrankenPHP.
        let mut ng = create_on(&conn, new_ng, "macos").unwrap();
        let mut fp = create_on(&conn, new_fp, "macos").unwrap();
        assert!(ng.enabled && fp.enabled, "a site is served the moment it is created");

        assert!(gets_nginx_block(&ng), "a served nginx site has a block");
        assert!(!gets_nginx_block(&fp), "an override site never had one");

        ng.enabled = false;
        fp.enabled = false;
        assert!(!gets_nginx_block(&ng), "the user stopped this site — it must not be served");
        assert!(!gets_nginx_block(&fp));

        // And starting it again is exactly the flag going back: nothing else on
        // the row participates in the decision.
        ng.enabled = true;
        assert!(gets_nginx_block(&ng));
    }

    #[test]
    fn override_sites_route_to_backend_and_skip_nginx() {
        let conn = db::open_in_memory().unwrap();
        let mut ng = sample("NG", "ng.test");
        ng.site_type = SiteType::Php;
        ng.web_server = WebServer::Nginx;
        let mut fp = sample("FP", "fp.test");
        fp.site_type = SiteType::Php;
        fp.web_server = WebServer::Frankenphp;
        let ng = create_on(&conn, ng, "macos").unwrap();
        let fp = create_on(&conn, fp, "macos").unwrap();

        // Nginx serves only the nginx site; the FrankenPHP site is excluded.
        assert!(is_nginx_served(&ng));
        assert!(!is_nginx_served(&fp));

        // Edge upstream: nginx site → shared nginx port; FrankenPHP site → its
        // RECORDED backend port (allocated at create, B20 §4 — not re-derived).
        assert_eq!(site_upstream(&ng, 18088), "127.0.0.1:18088");
        let fp_port = fp.override_port.expect("frankenphp site has a recorded port");
        assert_eq!(site_upstream(&fp, 18088), format!("127.0.0.1:{fp_port}"));
        // The recorded port is in the FrankenPHP override range, not the nginx port.
        assert!(
            (frankenphp::FRANKENPHP_BASE_PORT..frankenphp::FRANKENPHP_BASE_PORT + 100)
                .contains(&fp_port)
        );
        assert_ne!(fp_port, 18088);
    }

    #[test]
    fn set_php_version_updates_only_the_column() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("Sw", "sw.test");
        new.php_version = "8.1".into();
        let site = create(&conn, new).unwrap();

        // Switch 8.1 → 8.3: only php_version changes; path/domain/type untouched.
        let updated = set_php_version(&conn, &site.id, "8.3").unwrap().expect("exists");
        assert_eq!(updated.php_version, "8.3");
        assert_eq!(updated.path, site.path);
        assert_eq!(updated.domain, site.domain);
        assert_eq!(updated.id, site.id);

        // A patch-form value normalizes to its minor.
        let u2 = set_php_version(&conn, &site.id, "8.2.31").unwrap().unwrap();
        assert_eq!(u2.php_version, "8.2");

        // Unsupported version is rejected; unknown id is a no-op (None).
        assert!(set_php_version(&conn, &site.id, php::unshipped_minor()).is_err());
        assert!(set_php_version(&conn, "nope", "8.3").unwrap().is_none());
    }

    /// FrankenPHP serves a site with the PHP compiled into FrankenPHP, so a
    /// 7.4 site on FrankenPHP would silently run 8.5 — a different language, not
    /// a version skew. All THREE ways to form that pair are refused; covering
    /// only create and server-switch would leave the php-switch door open, which
    /// is the shape of guard this repo keeps getting bitten by.
    #[test]
    fn frankenphp_refuses_a_php_major_it_cannot_run() {
        let conn = db::open_in_memory().unwrap();

        // (1) create
        let mut n = sample("FP", "fp.test");
        n.web_server = WebServer::Frankenphp;
        n.php_version = "7.4".into();
        // `_on("macos")` so the refusal under test is the PHP-version one. On Windows the
        // server gate refuses first, with a different sentence — a pass for the wrong
        // reason (W12).
        let err = create_on(&conn, n, "macos").unwrap_err().to_string();
        assert!(err.contains("FrankenPHP embeds its own PHP"), "{err}");
        assert!(err.contains("7.4") && err.contains("8.5"), "names both: {err}");

        // (2) switch the SERVER of an existing 7.4 site
        let mut a = sample("A", "a.test");
        a.php_version = "7.4".into();
        let a = create(&conn, a).unwrap();
        assert!(set_web_server(&conn, &a.id, WebServer::Frankenphp).is_err());
        // …and the row is untouched by the refusal.
        assert_eq!(get(&conn, &a.id).unwrap().unwrap().web_server, WebServer::Nginx);

        // (3) switch the PHP of an existing FrankenPHP site
        let mut b = sample("B", "b.test");
        b.web_server = WebServer::Frankenphp;
        b.php_version = "8.5".into();
        let b = create_on(&conn, b, "macos").unwrap();
        assert!(set_php_version(&conn, &b.id, "7.4").is_err());
        assert_eq!(get(&conn, &b.id).unwrap().unwrap().php_version, "8.5");

        // The SAME-major mismatch is deliberately allowed — it predates this and
        // refusing it would break FrankenPHP sites that work today.
        assert!(set_php_version(&conn, &b.id, "8.1").is_ok());
    }

    #[test]
    fn set_web_server_updates_only_the_column() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("SW", "sws.test");
        new.web_server = WebServer::Nginx;
        let site = create(&conn, new).unwrap();

        // Nginx → FrankenPHP → Nginx: only web_server changes; path/domain untouched.
        let fp = set_web_server_on(&conn, &site.id, WebServer::Frankenphp, "macos")
            .unwrap()
            .expect("exists");
        assert!(matches!(fp.web_server, WebServer::Frankenphp));
        assert_eq!(fp.path, site.path);
        let back =
            set_web_server_on(&conn, &site.id, WebServer::Nginx, "macos").unwrap().unwrap();
        assert!(matches!(back.web_server, WebServer::Nginx));

        // Apache and OpenLiteSpeed are real backends; unknown id → None.
        // `_on("macos")` for the same reason the FrankenPHP legs above name it: Apache has
        // no Windows pin (D4), so on the Dell the switch is refused by the OS gate before
        // the column behaviour under test is reached (W12).
        let ap = set_web_server_on(&conn, &site.id, WebServer::Apache, "macos")
            .unwrap()
            .expect("exists");
        assert!(matches!(ap.web_server, WebServer::Apache));
        set_web_server_on(&conn, &site.id, WebServer::Nginx, "macos").unwrap();
        // OpenLiteSpeed: its own port range, and a switch to it refuses an env value it cannot
        // write (both quote characters — ledger #775) BEFORE the column changes.
        let ols = set_web_server_on(&conn, &site.id, WebServer::Openlitespeed, "macos")
            .unwrap()
            .expect("exists");
        assert!(matches!(ols.web_server, WebServer::Openlitespeed));
        let port = ols.override_port.expect("a recorded port");
        assert!((8400..8500).contains(&port), "{port}");
        assert!(!is_nginx_served(&ols), "an OLS site must never get a shared-nginx block");
        set_web_server_on(&conn, &site.id, WebServer::Nginx, "macos").unwrap();
        store::replace_site_env(&conn, &site.id, &[("Q".into(), "it's \"both\"".into())]).unwrap();
        let e = set_web_server_on(&conn, &site.id, WebServer::Openlitespeed, "macos")
            .expect_err("a value with both quotes cannot be written for OLS");
        assert!(e.to_string().contains("both"), "{e}");
        assert!(matches!(get(&conn, &site.id).unwrap().unwrap().web_server, WebServer::Nginx));
        assert!(set_web_server_on(&conn, "nope", WebServer::Nginx, "macos").unwrap().is_none());

        // The checks the switch COMMAND asks before it fetches anything (#786): a server with
        // no build for the OS answers with the OS's refusal sentence, not a download failure.
        let e = check_switch_on(&conn, &site.id, WebServer::Openlitespeed, "windows")
            .expect_err("OpenLiteSpeed has no Windows build");
        assert_eq!(e.to_string(), crate::platform::words::for_os("windows").openlitespeed_unavailable);
        assert!(check_switch_on(&conn, &site.id, WebServer::Apache, "linux").is_err());
        assert!(check_switch_on(&conn, &site.id, WebServer::Nginx, "windows").is_ok());
    }

    #[test]
    fn nginx_site_maps_php_version_to_its_pool_port() {
        let conn = db::open_in_memory().unwrap();
        let mut a = sample("A", "a.test");
        a.site_type = SiteType::Php;
        a.php_version = "8.1".into();
        let mut b = sample("B", "b.test");
        b.site_type = SiteType::Php;
        b.php_version = "8.3".into();
        let a = create(&conn, a).unwrap();
        let b = create(&conn, b).unwrap();

        let no_limits = std::collections::HashMap::new();
        let no_env = std::collections::HashMap::new();
        let na = nginx_site_for(&a, &no_limits, &no_env, &Default::default());
        let nb = nginx_site_for(&b, &no_limits, &no_env, &Default::default());
        // Each site routes to its own version's pool port — not a hardcoded one.
        assert_eq!(na.php_fpm_port, php::fpm_port("8.1").unwrap()); // 9781
        assert_eq!(nb.php_fpm_port, php::fpm_port("8.3").unwrap()); // 9783
        assert_ne!(na.php_fpm_port, nb.php_fpm_port);

        // A patch-form version still resolves to its minor's pool.
        let mut c = sample("C", "c.test");
        c.php_version = "8.2.31".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &no_limits, &no_env, &Default::default()).php_fpm_port, php::fpm_port("8.2").unwrap());

        // An unknown version falls back to the default pool.
        let mut d = sample("D", "d.test");
        d.php_version = php::unshipped_minor().into();
        let d = create(&conn, d).unwrap();
        assert_eq!(nginx_site_for(&d, &no_limits, &no_env, &Default::default()).php_fpm_port, services::PHP_FPM_PORT);
    }

    #[test]
    fn nginx_site_body_limit_follows_its_versions_settings() {
        let conn = db::open_in_memory().unwrap();
        let mut a = sample("A", "a.test");
        a.php_version = "8.3".into();
        let a = create(&conn, a).unwrap();
        let limits: std::collections::HashMap<String, u64> =
            [("8.3".to_string(), 64u64 << 20)].into();
        let no_env = std::collections::HashMap::new();
        // The site's minor has a limit → per-server client_max_body_size in bytes.
        assert_eq!(nginx_site_for(&a, &limits, &no_env, &Default::default()).body_limit, Some(64 << 20));
        // A patch-form version maps through its minor; an uncovered minor gets none.
        let mut b = sample("B", "b.test");
        b.php_version = "8.3.31".into();
        let b = create(&conn, b).unwrap();
        assert_eq!(nginx_site_for(&b, &limits, &no_env, &Default::default()).body_limit, Some(64 << 20));
        let mut c = sample("C", "c.test");
        c.php_version = "8.1".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &limits, &no_env, &Default::default()).body_limit, None);
    }

    #[test]
    fn needs_database_by_type() {
        assert!(!needs_database(SiteType::Php));
        assert!(needs_database(SiteType::Wordpress));
        assert!(needs_database(SiteType::Laravel));
    }

    #[test]
    fn enum_values_persist_as_canonical_strings() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("LO", "lo.test");
        new.site_type = SiteType::Php;
        new.web_server = WebServer::Apache;
        // Apache ships on macOS only (D4 left Apache Lounge out of the Windows v1), and
        // this test is about how the enum PERSISTS, not about who ships it.
        create_on(&conn, new, "macos").unwrap();

        // An unavailable server is refused at CREATE too (core guard, M7) —
        // not just at switch time: OpenLiteSpeed on Windows.
        let mut ols = sample("OLS", "ols.test");
        ols.web_server = WebServer::Openlitespeed;
        assert!(create_on(&conn, ols, "windows").is_err());

        // Read the raw TEXT to confirm DB storage matches the wire format.
        let (t, ws): (String, String) = conn
            .query_row(
                "SELECT type, web_server FROM sites WHERE domain='lo.test'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(t, "php");
        assert_eq!(ws, "apache");
    }

    /// **A site an agent makes FOR the user is the user's — no badge, no clock,
    /// never reaped — and it never prompts.** The third ownership value (MCP
    /// parity): recorded exactly as `User` at the insert, because the grant was
    /// the asking; refused the resolver prompt exactly as `Agent`, because a
    /// grant is not an administrator password. Cloning is NOT refused for it
    /// here — that is the `run` scope's decision in the tool layer.
    #[test]
    fn a_site_an_agent_makes_for_the_user_is_the_users_and_never_prompts() {
        use crate::state::models::SiteOrigin;
        let conn = db::open_in_memory().unwrap();
        let (dir, new) = docroot_fixture("byagent");
        let ownership = Ownership::UserByAgent { client: "claude-code".into() };
        assert_eq!(ownership.resolver_prompt(), crate::core::dns::ResolverPrompt::Never);
        let site = create_recording_ownership(
            &conn,
            NewSite { domain: "byagent.rex".into(), ..new.clone() },
            true,
            ownership.clone(),
            None,
        )
        .unwrap();
        assert_eq!(site.origin, SiteOrigin::User);
        assert!(!site.is_scratch());
        assert_eq!(site.agent_client, None, "a user's site never wears the scratch badge");
        assert_eq!(site.expires_at, None, "…and never has a clock");
        let read = store::get_site(&conn, &site.id).unwrap().unwrap();
        assert!(!read.is_scratch() && read.expires_at.is_none());
        // Git is a `run`-scope question, not an ownership one: the shape check
        // passes for this value where it refuses the scratch one.
        let git = NewSite { git_url: "https://github.com/octocat/Hello-World.git".into(), path: String::new(), ..new };
        assert!(validate_git_source(&git, &ownership).is_ok());
        assert!(validate_git_source(&git, &Ownership::Agent { client: "x".into(), ttl_hours: 1 }).is_err());
        let _ = dir;
    }

    #[test]
    fn an_agent_create_records_origin_client_and_a_ttl_at_the_insert() {
        // Ownership is recorded where the row is BORN, from the same value that
        // decided the prompt policy — so "this site is the agent's" and "this
        // create may not prompt" cannot disagree. And the clock starts here, at
        // the insert, so a create that fails LATER still expires and still gets
        // reaped: a half-built scratch site is exactly the kind that would
        // otherwise linger forever.
        use crate::state::models::SiteOrigin;
        let conn = db::open_in_memory().unwrap();
        let (dir, new) = docroot_fixture("scratchborn");
        let site = create_recording_ownership(
            &conn,
            NewSite { domain: "probe.scratch.rex".into(), ..new },
            true,
            Ownership::Agent { client: "Claude Code".into(), ttl_hours: 24 },
            None,
        )
        .unwrap();
        assert_eq!(site.origin, SiteOrigin::Agent);
        assert!(site.is_scratch());
        assert_eq!(site.agent_client.as_deref(), Some("Claude Code"));
        let expiry = site.expires_at.clone().expect("a scratch site expires");
        let now = store::db_now(&conn).unwrap();
        assert!(expiry > now, "the TTL is in the future: {expiry} vs {now}");
        // ...and it round-trips through the row, not just the returned struct.
        let read = store::get_site(&conn, &site.id).unwrap().unwrap();
        assert!(read.is_scratch() && read.expires_at == site.expires_at);
        // Same fixture, user ownership: no client, no clock, never reapable.
        let (dir2, new2) = docroot_fixture("userborn");
        let mine = create_recording_ownership(&conn, new2, true, Ownership::User, None).unwrap();
        assert_eq!(mine.origin, SiteOrigin::User);
        assert_eq!(mine.agent_client, None);
        assert_eq!(mine.expires_at, None);
        assert!(!mine.reap_due("2099-01-01 00:00:00"));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn a_scratch_name_is_a_single_label_and_the_refusal_shows_the_shape() {
        use crate::core::scratch::scratch_domain;
        assert_eq!(scratch_domain("plugin-test", "rex").unwrap(), "plugin-test.scratch.rex");
        assert_eq!(scratch_domain("  probe  ", "rex").unwrap(), "probe.scratch.rex");
        // An agent must not be able to nest a namespace or squat `scratch.rex`.
        let err = scratch_domain("a.b", "rex").unwrap_err().to_string();
        assert!(err.contains("single word"), "names the rule: {err}");
        assert!(err.contains("plugin-test.scratch.rex"), "SHOWS the shape wanted: {err}");
        assert!(scratch_domain("", "rex").is_err());
        assert!(scratch_domain("-lead", "rex").is_err());
        assert!(scratch_domain("trail-", "rex").is_err());
        // And the full domain still passes the ordinary validator.
        validate_domain(&scratch_domain("probe", "rex").unwrap()).unwrap();
    }

    #[test]
    fn the_cap_refusal_names_the_sites_and_two_ways_forward() {
        // A refusal that states the rule and stops is where a model starts
        // improvising — a different name, then another, then something else
        // entirely. So the ceiling has to hand it the actions that exist.
        use crate::core::scratch::{ensure_capacity, MAX_SCRATCH_SITES};
        use crate::state::models::{test_site, SiteOrigin};
        let conn = db::open_in_memory().unwrap();
        // The user's own sites do NOT count toward the agent's ceiling.
        for i in 0..8 {
            let s = test_site(&format!("u{i}-0000-4000-8000-00000000000{i}"), &format!("mine{i}.rex"), SiteOrigin::User);
            store::insert_site(&conn, &s).unwrap();
        }
        ensure_capacity(&conn).expect("the user's sites are not the agent's quota");

        for i in 0..MAX_SCRATCH_SITES {
            let mut s = test_site(
                &format!("a{i}-0000-4000-8000-00000000000{i}"),
                &format!("probe{i}.scratch.rex"),
                SiteOrigin::Agent,
            );
            s.expires_at = Some("2099-01-01 00:00:00".into());
            store::insert_site(&conn, &s).unwrap();
            if i + 1 < MAX_SCRATCH_SITES {
                ensure_capacity(&conn).expect("below the cap");
            }
        }
        let err = ensure_capacity(&conn).unwrap_err().to_string();
        assert!(err.contains(&MAX_SCRATCH_SITES.to_string()), "names the limit: {err}");
        assert!(err.contains("probe0.scratch.rex") && err.contains("probe4.scratch.rex"),
                "lists what it can delete: {err}");
        assert!(!err.contains("mine0.rex"), "never offers the USER's sites for deletion: {err}");
        assert!(err.contains("scratch_delete_site"), "way forward #1 — delete one: {err}");
        assert!(err.contains("ask the person"), "way forward #2 — ask the user: {err}");
        // Keep frees a slot immediately (the test below proves it), so the
        // ceiling names it: it turns a wait into something the human can DO.
        assert!(err.contains("frees a slot"), "way forward #3 — Keep, now that it is true: {err}");
        assert!(err.contains("expire"), "and the passive one: {err}");
    }


    #[test]
    fn keep_is_one_write_and_frees_a_slot_for_the_agent_immediately() {
        // Keep expresses ONE decision — "not disposable any more" — through two
        // columns, so it is one atomic statement rather than two writes that
        // must agree. And the cap counts scratch sites, so keeping one at the
        // ceiling frees a slot straight away: Keep is a pressure valve, not a
        // trap that leaves the agent stuck.
        use crate::core::scratch::{ensure_capacity, MAX_SCRATCH_SITES};
        use crate::state::models::{test_site, SiteOrigin};
        let conn = db::open_in_memory().unwrap();
        for i in 0..MAX_SCRATCH_SITES {
            let mut s = test_site(
                &format!("a{i}-0000-4000-8000-00000000000{i}"),
                &format!("probe{i}.scratch.rex"),
                SiteOrigin::Agent,
            );
            s.expires_at = Some("2099-01-01 00:00:00".into());
            store::insert_site(&conn, &s).unwrap();
        }
        assert!(ensure_capacity(&conn).is_err(), "at the ceiling");

        let kept = "a0-0000-4000-8000-000000000000";
        assert!(store::keep_site(&conn, kept).unwrap());
        let row = store::get_site(&conn, kept).unwrap().unwrap();
        assert_eq!(row.origin, SiteOrigin::User, "it is the user's now");
        assert_eq!(row.expires_at, None, "and carries no clock");
        assert!(!row.reap_due("2099-01-01 00:00:00"), "so the reaper can never take it");
        ensure_capacity(&conn).expect("keeping one frees a slot immediately");

        // Idempotent, and it never reaches a site that is already the user's.
        assert!(!store::keep_site(&conn, kept).unwrap(), "keeping twice changes nothing");
        assert!(!store::keep_site(&conn, "00000000-0000-4000-8000-000000000000").unwrap());
    }

    /// **A starter database is refused exactly where `create` would DROP it.**
    ///
    /// `create` records the field through
    /// `(site_type == Php && docroot_managed && git.is_none()).then_some(...)`.
    /// Every other shape becomes `None` with no error — so a caller who asks
    /// gets a site that silently is not what they asked for. The refusal has to
    /// cover the same set: narrower and the silence comes back on the shapes it
    /// missed, wider and it refuses a site that would have been seeded fine.
    #[test]
    fn a_starter_database_is_refused_on_exactly_the_shapes_create_would_drop() {
        for site_type in [SiteType::Php, SiteType::Wordpress] {
            for path in ["", "/Users/me/existing"] {
                for cloning in [false, true] {
                    let would_record =
                        site_type == SiteType::Php && path.is_empty() && !cloning;
                    let refusal = starter_db_refusal(site_type, path, cloning);
                    assert_eq!(
                        refusal.is_none(),
                        would_record,
                        "type={site_type:?} path={path:?} cloning={cloning} — the refusal and \
                         the `.then_some` in `create` disagree, so this shape either loses the \
                         field in silence or is refused when it would have worked"
                    );
                    if let Some(why) = refusal {
                        assert!(
                            why.contains("--starter-db"),
                            "a refusal that does not name the flag leaves the user guessing \
                             which argument to drop: {why}"
                        );
                    }
                }
            }
        }
    }

}
