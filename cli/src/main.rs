//! `rex` — remote control for the RUNNING rexenv app.
//!
//! Design (see src-tauri/src/cli_server.rs for the server half): this binary
//! is a pure client. It never opens the app database and never touches a
//! process — every command is one JSON line over the app's private `0600`
//! unix socket, executed by the app itself through the same code path the UI
//! uses. If the app isn't running, `rex` says so and exits; it never starts a
//! second backend.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::exit;

const NOT_RUNNING: &str =
    "rexenv isn't running — open the app first (the CLI controls the running app).";

const USAGE: &str = "\
rex — control the running rexenv app

USAGE:
  rex [--json] <command>

COMMANDS:
  status        Services + DNS state (the app's ownership-and-liveness truth)
  start         Start the shared stack (same as the app's Start all)
  stop          Stop the shared stack (same as Stop all)
  restart       stop, then start
  site list     All sites + whether each is actually serving
  site info <domain>    Full detail: config, serving state, cert, resources, WP
  site open <domain>    Open https://<domain> in the browser
  site login <domain>   Open a logged-in wp-admin (magic link; --print to not open)
  site create <domain> [--name N] [--type wordpress|php|laravel] [--php 8.3]
              [--server nginx|frankenphp|apache] [--db mysql|mariadb]
                Create a site (defaults mirror the app's New Site dialog;
                WordPress sites get the one-click install)
  site delete <domain> [--yes]
                Delete a site — drops its database and docroot (asks first)
  site logs <domain> [--source K] [--lines N] [--follow]
                Tail a site's log sources (no --source lists them)
  logs [key] [--lines N] [--follow]
                Tail any service log (no key lists all log files)
  doctor        Diagnose: DNS mode, edge wire identity, port conflicts, CLI link
  db export <domain>
                Dump the site's database to ~/Downloads (prints the path)
  db import <domain> <file.sql> [--yes]
                Import a dump — OVERWRITES the site's tables (asks first)
  php list      Pinned PHP versions: installed, default, pool port
  php default <minor>      Default version for new sites
  php install <minor> / php uninstall <minor>
  site php <domain> <minor>          Switch a site's PHP version
  site xdebug <domain> on|off        Toggle the site's Xdebug debug pool
  wp <domain> plugin list|install|activate|deactivate|update|delete [slug…] [--activate]
  wp <domain> theme  list|install|activate|update|delete [slug…] [--activate]
  wp <domain> user   list|create|set-password|set-role …
                WordPress manager (vetted WP-CLI ops; passwords are
                auto-generated and printed once — never passed on argv)
  service start|stop <mysql|mariadb|postgres|redis|mailpit>
                Start/stop one optional service (web tier stays via rex start/stop)
  mail          List caught messages (Mailpit)
  mail open     Open the Mailpit web UI · mail clear [--yes] deletes ALL messages
  tunnel list | tunnel start|stop <domain>
                Public cloudflared tunnels (start prints the public URL)
  tld [--set <tld>]
                Default TLD for new sites
  version       App + CLI versions
  help          Show this help

OPTIONS:
  --json        Machine-readable output (raw response data)

EXIT CODES:
  0 ok · 1 command failed · 2 rexenv isn't running";

fn socket_path() -> PathBuf {
    // Test/dev override only — there is no discovery protocol, the path is fixed.
    if let Ok(p) = std::env::var("REXENV_CLI_SOCKET") {
        return PathBuf::from(p);
    }
    // Mirrors the app's platform paths (macOS `directories::ProjectDirs`
    // with qualifier "dev", org "rexenv", name "rexenv").
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/dev.rexenv.rexenv/config/rexenv-cli.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("rex: this platform is not supported yet");
        exit(1)
    }
}

/// One request line out, one reply line back. Exits the process on transport
/// or command errors — callers only ever see successful data.
fn request(cmd: &str, args: Value) -> Value {
    let path = socket_path();
    // ENOENT (app never bound) and ECONNREFUSED (stale file after a crash)
    // mean the same thing to the user: the app isn't there to take commands.
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("{NOT_RUNNING}");
            exit(2);
        }
    };
    let line = json!({ "cmd": cmd, "args": args }).to_string();
    if stream
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|_| stream.flush())
        .is_err()
    {
        eprintln!("{NOT_RUNNING}");
        exit(2);
    }
    let mut reply = String::new();
    // No read timeout on purpose: mutating commands (site create) legitimately
    // run for minutes; the app closes the connection when it's done.
    if BufReader::new(stream).read_line(&mut reply).is_err() || reply.trim().is_empty() {
        eprintln!("rex: the app closed the connection without replying");
        exit(1);
    }
    let envelope: Value = match serde_json::from_str(reply.trim()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rex: unreadable reply from the app: {e}");
            exit(1);
        }
    };
    if envelope["ok"] == json!(true) {
        envelope["data"].clone()
    } else {
        let msg = envelope["error"].as_str().unwrap_or("unknown error");
        eprintln!("rex: {msg}");
        exit(1);
    }
}

fn main() {
    let mut json_output = false;
    let mut words: Vec<String> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--json" => json_output = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return;
            }
            _ => words.push(arg),
        }
    }
    match words.first().map(String::as_str) {
        None => println!("{USAGE}"),
        Some("status") => cmd_status(json_output),
        Some("start") => cmd_lifecycle(&["start"], json_output),
        Some("stop") => cmd_lifecycle(&["stop"], json_output),
        Some("restart") => cmd_lifecycle(&["stop", "start"], json_output),
        Some("logs") => cmd_logs(&words[1..], json_output),
        Some("doctor") => cmd_doctor(json_output),
        Some("php") => cmd_php(&words[1..], json_output),
        Some("wp") => cmd_wp(&words[1..], json_output),
        Some("service") => cmd_service(&words[1..], json_output),
        Some("mail") => cmd_mail(&words[1..], json_output),
        Some("tunnel") => cmd_tunnel(&words[1..], json_output),
        Some("tld") => cmd_tld(&words[1..], json_output),
        Some("version") => cmd_version(json_output),
        Some("db") => match words.get(1).map(String::as_str) {
            Some("export") => cmd_db_export(&words[2..], json_output),
            Some("import") => cmd_db_import(&words[2..], json_output),
            _ => {
                eprintln!("rex: usage: rex db <export|import>\n\n{USAGE}");
                exit(1);
            }
        },
        Some("site") => match words.get(1).map(String::as_str) {
            Some("list") => cmd_site_list(json_output),
            Some("create") => cmd_site_create(&words[2..], json_output),
            Some("delete") => cmd_site_delete(&words[2..], json_output),
            Some("info") => cmd_site_info(&words[2..], json_output),
            Some("logs") => cmd_site_logs(&words[2..], json_output),
            Some("php") => cmd_site_php(&words[2..], json_output),
            Some("xdebug") => cmd_site_xdebug(&words[2..], json_output),
            Some("open") => cmd_site_open(&words[2..]),
            Some("login") => cmd_site_login(&words[2..], json_output),
            _ => {
                eprintln!("rex: usage: rex site <list|create|delete|info|open|login>\n\n{USAGE}");
                exit(1);
            }
        },
        Some(other) => {
            eprintln!("rex: unknown command `{other}`\n\n{USAGE}");
            exit(1);
        }
    }
}

fn print_json(data: &Value) {
    println!("{}", serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()));
}

// ── start / stop / restart ───────────────────────────────────────────────────

/// Runs each lifecycle step as its own request; `request` exits on the first
/// failure, so a failed stop never chains into a start. A start can run for a
/// while on a cold cache (the app downloads binaries) — say so up front.
fn cmd_lifecycle(steps: &[&str], json_output: bool) {
    for step in steps {
        if !json_output {
            match *step {
                "start" => println!("starting services… (first run may download binaries)"),
                _ => println!("stopping services…"),
            }
        }
        request(step, Value::Null);
        if !json_output {
            println!("✓ {step} done");
        }
    }
    if json_output {
        print_json(&json!({ "ok": true }));
    }
}

// ── site list ────────────────────────────────────────────────────────────────

fn cmd_site_list(json_output: bool) {
    let data = request("site.list", Value::Null);
    if json_output {
        return print_json(&data);
    }
    let Some(sites) = data["sites"].as_array() else {
        return println!("(no sites)");
    };
    if sites.is_empty() {
        return println!("no sites yet — create one with the app or `rex site create <domain>`");
    }
    // `serving` is the live wire truth (edge up AND the site's upstream up);
    // sites the stack isn't serving right now show "down".
    let serving: Vec<(&str, bool)> = data["serving"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| Some((r["domain"].as_str()?, r["serving"] == json!(true))))
                .collect()
        })
        .unwrap_or_default();
    let col = |key: &str, min: usize| -> usize {
        sites
            .iter()
            .filter_map(|s| s[key].as_str())
            .map(str::len)
            .max()
            .unwrap_or(min)
            .max(min)
    };
    let (dw, nw) = (col("domain", 6), col("name", 4));
    println!("{:<dw$}  {:<nw$}  {:<9}  {:<5}  {:<10}  {:<7}  STATE", "DOMAIN", "NAME", "TYPE", "PHP", "SERVER", "DB");
    for s in sites {
        let domain = s["domain"].as_str().unwrap_or("?");
        let up = serving.iter().any(|(d, up)| *d == domain && *up);
        println!(
            "{:<dw$}  {:<nw$}  {:<9}  {:<5}  {:<10}  {:<7}  {}",
            domain,
            s["name"].as_str().unwrap_or("?"),
            s["type"].as_str().unwrap_or("?"),
            s["phpVersion"].as_str().unwrap_or("?"),
            s["webServer"].as_str().unwrap_or("?"),
            s["dbEngine"].as_str().unwrap_or("?"),
            if up { "serving" } else { "down" },
        );
    }
}

// ── site create / delete ─────────────────────────────────────────────────────

fn flag_value(words: &[String], flag: &str) -> Option<String> {
    words
        .iter()
        .position(|w| w == flag)
        .and_then(|i| words.get(i + 1))
        .cloned()
}

fn cmd_site_create(words: &[String], json_output: bool) {
    let Some(domain) = words.first().filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site create <domain> [--name N] [--type T] [--php V] [--server S] [--db D]");
        exit(1);
    };
    let mut args = serde_json::Map::new();
    args.insert("domain".into(), json!(domain));
    for (flag, key) in
        [("--name", "name"), ("--type", "type"), ("--php", "php"), ("--server", "server"), ("--db", "db")]
    {
        if let Some(v) = flag_value(words, flag) {
            args.insert(key.into(), json!(v));
        }
    }
    if !json_output {
        println!("creating {domain}… (WordPress sites install on first create — this can take a minute)");
    }
    let created = request("site.create", Value::Object(args));
    if json_output {
        return print_json(&created);
    }
    println!(
        "✓ created {} ({}, PHP {}, {}, {}) → https://{}",
        created["domain"].as_str().unwrap_or(domain),
        created["type"].as_str().unwrap_or("?"),
        created["phpVersion"].as_str().unwrap_or("?"),
        created["webServer"].as_str().unwrap_or("?"),
        created["dbEngine"].as_str().unwrap_or("?"),
        created["domain"].as_str().unwrap_or(domain),
    );
}

/// Resolve a `<domain>` argument to the site object via the app's own list —
/// the same lookup `site delete` does; exits with a helpful error otherwise.
fn find_site(words: &[String], usage: &str) -> Value {
    let Some(domain) = words.first().filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: {usage}");
        exit(1);
    };
    let data = request("site.list", Value::Null);
    let site = data["sites"]
        .as_array()
        .and_then(|sites| sites.iter().find(|s| s["domain"] == json!(domain)))
        .cloned();
    match site {
        Some(site) => site,
        None => {
            eprintln!("rex: no site with domain `{domain}` (see `rex site list`)");
            exit(1);
        }
    }
}

/// macOS default-browser open; prints the URL either way so the command is
/// still useful over SSH or when `open` is unavailable.
fn open_url(url: &str) {
    println!("{url}");
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).status();
    }
}

fn cmd_site_info(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site info <domain>");
    let data = request("site.info", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    let s = &data["site"];
    let field = |label: &str, v: String| println!("{label:<12} {v}");
    let str_of = |v: &Value| v.as_str().unwrap_or("?").to_string();
    field("domain", format!("https://{}", str_of(&s["domain"])));
    field("name", str_of(&s["name"]));
    field("state", if data["serving"] == json!(true) { "serving".into() } else { "down".into() });
    field(
        "type",
        format!(
            "{}{}",
            str_of(&s["type"]),
            data["wp"]["version"].as_str().map(|v| format!(" {v}")).unwrap_or_default()
        ),
    );
    if data["wp"]["multisite"] == json!(true) || s["multisite"].as_str().is_some_and(|m| m != "none") {
        field("multisite", str_of(&s["multisite"]));
    }
    field(
        "php",
        format!(
            "{}{}",
            str_of(&s["phpVersion"]),
            if s["xdebug"] == json!(true) { " (Xdebug)" } else { "" }
        ),
    );
    field("server", str_of(&s["webServer"]));
    field("database", format!("{} ({})", str_of(&s["dbEngine"]), str_of(&s["dbName"])));
    field("path", str_of(&s["path"]));
    if let Some(days) = data["cert"]["daysLeft"].as_i64() {
        field("cert", format!("{days} days left (expires {})", str_of(&data["cert"]["notAfter"])));
    }
    let res = &data["resources"];
    if res.is_object() {
        let mut parts = Vec::new();
        if let Some(c) = res["cpuPercent"].as_f64() {
            parts.push(format!("cpu {c:.1}%"));
        }
        if let Some(r) = res["ramMb"].as_u64() {
            parts.push(format!("ram {r} MB"));
        }
        if let Some(r) = res["requestsPerMin"].as_u64() {
            parts.push(format!("{r} req/min"));
        }
        if !parts.is_empty() {
            field("resources", parts.join(" · "));
        }
    }
    field("created", str_of(&s["createdAt"]));
}

fn cmd_site_open(words: &[String]) {
    let site = find_site(words, "rex site open <domain>");
    open_url(&format!("https://{}", site["domain"].as_str().unwrap_or_default()));
}

fn cmd_site_login(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site login <domain> [--print]");
    if site["type"] != json!("wordpress") {
        eprintln!("rex: `{}` is not a WordPress site", site["domain"].as_str().unwrap_or("?"));
        exit(1);
    }
    let data = request("site.login", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    let url = data["url"].as_str().unwrap_or_default();
    if words.iter().any(|w| w == "--print") {
        println!("{url}");
    } else {
        open_url(url);
    }
}

// ── php versions ─────────────────────────────────────────────────────────────

fn cmd_php(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        Some("list") | None => {
            let data = request("php.list", Value::Null);
            if json_output {
                return print_json(&data);
            }
            let Some(versions) = data["versions"].as_array() else { return println!("(none)") };
            println!("{:<7} {:<9} {:<6} {:<10} DEFAULT", "MINOR", "PATCH", "PORT", "INSTALLED");
            for v in versions {
                println!(
                    "{:<7} {:<9} {:<6} {:<10} {}",
                    v["minor"].as_str().unwrap_or("?"),
                    v["patch"].as_str().unwrap_or("?"),
                    v["fpmPort"].as_u64().unwrap_or(0),
                    if v["installed"] == json!(true) { "yes" } else { "-" },
                    if v["isDefault"] == json!(true) { "✓" } else { "" },
                );
            }
        }
        Some("default") => {
            let Some(minor) = words.get(1) else {
                eprintln!("rex: usage: rex php default <minor>");
                exit(1);
            };
            request("php.default", json!({ "minor": minor }));
            println!("✓ PHP {minor} is the default for new sites");
        }
        Some(action @ ("install" | "uninstall")) => {
            let Some(minor) = words.get(1) else {
                eprintln!("rex: usage: rex php {action} <minor>");
                exit(1);
            };
            if action == "install" {
                println!("installing PHP {minor}… (binaries download on first start)");
            }
            request("php.installed", json!({ "minor": minor, "installed": action == "install" }));
            println!("✓ PHP {minor} {}", if action == "install" { "installed" } else { "uninstalled" });
        }
        _ => {
            eprintln!("rex: usage: rex php <list|default|install|uninstall>\n\n{USAGE}");
            exit(1);
        }
    }
}

/// Print the post-switch site line the backend returns (the updated row).
fn print_site_update(site: &Value) {
    println!(
        "✓ {} — PHP {}{} on {}",
        site["domain"].as_str().unwrap_or("?"),
        site["phpVersion"].as_str().unwrap_or("?"),
        if site["xdebug"] == json!(true) { " (Xdebug)" } else { "" },
        site["webServer"].as_str().unwrap_or("?"),
    );
}

fn cmd_site_php(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site php <domain> <minor>");
    let Some(minor) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site php <domain> <minor>");
        exit(1);
    };
    if !json_output {
        println!("switching {} to PHP {minor}…", site["domain"].as_str().unwrap_or("?"));
    }
    let updated = request("site.php", json!({ "id": site["id"], "version": minor }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

fn cmd_site_xdebug(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site xdebug <domain> on|off");
    let enabled = match words.get(1).map(String::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => {
            eprintln!("rex: usage: rex site xdebug <domain> on|off");
            exit(1);
        }
    };
    let updated = request("site.xdebug", json!({ "id": site["id"], "enabled": enabled }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

// ── service / mail / tunnel / tld / version ──────────────────────────────────

fn cmd_service(words: &[String], json_output: bool) {
    let (action, name) = (words.first().map(String::as_str), words.get(1).map(String::as_str));
    let (Some(action @ ("start" | "stop")), Some(name)) = (action, name) else {
        eprintln!("rex: usage: rex service start|stop <mysql|mariadb|postgres|redis|mailpit>");
        exit(1);
    };
    let running = action == "start";
    let r = if name == "mailpit" {
        request("service.mail", json!({ "running": running }))
    } else {
        request("service.db", json!({ "key": name, "running": running }))
    };
    if json_output {
        return print_json(&r);
    }
    println!("✓ {name} {}", if running { "started" } else { "stopped" });
}

fn cmd_mail(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        None | Some("list") => {
            let data = request("mail.list", Value::Null);
            if json_output {
                return print_json(&data);
            }
            let (total, unread) = (data["total"].as_i64().unwrap_or(0), data["unread"].as_i64().unwrap_or(0));
            println!("{total} message{} ({unread} unread)", if total == 1 { "" } else { "s" });
            for m in data["messages"].as_array().map(Vec::as_slice).unwrap_or_default() {
                println!(
                    "{} {:<28} {}",
                    if m["read"] == json!(true) { " " } else { "•" },
                    m["from"]["address"].as_str().unwrap_or("?"),
                    m["subject"].as_str().unwrap_or(""),
                );
            }
        }
        Some("open") => {
            let status = request("mail.status", Value::Null);
            match status["uiUrl"].as_str().filter(|u| !u.is_empty()) {
                Some(url) => open_url(url),
                None => {
                    let port = status["httpPort"].as_u64().unwrap_or(18025);
                    open_url(&format!("http://127.0.0.1:{port}"));
                }
            }
        }
        Some("clear") => {
            if !words.iter().any(|w| w == "--yes") {
                eprint!("delete ALL caught messages? [y/N] ");
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted");
                    exit(1);
                }
            }
            let r = request("mail.clear", Value::Null);
            if json_output {
                return print_json(&r);
            }
            println!("✓ mailbox cleared");
        }
        _ => {
            eprintln!("rex: usage: rex mail [list|open|clear]");
            exit(1);
        }
    }
}

fn cmd_tunnel(words: &[String], json_output: bool) {
    match words.first().map(String::as_str) {
        None | Some("list") => {
            let data = request("tunnel.list", Value::Null);
            if json_output {
                return print_json(&data);
            }
            let tunnels = data["tunnels"].as_array().map(Vec::as_slice).unwrap_or_default();
            if tunnels.is_empty() {
                return println!("no public tunnels running");
            }
            for t in tunnels {
                println!("{:<24} {}", t["domain"].as_str().unwrap_or("?"), t["url"].as_str().unwrap_or(""));
            }
        }
        Some(act @ ("start" | "stop")) => {
            let site = find_site(&words[1..], "rex tunnel start|stop <domain>");
            if act == "start" && !words.iter().any(|w| w == "--yes") {
                eprint!(
                    "expose {} PUBLICLY via a cloudflared tunnel? [y/N] ",
                    site["domain"].as_str().unwrap_or("?")
                );
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted (nothing exposed)");
                    exit(1);
                }
            }
            let r = request(&format!("tunnel.{act}"), json!({ "id": site["id"] }));
            if json_output {
                return print_json(&r);
            }
            if act == "start" {
                println!("✓ public URL: {}", r["url"].as_str().unwrap_or("?"));
            } else {
                println!("✓ tunnel stopped");
            }
        }
        _ => {
            eprintln!("rex: usage: rex tunnel [list|start <domain>|stop <domain>]");
            exit(1);
        }
    }
}

fn cmd_tld(words: &[String], json_output: bool) {
    if let Some(tld) = flag_value(words, "--set") {
        let r = request("tld.set", json!({ "tld": tld }));
        if json_output {
            return print_json(&r);
        }
        return println!("✓ new sites default to .{tld}");
    }
    let data = request("tld.get", Value::Null);
    if json_output {
        return print_json(&data);
    }
    println!(".{}", data["tld"].as_str().unwrap_or("?"));
}

fn cmd_version(json_output: bool) {
    let data = request("version", Value::Null);
    if json_output {
        return print_json(&json!({ "app": data, "cli": env!("CARGO_PKG_VERSION") }));
    }
    println!(
        "rexenv {} ({}) · rex {}",
        data["version"].as_str().unwrap_or("?"),
        data["platform"].as_str().unwrap_or("?"),
        env!("CARGO_PKG_VERSION"),
    );
}

// ── wp: plugins / themes / users ─────────────────────────────────────────────

const WP_USAGE: &str = "rex wp <domain> <plugin|theme|user> <action> …";

/// 16-char password from /dev/urandom — generated and PRINTED ONCE instead of
/// ever accepting one on argv (argv is world-readable via ps).
fn generate_password() -> String {
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 16];
    if std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").expect("urandom"),
        &mut bytes,
    )
    .is_err()
    {
        eprintln!("rex: could not read /dev/urandom");
        exit(1);
    }
    bytes.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect()
}

fn cmd_wp(words: &[String], json_output: bool) {
    let site = find_site(words, WP_USAGE);
    if site["type"] != json!("wordpress") {
        eprintln!("rex: `{}` is not a WordPress site", site["domain"].as_str().unwrap_or("?"));
        exit(1);
    }
    let id = site["id"].clone();
    let (area, action) = (words.get(1).map(String::as_str), words.get(2).map(String::as_str));
    let rest: Vec<String> =
        words.iter().skip(3).filter(|w| !w.starts_with("--")).cloned().collect();
    let activate = words.iter().any(|w| w == "--activate");
    match (area, action) {
        (Some("plugin"), Some("list")) | (Some("plugin"), None) => {
            let data = request("wp.plugins", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["plugins"].as_array() else { return println!("(none)") };
            for p in rows {
                println!(
                    "{:<32} {:<9} {:<10} {}",
                    p["name"].as_str().unwrap_or("?"),
                    p["status"].as_str().unwrap_or(""),
                    p["version"].as_str().unwrap_or(""),
                    if p["update"] == json!("available") { "update available" } else { "" },
                );
            }
        }
        (Some("plugin"), Some("install")) => {
            let Some(slug) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> plugin install <slug> [--activate]");
                exit(1);
            };
            let r = request("wp.plugin.install", json!({ "id": id, "slug": slug, "activate": activate }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ installed {slug}{}", if activate { " (activated)" } else { "" });
        }
        (Some("plugin"), Some(act @ ("activate" | "deactivate" | "update" | "delete"))) => {
            if rest.is_empty() {
                eprintln!("rex: usage: rex wp <domain> plugin {act} <name…>");
                exit(1);
            }
            let r = request(&format!("wp.plugin.{act}"), json!({ "id": id, "names": rest }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {act}d: {}", rest.join(", "));
        }
        (Some("theme"), Some("list")) | (Some("theme"), None) => {
            let data = request("wp.themes", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["themes"].as_array() else { return println!("(none)") };
            for t in rows {
                println!(
                    "{:<32} {:<9} {:<10} {}",
                    t["name"].as_str().unwrap_or("?"),
                    t["status"].as_str().unwrap_or(""),
                    t["version"].as_str().unwrap_or(""),
                    if t["update"] == json!("available") { "update available" } else { "" },
                );
            }
        }
        (Some("theme"), Some("install")) => {
            let Some(slug) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> theme install <slug> [--activate]");
                exit(1);
            };
            let r = request("wp.theme.install", json!({ "id": id, "slug": slug, "activate": activate }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ installed {slug}{}", if activate { " (activated)" } else { "" });
        }
        (Some("theme"), Some("activate")) => {
            let Some(name) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> theme activate <name>");
                exit(1);
            };
            let r = request("wp.theme.activate", json!({ "id": id, "name": name }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ activated {name}");
        }
        (Some("theme"), Some(act @ ("update" | "delete"))) => {
            if rest.is_empty() {
                eprintln!("rex: usage: rex wp <domain> theme {act} <name…>");
                exit(1);
            }
            let r = request(&format!("wp.theme.{act}"), json!({ "id": id, "names": rest }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {act}d: {}", rest.join(", "));
        }
        (Some("user"), Some("list")) | (Some("user"), None) => {
            let data = request("wp.users", json!({ "id": id }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["users"].as_array() else { return println!("(none)") };
            println!("{:<5} {:<20} {:<30} ROLES", "ID", "LOGIN", "EMAIL");
            for u in rows {
                println!(
                    "{:<5} {:<20} {:<30} {}",
                    u["id"].as_u64().unwrap_or(0),
                    u["login"].as_str().unwrap_or("?"),
                    u["email"].as_str().unwrap_or(""),
                    u["roles"].as_str().unwrap_or(""),
                );
            }
        }
        (Some("user"), Some("create")) => {
            let (Some(login), Some(email)) = (rest.first(), rest.get(1)) else {
                eprintln!("rex: usage: rex wp <domain> user create <login> <email> [--role R]");
                exit(1);
            };
            let role = flag_value(words, "--role").unwrap_or_else(|| "subscriber".into());
            let password = generate_password();
            let r = request(
                "wp.user.create",
                json!({ "id": id, "login": login, "email": email, "role": role, "password": password }),
            );
            if json_output {
                return print_json(&r);
            }
            println!("✓ created {login} ({role})\n  password: {password}   (shown once — store it now)");
        }
        (Some("user"), Some(act @ ("set-password" | "set-role"))) => {
            let Some(who) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> user {act} <login|id> [role]");
                exit(1);
            };
            // Accept a login or a numeric id; resolve via the app's own list.
            let user_id = who.parse::<u64>().ok().unwrap_or_else(|| {
                request("wp.users", json!({ "id": id }))["users"]
                    .as_array()
                    .and_then(|users| {
                        users.iter().find(|u| u["login"] == json!(who)).and_then(|u| u["id"].as_u64())
                    })
                    .unwrap_or_else(|| {
                        eprintln!("rex: no user `{who}` on this site (see `rex wp … user list`)");
                        exit(1);
                    })
            });
            if act == "set-password" {
                let password = generate_password();
                let r = request(
                    "wp.user.password",
                    json!({ "id": id, "userId": user_id, "password": password }),
                );
                if json_output {
                    return print_json(&r);
                }
                println!("✓ password reset for {who}\n  password: {password}   (shown once — store it now)");
            } else {
                let Some(role) = rest.get(1) else {
                    eprintln!("rex: usage: rex wp <domain> user set-role <login|id> <role>");
                    exit(1);
                };
                let r = request("wp.user.role", json!({ "id": id, "userId": user_id, "role": role }));
                if json_output {
                    return print_json(&r);
                }
                println!("✓ {who} is now {role}");
            }
        }
        _ => {
            eprintln!("rex: usage: {WP_USAGE}\n\n{USAGE}");
            exit(1);
        }
    }
}

// ── db export / import ───────────────────────────────────────────────────────

fn cmd_db_export(words: &[String], json_output: bool) {
    let site = find_site(words, "rex db export <domain>");
    if !json_output {
        println!("exporting {}…", site["domain"].as_str().unwrap_or("?"));
    }
    let data = request("db.export", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    println!("✓ exported → {}", data["path"].as_str().unwrap_or("?"));
}

fn cmd_db_import(words: &[String], json_output: bool) {
    let site = find_site(words, "rex db import <domain> <file.sql> [--yes]");
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    let Some(file) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex db import <domain> <file.sql> [--yes]");
        exit(1);
    };
    // Absolute path client-side: the APP resolves relative paths against ITS
    // cwd, not this shell's.
    let file = match std::fs::canonicalize(file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rex: cannot read {file}: {e}");
            exit(1);
        }
    };
    if !words.iter().any(|w| w == "--yes") {
        eprint!(
            "import into {domain}? The dump's tables OVERWRITE existing ones \
             (tip: `rex db export {domain}` first). [y/N] "
        );
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err()
            || !matches!(answer.trim(), "y" | "Y" | "yes")
        {
            eprintln!("aborted (nothing imported)");
            exit(1);
        }
    }
    let result = request(
        "db.import",
        json!({ "id": site["id"], "path": file.to_string_lossy() }),
    );
    if json_output {
        return print_json(&result);
    }
    println!("✓ imported {} into {domain}", file.display());
}

// ── doctor ───────────────────────────────────────────────────────────────────

/// Exit 0 = healthy; exit 1 = at least one finding (scriptable gate).
fn cmd_doctor(json_output: bool) {
    let data = request("doctor", Value::Null);
    if json_output {
        print_json(&data);
        // json mode still gates the exit code so CI can use it
    }
    let mut findings = 0;
    let mut line = |ok: bool, warn: bool, label: &str, msg: String| {
        let mark = if ok { "✓" } else if warn { "⚠" } else { "✗" };
        if !ok {
            findings += 1;
        }
        if !json_output {
            println!("{mark} {label:<9} {msg}");
        }
    };

    let app = &data["app"];
    if !json_output {
        println!("rexenv {} ({})", app["version"].as_str().unwrap_or("?"), app["platform"].as_str().unwrap_or("?"));
    }

    let dns = &data["dns"];
    let dns_running = dns["running"] == json!(true);
    let mode = dns["mode"].as_str().unwrap_or("down");
    let resolver = dns["resolverInstalled"] == json!(true);
    let ca = dns["caTrusted"] == json!(true);
    line(
        dns_running && mode == "agent" && resolver && ca,
        dns_running, // running-but-degraded = warning, not failure
        "DNS",
        if !dns_running {
            "not answering — sites won't resolve (open the app / check Settings)".into()
        } else {
            format!(
                "{} · resolver {} · CA {}",
                if mode == "agent" { "agent (always on)".to_string() } else { format!("{mode} — stops when the app quits") },
                if resolver { "installed" } else { "MISSING (run system setup)" },
                if ca { "trusted" } else { "NOT TRUSTED (Settings → Re-trust)" },
            )
        },
    );

    let edge = &data["edge"];
    if edge["running"] == json!(true) {
        if edge["wireOurs"] == json!(true) {
            line(true, false, "Edge", "answering as rexenv on :443".into());
        } else {
            let holder = edge["conflict"]["holder"].as_str().unwrap_or("another proxy");
            let fix = edge["conflict"]["fix"].as_str().map(|f| format!("\n            $ {f}")).unwrap_or_default();
            line(false, false, "Edge", format!("{holder} answers :443 IN FRONT of rexenv — sites unreachable{fix}"));
        }
    } else {
        line(false, true, "Edge", "not running — Start all to serve sites".into());
    }

    let services = data["services"].as_array().cloned().unwrap_or_default();
    let up = services.iter().filter(|s| s["running"] == json!(true)).count();
    line(true, false, "Services", format!("{up}/{} running", services.len()));

    let conflicts = data["portConflicts"].as_array().cloned().unwrap_or_default();
    if conflicts.is_empty() {
        line(true, false, "Ports", "no foreign holders on rexenv ports".into());
    } else {
        for c in &conflicts {
            let fix = c["fix"].as_str().map(|f| format!("\n            $ {f}")).unwrap_or_default();
            line(
                false,
                false,
                "Ports",
                format!(
                    "port {} (needed by {}) held by {}{fix}",
                    c["port"],
                    c["service"].as_str().unwrap_or("?"),
                    c["holder"].as_str().unwrap_or("an unknown process"),
                ),
            );
        }
    }

    let cli = &data["cli"];
    if cli.is_object() {
        line(
            cli["current"] == json!(true),
            true, // absent/stale link is a warning, not a fault
            "CLI",
            if cli["current"] == json!(true) {
                format!("{} → this app", cli["linkPath"].as_str().unwrap_or("?"))
            } else if cli["installed"] == json!(true) {
                "rex on PATH points at a different copy (Settings → Reinstall)".into()
            } else {
                "rex not on PATH (Settings → Command-line tool → Install)".into()
            },
        );
    }

    if findings > 0 {
        if !json_output {
            println!("\n{findings} finding{}", if findings == 1 { "" } else { "s" });
        }
        exit(1);
    }
}

// ── logs ─────────────────────────────────────────────────────────────────────

/// Print a tail, then (--follow) poll every second and print only the lines
/// beyond the largest tail/head overlap of consecutive windows — the same
/// near-real-time model as the app's Logs tab. Repeated identical lines can
/// fool the overlap occasionally; fine for a log follower.
fn tail_loop(base_args: Value, lines: u64, follow: bool) {
    let fetch = |n: u64| -> Vec<String> {
        let mut a = base_args.clone();
        a["lines"] = json!(n);
        request("logs.tail", a)["lines"]
            .as_array()
            .map(|v| v.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let mut prev = fetch(lines);
    for l in &prev {
        println!("{l}");
    }
    if !follow {
        return;
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let new = fetch(200);
        let overlap = (1..=prev.len().min(new.len()))
            .rev()
            .find(|&k| prev[prev.len() - k..] == new[..k])
            .unwrap_or(0);
        for l in &new[overlap..] {
            println!("{l}");
        }
        prev = new;
    }
}

fn lines_flag(words: &[String]) -> u64 {
    flag_value(words, "--lines").and_then(|v| v.parse().ok()).unwrap_or(100)
}

fn cmd_logs(words: &[String], json_output: bool) {
    let key = words.first().filter(|w| !w.starts_with("--"));
    let Some(key) = key else {
        let data = request("logs.list", Value::Null);
        if json_output {
            return print_json(&data);
        }
        let Some(files) = data["files"].as_array() else { return println!("(no logs)") };
        for f in files {
            println!("{:>9}  {}", format!("{} B", f["bytes"].as_u64().unwrap_or(0)), f["key"].as_str().unwrap_or("?"));
        }
        return;
    };
    if json_output {
        return print_json(&request("logs.tail", json!({ "key": key, "lines": lines_flag(words) })));
    }
    tail_loop(json!({ "key": key }), lines_flag(words), words.iter().any(|w| w == "--follow"));
}

fn cmd_site_logs(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site logs <domain> [--source K] [--lines N] [--follow]");
    let id = site["id"].clone();
    let Some(source) = flag_value(words, "--source") else {
        let data = request("logs.targets", json!({ "id": id }));
        if json_output {
            return print_json(&data);
        }
        let Some(targets) = data["targets"].as_array() else { return println!("(no sources)") };
        println!("sources (pass one via --source):");
        for t in targets {
            println!("  {:<28} {}", t["key"].as_str().unwrap_or("?"), t["label"].as_str().unwrap_or(""));
        }
        return;
    };
    // `id` rides along for the wp-debug pseudo-source (docroot-based tail).
    let base = json!({ "key": source, "id": id });
    if json_output {
        let mut a = base;
        a["lines"] = json!(lines_flag(words));
        return print_json(&request("logs.tail", a));
    }
    tail_loop(base, lines_flag(words), words.iter().any(|w| w == "--follow"));
}

fn cmd_site_delete(words: &[String], json_output: bool) {
    let Some(domain) = words.first().filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site delete <domain> [--yes]");
        exit(1);
    };
    // Resolve domain → id through the app (same list the UI shows).
    let data = request("site.list", Value::Null);
    let site = data["sites"]
        .as_array()
        .and_then(|sites| sites.iter().find(|s| s["domain"] == json!(domain)))
        .cloned();
    let Some(site) = site else {
        eprintln!("rex: no site with domain `{domain}` (see `rex site list`)");
        exit(1);
    };
    // Destructive: database + docroot go away. Ask unless --yes (and always
    // require --yes when stdin isn't a terminal-driven human).
    if !words.iter().any(|w| w == "--yes") {
        eprint!("delete {domain}? This drops its database and docroot. [y/N] ");
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err()
            || !matches!(answer.trim(), "y" | "Y" | "yes")
        {
            eprintln!("aborted (nothing deleted)");
            exit(1);
        }
    }
    let result = request("site.delete", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&result);
    }
    println!("✓ deleted {domain} (database + files removed)");
}

// ── status ───────────────────────────────────────────────────────────────────

fn cmd_status(json_output: bool) {
    let data = request("status", Value::Null);
    if json_output {
        return print_json(&data);
    }
    if let Some(dns) = data["dns"].as_object() {
        let running = dns["running"] == json!(true);
        let mode = dns["mode"].as_str().unwrap_or("?");
        let port = &dns["port"];
        let resolver = dns["resolverInstalled"] == json!(true);
        let ca = dns["caTrusted"] == json!(true);
        println!(
            "DNS      {} ({mode}, udp {port}) · resolver {} · CA {}",
            if running { "answering" } else { "DOWN" },
            if resolver { "installed" } else { "MISSING" },
            if ca { "trusted" } else { "NOT TRUSTED" },
        );
    }
    let Some(services) = data["services"].as_array() else {
        return println!("(no services reported)");
    };
    let name_w = services
        .iter()
        .filter_map(|s| s["name"].as_str())
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!("{:<name_w$}  {:<8} {:>7}  {:>6}  {:>6}  {:>8}", "NAME", "STATE", "PID", "PORT", "CPU%", "RAM");
    for s in services {
        let running = s["running"] == json!(true);
        let pid = s["pid"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        println!(
            "{:<name_w$}  {:<8} {:>7}  {:>6}  {:>6.1}  {:>6} MB",
            s["name"].as_str().unwrap_or("?"),
            if running { "running" } else { "idle" },
            pid,
            s["port"].as_u64().unwrap_or(0),
            s["cpuPercent"].as_f64().unwrap_or(0.0),
            s["ramMb"].as_u64().unwrap_or(0),
        );
    }
}
