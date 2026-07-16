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
  site create <domain> [--name N] [--type wordpress|php|laravel] [--php 8.3]
              [--server nginx|frankenphp|apache] [--db mysql|mariadb]
                Create a site (defaults mirror the app's New Site dialog;
                WordPress sites get the one-click install)
  site delete <domain> [--yes]
                Delete a site — drops its database and docroot (asks first)
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
        Some("site") => match words.get(1).map(String::as_str) {
            Some("list") => cmd_site_list(json_output),
            Some("create") => cmd_site_create(&words[2..], json_output),
            Some("delete") => cmd_site_delete(&words[2..], json_output),
            _ => {
                eprintln!("rex: usage: rex site <list|create|delete>\n\n{USAGE}");
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
