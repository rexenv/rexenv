//! core::sites — site lifecycle domain logic (PLATFORM-AGNOSTIC).
//!
//! Phase 1 task 1.2: create / list / get / delete persisted via `state::store`.
//! Later tasks extend `create` to also generate the docroot, cert, vhost, and
//! edge-router route (§7); this module stays the single entry point for site
//! operations so commands/ remain thin.

use crate::core::{adminer, frankenphp, php, proxy, services, ssl, tld, tunnels};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{MultisiteMode, NewSite, ServiceStatus, Site, SiteType, WebServer};
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
fn validate_domain(domain: &str) -> Result<()> {
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

/// Servers with a real backend on this platform. OpenLiteSpeed is BLOCKED on
/// external work (no upstream macOS binary, no homebrew-core bottle; a
/// maintainer self-build + self-host is the only path — see docs/TODO.md) —
/// refused here in CORE, so no IPC path can create a site the stack would
/// silently serve through nginx while claiming another server (M7).
fn ensure_server_available(server: WebServer) -> Result<()> {
    if matches!(server, WebServer::Nginx | WebServer::Frankenphp | WebServer::Apache) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "web server {} is not available on this platform yet",
            server.as_db()
        )))
    }
}

/// The per-site override backend port for `server`, or `None` for nginx (which
/// has no per-site port — it vhosts by `server_name` on the shared stack).
/// FrankenPHP/Apache hash the domain into a small loopback range, so two domains
/// of the same server type can land on the same port.
fn override_port(domain: &str, server: WebServer) -> Option<u16> {
    match server {
        WebServer::Frankenphp => Some(super::frankenphp::site_port(domain)),
        WebServer::Apache => Some(super::apache::site_port(domain)),
        _ => None,
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
        _ => None,
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
/// file (dead origin) and the login file (domain baked in; its owner rule:
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

/// A UNIQUE, ≤64-char database name for a NEW site. Prefers the clean
/// [`wordpress::db_name_for`] base (what existing sites already store); falls
/// back to a hash-suffixed form when that base would COLLIDE with an existing
/// site or exceed MySQL's 64-char identifier limit. Without this, `db_name_for`
/// (not injective — `a-b.test` and `a.b.test` both reduce to `wp_a_b_test`)
/// would let two distinct domains silently share ONE database (data bleed, and
/// deleting either drops both — finding B21). The check-then-use is atomic under
/// the app-wide db lock that already serializes create (same as `domain_exists`).
fn unique_db_name(conn: &Connection, domain: &str) -> Result<String> {
    let base = super::wordpress::db_name_for(domain);
    if base.len() <= super::wordpress::DB_NAME_MAX && !store::db_name_exists(conn, &base)? {
        return Ok(base);
    }
    // Base collides or overflows 64 chars — disambiguate with a hash of the full
    // domain. A remaining collision here needs two distinct domains to share
    // both the truncated slug AND the 32-bit hash (~1 in 4 billion): refuse
    // rather than risk a silent shared database.
    let disambiguated = super::wordpress::db_name_disambiguated(domain);
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
    create_recording_ownership(conn, new, true)
}

/// [`create`], with the docroot-ownership answer supplied by the caller that
/// KNOWS it (see [`Site::docroot_managed`](crate::state::models::Site)).
fn create_recording_ownership(
    conn: &Connection,
    new: NewSite,
    docroot_managed: bool,
) -> Result<Site> {
    validate_domain(&new.domain)?;
    validate_docroot_path(&new.path)?;
    ensure_server_available(new.web_server)?;
    if store::domain_exists(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "domain already in use: {}",
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
    let db_name = unique_db_name(conn, &new.domain)?;
    // Recorded ONCE from the stored path's own markers (v24) — Bedrock/
    // Radicle linked docroots keep mu-plugins out of a dead `wp-content/`.
    // Non-WP sites never consult it.
    let content_dir = (new.site_type == SiteType::Wordpress)
        .then(|| detect_content_dir_rel(Path::new(&new.path)).to_string());
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
    };
    store::insert_site(conn, &site)?;
    Ok(site)
}

/// All sites, newest first.
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
    if store::domain_exists(conn, new_domain)? {
        return Err(Error::Other(format!("domain already in use: {new_domain}")));
    }
    Ok(())
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
            label: "WordPress (Bedrock)",
            existing_install: true,
        };
    }
    if has("public/wp-config.php") && has("bedrock/application.php") {
        return DetectedProject {
            site_type: SiteType::Wordpress,
            docroot_rel: "public".into(),
            label: "WordPress (Radicle)",
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
            docroot_rel: "public".into(),
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
pub fn validate_linked_docroot(
    conn: &Connection,
    platform: &dyn Platform,
    path: &str,
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
    let mut blast_radius: Vec<PathBuf> = vec![PathBuf::from("/"), PathBuf::from("/Users")];
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
    if blast_radius.contains(&canon) {
        return Err(Error::Other(format!(
            "{} is too broad to serve — a site's folder can be shared publicly with one \
             click, which would expose everything inside it. Pick the project folder itself.",
            canon.display()
        )));
    }

    // Our own app data holds the CA key, every site certificate and the app
    // database — never serve it.
    if let Ok(app_data) = platform.paths().app_data_dir() {
        if canon == app_data || canon.starts_with(&app_data) {
            return Err(Error::Other(
                "that folder is rexenv's own application data — pick your project folder".into(),
            ));
        }
    }

    // Inside the managed sites folder there is nothing to link: that's a normal
    // site, and linking would only opt its docroot out of cleanup.
    let managed = sites_dir(conn, platform)?;
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
    Ok(canon)
}

fn validate_docroot_path(path: &str) -> Result<()> {
    if let Some(c) =
        path.chars().find(|&c| matches!(c, '"' | '$' | '{' | '}' | '\\') || c.is_control())
    {
        return Err(Error::Other(format!(
            "the site folder path contains {c:?}, which can't be used in the web-server \
             config — pick a folder without any of \" $ {{ }} \\ or control characters"
        )));
    }
    Ok(())
}

/// Switch a site's web server (Phase 2 §4.1): update ONLY the `web_server` column
/// — no docroot/cert/DB rebuild — and return the updated site. Nginx, FrankenPHP
/// and Apache have backends (OLS is still deferred). The caller brings the new
/// backend up / old down and reloads the edge.
pub fn set_web_server(conn: &Connection, id: &str, server: WebServer) -> Result<Option<Site>> {
    ensure_server_available(server)?;
    let Some(_site) = get(conn, id)? else { return Ok(None) };
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
        return Err(Error::Other(format!("unsupported PHP version: {version}")));
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
        if !crate::core::binaries::xdebug_supported(&minor) {
            return Err(Error::Other(format!(
                "Xdebug isn't available for PHP {minor} — its static build can't load \
                 extensions. Switch the site to PHP 8.1 or newer first."
            )));
        }
    }
    if !store::set_site_xdebug(conn, id, enabled)? {
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

    // Remove per-site config/log artifacts (best-effort): the FrankenPHP
    // override config + log (if the site ever ran the override server) and the
    // tunnel log (if it was ever shared). Paths come from the owning modules so
    // the names can't drift.
    if let Ok(conf) = frankenphp::config_path(platform, &site.domain) {
        let _ = std::fs::remove_file(conf);
    }
    if let Ok(log) = frankenphp::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }
    if let Ok(log) = tunnels::log_path(platform, &site.domain) {
        let _ = std::fs::remove_file(log);
    }

    // Remove the docroot, but only if it's under a managed sites dir — the
    // configured one, the current default, OR the legacy app-data default (so
    // neither changing the setting nor the default-change to ~/rexenv/Sites
    // strands teardown of pre-existing sites).
    let owned = match site.docroot_managed {
        Some(owned) => owned,
        // Pre-v17 row, backfill hasn't run: the legacy answer, which is what
        // the backfill records anyway.
        None => docroot_under_managed_root(
            &site.path,
            &sites_dir(conn, platform)?,
            &default_sites_dir()?,
            &legacy_sites_dir(platform)?,
        ),
    };
    let docroot_removed =
        owned && !site.path.is_empty() && std::fs::remove_dir_all(&site.path).is_ok();

    Ok(Teardown { existed: true, docroot_removed })
}

/// Whether a site type needs a database provisioned (the pluggable DB stage —
/// Blank PHP: none; WordPress/Laravel: MySQL, done in §8/§9).
pub fn needs_database(site_type: SiteType) -> bool {
    matches!(site_type, SiteType::Wordpress | SiteType::Laravel)
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

/// Settings key for the configurable sites root.
pub const SITES_DIR_KEY: &str = "sites_dir";

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

/// Default site docroot root: `~/rexenv/Sites` — user-visible and Finder
/// friendly (`directories` resolves the home dir correctly per OS). Only a
/// DEFAULT: an explicitly configured `sites_dir` setting always wins, and
/// existing site rows hold absolute paths, so changing the default never
/// orphans previously created sites.
fn default_sites_dir() -> Result<PathBuf> {
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
///   for Blank PHP a `phpinfo()` `index.php` is dropped in. Ours to delete.
/// - **non-empty path — LINK an existing folder**: it is validated
///   ([`validate_linked_docroot`]) and served in place. We never create it,
///   never write into it here, and record that we don't own it, so deleting the
///   site can never remove it.
///
/// The DB-provisioning step branches on [`needs_database`] (a hook for 9.2).
/// Does NOT (re)write the shared server configs — call [`rebuild_configs`] +
/// reload after, so one apply covers any number of changes.
pub fn provision(
    conn: &Connection,
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    mut new: NewSite,
) -> Result<Site> {
    validate_domain(&new.domain)?; // before any docroot/cert/DB use of the domain
    if store::domain_exists(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "domain already in use: {}",
            new.domain
        )));
    }

    // A caller-supplied path means LINK: adopt the folder as-is. Nothing is
    // created and nothing is written into it — not even the Blank-PHP probe
    // file, which would land in the user's own project.
    let linked = !new.path.trim().is_empty();
    let docroot = if linked {
        validate_linked_docroot(conn, platform, &new.path)?
    } else {
        let docroot = sites_dir(conn, platform)?.join(&new.domain);
        std::fs::create_dir_all(&docroot)?;
        if matches!(new.site_type, SiteType::Php) {
            std::fs::write(docroot.join("index.php"), "<?php phpinfo();\n")?;
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
    create_recording_ownership(conn, new, !linked)
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
    ("db", 5),            // spawn engine + readiness probe
    ("core_download", 35), // wp core download ~25MB — no byte signal (B25)
    ("configure", 5),     // wp-config + CREATE DATABASE
    ("core_install", 10), // wp core install
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
/// Apache) run their own backend process and are excluded. Also the single
/// source of truth for tunnel eligibility (`tunnels::ensure_tunnelable`):
/// tunnels originate from the shared nginx, so "in the nginx config" and
/// "safe to tunnel" must be the same predicate — a drift between them is the
/// wrong-vhost exposure all over again.
pub(crate) fn is_nginx_served(s: &Site) -> bool {
    !matches!(s.web_server, WebServer::Frankenphp | WebServer::Apache)
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
) -> services::NginxSite {
    services::NginxSite {
        domain: s.domain.clone(),
        docroot: PathBuf::from(&s.path),
        php_fpm_port: pool_port_for_site(s),
        rewrite: rewrite_mode_for(s.multisite),
        body_limit: body_limits.get(&php::minor_of(&s.php_version)).copied(),
        env: site_env.get(&s.id).cloned().unwrap_or_default(),
    }
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
    rebuild_configs_for(
        &sites,
        platform,
        ca,
        nginx_http_port,
        caddy_http_port,
        caddy_https_port,
        &body_limits,
        &site_env,
    )
}

/// Like [`rebuild_configs`] but from an explicit site list + precomputed nginx
/// body limits and per-site env vars (so callers holding an async lock don't
/// keep the DB connection borrowed across `.await`). `body_limits` comes from
/// `php::nginx_body_limits`; `site_env` from `store::all_site_env`, keyed by
/// site id.
pub fn rebuild_configs_for(
    sites: &[Site],
    platform: &dyn Platform,
    ca: &ssl::LocalCa,
    nginx_http_port: u16,
    caddy_http_port: u16,
    caddy_https_port: u16,
    body_limits: &std::collections::HashMap<String, u64>,
    site_env: &std::collections::HashMap<String, Vec<(String, String)>>,
) -> Result<RebuiltConfigs> {
    // Only nginx-served sites get a server block; override servers (§2/§3) have
    // their own backend process.
    let mut nginx_sites: Vec<services::NginxSite> = sites
        .iter()
        .filter(|s| is_nginx_served(s))
        .map(|s| nginx_site_for(s, body_limits, site_env))
        .collect();
    // Internal Adminer vhost (§5.2): served by the default php-fpm pool, rooted at
    // its isolated docroot. Not a Site → never a tunnel origin (§9).
    nginx_sites.push(services::NginxSite {
        domain: adminer::ADMINER_HOST.to_string(),
        docroot: adminer::docroot(platform)?,
        php_fpm_port: services::PHP_FPM_PORT,
        rewrite: services::RewriteMode::Single,
        body_limit: None,
        env: Vec::new(),
    });
    let (nginx_conf, nginx_prefix) =
        services::write_nginx_config(platform, nginx_http_port, nginx_sites)?;

    // Every site (nginx- or override-served) gets a Caddy edge route: TLS with the
    // local CA, reverse-proxy to that site's upstream (shared nginx, or its own
    // override backend port).
    let mut routes = Vec::with_capacity(sites.len());
    for s in sites {
        let cert = ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, &s.domain)?;
        routes.push(proxy::SiteRoute {
            host: s.domain.clone(),
            // Subdomain multisite serves every `*.mysite.rex` sub-site from the
            // one wildcard cert + backend (§10.2); other modes are single-host.
            wildcard: matches!(s.multisite, MultisiteMode::Subdomain),
            upstream: site_upstream(s, nginx_http_port),
            cert_path: cert.cert_path,
            key_path: cert.key_path,
        });
    }
    // Edge route for the internal Adminer vhost (TLS via local CA → shared nginx).
    let adminer_cert =
        ssl::ensure_site_cert(platform.paths(), platform.permissions(), ca, adminer::ADMINER_HOST)?;
    routes.push(proxy::SiteRoute {
        host: adminer::ADMINER_HOST.to_string(),
        wildcard: false,
        upstream: format!("127.0.0.1:{nginx_http_port}"),
        cert_path: adminer_cert.cert_path,
        key_path: adminer_cert.key_path,
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
    use super::*;
    use crate::state::db;
    use crate::state::models::{SiteType, WebServer};

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
        }
    }

    #[test]
    fn validate_docroot_path_allows_spaces_but_rejects_config_breaking_chars() {
        // Spaces (quoted in the config) and ordinary paths are fine — these are
        // the real cases, so existing sites keep working.
        for ok in ["/Sites/acme/public", "/Users/me/My Sites/blog", "~/Sites/a-b_1.test"] {
            assert!(validate_docroot_path(ok).is_ok(), "{ok} must be allowed");
        }
        // The unescapable-across-nginx/Caddy/Apache set + control chars → rejected.
        for bad in ["/a\"b", "/a$b", "/a{b", "/a}b", "/a\\b", "/a\nb", "/a\tb"] {
            assert!(validate_docroot_path(bad).is_err(), "{bad:?} must be rejected");
        }
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

        // Happy path: on, then off.
        let on = set_xdebug(&conn, &site.id, true).unwrap().unwrap();
        assert!(on.xdebug);
        let off = set_xdebug(&conn, &site.id, false).unwrap().unwrap();
        assert!(!off.xdebug);

        // FrankenPHP refused (embedded PHP — the pools never serve it).
        set_web_server(&conn, &site.id, WebServer::Frankenphp).unwrap();
        assert!(set_xdebug(&conn, &site.id, true).is_err());
        set_web_server(&conn, &site.id, WebServer::Nginx).unwrap();

        // PHP 8.0 refused (static build can't dlopen any .so).
        set_php_version(&conn, &site.id, "8.0").unwrap();
        assert!(set_xdebug(&conn, &site.id, true).is_err());
        set_php_version(&conn, &site.id, "8.4").unwrap();
        assert!(set_xdebug(&conn, &site.id, true).unwrap().unwrap().xdebug);

        // Disabling never validates (a stale flag must always be clearable).
        set_php_version(&conn, &site.id, "8.0").unwrap();
        assert!(!set_xdebug(&conn, &site.id, false).unwrap().unwrap().xdebug);

        // Unknown id → None, not an error.
        assert!(set_xdebug(&conn, "nope", true).unwrap().is_none());
        assert!(set_xdebug(&conn, "nope", false).unwrap().is_none());
    }

    #[test]
    fn pool_port_for_site_routes_xdebug_sites_to_the_debug_pool() {
        let conn = db::open_in_memory().unwrap();
        let mut site = create(&conn, sample("A", "a.test")).unwrap();
        site.php_version = "8.4".into();
        // Toggle off → the shared pool, exactly pool_port_for.
        assert_eq!(pool_port_for_site(&site), pool_port_for("8.4"));
        // Toggle on → the minor's debug port.
        site.xdebug = true;
        assert_eq!(pool_port_for_site(&site), php::debug_fpm_port("8.4").unwrap());
        // A stale flag on an unsupported minor falls back to the normal pool —
        // never a port nothing will ever listen on.
        site.php_version = "8.0".into();
        assert_eq!(pool_port_for_site(&site), pool_port_for("8.0"));
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
        let sa = create(&conn, a).unwrap();

        let mut b = sample("B", &db_);
        b.web_server = WebServer::Frankenphp;
        let sb = create(&conn, b).expect("second colliding site is NOT refused anymore");

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
        let created = create(&conn, a).unwrap();
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

    #[test]
    fn set_domain_rejects_invalid_duplicate_same_and_multisite() {
        let conn = db::open_in_memory().unwrap();
        let a = create(&conn, sample("A", "a.test")).unwrap();
        create(&conn, sample("B", "b.test")).unwrap();

        assert!(set_domain(&conn, &a.id, "../evil.test").is_err());
        assert!(set_domain(&conn, &a.id, "b.test").unwrap_err().to_string().contains("already in use"));
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
        let locked = root.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o555);
        std::fs::set_permissions(&locked, perms.clone()).unwrap();
        let denied = move_dir(&doc, &locked.join("acme.test")).unwrap_err().to_string();
        assert!(denied.contains("nothing changed"), "{denied}");
        assert!(doc.join("index.php").exists(), "source untouched on failure");
        assert!(!locked.join("acme.test").exists(), "no partial target left");
        perms.set_mode(0o755);
        std::fs::set_permissions(&locked, perms).unwrap();

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_domain_is_rejected() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, sample("One", "dup.test")).unwrap();
        let err = create(&conn, sample("Two", "dup.test")).unwrap_err();
        assert!(err.to_string().contains("domain already in use"));
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
            tunnels::log_path(&*platform, &site.domain).unwrap(),
        ];
        for p in &artifacts {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
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
        let linked = create_recording_ownership(&conn, new, false).unwrap();
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

    #[test]
    fn a_linked_docroot_is_never_created_or_populated_by_us() {
        // Retry re-ensures prepare's artifacts, and for a Blank-PHP site that
        // means writing a phpinfo() index.php. For a LINKED site that folder is
        // the user's, so the retry path branches on `docroot_managed !=
        // Some(false)` — this pins the predicate it relies on.
        let conn = db::open_in_memory().unwrap();
        let (dir, new) = docroot_fixture("noretrywrite");
        let linked = create_recording_ownership(&conn, new, false).unwrap();
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

    #[cfg(target_os = "macos")]
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

    #[test]
    fn override_sites_route_to_backend_and_skip_nginx() {
        let conn = db::open_in_memory().unwrap();
        let mut ng = sample("NG", "ng.test");
        ng.site_type = SiteType::Php;
        ng.web_server = WebServer::Nginx;
        let mut fp = sample("FP", "fp.test");
        fp.site_type = SiteType::Php;
        fp.web_server = WebServer::Frankenphp;
        let ng = create(&conn, ng).unwrap();
        let fp = create(&conn, fp).unwrap();

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
        assert!(set_php_version(&conn, &site.id, "7.4").is_err());
        assert!(set_php_version(&conn, "nope", "8.3").unwrap().is_none());
    }

    #[test]
    fn set_web_server_updates_only_the_column() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("SW", "sws.test");
        new.web_server = WebServer::Nginx;
        let site = create(&conn, new).unwrap();

        // Nginx → FrankenPHP → Nginx: only web_server changes; path/domain untouched.
        let fp = set_web_server(&conn, &site.id, WebServer::Frankenphp).unwrap().expect("exists");
        assert!(matches!(fp.web_server, WebServer::Frankenphp));
        assert_eq!(fp.path, site.path);
        let back = set_web_server(&conn, &site.id, WebServer::Nginx).unwrap().unwrap();
        assert!(matches!(back.web_server, WebServer::Nginx));

        // Apache is a real backend now; OLS stays deferred; unknown id → None.
        let ap = set_web_server(&conn, &site.id, WebServer::Apache).unwrap().expect("exists");
        assert!(matches!(ap.web_server, WebServer::Apache));
        set_web_server(&conn, &site.id, WebServer::Nginx).unwrap();
        assert!(set_web_server(&conn, &site.id, WebServer::Openlitespeed).is_err());
        assert!(set_web_server(&conn, "nope", WebServer::Nginx).unwrap().is_none());
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
        let na = nginx_site_for(&a, &no_limits, &no_env);
        let nb = nginx_site_for(&b, &no_limits, &no_env);
        // Each site routes to its own version's pool port — not a hardcoded one.
        assert_eq!(na.php_fpm_port, php::fpm_port("8.1").unwrap()); // 9781
        assert_eq!(nb.php_fpm_port, php::fpm_port("8.3").unwrap()); // 9783
        assert_ne!(na.php_fpm_port, nb.php_fpm_port);

        // A patch-form version still resolves to its minor's pool.
        let mut c = sample("C", "c.test");
        c.php_version = "8.2.31".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &no_limits, &no_env).php_fpm_port, php::fpm_port("8.2").unwrap());

        // An unknown version falls back to the default pool.
        let mut d = sample("D", "d.test");
        d.php_version = "7.4".into();
        let d = create(&conn, d).unwrap();
        assert_eq!(nginx_site_for(&d, &no_limits, &no_env).php_fpm_port, services::PHP_FPM_PORT);
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
        assert_eq!(nginx_site_for(&a, &limits, &no_env).body_limit, Some(64 << 20));
        // A patch-form version maps through its minor; an uncovered minor gets none.
        let mut b = sample("B", "b.test");
        b.php_version = "8.3.31".into();
        let b = create(&conn, b).unwrap();
        assert_eq!(nginx_site_for(&b, &limits, &no_env).body_limit, Some(64 << 20));
        let mut c = sample("C", "c.test");
        c.php_version = "8.1".into();
        let c = create(&conn, c).unwrap();
        assert_eq!(nginx_site_for(&c, &limits, &no_env).body_limit, None);
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
        create(&conn, new).unwrap();

        // An unavailable server is refused at CREATE too (core guard, M7) —
        // not just at switch time.
        let mut ols = sample("OLS", "ols.test");
        ols.web_server = WebServer::Openlitespeed;
        assert!(create(&conn, ols).is_err());

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
}
