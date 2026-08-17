//! commands::php — IPC for the PHP version registry (Phase 2 §1.5). Thin: all
//! logic (guards, registry reads/writes) lives in `core::php`.

use crate::core;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::PhpVersionView;
use tauri::State;

fn lock<'a>(
    state: &'a State<'a, AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))
}

/// All registered PHP versions (installed + available) for the Settings UI.
///
/// `try_lock` on the services, deliberately: this is a UI read, and a long
/// start/stop holding that lock must not block the Settings screen — the same
/// rule the status polls follow. A missed lock costs the `serving` line (the row
/// then says only what is pinned), never the list.
#[tauri::command]
pub fn list_php_versions(state: State<'_, AppState>) -> Result<Vec<PhpVersionView>> {
    let running = state
        .services
        .try_lock()
        .ok()
        .and_then(|mgr| mgr.running_php_patches(state.platform.as_ref()))
        .unwrap_or_default();
    let conn = lock(&state)?;
    let catalog = core::updates::cached(&conn);
    let arch = core::updates::catalog_arch(state.platform.binaries().arch());
    core::php::list_versions(&conn, &running, &catalog, arch)
}

/// The PHP minor FrankenPHP actually serves — its embedded build, never the
/// site's pool (ruled 15 Aug 2026: the SiteDetail picker goes read-only with
/// this annotation for FrankenPHP sites, instead of a `php_version` promise
/// the server cannot keep). Served from the ONE pin (`FRANKENPHP_EMBEDDED_PHP`)
/// so the UI can never carry a second copy that drifts.
#[tauri::command]
pub fn frankenphp_embedded_php() -> String {
    core::php::minor_of(core::binaries::FRANKENPHP_EMBEDDED_PHP)
}

/// Enable (install) or disable (remove) a PHP version. Guarded in `core::php`
/// (can't remove the default or a version a site is using). Installing
/// prefetches the version's FPM + CLI builds right away (hub batch with live
/// progress) instead of silently deferring the download to the next
/// `start_services`; the pool itself still reconciles on the next start.
#[tauri::command]
pub async fn set_php_version_installed(
    state: State<'_, AppState>,
    minor: String,
    installed: bool,
) -> Result<()> {
    {
        // Registry update under a brief DB lock, dropped before any await.
        let conn = lock(&state)?;
        core::php::set_installed(&conn, &minor, installed)?;
    }
    if installed {
        let patches = {
            let conn = lock(&state)?;
            core::php::effective_patches(&conn).unwrap_or_default()
        };
        let plan = core::downloads::plan_for_php_with(state.platform.as_ref(), &minor, &patches);
        core::downloads::prefetch(
            state.platform.as_ref(),
            &format!("Install PHP {minor}"),
            &plan,
        )
        .await?;
    }
    Ok(())
}

/// Make a PHP version the default for new sites (§4.4). Guarded in `core::php`
/// (must be installed). The New Site dialog reads this default.
#[tauri::command]
pub fn set_default_php_version(state: State<'_, AppState>, minor: String) -> Result<()> {
    let conn = lock(&state)?;
    core::php::set_default(&conn, &minor)
}

/// One editable ini setting for the Settings UI: the stored value (None = unset)
/// plus PHP's compiled default (what actually applies when unset — our static
/// builds load no php.ini).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpSettingView {
    pub key: &'static str,
    pub value: Option<String>,
    pub default: &'static str,
}

/// A submitted ini setting (key must be in `core::php::SETTINGS`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpSettingInput {
    pub key: String,
    pub value: String,
}

/// The whitelisted ini settings for one PHP minor: every editable key, with the
/// stored value if any. Keys the user never set report `value: None` and apply
/// the shown default.
#[tauri::command]
pub fn get_php_settings(state: State<'_, AppState>, minor: String) -> Result<Vec<PhpSettingView>> {
    let conn = lock(&state)?;
    let stored = crate::state::store::get_php_settings(&conn, &minor)?;
    Ok(core::php::SETTINGS
        .iter()
        .map(|s| PhpSettingView {
            key: s.key,
            value: stored.iter().find(|(k, _)| k == s.key).map(|(_, v)| v.clone()),
            default: s.default,
        })
        .collect())
}

/// Replace the ini settings for one PHP minor (submitted keys are stored; omitted
/// keys revert to PHP defaults), then make them live: validate → `php-fpm -t` a
/// candidate config → persist → restart that minor's pool → reload nginx (per-site
/// `client_max_body_size` tracks upload/post sizes). Ordered so a bad value costs
/// a restart rather than a pool: typed validation rejects it first, and the `-t`
/// gate runs against a candidate file the live pool never reads. FrankenPHP-override
/// sites are unaffected (own embedded PHP).
///
/// **And it REVERTS if the pool does not come back**, because the two gates above
/// are both parse-time and the failure that matters is not. A value php-fpm
/// accepts and then dies on — a `memory_limit` too small to start a worker — used
/// to stay stored, so every later start failed the same way with nothing on screen
/// connecting the two: a pool the user bricked from the Settings screen and could
/// not unbrick from it. The gate was also validating with the PIN's binary while
/// the pool restarts onto the user's selected patch, which tested a config against
/// an interpreter nobody runs (#353).
#[tauri::command]
pub async fn apply_php_settings(
    state: State<'_, AppState>,
    minor: String,
    settings: Vec<PhpSettingInput>,
) -> Result<()> {
    let pairs: Vec<(String, String)> =
        settings.into_iter().map(|s| (s.key, s.value)).collect();
    let pairs = core::php::validate_settings(&pairs)?;
    // The patch the pool WILL run after the restart below — not the pin. Gating a
    // candidate config with the pin's binary while the pool restarts onto the
    // selection tests a config against an interpreter nobody runs, and an ini key
    // the new patch rejects would then brick the pool the gate exists to protect.
    let patch = {
        let conn = state
            .db
            .lock()
            .map_err(|_| Error::Other("database lock poisoned".into()))?;
        core::php::patch_to_run(&conn, &minor)?
    };
    let patch = patch.as_str();
    let port = core::php::fpm_port(&minor)
        .ok_or_else(|| Error::Other(format!("no fpm port for {minor}")))?;
    let platform = state.platform.as_ref();

    // `php-fpm -t` gate on a CANDIDATE config (the live file is never touched) —
    // only when the binary is already cached: a settings edit must not trigger a
    // PHP download for an uninstalled version (typed validation already ran, and
    // no pool is running for it anyway).
    if core::binaries::is_cached(platform, "php-fpm", patch) {
        let bin = core::binaries::resolve(platform, "php-fpm", patch).await?; // cache hit
        let candidate =
            core::services::write_fpm_config_candidate(platform, &minor, port, &pairs)?;
        let test = core::services::test_fpm_config(platform, &bin, &candidate);
        let _ = std::fs::remove_file(&candidate);
        test?;
    }

    // Persist + snapshot under ONE brief DB lock, dropped before any await.
    // `previous` is what the revert below restores; read in the SAME lock as the
    // write, or a concurrent edit decides what "previous" means.
    let (previous, sites, all) = {
        let conn = lock(&state)?;
        let previous =
            crate::state::store::all_php_settings(&conn)?.remove(&minor).unwrap_or_default();
        crate::state::store::replace_php_settings(&conn, &minor, &pairs)?;
        (previous, core::sites::list(&conn)?, crate::state::store::all_php_settings(&conn)?)
    };

    // Swap the map in + restart the affected pools (normal + debug when both
    // live) + reload nginx, then await readiness OUTSIDE the services lock
    // (locking rule).
    let outcome = async {
        let checks = {
            let mut mgr = state.services.lock().await;
            mgr.apply_php_settings(platform, &state.ca, &sites, all, &minor).await?
        };
        if !checks.is_empty() {
            core::service_manager::await_ready(checks).await?;
        }
        Ok::<(), Error>(())
    }
    .await;

    // Revert if the pool did not come back. Without this the values stay stored,
    // so EVERY later start fails the same way with nothing on screen connecting
    // the two — a pool a user bricked from the Settings screen and cannot unbrick
    // from it. The `php-fpm -t` gate above catches values php-fpm rejects at
    // parse time; it cannot catch one it accepts and then dies on (a
    // `memory_limit` too small to start a worker), and that is the case this
    // exists for. Same shape as `php_update_apply`, for the same reason.
    if let Err(e) = outcome {
        let restore = {
            let conn = lock(&state)?;
            crate::state::store::replace_php_settings(&conn, &minor, &previous)?;
            (core::sites::list(&conn)?, crate::state::store::all_php_settings(&conn)?)
        };
        let back = async {
            let checks = {
                let mut mgr = state.services.lock().await;
                mgr.apply_php_settings(platform, &state.ca, &restore.0, restore.1, &minor).await?
            };
            if !checks.is_empty() {
                core::service_manager::await_ready(checks).await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        // BOTH results. Reporting only the first would say "put back" on the one
        // path where the pool is DOWN — the exact lie the update path told.
        return Err(Error::Other(match back {
            Ok(()) => format!(
                "PHP {minor} did not come back with those settings, so the previous ones \
                 were restored and the pool is running again: {e}"
            ),
            Err(back_err) => format!(
                "PHP {minor} did not come back with those settings ({e}), and restoring the \
                 previous ones ALSO failed ({back_err}) — the pool is DOWN. The stored \
                 settings are the previous ones, so starting the services again will use them."
            ),
        }));
    }
    Ok(())
}

/// Refresh the signed update manifest and report what each installed minor could
/// move to.
///
/// Best-effort by contract: a failure returns an error the row renders as
/// "couldn't check", never a blocked screen. Verification, the serial rule and
/// the structural limits all live in `core::updates`; this command only moves
/// bytes and hands the result to the resolve path.
#[tauri::command]
pub async fn php_update_check(state: State<'_, AppState>) -> Result<Vec<PhpVersionView>> {
    // Fetch UNLOCKED, verify + persist under one brief lock. `fetch` takes no
    // Connection precisely so this cannot be written the other way round.
    let (doc, sig) = core::updates::fetch().await?;
    {
        let conn = lock(&state)?;
        let cat = core::updates::accept(&conn, &doc, &sig)?;
        core::binaries::install_catalog(cat);
    }
    list_php_versions(state)
}

/// Move `minor` onto `patch`, or fail leaving it exactly where it was.
///
/// # The shape, and why each step is where it is
///
/// 1. **Refuse anything the catalog does not vouch for.** The patch must resolve
///    through `binaries::manifest`, which consults the compiled-in pins first and
///    only then the VERIFIED catalog — so a patch nobody signed cannot be named
///    here even by a caller that bypasses the UI.
/// 2. **Download BEFORE persisting.** The prefetch runs with no lock held and no
///    row written, so a failed or cancelled download leaves the minor untouched.
/// 3. **Persist, then restart.** Only after the bytes are cached does the choice
///    become the registry's.
/// 4. **Revert on a pool that does not come back.** The selection returns to what
///    it was and the pool is restarted onto it, so a bad patch costs a restart
///    rather than a broken stack. **The downloaded tree is NOT deleted** — it is
///    valid, verified, and deleting it makes the retry re-fetch ~100MB.
///
/// # What it reports, and why it is not `()`
///
/// It returns [`PhpUpdateOutcome`] rather than success/failure, because "the
/// update worked" and "the running interpreter changed" are different facts and
/// the UI was stating the second while only knowing the first. When no pool is
/// running for the minor there is nothing to restart: the selection is recorded
/// and takes effect at the next start, and a toast saying "PHP 8.2 is now on
/// 8.2.32" there names a process that does not exist.
///
/// The revert leg reports the same way. Its first version discarded BOTH the
/// respawn error and the readiness result while telling the user the minor "was
/// put back on the previous build" — so a revert that itself failed, which is
/// the state where PHP is DOWN, read as a tidy recovery.
#[tauri::command]
pub async fn php_update_apply(
    state: State<'_, AppState>,
    minor: String,
    patch: String,
) -> Result<PhpUpdateOutcome> {
    // One apply per minor at a time. The UI disables the row's button, but a
    // second window, the CLI or a repeated IPC call all reach here directly, and
    // two applies interleaving would race the selection against the restart:
    // whichever revert lands last decides the patch, with no relation to which
    // one failed.
    let _flight = InFlight::claim(&minor)?;
    let platform = state.platform.as_ref();
    if core::php::minor_of(&patch) != minor {
        return Err(Error::Other(format!("{patch} is not a patch of PHP {minor}")));
    }
    // (1) Only a version something vouches for. `manifest` returning a spec IS
    // the check: it means either a compiled-in pin or a signed catalog entry.
    if core::binaries::manifest(
        "php-fpm",
        &patch,
        std::env::consts::OS,
        platform.binaries().arch(),
    )
    .is_none()
    {
        return Err(Error::Other(format!(
            "PHP {patch} is not in this build's pins or in a verified update manifest"
        )));
    }

    // (2) Download first, with nothing written and no lock held.
    let plan = core::downloads::plan_for_php_patch(platform, &minor, &patch);
    core::downloads::prefetch(platform, &format!("Update PHP {minor}"), &plan).await?;

    // (3) Persist the choice, then hand the pool manager the new snapshot.
    let previous = {
        let conn = lock(&state)?;
        let prev = crate::state::store::list_php_versions(&conn)?
            .into_iter()
            .find(|v| v.minor == minor)
            .and_then(|v| v.selected_patch);
        crate::state::store::set_php_selected_patch(&conn, &minor, Some(&patch))?;
        prev
    };
    let checks = {
        let conn_patches = {
            let conn = lock(&state)?;
            core::php::effective_patches(&conn)?
        };
        let mut mgr = state.services.lock().await;
        mgr.set_php_patches(conn_patches);
        mgr.restart_pools_for(platform, std::slice::from_ref(&minor)).await
    };
    // No checks means `stop_one` found nothing to stop: this minor had no pool
    // running, so nothing moved onto the new build and nothing will until the
    // next start. `await_ready` on an empty list succeeds, which is correct and
    // is exactly why the count has to be read HERE.
    let nothing_to_restart = matches!(&checks, Ok(c) if c.is_empty());

    // (4) Revert if the restart failed to spawn OR failed to come back ready.
    let outcome = match checks {
        Err(e) => Err(e),
        Ok(c) => core::service_manager::await_ready(c).await,
    };
    let restarted = match outcome {
        Ok(()) => !nothing_to_restart,
        Err(e) => {
            let restore = {
                let conn = lock(&state)?;
                crate::state::store::set_php_selected_patch(&conn, &minor, previous.as_deref())?;
                core::php::effective_patches(&conn)?
            };
            let back = {
                let mut mgr = state.services.lock().await;
                mgr.set_php_patches(restore);
                mgr.restart_pools_for(platform, std::slice::from_ref(&minor)).await
            };
            // BOTH results, or the message is a guess. The revert failing is the
            // state where PHP is down, and it is the one the user most needs
            // named — the previous version threw both away and said "put back".
            let recovered = match back {
                Ok(c) => core::service_manager::await_ready(c).await,
                Err(e) => Err(e),
            };
            let prev_name = previous.as_deref().unwrap_or("the build this app ships");
            return Err(Error::Other(match recovered {
                Ok(()) => format!(
                    "PHP {minor} did not come back on {patch}, so it was put back on \
                     {prev_name} and is running again: {e}"
                ),
                Err(back_err) => format!(
                    "PHP {minor} did not come back on {patch} ({e}), and putting it back on \
                     {prev_name} ALSO failed ({back_err}) — the pool is DOWN. The selection is \
                     restored, so starting the services again will use {prev_name}."
                ),
            }));
        }
    };
    Ok(PhpUpdateOutcome { patch, restarted })
}

/// What an apply actually changed. See [`php_update_apply`] for why this is not
/// `()` — "recorded" and "now running" are different facts, and the UI was
/// asserting the second from the first.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpUpdateOutcome {
    pub patch: String,
    /// A pool was stopped and came back on the new build. `false` means no pool
    /// was running for this minor: the choice is saved and takes effect at the
    /// next start.
    pub restarted: bool,
}

type MinorSet = std::sync::Mutex<std::collections::HashSet<String>>;

/// The set of minors with an apply in flight, and the guard that releases on
/// every exit path — including a `?` and a panic.
struct InFlight {
    minor: String,
    set: &'static MinorSet,
}

impl InFlight {
    fn set() -> &'static MinorSet {
        static SET: std::sync::OnceLock<MinorSet> = std::sync::OnceLock::new();
        SET.get_or_init(Default::default)
    }

    fn claim(minor: &str) -> Result<Self> {
        Self::claim_in(Self::set(), minor)
    }

    /// [`claim`](Self::claim) against a caller-supplied set — the seam the tests
    /// use, so they exercise the real logic without sharing one global with each
    /// other. Two tests on one static is not a fixture, it is a race: the poison
    /// test broke the serialisation test on the first run, and the pair of them
    /// is what surfaced both bugs below.
    fn claim_in(set: &'static MinorSet, minor: &str) -> Result<Self> {
        // A panic mid-apply poisons the mutex. Recover rather than refuse
        // forever: this guard's job is to serialise, not to become a permanent
        // block on the one button this release exists for. `clear_poison` is the
        // load-bearing half — `into_inner()` alone empties the set while leaving
        // the mutex poisoned, so EVERY later lock still fails and the button
        // stays dead for the rest of the session.
        let mut guard = match set.lock() {
            Ok(g) => g,
            Err(p) => {
                set.clear_poison();
                let mut g = p.into_inner();
                g.clear();
                log::warn!("rexenv: the PHP update in-flight set was poisoned; cleared");
                drop(g);
                return Err(Error::Other(format!(
                    "a previous PHP update for {minor} ended unexpectedly — try again"
                )));
            }
        };
        if !guard.insert(minor.to_string()) {
            return Err(Error::Other(format!("a PHP {minor} update is already running")));
        }
        Ok(Self { minor: minor.to_string(), set })
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        // Poison-tolerant, because the state a claim MUST survive is the one
        // where something panicked. `if let Ok(..)` here silently leaks the
        // claim, and the row's button never comes back.
        let mut set = match self.set.lock() {
            Ok(g) => g,
            Err(p) => {
                self.set.clear_poison();
                p.into_inner()
            }
        };
        set.remove(&self.minor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two applies for the same minor cannot overlap, and the claim is released
    /// on EVERY exit — including the `?` paths, which is most of this command.
    ///
    /// The UI disables the row's button, but that is one client. A second window,
    /// `rex`, the MCP server and a repeated IPC call all reach the command
    /// directly, and two applies interleaving would race the selection against
    /// the restart: whichever revert lands last decides the patch, with no
    /// relation to which one failed.
    fn fresh_set() -> &'static MinorSet {
        Box::leak(Box::new(MinorSet::default()))
    }

    #[test]
    fn one_apply_per_minor_and_the_claim_always_comes_back() {
        let set = fresh_set();
        let a = InFlight::claim_in(set, "8.3").expect("the first claim");
        assert!(InFlight::claim_in(set, "8.3").is_err(), "a second 8.3 apply must be refused");
        // Per MINOR, not global: updating 8.3 must not block 8.2.
        let b = InFlight::claim_in(set, "8.2").expect("a different minor is unrelated work");
        drop(b);
        assert!(InFlight::claim_in(set, "8.2").is_ok(), "8.2's claim did not come back");
        drop(a);
        assert!(
            InFlight::claim_in(set, "8.3").is_ok(),
            "8.3 stayed claimed after its guard dropped — one failed apply would \
             disable the button until the app restarts"
        );
        // The REAL static is what production claims against; prove the wiring,
        // not just the seam, or this whole test could pass against a set nothing
        // reads.
        assert!(std::ptr::eq(InFlight::claim("9.9").unwrap().set, InFlight::set()));
    }

    /// A panic mid-apply must not disable the button for the rest of the session.
    /// A poisoned `Mutex` is the ordinary outcome of that, and the reflexive
    /// `.unwrap()` turns a one-off crash into a permanent refusal of the one
    /// feature this release exists for.
    #[test]
    fn a_poisoned_in_flight_set_recovers_rather_than_blocking_forever() {
        let set = fresh_set();
        let _ = std::panic::catch_unwind(|| {
            let _held = set.lock().unwrap();
            panic!("a panic while the set is held");
        });
        assert!(set.is_poisoned(), "the fixture did not poison it");
        // The first claim after poisoning reports the crash…
        assert!(InFlight::claim_in(set, "8.4").is_err());
        assert!(!set.is_poisoned(), "recovery left the mutex poisoned — every later lock fails");
        // …and the NEXT one works, because the recovery cleared BOTH.
        assert!(
            InFlight::claim_in(set, "8.4").is_ok(),
            "the button stayed dead after a recovered panic"
        );
    }

    /// A claim taken before a panic must still be released. This failed on the
    /// first run: `Drop` used `if let Ok(..)`, which does nothing at all on a
    /// poisoned mutex, so the minor stayed claimed and its button never came
    /// back for the rest of the session.
    #[test]
    fn a_claim_is_released_even_when_the_set_was_poisoned_while_it_was_held() {
        let set = fresh_set();
        let held = InFlight::claim_in(set, "8.5").expect("the claim");
        let _ = std::panic::catch_unwind(|| {
            let _g = set.lock().unwrap();
            panic!("something else panicked meanwhile");
        });
        assert!(set.is_poisoned());
        drop(held);
        assert!(!set.is_poisoned(), "Drop left it poisoned");
        assert!(
            InFlight::claim_in(set, "8.5").is_ok(),
            "the guard leaked its claim through a poisoned mutex"
        );
    }
}
