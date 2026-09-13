//! Live check (v44): **stopping ONE site takes that site off the air and leaves
//! every other site serving** — through the real edge, the real shared nginx and
//! the real generated configs.
//!
//!   cargo run --example site_stop_start_check      (sandbox tier)
//!
//! # Why this needs to run rather than be unit-tested
//!
//! The unit tests prove the two halves separately: a stopped site gets no nginx
//! server block (`gets_nginx_block`) and its edge route answers 503
//! (`generate_caddyfile`). Neither can say what a BROWSER gets, and that is the
//! whole feature — the failure this design exists to avoid is a stopped site
//! whose hostname falls through to a neighbouring block and quietly serves
//! somebody else's site (exactly what `override_fallthrough_check` measured for
//! override sites). So the assertions here are on bytes off the wire: the
//! neighbour's own marker, the stopped site's 503 with rexenv's words in it, and
//! the neighbour STILL answering while its neighbour is down.
//!
//! Everything is fixture-owned: sandboxed paths (so the generated configs, the
//! nginx prefix and the pid file are the sandbox's, never the running stack's),
//! fixture ports, static `index.html` markers instead of PHP, and every spawned
//! process behind a drop guard.
//!
//! # Why the edge is restarted rather than reloaded between legs
//!
//! `proxy::reload` talks to Caddy's admin UNIX socket, whose path lives under
//! the sandbox root — a temp directory deep enough to approach the kernel's
//! 104-byte sun_path limit. A run that failed there would be failing on the
//! sandbox's path length, not on anything this check is about. nginx IS reloaded
//! in place (its reload is a signal through the sandbox prefix), so the shared
//! tier gets the production path.

mod common;

use rexenv_lib::core::{binaries, proxy, services, sites, ssl};
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use rexenv_lib::state::store;
use std::path::Path;
use std::process::ExitCode;

/// Fixture ports (see common's port table) — never the production 18088/443.
const NGINX_PORT: u16 = 18139;
const EDGE_HTTPS: u16 = 18445;
const EDGE_HTTP: u16 = 18446;

const KEEP: &str = "keep.test";
const STOP: &str = "stopme.test";

fn marker(domain: &str) -> String {
    format!("REXENV-SITE-MARKER-{}", domain.replace('.', "-"))
}

/// The body the edge returned, or the curl error. Body rather than status,
/// because "503" alone does not distinguish rexenv saying the site is stopped
/// from a proxy failing to reach something.
fn body(host: &str, port: u16, ca_pem: &Path) -> String {
    let out = std::process::Command::new("curl").args(["--max-time", "60"])
        .args([
            "-s",
            "--resolve",
            &format!("{host}:{port}:127.0.0.1"),
            "--cacert",
            &ca_pem.display().to_string(),
            &format!("https://{host}:{port}/index.html"),
        ])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => format!("<probe error: {e}>"),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    common::require_ports_free(&[
        (NGINX_PORT, "this example's fixture nginx"),
        (EDGE_HTTPS, "this example's fixture edge (https)"),
        (EDGE_HTTP, "this example's fixture edge (http)"),
    ]);
    let mut checks = common::Check::new("site_stop_start_check");

    let (plat, _sandbox) = common::sandbox("sitestop");
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");

    for domain in [KEEP, STOP] {
        sites::provision(
            &conn,
            &*plat,
            &ca,
            NewSite {
                name: domain.into(),
                domain: domain.into(),
                site_type: SiteType::Php,
                php_version: "8.3".into(),
                web_server: WebServer::Nginx,
                path: String::new(),
                db_engine: SiteDbEngine::Mysql,
                git_url: String::new(),
                git_ref: None,
                git_migrate: false,
                git_build_assets: false,
                starter_db: false,
            },
        )
        .unwrap_or_else(|e| panic!("provision {domain}: {e}"));
    }
    // Static markers — this check never needs PHP, so it never starts a pool.
    for s in sites::list(&conn).expect("sites") {
        let root = Path::new(&s.path);
        std::fs::create_dir_all(root).ok();
        std::fs::write(root.join("index.html"), marker(&s.domain)).expect("marker");
    }

    let nginx_bin = binaries::resolve(&*plat, "nginx", binaries::NGINX_VERSION)
        .await
        .expect("nginx binary (warm cache)");
    let caddy_bin = binaries::resolve(&*plat, "caddy", binaries::CADDY_VERSION)
        .await
        .expect("caddy binary (warm cache)");

    let rebuild = || {
        sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, EDGE_HTTP, EDGE_HTTPS)
            .expect("generate configs")
    };
    let cfg = rebuild();

    let child = services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("start nginx");
    let mut nginx = common::Reaped::new(child, NGINX_PORT, "nginx");
    common::await_listening(NGINX_PORT, "the fixture nginx", None);

    // The edge is restarted per leg (see the header) — this closure is the ONE
    // place that starts it, so every leg gets the same guard and the same gate.
    let start_edge = |caddyfile: &Path| {
        let child = proxy::start(&*plat, &caddy_bin, caddyfile).expect("start caddy");
        let svc = common::OwnedService::new(child, "caddy");
        common::await_listening(EDGE_HTTPS, "the fixture edge", None);
        common::await_answering(KEEP, EDGE_HTTPS, &ca.cert_path, "the fixture edge");
        svc
    };
    let mut edge = start_edge(&cfg.caddyfile);

    // ── Leg 1: the controls ─────────────────────────────────────────────────
    // Without these, "the stopped site did not serve its marker" cannot be told
    // apart from "this fixture never served anything".
    let keep_1 = body(KEEP, EDGE_HTTPS, &ca.cert_path);
    let stop_1 = body(STOP, EDGE_HTTPS, &ca.cert_path);
    checks.is(
        "control: both sites serve their OWN marker through the edge",
        keep_1.contains(&marker(KEEP)) && stop_1.contains(&marker(STOP)),
        &format!("keep={keep_1:?} stop={stop_1:?}"),
    );

    // ── Leg 2: stop ONE site ────────────────────────────────────────────────
    let stop_id = sites::list(&conn)
        .expect("sites")
        .into_iter()
        .find(|s| s.domain == STOP)
        .expect("the site to stop")
        .id;
    store::set_site_enabled(&conn, &stop_id, false).expect("record the switch");
    let cfg = rebuild();
    services::reload_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix, NGINX_PORT)
        .expect("reload nginx");
    edge.stop();
    edge = start_edge(&cfg.caddyfile);

    let conf_text = std::fs::read_to_string(&cfg.nginx_conf).expect("read nginx conf");
    // The stopped site has a block of its OWN — not "no block", which is what
    // this check asserted for one day and what let a tunnel publish a
    // neighbour's site (nginx answers unmatched names from its DEFAULT server).
    let stopped_block = conf_text
        .split(&format!("server_name {STOP};"))
        .nth(1)
        .and_then(|b| b.split("\n\t}").next())
        .unwrap_or("")
        .to_string();
    checks.is(
        "the stopped site has a 503 block of its own, with no route to PHP",
        conf_text.contains(&format!("server_name {KEEP};"))
            && stopped_block.contains("return 503;")
            && !stopped_block.contains("fastcgi_pass"),
        &format!("stopped block was {stopped_block:?}"),
    );

    let stopped = body(STOP, EDGE_HTTPS, &ca.cert_path);
    checks.is(
        "the stopped site answers rexenv's own words, not a proxy error",
        stopped.contains(rexenv_lib::core::stopped_page::STOPPED_HEADLINE)
            && stopped.contains("Nothing is broken"),
        &format!("got {stopped:?}"),
    );
    // The page must name the SITE, not the address the request arrived on. Over
    // a tunnel those differ, and the first version — which read
    // `location.hostname` in the browser — printed the trycloudflare host and a
    // `rex site start …trycloudflare.com` nobody could run.
    checks.is(
        "the page names the site itself, and offers a command that would work",
        stopped.contains(STOP) && stopped.contains(&format!("rex site start {STOP}")),
        &format!("got {stopped:?}"),
    );
    checks.is(
        "…and it asks the browser nothing — no script decides what this page is about",
        !stopped.contains("location.hostname"),
        "the page still guesses its own identity from the client",
    );
    // The failure this whole design is shaped against: with no block of its own,
    // a Host can fall through to another site's — which would publish the
    // neighbour's content at the stopped site's address.
    checks.is(
        "the stopped site serves NOBODY else's content",
        !stopped.contains(&marker(KEEP)) && !stopped.contains(&marker(STOP)),
        &format!("got {stopped:?}"),
    );
    let neighbour = body(KEEP, EDGE_HTTPS, &ca.cert_path);
    checks.is(
        "the neighbour keeps serving while its neighbour is stopped",
        neighbour.contains(&marker(KEEP)),
        &format!("got {neighbour:?}"),
    );

    // ── The leg this check was MISSING, and the owner found live ────────────
    // A public tunnel does not pass the edge: cloudflared proxies straight to
    // the shared nginx with `--http-host-header <domain>`. With no server block
    // of its own, a stopped site's Host matched nothing and nginx answered from
    // its DEFAULT server — a NEIGHBOUR's site, published to the internet at the
    // stopped site's address. So the direct-to-nginx path is probed here with
    // exactly the request a tunnel makes.
    let direct = common::http_get(NGINX_PORT, STOP, "/index.html");
    checks.is(
        "a request straight to nginx (what a tunnel sends) gets the stop page, not a neighbour",
        direct.contains("503")
            && direct.contains(rexenv_lib::core::stopped_page::STOPPED_HEADLINE)
            && direct.contains(STOP)
            && !direct.contains(&marker(KEEP)),
        &format!("got {direct:?}"),
    );
    checks.is(
        "…and a deep path does too, rather than reaching PHP",
        common::http_get(NGINX_PORT, STOP, "/wp-admin/index.php").contains("503"),
        "a stopped site answered something other than 503 on a deep path",
    );
    // The control that makes the two above mean something: the same direct path
    // still serves the RUNNING site its own content.
    let direct_keep = common::http_get(NGINX_PORT, KEEP, "/index.html");
    checks.is(
        "control: the running site still answers directly on nginx",
        direct_keep.contains(&marker(KEEP)),
        &format!("got {direct_keep:?}"),
    );

    // ── Leg 3: start it again ───────────────────────────────────────────────
    store::set_site_enabled(&conn, &stop_id, true).expect("record the switch");
    let cfg = rebuild();
    services::reload_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix, NGINX_PORT)
        .expect("reload nginx");
    edge.stop();
    let mut edge = start_edge(&cfg.caddyfile);

    let back = body(STOP, EDGE_HTTPS, &ca.cert_path);
    checks.is(
        "starting it again serves its own content — on the certificate it kept",
        back.contains(&marker(STOP)),
        &format!("got {back:?}"),
    );

    edge.stop();
    nginx.reap();
    checks.verdict()
}
