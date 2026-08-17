//! core::downloads — the download manager hub: the SINGLE source of truth for
//! binary-download state (mirror of the ServiceManager pattern for service
//! status). `core::binaries` reports every real download into the hub as it
//! streams; actions (Start all, install PHP x.y, start a DB engine) group their
//! needed binaries into a *batch* so the UI can say "downloading 2 of 5".
//!
//! The hub is a process-wide singleton (`hub()`): `binaries::resolve*` is called
//! deep inside core with only a `&dyn Platform`, so threading a sink through
//! every call site would churn ~10 signatures for no gain. Platform-agnostic and
//! tauri-free — the app layer (lib.rs) subscribes via [`Hub::subscribe`] and
//! forwards snapshots to the frontend as Tauri events.
//!
//! It also owns [`user_downloads_dir`] — the USER'S `~/Downloads`, which is a
//! different thing from everything above (this module is otherwise about
//! downloads rexenv is performing). It lives here because three modules had
//! grown their own identical copy: one `UserDirs` call has nothing to drift,
//! but three is where "nothing to drift" stops being the argument.

use crate::core::db::DbEngine;
use crate::core::{binaries, php};
use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use crate::state::models::{Site, WebServer};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// The user's `~/Downloads` — where a database export, a log download and a
/// built plugin zip are delivered. ONE definition; `logs`, `database` and
/// `dist_archive` each had their own until 13 Aug 2026.
pub fn user_downloads_dir() -> Result<PathBuf> {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
        .ok_or_else(|| Error::Other("could not resolve the Downloads folder".into()))
}

/// Where a download item is in its life. `Preparing` covers everything after
/// the verified download (extract, relink, codesign) — it can take seconds for
/// the big DB trees. There is no separate "verifying" phase: the checksum is
/// hashed incrementally while streaming, so verification is instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Pending,
    Downloading,
    Preparing,
    Done,
    Cached,
    Failed,
}

impl Phase {
    /// Terminal phases are swept from the item list when a new batch begins.
    fn is_terminal(self) -> bool {
        matches!(self, Phase::Done | Phase::Cached | Phase::Failed)
    }
    fn is_complete(self) -> bool {
        matches!(self, Phase::Done | Phase::Cached)
    }
}

/// One binary's download state as exposed to the UI (camelCase for IPC).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSnapshot {
    /// Stable id: `<name>-<version>` (same key as the binary cache dir).
    pub id: String,
    /// Manifest name + pinned version — what `retry_download` takes back.
    pub name: String,
    pub version: String,
    /// Human label, e.g. "PHP 8.3 (FPM)".
    pub label: String,
    pub phase: Phase,
    pub downloaded_bytes: u64,
    /// `None` = the server sent no Content-Length → indeterminate progress.
    pub total_bytes: Option<u64>,
    /// Sliding-window transfer rate; `None` until enough samples exist.
    pub bytes_per_sec: Option<u64>,
    pub error: Option<String>,
}

/// The active action's batch: how many of its downloads are complete.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSnapshot {
    pub action: String,
    pub done: usize,
    pub total: usize,
}

/// Full hub state — emitted whole (small: at most a dozen items) so the UI can
/// simply replace its state; no event-ordering races.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub batch: Option<BatchSnapshot>,
    pub items: Vec<ItemSnapshot>,
}

/// One planned download for [`Hub::begin_batch`]: `cached` items are listed as
/// already-complete rows (the onboarding checklist wants them visible); only
/// missing items count toward the batch's done/total.
#[derive(Debug, Clone)]
pub struct Planned {
    pub name: String,
    pub version: String,
    pub cached: bool,
}

struct Item {
    snap: ItemSnapshot,
    /// (when, downloaded-bytes) samples for the sliding-window rate.
    samples: VecDeque<(Instant, u64)>,
}

struct HubState {
    batch: Option<(String, Vec<String>)>, // (action, missing item ids)
    items: Vec<Item>,
}

/// The download manager. All mutations bump a `watch` counter; subscribers pull
/// a fresh [`Snapshot`] when it changes (coalescing is free — watch keeps only
/// the latest value).
pub struct Hub {
    state: Mutex<HubState>,
    tx: watch::Sender<u64>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

/// The process-wide hub.
pub fn hub() -> &'static Hub {
    HUB.get_or_init(|| {
        let (tx, _) = watch::channel(0);
        Hub {
            state: Mutex::new(HubState { batch: None, items: Vec::new() }),
            tx,
        }
    })
}

/// Stable item id for `name`@`version` — matches the binary cache dir name.
pub fn item_id(name: &str, version: &str) -> String {
    format!("{name}-{version}")
}

/// Human label for a pinned binary. Falls back to `name version`.
pub fn label_for(name: &str, version: &str) -> String {
    let minor = |v: &str| v.rsplit_once('.').map(|(m, _)| m.to_string()).unwrap_or_else(|| v.into());
    match name {
        "php" => format!("PHP {} (CLI)", minor(version)),
        "php-fpm" => format!("PHP {} (FPM)", minor(version)),
        "php-debug" => format!("PHP {} debug (CLI)", minor(version)),
        "php-fpm-debug" => format!("PHP {} debug (FPM)", minor(version)),
        "caddy" => "Caddy (edge router)".into(),
        "nginx" => "Nginx (web server)".into(),
        "mysql" => format!("MySQL {}", minor(version)),
        "postgres" => format!("PostgreSQL {}", minor(version)),
        "mailpit" => "Mailpit (mail catcher)".into(),
        "adminer" => "Adminer (DB browser)".into(),
        "wp-cli" => "WP-CLI".into(),
        "frankenphp" => "FrankenPHP".into(),
        "cloudflared" => "cloudflared (tunnels)".into(),
        "redis" => format!("Redis {}", minor(version)),
        "mariadb" => format!("MariaDB {}", minor(version)),
        "httpd" => "Apache (httpd)".into(),
        n if n.starts_with("xdebug-") => {
            format!("Xdebug (PHP {})", n.trim_start_matches("xdebug-"))
        }
        _ => format!("{name} {version}"),
    }
}

/// LOGIN-SAFETY guard 1 (`commands::services::auto_start_inner`): the names a
/// start plan would have to DOWNLOAD. An unattended login start must abort
/// when this is non-empty — never stream downloads nobody asked for — so the
/// decision lives here as a pure function with its own test rather than
/// inline in an untestable command.
pub fn uncached_names(plan: &[PlannedBinary]) -> Vec<&str> {
    plan.iter().filter(|p| !p.cached).map(|p| p.name.as_str()).collect()
}

/// One binary an action needs: enough to plan (cached split), display (via
/// [`Planned`]) and fetch (via [`resolve_any`]'s name-based dispatch).
#[derive(Debug, Clone)]
pub struct PlannedBinary {
    pub name: String,
    pub version: String,
    pub cached: bool,
}

impl PlannedBinary {
    fn new(platform: &dyn Platform, name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            cached: binaries::is_cached(platform, name, version),
        }
    }
    fn planned(&self) -> Planned {
        Planned {
            name: self.name.clone(),
            version: self.version.clone(),
            cached: self.cached,
        }
    }
}

/// The full binary set a "Start all" needs, split into cached vs to-download:
/// the shared stack (edge, web server, MySQL, mail sink, DB browser), one
/// php-fpm per installed minor (the default minor always, mirroring
/// `start_core`), and FrankenPHP only when a site actually overrides to it.
pub fn plan_for_start(
    platform: &dyn Platform,
    sites: &[Site],
    php_minors: &[String],
    db_versions: &std::collections::HashMap<DbEngine, String>,
) -> Vec<PlannedBinary> {
    let db_ver = |e: DbEngine| -> String {
        db_versions
            .get(&e)
            .cloned()
            .unwrap_or_else(|| e.default_version().to_string())
    };
    let mysql_version = db_ver(DbEngine::Mysql);
    let mut set: Vec<(&str, &str)> = vec![
        ("caddy", binaries::CADDY_VERSION),
        ("nginx", binaries::NGINX_VERSION),
        ("mysql", &mysql_version),
        ("mailpit", binaries::MAILPIT_VERSION),
        ("adminer", binaries::ADMINER_VERSION),
    ];
    let mariadb_version = db_ver(DbEngine::Mariadb);
    let mut minors = php_minors.to_vec();
    let default_minor = php::minor_of(binaries::PHP_VERSION);
    if !minors.contains(&default_minor) {
        minors.push(default_minor);
    }
    for minor in &minors {
        if let Some(patch) = php::patch_for_minor(minor) {
            set.push(("php-fpm", patch));
        }
    }
    if sites.iter().any(|s| matches!(s.web_server, WebServer::Frankenphp)) {
        set.push(("frankenphp", binaries::FRANKENPHP_VERSION));
    }
    if sites.iter().any(|s| matches!(s.web_server, WebServer::Apache)) {
        set.push(("httpd", binaries::HTTPD_VERSION));
    }
    if sites.iter().any(|s| matches!(s.db_engine, crate::state::models::SiteDbEngine::Mariadb)) {
        set.push(("mariadb", &mariadb_version));
    }
    let mut plan: Vec<PlannedBinary> = set
        .into_iter()
        .map(|(n, v)| PlannedBinary::new(platform, n, v))
        .collect();
    // Xdebug bundles for toggled sites' minors — Start-all spawns those debug
    // pools, and spawn-under-lock must hit cache.
    let mut xdebug_minors: Vec<String> = sites
        .iter()
        .filter(|s| s.xdebug)
        .map(|s| php::minor_of(&s.php_version))
        .collect();
    xdebug_minors.sort_unstable();
    xdebug_minors.dedup();
    for minor in xdebug_minors {
        if let Some((name, version)) = binaries::xdebug_bundle_id(&minor) {
            plan.push(PlannedBinary::new(platform, &name, version));
        }
    }
    plan
}

/// The binary set for starting one DB engine on demand. Empty for engines
/// without a pinned portable build (their spawn errors with the real message).
pub fn plan_for_engine(
    platform: &dyn Platform,
    engine: DbEngine,
    version: &str,
) -> Vec<PlannedBinary> {
    vec![PlannedBinary::new(platform, engine.key(), version)]
}

/// Just Mailpit's binary — for the standalone `start_mail` toggle, whose
/// `spawn_mailpit` resolves under the services lock and must hit cache
/// (prefetch-before-lock, §5). Small binary, but a cold-cache toggle would
/// otherwise download WHILE holding the lock and freeze every status read.
pub fn plan_for_mailpit(platform: &dyn Platform) -> Vec<PlannedBinary> {
    vec![PlannedBinary::new(platform, "mailpit", binaries::MAILPIT_VERSION)]
}

/// The binary set for installing a PHP version: its FPM build (the pool) plus
/// its CLI build (WP-CLI operations). Empty if the minor has no pinned build.
pub fn plan_for_php(platform: &dyn Platform, minor: &str) -> Vec<PlannedBinary> {
    match php::patch_for_minor(minor) {
        Some(patch) => vec![
            PlannedBinary::new(platform, "php-fpm", patch),
            PlannedBinary::new(platform, "php", patch),
        ],
        None => Vec::new(),
    }
}

/// Both binaries of ONE EXPLICIT patch — the in-app update path, which names a
/// patch the pin table may not contain.
///
/// Separate from [`plan_for_php`] rather than a parameter on it, because the two
/// answer different questions: that one asks "what does this minor run", which is
/// resolution, and this one says "fetch exactly this", which is a decision the
/// user already made. Collapsing them would let a caller quietly plan a patch
/// nobody chose.
pub fn plan_for_php_patch(platform: &dyn Platform, minor: &str, patch: &str) -> Vec<PlannedBinary> {
    if php::minor_of(patch) != minor {
        return Vec::new();
    }
    vec![
        PlannedBinary::new(platform, "php-fpm", patch),
        PlannedBinary::new(platform, "php", patch),
    ]
}

/// Just one minor's php-fpm pool binary — for the site-create / PHP-switch
/// paths, whose `ensure_php_pool` runs under the services lock and must hit
/// cache. Empty if the minor has no pinned build (`ensure` then errors clearly).
pub fn plan_for_pool(platform: &dyn Platform, minor: &str) -> Vec<PlannedBinary> {
    match php::patch_for_minor(minor) {
        Some(patch) => vec![PlannedBinary::new(platform, "php-fpm", patch)],
        None => Vec::new(),
    }
}

/// The Xdebug toggle-on set: the minor's pool binary + its pinned `xdebug.so`
/// bundle — `ensure_php_debug_pool` runs under the services lock and must hit
/// cache. Empty when the minor has no Xdebug support (core refuses the toggle
/// with the real message).
pub fn plan_for_xdebug(platform: &dyn Platform, minor: &str) -> Vec<PlannedBinary> {
    let mut plan = plan_for_pool(platform, minor);
    if let Some((name, version)) = binaries::xdebug_bundle_id(minor) {
        plan.push(PlannedBinary::new(platform, &name, version));
    }
    plan
}

/// WP install tooling for a site create: the minor's PHP CLI build + WP-CLI.
pub fn plan_for_wp_tooling(platform: &dyn Platform, minor: &str) -> Vec<PlannedBinary> {
    let mut plan = Vec::new();
    if let Some(patch) = php::patch_for_minor(minor) {
        plan.push(PlannedBinary::new(platform, "php", patch));
    }
    plan.push(PlannedBinary::new(platform, "wp-cli", binaries::WP_CLI_VERSION));
    plan
}

/// Composer tooling for a site create: the minor's PHP CLI build (which runs the
/// phar, so Composer's platform checks match the site's own PHP) and the pinned
/// Composer phar itself.
///
/// Named for COMPOSER rather than for Laravel since a cloned Symfony, Craft or
/// Statamic site needs exactly the same two binaries — a name that says
/// "laravel" sends the next reader looking for a second, identical plan.
pub fn plan_for_composer_tooling(platform: &dyn Platform, minor: &str) -> Vec<PlannedBinary> {
    let mut plan = Vec::new();
    if let Some(patch) = php::patch_for_minor(minor) {
        plan.push(PlannedBinary::new(platform, "php", patch));
    }
    plan.push(PlannedBinary::new(platform, "composer", binaries::COMPOSER_VERSION));
    plan
}

/// The FrankenPHP override backend — for site create/switch onto FrankenPHP,
/// whose `reconcile_overrides` (inside the locked reload) must hit cache.
pub fn plan_for_override(platform: &dyn Platform, server: WebServer) -> Vec<PlannedBinary> {
    match server {
        WebServer::Frankenphp => {
            vec![PlannedBinary::new(platform, "frankenphp", binaries::FRANKENPHP_VERSION)]
        }
        WebServer::Apache => vec![PlannedBinary::new(platform, "httpd", binaries::HTTPD_VERSION)],
        _ => Vec::new(),
    }
}

/// Resolve one binary through whichever resolver its distribution shape needs.
/// The per-item retry command reuses this — idempotent, so retrying something
/// that meanwhile resolved returns instantly.
pub async fn resolve_any(platform: &dyn Platform, name: &str, version: &str) -> Result<()> {
    match binaries::shape_of(name) {
        binaries::Shape::Dir => binaries::resolve_dir(platform, name, version).await.map(drop),
        binaries::Shape::Bundle => {
            binaries::resolve_bundle(platform, name, version).await.map(drop)
        }
        binaries::Shape::File => binaries::resolve_file(platform, name, version).await.map(drop),
        binaries::Shape::Single => binaries::resolve(platform, name, version).await.map(drop),
    }
}

/// How many downloads stream at once during a prefetch. Purely a bandwidth
/// choice — bodies stream to disk, so RAM does not scale with this.
const PREFETCH_CONCURRENCY: usize = 2;

/// Download everything an action's `plan` is missing, as one hub batch with
/// live progress. Runs WITHOUT any service lock — callers prefetch first, then
/// take the services lock for the actual start (which then hits cache).
///
/// A failure does NOT stop the batch: remaining items still download (each
/// failure lands on its hub row for per-item retry), and the returned error
/// names EVERY failed binary — not just the first.
pub async fn prefetch(
    platform: &dyn Platform,
    action: &str,
    plan: &[PlannedBinary],
) -> Result<()> {
    let missing: Vec<&PlannedBinary> = plan.iter().filter(|p| !p.cached).collect();
    // Nothing to download → no batch, no events: warm-cache actions (every site
    // create / PHP switch after first run) stay UI-silent. Cached rows are only
    // shown alongside an actual download (mixed batch).
    if missing.is_empty() {
        return Ok(());
    }
    hub().begin_batch(action, &plan.iter().map(PlannedBinary::planned).collect::<Vec<_>>());
    let mut failures: Vec<String> = Vec::new();
    for pair in missing.chunks(PREFETCH_CONCURRENCY) {
        // Bounded fan-out without spawning (platform is a borrow): the pair's
        // futures interleave on this task — downloads are IO-bound.
        let results: Vec<(&PlannedBinary, Result<()>)> = match pair {
            [a] => vec![(a, resolve_any(platform, &a.name, &a.version).await)],
            [a, b] => {
                let (ra, rb) = tokio::join!(
                    resolve_any(platform, &a.name, &a.version),
                    resolve_any(platform, &b.name, &b.version),
                );
                vec![(a, ra), (b, rb)]
            }
            _ => unreachable!("chunks({PREFETCH_CONCURRENCY})"),
        };
        for (p, r) in results {
            if let Err(e) = r {
                failures.push(format!("{}: {e}", label_for(&p.name, &p.version)));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "{} of {} downloads failed — fix the connection (or retry per item in the download panel), then start again. {}",
            failures.len(),
            missing.len(),
            failures.join("; ")
        )))
    }
}

/// Minimum sample span before a rate is computed (avoids nonsense rates from
/// two near-simultaneous chunks) and the window the rate is averaged over.
const RATE_MIN_SPAN: Duration = Duration::from_millis(300);
const RATE_WINDOW: Duration = Duration::from_secs(3);

/// Sliding-window rate from progress samples: bytes/sec across the retained
/// window, `None` until the span is meaningful. Pure — unit-testable with
/// synthetic instants.
fn rate_of(samples: &VecDeque<(Instant, u64)>) -> Option<u64> {
    let (first, oldest) = samples.front()?;
    let (last, newest) = samples.back()?;
    let span = last.duration_since(*first);
    if span < RATE_MIN_SPAN || newest <= oldest {
        return None;
    }
    Some(((newest - oldest) as f64 / span.as_secs_f64()) as u64)
}

fn prune_samples(samples: &mut VecDeque<(Instant, u64)>, now: Instant) {
    while let Some((t, _)) = samples.front() {
        if now.duration_since(*t) > RATE_WINDOW {
            samples.pop_front();
        } else {
            break;
        }
    }
}

impl Hub {
    /// Run `f` on the locked state, then notify subscribers.
    fn mutate<R>(&self, f: impl FnOnce(&mut HubState) -> R) -> R {
        let r = {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut s)
        };
        self.tx.send_modify(|v| *v = v.wrapping_add(1));
        r
    }

    fn with_item<R>(s: &mut HubState, id: &str, f: impl FnOnce(&mut Item) -> R) -> Option<R> {
        s.items.iter_mut().find(|i| i.snap.id == id).map(f)
    }

    /// Start a new action's batch: terminal leftovers from prior work are swept,
    /// every planned item gets a visible row (cached ones as `Cached`), and the
    /// batch counts only the missing ones. In-flight items from another action
    /// keep streaming untouched.
    pub fn begin_batch(&self, action: &str, planned: &[Planned]) {
        self.mutate(|s| {
            s.items.retain(|i| !i.snap.phase.is_terminal());
            let mut missing = Vec::new();
            for p in planned {
                let id = item_id(&p.name, &p.version);
                if !p.cached {
                    missing.push(id.clone());
                }
                if s.items.iter().any(|i| i.snap.id == id) {
                    continue; // already in flight — keep its live state
                }
                s.items.push(Item {
                    snap: ItemSnapshot {
                        id,
                        name: p.name.clone(),
                        version: p.version.clone(),
                        label: label_for(&p.name, &p.version),
                        phase: if p.cached { Phase::Cached } else { Phase::Pending },
                        downloaded_bytes: 0,
                        total_bytes: None,
                        bytes_per_sec: None,
                        error: None,
                    },
                    samples: VecDeque::new(),
                });
            }
            s.batch = Some((action.to_string(), missing));
        });
    }

    /// A download began (or a retry attempt restarted it). Creates the row if
    /// the action didn't plan it (on-demand resolves, e.g. cloudflared).
    pub fn item_started(&self, name: &str, version: &str) {
        let id = item_id(name, version);
        self.mutate(|s| {
            if Self::with_item(s, &id, |i| {
                i.snap.phase = Phase::Downloading;
                i.snap.downloaded_bytes = 0;
                i.snap.bytes_per_sec = None;
                i.snap.error = None;
                i.samples.clear();
            })
            .is_none()
            {
                s.items.push(Item {
                    snap: ItemSnapshot {
                        id,
                        name: name.to_string(),
                        version: version.to_string(),
                        label: label_for(name, version),
                        phase: Phase::Downloading,
                        downloaded_bytes: 0,
                        total_bytes: None,
                        bytes_per_sec: None,
                        error: None,
                    },
                    samples: VecDeque::new(),
                });
            }
        });
    }

    /// Byte progress from the stream. `total` is the Content-Length when the
    /// server sent one (`None` → the UI shows an indeterminate bar).
    pub fn item_progress(&self, id: &str, downloaded: u64, total: Option<u64>) {
        let now = Instant::now();
        self.mutate(|s| {
            Self::with_item(s, id, |i| {
                i.snap.phase = Phase::Downloading;
                i.snap.downloaded_bytes = downloaded;
                i.snap.total_bytes = total;
                i.samples.push_back((now, downloaded));
                prune_samples(&mut i.samples, now);
                if let Some(r) = rate_of(&i.samples) {
                    i.snap.bytes_per_sec = Some(r);
                }
            });
        });
    }

    /// Download verified — now extracting/relinking/codesigning.
    pub fn item_preparing(&self, id: &str) {
        self.mutate(|s| {
            Self::with_item(s, id, |i| {
                i.snap.phase = Phase::Preparing;
                i.snap.bytes_per_sec = None;
            });
        });
    }

    pub fn item_done(&self, id: &str) {
        self.mutate(|s| {
            Self::with_item(s, id, |i| {
                i.snap.phase = Phase::Done;
                i.snap.bytes_per_sec = None;
            });
        });
    }

    pub fn item_failed(&self, id: &str, error: &str) {
        self.mutate(|s| {
            Self::with_item(s, id, |i| {
                i.snap.phase = Phase::Failed;
                i.snap.bytes_per_sec = None;
                i.snap.error = Some(error.to_string());
            });
        });
    }

    /// Current full state (batch progress + all item rows).
    pub fn snapshot(&self) -> Snapshot {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let batch = s.batch.as_ref().map(|(action, ids)| BatchSnapshot {
            action: action.clone(),
            done: ids
                .iter()
                .filter(|id| {
                    s.items
                        .iter()
                        .any(|i| &i.snap.id == *id && i.snap.phase.is_complete())
                })
                .count(),
            total: ids.len(),
        });
        Snapshot {
            batch,
            items: s.items.iter().map(|i| i.snap.clone()).collect(),
        }
    }

    /// Change signal: the value bumps on every mutation; pull [`Hub::snapshot`]
    /// when it does. The app layer throttles + forwards to the frontend.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LOGIN-SAFETY guard 1: the decision auto-start aborts on. Exactly the
    /// uncached names, in plan order; an all-cached plan clears the gate.
    #[test]
    fn uncached_names_lists_exactly_what_a_login_start_must_refuse() {
        let plan = vec![
            PlannedBinary { name: "caddy".into(), version: "1".into(), cached: true },
            PlannedBinary { name: "mysql".into(), version: "2".into(), cached: false },
            PlannedBinary { name: "php-fpm-8.3".into(), version: "3".into(), cached: false },
        ];
        assert_eq!(uncached_names(&plan), vec!["mysql", "php-fpm-8.3"]);
        let warm = vec![PlannedBinary { name: "caddy".into(), version: "1".into(), cached: true }];
        assert!(uncached_names(&warm).is_empty());
        assert!(uncached_names(&[]).is_empty());
    }

    /// A fresh hub per test (the global one is shared across the test binary).
    fn fresh() -> Hub {
        let (tx, _) = watch::channel(0);
        Hub {
            state: Mutex::new(HubState { batch: None, items: Vec::new() }),
            tx,
        }
    }

    fn planned(name: &str, version: &str, cached: bool) -> Planned {
        Planned { name: name.into(), version: version.into(), cached }
    }

    #[test]
    fn item_lifecycle_started_progress_preparing_done() {
        let h = fresh();
        h.item_started("nginx", "1.30.3");
        h.item_progress("nginx-1.30.3", 1024, Some(4096));
        let s = h.snapshot();
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].phase, Phase::Downloading);
        assert_eq!(s.items[0].downloaded_bytes, 1024);
        assert_eq!(s.items[0].total_bytes, Some(4096));

        h.item_preparing("nginx-1.30.3");
        assert_eq!(h.snapshot().items[0].phase, Phase::Preparing);
        h.item_done("nginx-1.30.3");
        assert_eq!(h.snapshot().items[0].phase, Phase::Done);
    }

    #[test]
    fn unknown_total_stays_none_for_indeterminate_ui() {
        let h = fresh();
        h.item_started("x", "1");
        h.item_progress("x-1", 500, None);
        let i = &h.snapshot().items[0];
        assert_eq!(i.total_bytes, None);
        assert_eq!(i.downloaded_bytes, 500);
    }

    #[test]
    fn failed_keeps_error_and_new_batch_sweeps_it() {
        let h = fresh();
        h.item_started("caddy", "2.11.4");
        h.item_failed("caddy-2.11.4", "checksum mismatch for https://x");
        let s = h.snapshot();
        assert_eq!(s.items[0].phase, Phase::Failed);
        assert!(s.items[0].error.as_deref().unwrap().contains("checksum"));

        h.begin_batch("Start all", &[planned("nginx", "1.30.3", false)]);
        let s = h.snapshot();
        assert_eq!(s.items.len(), 1, "failed leftover swept");
        assert_eq!(s.items[0].id, "nginx-1.30.3");
        assert_eq!((s.items[0].name.as_str(), s.items[0].version.as_str()), ("nginx", "1.30.3"));
    }

    #[test]
    fn batch_counts_only_missing_and_completes_with_done_and_cached() {
        let h = fresh();
        h.begin_batch(
            "Start all",
            &[
                planned("caddy", "2.11.4", true), // cached — visible row, not counted
                planned("nginx", "1.30.3", false),
                planned("mysql", "8.4.6", false),
            ],
        );
        let s = h.snapshot();
        let b = s.batch.as_ref().unwrap();
        assert_eq!((b.done, b.total), (0, 2));
        assert_eq!(s.items.len(), 3);
        assert_eq!(s.items[0].phase, Phase::Cached);

        h.item_started("nginx", "1.30.3");
        h.item_done("nginx-1.30.3");
        let b = h.snapshot().batch.unwrap();
        assert_eq!((b.done, b.total), (1, 2));
    }

    #[test]
    fn retry_restart_resets_bytes_and_error() {
        let h = fresh();
        h.item_started("wp-cli", "2.12.0");
        h.item_progress("wp-cli-2.12.0", 9000, Some(10000));
        h.item_failed("wp-cli-2.12.0", "download stalled");
        h.item_started("wp-cli", "2.12.0"); // retry
        let i = &h.snapshot().items[0];
        assert_eq!(i.phase, Phase::Downloading);
        assert_eq!(i.downloaded_bytes, 0);
        assert_eq!(i.error, None);
    }

    #[test]
    fn rate_needs_span_then_averages_window() {
        let now = Instant::now();
        let mut s: VecDeque<(Instant, u64)> = VecDeque::new();
        s.push_back((now, 0));
        assert_eq!(rate_of(&s), None, "single sample → no rate");
        s.push_back((now + Duration::from_millis(10), 4096));
        assert_eq!(rate_of(&s), None, "span under minimum → no rate");
        s.push_back((now + Duration::from_secs(1), 1_000_000));
        let r = rate_of(&s).unwrap();
        assert!((900_000..=1_100_000).contains(&r), "~1MB/s, got {r}");
    }

    #[test]
    fn prune_drops_samples_outside_window() {
        let now = Instant::now();
        let mut s: VecDeque<(Instant, u64)> = VecDeque::new();
        s.push_back((now, 0));
        s.push_back((now + Duration::from_secs(2), 100));
        s.push_back((now + Duration::from_secs(5), 200));
        prune_samples(&mut s, now + Duration::from_secs(5));
        assert_eq!(s.len(), 2, "sample older than the window dropped");
        assert_eq!(s.front().unwrap().1, 100);
    }

    #[test]
    fn labels_and_ids() {
        assert_eq!(item_id("php-fpm", "8.3.31"), "php-fpm-8.3.31");
        assert_eq!(label_for("php-fpm", "8.3.31"), "PHP 8.3 (FPM)");
        assert_eq!(label_for("php", "8.1.34"), "PHP 8.1 (CLI)");
        assert_eq!(label_for("mysql", "8.4.6"), "MySQL 8.4");
        assert_eq!(label_for("caddy", "2.11.4"), "Caddy (edge router)");
        assert_eq!(label_for("something", "9.9"), "something 9.9");
    }

    fn site(ws: WebServer) -> Site {
        use crate::state::models::{MultisiteMode, ServiceStatus, SiteOrigin, SiteType};
        Site {
            id: "s.test".into(),
            name: "s.test".into(),
            domain: "s.test".into(),
            site_type: SiteType::Php,
            status: ServiceStatus::Stopped,
            php_version: "8.3".into(),
            web_server: ws,
            ssl: true,
            path: "/tmp/s.test".into(),
            created_at: "now".into(),
            multisite: MultisiteMode::None,
            db_name: "wp_s_test".into(),
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
        }
    }

    #[test]
    fn plan_for_start_covers_stack_pools_and_conditional_frankenphp() {
        let plat = crate::platform::current();
        let plan =
            plan_for_start(&*plat, &[site(WebServer::Nginx)], &["8.1".into()], &Default::default());
        let names: Vec<(&str, &str)> = plan
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();
        for expected in [
            ("caddy", binaries::CADDY_VERSION),
            ("nginx", binaries::NGINX_VERSION),
            ("mysql", binaries::MYSQL_VERSION),
            ("mailpit", binaries::MAILPIT_VERSION),
            ("adminer", binaries::ADMINER_VERSION),
        ] {
            assert!(names.contains(&expected), "missing {expected:?} in {names:?}");
        }
        // Requested minor's pool AND the always-included default minor's pool
        // (mirrors start_core), each exactly once.
        let fpms: Vec<&&str> = names.iter().filter(|(n, _)| *n == "php-fpm").map(|(_, v)| v).collect();
        assert!(fpms.contains(&&php::patch_for_minor("8.1").unwrap()));
        assert!(fpms.contains(&&binaries::PHP_VERSION));
        assert_eq!(fpms.len(), 2, "{names:?}");
        // No FrankenPHP: no site overrides to it.
        assert!(!names.iter().any(|(n, _)| *n == "frankenphp"));

        let plan =
            plan_for_start(&*plat, &[site(WebServer::Frankenphp)], &[], &Default::default());
        assert!(plan.iter().any(|p| p.name == "frankenphp"));
    }

    #[test]
    fn plan_for_php_maps_minor_to_pinned_fpm_and_cli() {
        let plat = crate::platform::current();
        let plan = plan_for_php(&*plat, "8.2");
        let patch = php::patch_for_minor("8.2").unwrap();
        let names: Vec<(&str, &str)> = plan
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();
        assert_eq!(names, vec![("php-fpm", patch), ("php", patch)]);
        assert!(plan_for_php(&*plat, "7.0").is_empty(), "unpinned minor → nothing to fetch");
    }

    #[test]
    fn plan_for_mailpit_targets_the_pinned_binary() {
        // start_mail prefetches exactly this before taking the services lock, so
        // spawn_mailpit's resolve is a cache hit (§5 prefetch-before-lock, B19).
        let plat = crate::platform::current();
        let plan = plan_for_mailpit(&*plat);
        let names: Vec<(&str, &str)> =
            plan.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect();
        assert_eq!(names, vec![("mailpit", binaries::MAILPIT_VERSION)]);
    }

    #[test]
    fn in_flight_item_survives_new_batch_planning_it() {
        let h = fresh();
        h.item_started("mysql", "8.4.6");
        h.item_progress("mysql-8.4.6", 5_000_000, Some(600_000_000));
        h.begin_batch("Start all", &[planned("mysql", "8.4.6", false)]);
        let i = &h.snapshot().items[0];
        assert_eq!(i.phase, Phase::Downloading, "live download not reset by plan");
        assert_eq!(i.downloaded_bytes, 5_000_000);
    }
}
