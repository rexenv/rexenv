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
