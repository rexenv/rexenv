//! Live check (#13's PREMISE): a site on an override server has no nginx vhost,
//! so a request carrying its Host falls through to a DIFFERENT site's content.
//!
//!   cargo run --example override_fallthrough_check      (service tier)
//!
//! This is the premise under `tunnels.rs:110`'s refusal to share an override-server
//! site. The refusal itself is unit-proven; what was never shown is that the thing
//! it refuses would actually publish someone else's site.
//!
//! # Why there is no tunnel here
//!
//! The first design stood a real quick tunnel up. It does not need one, and
//! saying why is the useful part: a tunnel's ENTIRE contribution to this
//! mechanism is `--http-host-header <domain>`, which is a Host header on a
//! loopback request to the shared nginx. `tunnel_exposure_check` leg 3 already
//! measured, through a real tunnel, that the Host arriving at nginx is the
//! site's own domain. So the premise is two halves — "the tunnel supplies that
//! Host" (proven live, there) and "nginx with an unknown Host serves another
//! site" (proven here) — and re-standing a public tunnel would re-prove the half
//! that is already done while adding a public URL to a question that has nothing
//! to do with Cloudflare.
//!
//! Everything runs on FIXTURE ports against static files: no php-fpm, no edge,
//! no public exposure.

mod common;

use rexenv_lib::core::{services, sites, ssl};
use rexenv_lib::state::models::{NewSite, SiteDbEngine, SiteType, WebServer};
use std::path::Path;
use std::time::Duration;

/// Fixture ports (see common's port table) — never the production 18088.
const NGINX_PORT: u16 = 18137;

const FIRST: &str = "first.test";
const SECOND: &str = "second.test";
const OVERRIDE_SITE: &str = "fpsite.test";

fn marker(domain: &str) -> String {
    format!("REXENV-SITE-MARKER-{}", domain.replace('.', "-"))
}

#[tokio::main]
async fn main() {
    common::require_ports_free(&[(NGINX_PORT, "this example's fixture nginx")]);

    let (plat, _sandbox) = common::sandbox("ovfall");
    let conn = common::sandbox_db(&*plat);
    let ca = ssl::load_or_create(plat.paths(), plat.permissions()).expect("sandbox CA");

    let mk = |domain: &str, server: WebServer| {
        sites::provision(
            &conn,
            &*plat,
            &ca,
            NewSite {
                name: domain.into(),
                domain: domain.into(),
                site_type: SiteType::Php,
                php_version: "8.3".into(),
                web_server: server,
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
    };
    mk(FIRST, WebServer::Nginx);
    mk(SECOND, WebServer::Nginx);
    mk(OVERRIDE_SITE, WebServer::Frankenphp);

    // Static markers: this check never needs PHP, so it never starts a pool.
    let all = sites::list(&conn).expect("sites");
    for s in &all {
        let root = Path::new(&s.path);
        std::fs::create_dir_all(root).ok();
        std::fs::write(root.join("index.html"), marker(&s.domain)).expect("marker");
    }

    let cfg = sites::rebuild_configs(&conn, &*plat, &ca, NGINX_PORT, 8080, 8443).expect("configs");
    let conf_text = std::fs::read_to_string(&cfg.nginx_conf).expect("read nginx conf");

    // The structural half, asserted before anything is served: the override site
    // has NO server block. If this ever changes, the fallthrough below stops
    // being the mechanism and this check should be re-read rather than re-run.
    let has_first = conf_text.contains(&format!("server_name {FIRST};"));
    let has_second = conf_text.contains(&format!("server_name {SECOND};"));
    let has_override = conf_text.contains(&format!("server_name {OVERRIDE_SITE};"));
    println!("nginx vhosts — {FIRST}={has_first} {SECOND}={has_second} {OVERRIDE_SITE}={has_override}");
    assert!(has_first && has_second, "the two nginx sites must have vhosts");
    assert!(
        !has_override,
        "the override site HAS an nginx vhost — #13's premise no longer holds, and the \
         refusal it justifies needs re-examining rather than this check re-running"
    );

    let nginx_bin = rexenv_lib::core::binaries::resolve(
        &*plat,
        "nginx",
        rexenv_lib::core::binaries::pins().nginx,
    )
    .await
    .expect("nginx");
    let child = services::start_nginx(&*plat, &nginx_bin, &cfg.nginx_conf, &cfg.nginx_prefix)
        .expect("start nginx");
    // Fixture port, so `Reaped`'s sweep is safe here (see its contract).
    let mut nginx = common::Reaped::new(child, NGINX_PORT, "nginx");
    // Gate on the socket, not the clock (`common::await_listening`).
    common::await_listening(NGINX_PORT, "nginx", None);

    let get = |host: &str| -> String {
        // "/index.html", not "/": the generated vhost's try_files sends "/" to
        // index.php and thence to php-fpm, which this check deliberately does not
        // start (502). The static path exercises the same server_name selection
        // — which is the whole mechanism under test — without a pool.
        common::http_get_timeout(NGINX_PORT, host, "/index.html", Duration::from_secs(5))
    };

    // Controls FIRST: vhost routing works, so a fallthrough below is a
    // fallthrough and not "this nginx serves one thing to everybody".
    println!(
        "nginx listening on :{NGINX_PORT} = {}",
        rexenv_lib::core::ports::is_listening(NGINX_PORT)
    );
    let a = get(FIRST);
    let b = get(SECOND);
    let a_ok = a.contains(&marker(FIRST));
    let b_ok = b.contains(&marker(SECOND));
    println!("control — Host {FIRST} serves its own marker: {a_ok}");
    println!("control — Host {SECOND} serves its own marker: {b_ok}");

    // The premise: the override site's Host, which is exactly what cloudflared
    // would send with `--http-host-header`.
    let o = get(OVERRIDE_SITE);
    let served_first = o.contains(&marker(FIRST));
    let served_second = o.contains(&marker(SECOND));
    let served_own = o.contains(&marker(OVERRIDE_SITE));
    println!(
        "premise  — Host {OVERRIDE_SITE} served: first={served_first} second={served_second} own={served_own}"
    );

    nginx.reap();

    let controls_ok = a_ok && b_ok;
    // The premise is CONFIRMED when another site's bytes come back. Note the
    // assertion is that the bad thing happens: this check exists to show the
    // refusal in tunnels.rs is load-bearing, so a green run here means the
    // refusal is protecting against something real.
    let premise_confirmed = (served_first || served_second) && !served_own;

    if !controls_ok {
        eprintln!(
            "\n✗ CONTROLS BROKEN — this run proves NOTHING about the fallthrough.\n  \
             Host-based routing did not serve each nginx site its own marker, so \"another \
             site's content came back\" cannot be distinguished from \"this nginx serves the \
             same thing to every Host\"."
        );
        std::process::exit(1);
    }
    if premise_confirmed {
        println!(
            "\nOK — #13's premise CONFIRMED: an override site has no nginx vhost, and a request \
             carrying its Host is served ANOTHER site's content.\n     \
             That is what tunnels.rs:110 refuses to publish, and it is refusing something real."
        );
    } else {
        eprintln!(
            "\n✗ #13's premise did NOT reproduce: Host {OVERRIDE_SITE} returned neither other \
             site's marker (own={served_own}).\n  \
             If nginx now rejects an unknown Host instead of falling through, the refusal in \
             tunnels.rs may be guarding a hazard that no longer exists — worth re-reading \
             rather than deleting, since the refusal is cheap and the fallthrough is \
             configuration-dependent.\n  body: {o:.200}"
        );
        std::process::exit(1);
    }
}
