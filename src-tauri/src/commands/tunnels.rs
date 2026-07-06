//! commands::tunnels — Tauri IPC for per-site cloudflared quick tunnels (§9.1).
//!
//! Owns the live tunnels in a Tauri-managed registry keyed by site domain. A
//! tunnel can only be started for a real site (looked up by id) — internal
//! tooling vhosts aren't sites, so they can never be shared.

use crate::core::{binaries, services, tunnels, wp_tunnel};
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{Site, SiteType};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

/// Look up the site a tunnel command targets (brief DB lock).
fn tunnel_site(state: &State<'_, AppState>, id: &str) -> Result<Site> {
    let conn = state
        .db
        .lock()
        .map_err(|_| Error::Other("database lock poisoned".into()))?;
    crate::core::sites::get(&conn, id)?.ok_or_else(|| Error::Other(format!("no site {id}")))
}

struct TunnelEntry {
    child: Child,
    url: String,
}

/// Tauri-managed registry of live tunnels, keyed by site domain.
#[derive(Default)]
pub struct Tunnels(Mutex<HashMap<String, TunnelEntry>>);

impl Tunnels {
    /// Remove and kill a site's live tunnel; `true` if one was running. No-op
    /// when the site isn't shared. Used by `stop_tunnel` and site deletion (a
    /// deleted site must not stay publicly reachable).
    pub fn stop_for_domain(&self, platform: &dyn crate::platform::traits::Platform, domain: &str) -> bool {
        let entry = self.0.lock().ok().and_then(|mut m| m.remove(domain));
        match entry {
            Some(mut e) => {
                let _ = tunnels::stop(platform, e.child.id());
                let _ = e.child.wait();
                true
            }
            None => false,
        }
    }
}

/// A tunnel's public status for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub domain: String,
    pub url: String,
    pub running: bool,
}

/// Max time to wait for cloudflared to print the public URL.
const URL_TIMEOUT: Duration = Duration::from_secs(30);

/// Start (or return the existing) public quick tunnel for a site. Returns the
/// public `trycloudflare.com` URL once cloudflared reports it.
#[tauri::command]
pub async fn start_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, Tunnels>,
    id: String,
) -> Result<TunnelInfo> {
    let site = tunnel_site(&state, &id)?;
    let domain = site.domain.clone();

    // Already sharing this site? Return the live URL.
    {
        let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
        if let Some(e) = map.get(&domain) {
            return Ok(TunnelInfo { domain, url: e.url.clone(), running: true });
        }
    }

    let platform = state.platform.as_ref();
    let bin = binaries::resolve(platform, "cloudflared", binaries::CLOUDFLARED_VERSION).await?;
    let child = tunnels::start(platform, &bin, &domain, services::NGINX_HTTP_PORT)?;

    // Poll the log for the public URL (async sleeps — don't block the executor).
    let deadline = Instant::now() + URL_TIMEOUT;
    let url = loop {
        if let Some(u) = tunnels::read_url(platform, &domain) {
            break u;
        }
        if Instant::now() >= deadline {
            let _ = tunnels::stop(platform, child.id());
            let mut c = child;
            let _ = c.wait();
            return Err(Error::Other("cloudflared did not report a public URL in time".into()));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    };

    // WordPress: bake the public origin into the URL-rewrite mu-plugin so the
    // whole site (admin, menus, permalinks, media, previews) is navigable from
    // any device through the tunnel — not just on this machine (§9.2). Sharing
    // without it is broken enough that a write failure fails the start.
    if site.site_type == SiteType::Wordpress {
        if let Err(e) = wp_tunnel::enable(Path::new(&site.path), &url) {
            let _ = tunnels::stop(state.platform.as_ref(), child.id());
            let mut c = child;
            let _ = c.wait();
            return Err(Error::Other(format!(
                "tunnel started but the URL-rewrite mu-plugin could not be written: {e}"
            )));
        }
    }

    tunnels
        .0
        .lock()
        .map_err(|_| Error::Other("tunnel registry poisoned".into()))?
        .insert(domain.clone(), TunnelEntry { child, url: url.clone() });
    Ok(TunnelInfo { domain, url, running: true })
}

/// Stop a site's tunnel (no-op if not sharing).
#[tauri::command]
pub async fn stop_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, Tunnels>,
    id: String,
) -> Result<()> {
    let site = tunnel_site(&state, &id)?;
    tunnels.stop_for_domain(state.platform.as_ref(), &site.domain);
    // Best-effort: the tunnel is already down, so a leftover mu-plugin is inert
    // (its dead URL receives no requests) — don't fail the stop over it.
    if site.site_type == SiteType::Wordpress {
        if let Err(e) = wp_tunnel::disable(Path::new(&site.path)) {
            log::warn!("rexenv: could not remove the tunnel mu-plugin for {}: {e}", site.domain);
        }
    }
    Ok(())
}

/// All active tunnels (domain → public URL).
#[tauri::command]
pub async fn tunnels_status(tunnels: State<'_, Tunnels>) -> Result<Vec<TunnelInfo>> {
    let map = tunnels.0.lock().map_err(|_| Error::Other("tunnel registry poisoned".into()))?;
    let mut out: Vec<TunnelInfo> = map
        .iter()
        .map(|(domain, e)| TunnelInfo { domain: domain.clone(), url: e.url.clone(), running: true })
        .collect();
    out.sort_by(|a, b| a.domain.cmp(&b.domain));
    Ok(out)
}
