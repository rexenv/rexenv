//! Live check: what a PUBLIC quick tunnel does and does not expose — the
//! Tier-1 cluster's cross-site and replay claims, through a real tunnel.
//!
//!   cargo run --example tunnel_exposure_check       (NETWORK + stack stopped)
//!
//! Carries SIX ledger claims on one fixture, because standing up a real
//! cloudflared is the expensive part and each claim needs the same thing in
//! front of it.
//!
//! It was written to carry seven. Two — a magic-link token replayed through the
//! tunnel, with and without a spoofed `X-Forwarded-For` — PASSED with the
//! `wp_login` Cloudflare-header denial deleted from the mu-plugin, so they were
//! not tests and they are gone. Leg 7 passed with `tunnels::stop` neutered until
//! it was changed to leave the ORIGIN running; before that it could not tell a
//! stopped tunnel from a stopped nginx. Both are recorded here because the
//! lesson outlived them: on a fixture this expensive, "we saw it work" is the
//! default failure mode, and only planting tells a leg from a demonstration.
//!
//! # The premise everything else has been assuming
//!
//! `tunnel_muplugin_check` proves the rewriter's behaviour by SIMULATING
//! `HTTP_CF_RAY` in PHP, and `wp_login_check` leg C proves the login denial the
//! same way. Both stand on a premise neither can test: that a real quick tunnel
//! actually supplies those headers. That is a fact about **Cloudflare's edge**,
//! not about our code — legs 1–3 are the only place it is ever checked.
//!
//! # What it does to the machine
//!
//! Creates a sandbox app-data root under the OS temp dir, two fixture sites
//! (`share.test`, `other.test`) with their own docroots, and publishes ONE of
//! them at a random `*.trycloudflare.com` URL for the duration of the legs
//! (~30–60s). The published docroot holds two files: a hello page and an
//! endpoint that echoes the request headers it saw. No site of the user's is
//! reachable through it — leg 6 is what proves that rather than asserting it.
//!
//! # If this dies midway
//!
//! A leaked public tunnel is the one outcome this must not produce, and
//! `process::exit` runs no destructors — which is how a guard in this repo
//! nearly leaked a Mailpit earlier. So the reap is idempotent and runs from
//! THREE places: a Drop guard, every `fail()` before it exits, and a panic hook
//! installed before the tunnel starts. The public URL and the cloudflared pid
//! are printed the moment they exist, so if all of that somehow fails there is
//! something to kill by hand.
//!
//! NOT covered, stated rather than implied: `kill -9` on this example, or the
//! machine losing power. A cloudflared child would survive with a live public
//! URL to a temp docroot. That is the same residual `tunnel_check` already
//! carries — this is not a new exposure.

mod common;

use rexenv_lib::core::service_manager::{Ports, ServiceManager};
use rexenv_lib::core::{binaries, db as coredb, frankenphp, php as corephp, ports as coreports, services, sites, ssl, tunnels, wordpress, wp_login, wp_tunnel};
use rexenv_lib::platform;
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::{Path, PathBuf};

use std::time::Duration;

const HTTPS: u16 = 8443;
const SHARED: &str = "share.test";
const OTHER: &str = "other.test";
/// The OVERRIDE-backed site (leg 9): FrankenPHP, served from its own loopback
/// port rather than the shared nginx.
const OVERRIDE: &str = "fpshare.test";
/// Echoes the headers PHP actually saw. Legs 1–3 are this file, fetched twice.
const ECHO_PHP: &str = r#"<?php
header('Content-Type: text/plain');
foreach (['HTTP_CF_RAY','HTTP_CF_CONNECTING_IP','HTTP_HOST','HTTP_X_FORWARDED_FOR','REMOTE_ADDR'] as $k) {
    echo $k, '=', ($_SERVER[$k] ?? ''), "\n";
}
"#;
const HELLO: &str = "<!doctype html><title>shared</title><p>REXENV-SHARED-SITE</p>";
const HELLO_OTHER: &str = "<!doctype html><title>other</title><p>REXENV-OTHER-SITE</p>";
const HELLO_OVERRIDE: &str = "<!doctype html><title>fp</title><p>REXENV-OVERRIDE-SITE</p>";

/// The STACK this example started. Reaching it from `fail` is what the first
/// version got wrong: the tunnel was reaped from every path and the services
/// were not, so a failed run left nginx, php-fpm and MySQL holding the shared
/// ports — sandbox CONFIG, real PORTS, and the next run refused because of it.
/// Services outliving the app is production's rule; here it is litter.
static STACK: std::sync::Mutex<Option<ServiceManager>> = std::sync::Mutex::new(None);
/// Leg 9's FrankenPHP override backend, reachable from every teardown path.
///
/// **A local `OwnedService` was not enough, and the leak proved it.** `fail()`
/// ends in `process::exit`, which runs NO destructors, so a leg that died after
/// starting the backend left it running — two orphaned FrankenPHP processes on
/// the fixture's override port, found 24 Aug 2026 after two failed runs of this
/// very leg. `reap_all` already drains the tunnel and the stack for exactly this
/// reason; the backend had to join them rather than trust `Drop`.
static OVERRIDE_BACKEND: std::sync::Mutex<Option<common::OwnedService>> =
    std::sync::Mutex::new(None);

/// The tunnel half lives in `common::reap_public_tunnel` now — shared, because a
/// second copy of the thing that stops a public tunnel leaking is a second thing
/// to keep correct, and `common::tunnel_guard_check` proves it fires from Drop,
/// from a panic and from `fail()`'s explicit call.
///
/// Kept separate from the stack teardown on purpose: leg 7 needs the ORIGIN
/// still up, or it cannot tell "the tunnel stopped" from "nginx went away" —
/// which is how its first version passed with `tunnels::stop` neutered.
fn reap_tunnel() {
    common::reap_public_tunnel();
}

fn reap_all() {
    let plat = platform::current();
    reap_tunnel();
    // Before the stack: this backend is the fixture's own, and stopping it first
    // means a failure inside `stop_all` cannot strand it.
    if let Some(mut fp) = OVERRIDE_BACKEND.lock().ok().and_then(|mut b| b.take()) {
        fp.stop();
        eprintln!("tunnel_exposure_check: override backend stopped");
    }
    if let Some(mut mgr) = STACK.lock().ok().and_then(|mut m| m.take()) {
        let _ = mgr.stop_all(&*plat);
        eprintln!("tunnel_exposure_check: stack stopped");
    }
}

struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) {
        reap_all();
    }
}

fn fail(step: &str, why: &str) -> ! {
    reap_all();
    eprintln!("\n✗ {step}\n  {why}\n");
    std::process::exit(1);
}

/// `key=value` out of the echo endpoint's body.
fn field<'a>(body: &'a str, key: &str) -> &'a str {
    body.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap_or("")
        .trim()
}

#[tokio::main]
async fn main() {
    // FIRST STATEMENT, before the sandbox exists and before anything is
    // created. The first version of this ran AFTER provisioning two sites, then
    // reported that it had protected the machine — a precondition that runs
    // after the side effect is not a precondition, and it was believed because
    // of the message it printed rather than what it did (13 Aug 2026).
    common::require_ports_free(&[
        (services::NGINX_HTTP_PORT, "the shared nginx"),
        (coredb::DbEngine::Mysql.port(), "MySQL — a fixture database would land in the USER'S server"),
        (HTTPS, "the edge"),
    ]);

    let (plat, _sandbox) = common::sandbox("tunnel-exposure");
    // Reap before the default hook, so an `.expect()` anywhere still tears the
    // tunnel down — Drop alone does not survive `process::exit`.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reap_all();
        default_hook(info);
    }));
    let _guard = RunGuard;

    // `sandbox_db`, not `db::open_for_platform`: it pins `sites_dir` into the
    // sandbox. Without that pin `sites::provision` writes the docroot to
    // `~/rexenv/Sites` — the user's own folder — because the docroot comes from
    // a SETTING, not from `Paths`.
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");

    // Checked BEFORE anything is provisioned, because after the fact it is a
    // report rather than a guard.
    let sandbox_root = plat.paths().app_data_dir().expect("sandbox root");
    let resolved = sites::sites_dir(&conn, &*plat).expect("resolve the sites dir");
    if !resolved.starts_with(&sandbox_root) {
        fail(
            "REFUSING TO PROVISION — the sites dir is not inside the sandbox",
            &format!(
                "docroots would be created in {}\n  sandbox root is {}\n  \
                 That is the user's own Sites folder. Open the database with \
                 `common::sandbox_db`, which pins the setting.",
                resolved.display(),
                sandbox_root.display()
            ),
        );
    }
    println!("sites dir pinned to {}", resolved.display());

    let mk = |domain: &str, name: &str| {
        sites::provision(
            &conn,
            &*plat,
            &ca,
            NewSite {
                name: name.into(),
                domain: domain.into(),
                site_type: SiteType::Wordpress,
                php_version: "8.3".into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: true,
                git_build_assets: false,
                starter_db: false,
            },
        )
        .expect("provision")
    };
    mk(SHARED, "Shared");
    mk(OTHER, "Other");
    let all = sites::list(&conn).expect("sites");
    let shared = all.iter().find(|s| s.domain == SHARED).expect("shared site").clone();
    let other = all.iter().find(|s| s.domain == OTHER).expect("other site").clone();
    let docroot = PathBuf::from(&shared.path);
    std::fs::write(docroot.join("echo.php"), ECHO_PHP).expect("echo endpoint");
    std::fs::write(docroot.join("hello.html"), HELLO).expect("hello page");
    std::fs::write(Path::new(&other.path).join("hello.html"), HELLO_OTHER).expect("other hello");

    let ports = Ports { http: 8080, https: HTTPS, nginx: services::NGINX_HTTP_PORT };
    let mut mgr = ServiceManager::with_ports(ports);
    let minors = corephp::installed_minors(&conn).expect("php minors");
    let start = mgr.start_all(&*plat, &ca, &all, &minors, binaries::pins().adminer, true).await;
    // Handed over BEFORE the result is inspected: a partial start leaves
    // children running, and `fail` must be able to stop them.
    *STACK.lock().expect("stack slot") = Some(mgr);
    if let Err(e) = start {
        fail("FIXTURE — the stack did not start", &format!("{e}\n  This is a `network`-tier check and needs the stack STOPPED first."));
    }
    for _ in 0..40 {
        if coreports::is_listening(HTTPS) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    // WordPress IS installed here again (14 Aug 2026). The comment this replaces
    // said "nothing left here needs it", which was true when it was written and
    // stopped being true the moment leg 8 came back — so it is spelled out:
    // **do not drop this again without checking leg 8 first.** It costs ~40s and
    // a database on every run of this fixture, including the five claims that do
    // not need it, and that price was paid deliberately: the alternative was a
    // second copy of this file's tunnel-safety scaffolding (panic hook, triple
    // reap, URL capture) living in an example that already had WordPress, and a
    // second copy of the thing that stops a public tunnel leaking is a second
    // thing to keep correct.
    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.expect("php");
    let wp = binaries::resolve_file(&*plat, "wp-cli", binaries::pins().wp_cli).await.expect("wp-cli");
    let (db_client, _) = rexenv_lib::core::db::DbEngine::Mysql
        .sql_client_bins(&*plat, rexenv_lib::core::binaries::pins().mysql)
        .await
        .expect("bundled MySQL client");
    if let Err(e) = wordpress::install_for_site(
        &php,
        &wp,
        &docroot,
        SHARED,
        "Shared",
        &wordpress::db_name_for(SiteType::Wordpress, SHARED),
        &format!("127.0.0.1:{}", coredb::DbEngine::Mysql.port()),
        // The CLIENT binary, never the basedir — the nine-example fix of
        // 15 Jul→14 Aug (docs/archive/SHIPPED-2026-08.md).
        &db_client,
        &Default::default(),
    ) {
        fail("FIXTURE — WordPress did not install on the shared site", &format!("{e}"));
    }

    // ── The tunnel, started as late as possible ─────────────────────────────
    let bin = binaries::resolve(&*plat, "cloudflared", binaries::pins().cloudflared)
        .await
        .expect("cloudflared");
    // Not `wait()`ed on deliberately: the pid is handed to `adopt_public_tunnel`
    // below, and that guard owns the reaping (stop → SIGTERM → SIGKILL) from
    // every teardown path. Waiting here would block for the tunnel's lifetime.
    #[allow(clippy::zombie_processes)]
    let child = tunnels::start(&*plat, &bin, SHARED, services::NGINX_HTTP_PORT).expect("start tunnel");
    let pid = child.id();
    // The shared guard: registers the pid for every teardown path AND proves the
    // process actually died, escalating stop → SIGTERM → SIGKILL and shouting the
    // pid and URL if all three fail. The old `let _ = tunnels::stop(..)` here was
    // silent best-effort — a tunnel that ignored it stayed public with nothing said.
    let _tunnel = common::adopt_public_tunnel(pid, "(URL not captured yet)");
    let mut public = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    while std::time::Instant::now() < deadline {
        if let Some(u) = tunnels::read_url(&*plat, SHARED) {
            public = Some(u);
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let Some(public) = public else {
        fail(
            "FIXTURE — no public URL",
            &format!("cloudflared started (pid {pid}) but never printed a trycloudflare URL"),
        )
    };
    println!("PUBLIC URL {public}  (cloudflared pid {pid} — kill it by hand if this run dies)");
    wp_tunnel::enable(&docroot, "wp-content", &public).expect("tunnel mu-plugin");

    // ── Wait for the name at 1.1.1.1, and PIN it — never the system resolver
    //
    // This is not an optimisation, it is the trap this codebase already paid
    // for. `tunnels::gate_opens`' doc records it: a system query for a quick
    // tunnel's hostname before it propagates lands inside Cloudflare's own
    // window, and `trycloudflare.com`'s SOA MINIMUM of 1800s means that ONE
    // query negative-caches the name on the LAN for up to THIRTY MINUTES
    // (measured 28 Jul 2026 — "a phone worked only on cellular"). Production
    // has a whole phase gate to avoid it; the first version of this example
    // walked straight into it and then could not reach its own tunnel.
    //
    // So: ask 1.1.1.1 directly (the same helper the prober uses — one truth),
    // then hand reqwest the address so no system lookup ever happens.
    common::note_public_tunnel_url(&public);
    let host = public.trim_start_matches("https://").trim_end_matches('/').to_string();
    let mut edge_ip = None;
    for _ in 0..30 {
        if let Some(ips) = tunnels::resolve_at_1111(&host).await {
            if let Some(ip) = ips.first().copied() {
                edge_ip = Some(ip);
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let Some(edge_ip) = edge_ip else {
        fail(
            "FIXTURE — the tunnel hostname never appeared at 1.1.1.1",
            &format!("{host} did not resolve within 60s, so the tunnel never registered. \
                      Nothing downstream can be concluded."),
        )
    };
    println!("tunnel hostname resolves at 1.1.1.1 → {edge_ip} (system resolver never asked)");

    let net = reqwest::Client::builder()
        .resolve(&host, std::net::SocketAddr::from((edge_ip, 443)))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("client");
    // Leg 2 goes to the ORIGIN, not the edge. The claim is "a local request
    // carries no CF headers" — nginx with the site's Host is exactly that, and
    // it does not depend on the edge (Caddy :443) being up, which in a sandbox
    // it need not be.
    let origin = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("origin client");
    let origin_url = format!("http://127.0.0.1:{}/echo.php", services::NGINX_HTTP_PORT);

    // The local origin FIRST: if nginx does not serve the endpoint, nothing
    // about the tunnel can be concluded, and the two failures look identical
    // from the public side.
    let origin_probe = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/echo.php", services::NGINX_HTTP_PORT))
        .header("Host", SHARED)
        .send()
        .await;
    match origin_probe {
        Ok(r) if r.status().is_success() => {}
        other => fail(
            "FIXTURE — the LOCAL origin does not serve the echo endpoint",
            &format!(
                "nginx on :{} with Host {SHARED} said {:?}. The tunnel is not the problem; \
                 nothing downstream of here can be concluded.",
                services::NGINX_HTTP_PORT,
                other.map(|r| r.status().as_u16())
            ),
        ),
    }

    let mut public_echo = String::new();
    let mut last = String::from("(never answered)");
    for _ in 0..12 {
        match net.get(format!("{public}/echo.php")).send().await {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                if status.is_success() && !body.is_empty() {
                    public_echo = body;
                    break;
                }
                last = format!("status={} body={:.160}", status.as_u16(), body);
            }
            Err(e) => last = format!("request failed: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    if public_echo.is_empty() {
        fail(
            "FIXTURE — the public URL never served the echo endpoint",
            &format!("{public}/echo.php over 36s. Last answer: {last}"),
        );
    }

    // ── 1 · a tunnelled request CARRIES the Cloudflare header set ───────────
    //
    // #2's "sound" half and #33's premise. Observed, not assumed: every other
    // check in this repo simulates these headers in PHP.
    let ray = field(&public_echo, "HTTP_CF_RAY");
    let cip = field(&public_echo, "HTTP_CF_CONNECTING_IP");
    if ray.is_empty() || cip.is_empty() {
        fail(
            "1 — a real tunnel did NOT supply the Cloudflare headers",
            &format!(
                "CF-Ray={ray:?} CF-Connecting-IP={cip:?}. Every tunnel-aware guard in rexenv \
                 keys on these: the URL rewriter activates on them, and the magic-link login \
                 DENIES on them. If Cloudflare stopped sending them, the login denial silently \
                 stops firing for tunnelled requests — this leg is the only thing watching.\n  \
                 echo said:\n{public_echo}"
            ),
        );
    }
    println!("1 ok — through the tunnel: CF-Ray={ray} CF-Connecting-IP={cip}");
    println!(
        "     …and what the login gates see: X-Forwarded-For={:?} REMOTE_ADDR={:?}",
        field(&public_echo, "HTTP_X_FORWARDED_FOR"),
        field(&public_echo, "REMOTE_ADDR")
    );

    // ── 2 · the SAME endpoint locally carries NEITHER ───────────────────────
    let local_echo = match origin.get(&origin_url).header("Host", SHARED).send().await {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => fail("2 — the local origin stopped answering", &format!("{e}")),
    };
    if !field(&local_echo, "HTTP_CF_RAY").is_empty()
        || !field(&local_echo, "HTTP_CF_CONNECTING_IP").is_empty()
    {
        fail(
            "2 — a LOCAL request carried Cloudflare headers",
            &format!(
                "the discriminator is not complete: local traffic would be rewritten as though \
                 it were public, and a local magic-link login would be DENIED.\n{local_echo}"
            ),
        );
    }
    println!("2 ok — the same endpoint locally: no CF headers");

    // ── 3 · Host arrives LOCAL through the tunnel ───────────────────────────
    //
    // Why the Host check cannot be what protects a tunnel replay: cloudflared
    // rewrites Host back to the site's own domain, which is in wp_login's
    // allow-list by construction. The tunnel case rests on the CF-header gate
    // and the client-IP gate, and this is the leg that shows it.
    let host = field(&public_echo, "HTTP_HOST");
    if !host.starts_with(SHARED) {
        fail(
            "3 — Host did not arrive as the local domain",
            &format!("Host={host:?}; the tunnel's --http-host-header pin is what keeps WordPress generating local URLs"),
        );
    }
    println!("3 ok — Host through the tunnel is {host} (so the Host gate cannot be the tunnel protection)");

    // ── The magic-link legs are NOT here, deliberately ──────────────────────
    //
    // A leg asserting "a token cannot be replayed through the tunnel" lived
    // here and PASSED with `wp_login`'s Cloudflare-header denial deleted from
    // the mu-plugin — twice, once because the plant itself silently failed to
    // apply. It could not fail, so it was not a test, and it is gone rather
    // than left looking like coverage. What denies the replay is now an open
    // question (ledger #33, and the finding below), and answering it is its own
    // piece of work: it decides whether ONE gate is load-bearing.
    //
    // What IS kept is the measurement that produced the finding — no pass/fail
    // claim attached, because it is an observation about Cloudflare, not about
    // rexenv.
    let spoof_echo = net
        .get(format!("{public}/echo.php"))
        .header("X-Forwarded-For", "127.0.0.1")
        .send()
        .await
        .expect("spoofed echo")
        .text()
        .await
        .unwrap_or_default();
    println!(
        "· measured — a caller-supplied XFF arrives LEFTMOST: X-Forwarded-For={:?}",
        field(&spoof_echo, "HTTP_X_FORWARDED_FOR")
    );
    println!(
        "  wp_login reads explode(',')[0], so that gate is satisfiable by the caller (ledger #307)"
    );

    // ── 6 · the tunnel serves ONE site ──────────────────────────────────────
    //
    // The cross-site negative (#10, #13). The tunnel is pinned to SHARED's
    // Host; asking it for OTHER's content must not produce OTHER's page.
    let shared_page = net
        .get(format!("{public}/hello.html"))
        .send()
        .await
        .expect("shared hello")
        .text()
        .await
        .unwrap_or_default();
    if !shared_page.contains("REXENV-SHARED-SITE") {
        fail("6 — the tunnel did not serve the site it was started for", &format!("got: {shared_page:.200}"));
    }
    let cross = net
        .get(format!("{public}/hello.html"))
        .header("Host", OTHER)
        .send()
        .await
        .expect("cross-site attempt");
    let cross_body = cross.text().await.unwrap_or_default();
    if cross_body.contains("REXENV-OTHER-SITE") {
        fail(
            "6 — a SECOND site was served through a tunnel started for the first",
            &format!(
                "sharing one site published another. cloudflared's --http-host-header pin is \
                 what makes a tunnel single-site; if a caller's Host can override it, every \
                 site on the machine is reachable from the public URL.\n  got: {cross_body:.200}"
            ),
        );
    }
    println!("6 ok — the other site is not reachable through this tunnel");

    // ── 9 · an OVERRIDE site is shared from ITS OWN backend port ────────────
    //
    // The claim (`tunnels::origin_port`, ledger row in docs/TODO.md): a tunnel
    // for a site rexenv does not serve through nginx must point at that site's
    // OWN backend, never at nginx's default. `commands/tunnels.rs` calls
    // `origin_port` and hands the result to `tunnels::start`, and L0 covers
    // every `WebServer` variant — but nothing had ever put a real cloudflared in
    // front of an override backend, which is the one part neither can check.
    //
    // Built as a SECOND tunnel on the same fixture rather than a second fixture:
    // standing up cloudflared is the expensive part, and this needs nothing the
    // first one has except the machine.
    let fp_site = sites::provision(
        &conn,
        &*plat,
        &ca,
        NewSite {
            name: "FrankenPHP share".into(),
            domain: OVERRIDE.into(),
            site_type: SiteType::Php,
            php_version: "8.3".into(),
            web_server: WebServer::Frankenphp,
            path: String::new(),
            db_engine: SiteDbEngine::Mysql,
            git_url: String::new(),
            git_ref: None,
            git_migrate: false,
            git_build_assets: false,
            starter_db: false,
        },
    )
    .expect("provision the override site");
    std::fs::write(Path::new(&fp_site.path).join("hello.html"), HELLO_OVERRIDE)
        .expect("override hello");

    // The port under test: resolved the way production resolves it, not a
    // literal. If this returned nginx's port the leg would still pass while
    // proving the opposite of the claim, so it is asserted BEFORE anything is
    // started.
    let fp_port = tunnels::origin_port(&fp_site).expect("origin port for the override site");
    if fp_port == services::NGINX_HTTP_PORT {
        fail(
            "9 — origin_port handed back NGINX's port for an override site",
            "that is the defect the claim is about: the tunnel would publish whatever \
             nginx serves on that port instead of this site's own backend",
        );
    }
    println!("9 · origin_port({OVERRIDE}) = {fp_port} (nginx is {})", services::NGINX_HTTP_PORT);

    let fp_bin = binaries::resolve(&*plat, "frankenphp", binaries::pins().frankenphp)
        .await
        .expect("frankenphp binary");
    let fp_conf = frankenphp::write_config(
        &*plat,
        &fp_site.domain,
        Path::new(&fp_site.path),
        fp_port,
        rexenv_lib::core::services::RewriteMode::Single,
        &[],
        None,
    )
    .expect("frankenphp config");
    // Handed to the teardown registry rather than held as a local: `fail()` exits
    // the process and `Drop` never runs, which is how two of these leaked.
    *OVERRIDE_BACKEND.lock().expect("override slot") = Some(common::OwnedService::new(
        frankenphp::start(&*plat, &fp_bin, &fp_site.domain, &fp_conf, &[]).expect("start frankenphp"),
        "frankenphp",
    ));
    common::await_listening(fp_port, "the frankenphp override backend", None);
    // …and then wait for it to ANSWER, which is a different fact — the lesson
    // `frankenphp_edge_serve` already carries and this leg re-learned on its
    // first run. FrankenPHP is Caddy underneath: it binds the listener before
    // routes and certificates are loaded, so `await_listening` was satisfied and
    // the very next request came back EMPTY. The control below caught it and
    // refused to conclude anything about the tunnel, which is the control doing
    // exactly its job — but the fixture was the thing at fault, not the backend.
    common::await_ready("the frankenphp override backend to ANSWER", None, || {
        common::http_get(fp_port, OVERRIDE, "/hello.html").contains("REXENV-OVERRIDE-SITE")
    });

    // The LOCAL origin first, for the reason leg 1 does it: if the backend does
    // not serve the page, a public failure and a broken fixture look identical.
    let local_fp = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{fp_port}/hello.html"))
        .header("Host", OVERRIDE)
        .send()
        .await
        .expect("local override request")
        .text()
        .await
        .unwrap_or_default();
    if !local_fp.contains("REXENV-OVERRIDE-SITE") {
        fail(
            "9 — the override backend does not serve its own page locally",
            &format!("nothing about the tunnel can be concluded. got: {local_fp:.200}"),
        );
    }

    #[allow(clippy::zombie_processes)]
    let fp_child = tunnels::start(&*plat, &bin, OVERRIDE, fp_port).expect("start the second tunnel");
    let fp_pid = fp_child.id();
    let _fp_tunnel = common::adopt_public_tunnel(fp_pid, "(second tunnel — URL not captured yet)");
    let mut fp_public = None;
    let fp_deadline = std::time::Instant::now() + Duration::from_secs(45);
    while std::time::Instant::now() < fp_deadline {
        if let Some(u) = tunnels::read_url(&*plat, OVERRIDE) {
            fp_public = Some(u);
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let Some(fp_public) = fp_public else {
        fail(
            "FIXTURE — the second tunnel never printed a URL",
            &format!("cloudflared pid {fp_pid} started for {OVERRIDE} but no trycloudflare URL appeared"),
        )
    };
    common::note_public_tunnel_url(&fp_public);
    println!("9 · second PUBLIC URL {fp_public} (pid {fp_pid})");

    // Same 1.1.1.1 pin as the first tunnel, and for the same reason: one system
    // lookup before propagation negative-caches the name on the LAN for up to
    // thirty minutes.
    let fp_host = fp_public.trim_start_matches("https://").trim_end_matches('/').to_string();
    let mut fp_ip = None;
    for _ in 0..30 {
        if let Some(ips) = tunnels::resolve_at_1111(&fp_host).await {
            if let Some(ip) = ips.first().copied() {
                fp_ip = Some(ip);
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let Some(fp_ip) = fp_ip else {
        fail(
            "FIXTURE — the second tunnel hostname never appeared at 1.1.1.1",
            &format!("{fp_host} did not resolve within 60s; nothing downstream can be concluded"),
        )
    };
    let fp_net = reqwest::Client::builder()
        .resolve(&fp_host, std::net::SocketAddr::from((fp_ip, 443)))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .expect("second client");

    let via_tunnel = fp_net
        .get(format!("{fp_public}/hello.html"))
        .send()
        .await
        .expect("override site through the tunnel")
        .text()
        .await
        .unwrap_or_default();
    if !via_tunnel.contains("REXENV-OVERRIDE-SITE") {
        fail(
            "9 — the public URL did not serve the override site",
            &format!(
                "the tunnel was started on port {fp_port}, which `origin_port` resolved from \
                 the site's recorded backend. got: {via_tunnel:.200}"
            ),
        );
    }
    // …and it must NOT be reaching nginx: the shared site's page must not come
    // back from this tunnel. Without this the leg would pass if `origin_port`
    // were wrong AND nginx happened to serve something containing the marker.
    let fp_cross = fp_net
        .get(format!("{fp_public}/hello.html"))
        .header("Host", SHARED)
        .send()
        .await
        .expect("cross attempt on the override tunnel")
        .text()
        .await
        .unwrap_or_default();
    if fp_cross.contains("REXENV-SHARED-SITE") {
        fail(
            "9 — the override tunnel reached the SHARED nginx site",
            "the tunnel is not pinned to the override backend",
        );
    }
    println!("9 ok — an override site is published from its OWN backend port, through a real tunnel");

    // ── 8 · a magic-link token cannot be replayed through the public URL ───
    //
    // WHAT THIS PROVES, and what it cannot. It proves the PAIR is load-bearing:
    // with the Cloudflare-header gate deleted the Host gate denies in its place,
    // and only with BOTH removed does the replay succeed. It does NOT prove
    // which gate fires on the shipped code — that needed markers compiled into
    // the mu-plugin, run by hand on 14 Aug 2026 (CLAIM-LEDGER #307/#33: `cf`
    // fires first, and the Host gate is never reached). A leg claiming to
    // re-prove that would be claiming what it cannot see.
    //
    // Verified by a TWO-PART plant, because a one-part plant could not fail:
    // deleting the CF gate alone leaves the Host gate denying, which is exactly
    // how the two legs this file used to carry passed while proving nothing.
    //
    // THE CONTROL IS THE SAME TOKEN, deliberately. `wp_login` rejects a tunnel
    // attempt BEFORE consuming the stored option — so a remote caller cannot
    // burn a user's pending token — which means one token shows both halves:
    // denied through the public URL, then still good locally. A second fresh
    // token would leave "denied because it was already spent" open, and that
    // reading is indistinguishable from the real one in the response.
    let (token, _) =
        wp_login::issue(&php, &wp, &docroot, "wp-content", SHARED, 1, wp_login::LOGIN_TTL_SECS)
            .expect("issue a one-time login");
    let magic = format!("?rexenv_login={token}&rexenv_user=1");
    let logged_in = |r: &reqwest::Response| {
        r.headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|c| c.contains("wordpress_logged_in"))
    };
    let replay = net
        .get(format!("{public}/{magic}"))
        .send()
        .await
        .expect("replay through the tunnel");
    let replay_in = logged_in(&replay);
    let control = origin
        .get(format!("http://127.0.0.1:{}/{magic}", services::NGINX_HTTP_PORT))
        .header("Host", SHARED)
        .send()
        .await
        .expect("same token, presented locally");
    let control_in = logged_in(&control);
    // ORDER MATTERS, and the first version had it backwards. A SUCCESSFUL replay
    // consumes the token, so it necessarily breaks the control that follows it —
    // and a control-first check then reports the worst outcome available as
    // "this leg proves NOTHING". Measured 14 Aug 2026 with all three deniers
    // planted out: the replay logged in and the leg blamed its own control.
    // Read the replay first; a broken control CORROBORATES it rather than
    // obscuring it.
    if replay_in {
        fail(
            "8 — a magic-link token REPLAYED through the public tunnel LOGGED IN",
            &format!(
                "a link captured while a site is shared granted a session to whoever held it.\n  \
                 same token still valid locally afterwards: {control_in} (false is EXPECTED here — \
                 the replay consumed it, which is itself confirmation the replay went through)"
            ),
        );
    }
    if !control_in {
        fail(
            "8 — CONTROL BROKEN, this leg proves NOTHING",
            "the replay was denied, but the SAME token did not log in locally either — so \
             \"denied\" is indistinguishable from \"the token was never valid\". Fix the \
             control before believing the denial.",
        );
    }
    println!("8 ok — magic link denied through the public URL; the SAME token then logged in locally");

    // ── 7 · stopping the share really unpublishes it ────────────────────────
    // The control that makes this leg mean anything: the ORIGIN must still be
    // serving after the tunnel dies, or "the URL stopped answering" is just
    // "nginx went away". The first version reaped the stack here too and passed
    // with `tunnels::stop` neutered.
    reap_tunnel();
    let _ = wp_tunnel::disable(&docroot);
    match origin.get(&origin_url).header("Host", SHARED).send().await {
        Ok(r) if r.status().is_success() => {}
        other => fail(
            "7 — the origin died with the tunnel, so this leg proves nothing",
            &format!("nginx said {:?} after the tunnel was stopped", other.map(|r| r.status().as_u16())),
        ),
    }
    let mut still_up = true;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        match net.get(format!("{public}/hello.html")).send().await {
            Ok(r) if r.status().is_success() => continue,
            _ => {
                still_up = false;
                break;
            }
        }
    }
    if still_up {
        fail(
            "7 — the public URL still served the site after the tunnel was stopped",
            &format!("{public} answered for 20s after the stop. A share that outlives its stop is \
                      a site the user believes is private (#25, #190)"),
        );
    }
    println!("7 ok — the public URL stops answering once the share is stopped");

    println!(
        "\n✓ tunnel_exposure_check: a real tunnel carries the CF headers a local request does \
         not, Host arrives local, only the shared site is published, and stopping the tunnel \
         unpublishes it while the origin keeps serving"
    );
}
