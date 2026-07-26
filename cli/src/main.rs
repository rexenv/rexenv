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
              [--blueprint <name>] [--multisite subdomain|subdirectory]
              [--path <folder>]
                Create a site (defaults mirror the app's New Site dialog;
                WordPress sites get the one-click install). --path serves an
                EXISTING folder in place: it is adopted as-is, never written
                into, and never deleted with the site
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
  db reset <domain>       Drop + reinstall WordPress (type the domain to confirm)
  db versions [--set <engine> <version>]   Per-engine server versions
  db browse               Open Adminer in the browser
  php list      Pinned PHP versions: installed, default, pool port
  php default <minor>      Default version for new sites
  php install <minor> / php uninstall <minor>
  php settings <minor> [set K=V]     Whitelisted ini settings (set restarts the pool)
  site php <domain> <minor>          Switch a site's PHP version
  site xdebug <domain> on|off        Toggle the site's Xdebug debug pool
  site server <domain> nginx|frankenphp|apache   Switch the web server
  site rename <domain> <name>        Display name only (domain unchanged)
  site domain <domain> <new-domain>  Change the domain (URL rewrite; asks first)
  site move <domain> <dest-parent>   Move the docroot under a new parent folder
  site env <domain> [set K=V | unset K]          Per-site env vars
  site cert <domain> [--regenerate]  Certificate info / fresh leaf
  blueprints                         Saved blueprints (for site create --blueprint)
  wp <domain> plugin list|install|activate|deactivate|update|delete [slug…] [--activate]
  wp <domain> theme  list|install|activate|update|delete [slug…] [--activate]
  wp <domain> user   list|create|set-password|set-role …
                WordPress manager (vetted WP-CLI ops; passwords are
                auto-generated and printed once — never passed on argv)
  wp <domain> search-replace <from> <to> [--dry-run] [--yes]
  wp <domain> cache-flush | cron run | maintenance [on|off] | core update
  wp <domain> core versions | core switch <version>
  repo <domain> list [--status]        Git-backed plugins/themes (--status adds live state)
  repo <domain> status <dir> [--theme] Branch, changes, ahead/behind, remote, link target
  repo <domain> branches <dir> [--theme]   Local + remote branches + tags
  repo <domain> check <dir> [--theme] [--install]
                Zero-exec dependency check (composer/npm missing or stale?) —
                reports + offers steps, runs nothing itself; --install runs the
                offered steps in order, stopping at the first failure
  repo <domain> prs <dir> [--theme]    PR/MR head refs from the remote (checkout
                a listed ref lands detached — refs carry number + sha only)
  repo <domain> adopt <dir> [--theme]  Manage an existing checkout (metadata only)
  repo <domain> link <path> [--name N] [--theme]
                Symlink an external folder in (deleting later only unlinks)
  repo <domain> watch list | watch start <dir> <script> | watch stop <dir>
                Dev watchers — run inside the app, stop when it quits
  repo <domain> add <url> [--branch B] [--name N] [--theme] [--install]
                Clone a repo in (public https/owner-repo, private via YOUR ssh
                keys); --install also runs detected composer/npm/build steps.
                Output appears at completion — live view is in the app panel
  repo <domain> pull|fetch|push <dir> [--install]
  repo <domain> checkout <dir> <ref> [--install]
                Git ops on an asset (pull is --ff-only; push never forces;
                --install re-installs when the op changed lockfiles)
  repo <domain> run <dir> <script>     Run one package.json script to completion
  repo <domain> delete <dir> [--theme] [--yes]
                Delete with the loss-warning preview; symlinked assets are
                UNLINKED only (your real folder is never touched)
  repo tools [--refresh]               Detected git/node (login-shell resolution)
  service start|stop <mysql|mariadb|postgres|redis|mailpit>
                Start/stop one optional service (web tier stays via rex start/stop)
  mail          List caught messages (Mailpit)
  mail open     Open the Mailpit web UI · mail clear [--yes] deletes ALL messages
  tunnel list | tunnel start|stop <domain>
                Public cloudflared tunnels (start prints the public URL)
  tld [--set <tld>]
                Default TLD for new sites
  version       App + CLI versions (needs the app; -v/--version works without)
  completions zsh|bash    Print a shell completion script (eval or install it)
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

/// Best-effort request: `None` on any transport/command failure — for output
/// that must not require a running app (`--version`).
fn soft_request(cmd: &str) -> Option<Value> {
    let mut stream = UnixStream::connect(socket_path()).ok()?;
    let line = json!({ "cmd": cmd, "args": Value::Null }).to_string();
    stream.write_all(format!("{line}\n").as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).ok()?;
    let envelope: Value = serde_json::from_str(reply.trim()).ok()?;
    (envelope["ok"] == json!(true)).then(|| envelope["data"].clone())
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
            // Native version: must work WITHOUT the app (unlike `rex version`,
            // the app round-trip) — CLI version always, app version best-effort.
            "-v" | "-V" | "--version" => {
                print!("rex {}", env!("CARGO_PKG_VERSION"));
                if let Some(app) = soft_request("version") {
                    print!(" · rexenv {}", app["version"].as_str().unwrap_or("?"));
                }
                println!();
                return;
            }
            _ => words.push(arg),
        }
    }
    // Preflight: every subcommand below talks to the app. Probe the socket
    // ONCE up front so a not-running app prints only the honest message and
    // exits 2 — never after a misleading in-progress line ("starting
    // services…", "creating <domain>…"). The app can still die between this
    // probe and a request; `request` then prints the same message.
    match words.first().map(String::as_str) {
        None | Some("completions") => {} // native output, no app needed
        _ => {
            if UnixStream::connect(socket_path()).is_err() {
                eprintln!("{NOT_RUNNING}");
                exit(2);
            }
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
        Some("repo") => cmd_repo(&words[1..], json_output),
        Some("service") => cmd_service(&words[1..], json_output),
        Some("mail") => cmd_mail(&words[1..], json_output),
        Some("tunnel") => cmd_tunnel(&words[1..], json_output),
        Some("tld") => cmd_tld(&words[1..], json_output),
        Some("version") => cmd_version(json_output),
        Some("completions") => cmd_completions(words.get(1).map(String::as_str)),
        Some("blueprints") => {
            let data = request("blueprint.list", Value::Null);
            if json_output {
                print_json(&data);
            } else {
                match data["blueprints"].as_array().filter(|b| !b.is_empty()) {
                    None => println!("no saved blueprints (create them in the app: Settings → Blueprints)"),
                    Some(rows) => {
                        for b in rows {
                            println!("{}", b["name"].as_str().unwrap_or("?"));
                        }
                    }
                }
            }
        }
        Some("db") => match words.get(1).map(String::as_str) {
            Some("export") => cmd_db_export(&words[2..], json_output),
            Some("import") => cmd_db_import(&words[2..], json_output),
            Some("reset") => cmd_db_reset(&words[2..], json_output),
            Some("versions") => cmd_db_versions(&words[2..], json_output),
            Some("browse") => open_url("https://adminer.rexenv.rex"),
            _ => {
                eprintln!("rex: usage: rex db <export|import|reset|versions|browse>\n\n{USAGE}");
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
            Some("server") => cmd_site_server(&words[2..], json_output),
            Some("rename") => cmd_site_rename(&words[2..], json_output),
            Some("domain") => cmd_site_domain(&words[2..], json_output),
            Some("move") => cmd_site_move(&words[2..], json_output),
            Some("env") => cmd_site_env(&words[2..], json_output),
            Some("cert") => cmd_site_cert(&words[2..], json_output),
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
        eprintln!("rex: usage: rex site create <domain> [--name N] [--type T] [--php V] [--server S] [--db D] [--path FOLDER]");
        exit(1);
    };
    let mut args = serde_json::Map::new();
    args.insert("domain".into(), json!(domain));
    for (flag, key) in [
        ("--name", "name"),
        ("--type", "type"),
        ("--php", "php"),
        ("--server", "server"),
        ("--db", "db"),
        ("--blueprint", "blueprint"),
        ("--multisite", "multisite"),
        ("--path", "path"),
    ] {
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
        Some("settings") => {
            let Some(minor) = words.get(1).filter(|w| !w.starts_with("--")) else {
                eprintln!("rex: usage: rex php settings <minor> [set K=V]");
                exit(1);
            };
            match words.get(2).map(String::as_str) {
                None => {
                    let data = request("php.settings", json!({ "minor": minor }));
                    if json_output {
                        return print_json(&data);
                    }
                    for s in data["settings"].as_array().map(Vec::as_slice).unwrap_or_default() {
                        println!(
                            "{:<24} {}",
                            s["key"].as_str().unwrap_or("?"),
                            s["value"]
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("(default: {})", s["default"].as_str().unwrap_or("?"))),
                        );
                    }
                }
                Some("set") => {
                    let Some((k, v)) = words.get(3).and_then(|kv| kv.split_once('=')) else {
                        eprintln!("rex: usage: rex php settings <minor> set KEY=value");
                        exit(1);
                    };
                    // The backend applies the FULL submitted set — resend every
                    // stored value plus the change (unset keys stay default).
                    let current = request("php.settings", json!({ "minor": minor }));
                    let mut pairs: Vec<(String, String)> = current["settings"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|s| {
                                    Some((s["key"].as_str()?.to_string(), s["value"].as_str()?.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    pairs.retain(|(key, _)| key != k);
                    pairs.push((k.to_string(), v.to_string()));
                    let payload: Vec<Value> =
                        pairs.iter().map(|(key, val)| json!({ "key": key, "value": val })).collect();
                    let r = request("php.settings.set", json!({ "minor": minor, "settings": payload }));
                    if json_output {
                        return print_json(&r);
                    }
                    println!("✓ {k}={v} (PHP {minor} pool restarted if live)");
                }
                _ => {
                    eprintln!("rex: usage: rex php settings <minor> [set K=V]");
                    exit(1);
                }
            }
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

/// Long repo jobs hold the connection while the app runs them; the socket
/// can't stream, so output arrives AT COMPLETION — say so up front and tick
/// dots on stderr while waiting (live output is in the app panel / job log).
fn request_long(cmd: &str, args: Value, doing: &str) -> Value {
    eprintln!("{doing} — output appears when it finishes (watch live in the app panel)…");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = stop.clone();
    let ticker = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !s2.load(std::sync::atomic::Ordering::Relaxed) {
                eprint!(".");
            }
        }
    });
    let data = request(cmd, args);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = ticker.join();
    eprintln!();
    data
}

/// Print a settled repo job: step glyphs, the job log, and exit non-zero on
/// any failed/cancelled step (after printing everything).
fn print_repo_job(data: &Value, json_output: bool) {
    if json_output {
        print_json(data);
    } else {
        for st in data["job"]["steps"].as_array().unwrap_or(&vec![]) {
            let glyph = match st["status"].as_str().unwrap_or("") {
                "ok" => "✓",
                "failed" => "✕",
                "cancelled" => "–",
                // Never ran — an earlier step in a run-all failed/cancelled.
                "skipped" => "»",
                "running" => "…",
                _ => "·",
            };
            println!("{glyph} {}", st["label"].as_str().unwrap_or("?"));
            if let Some(e) = st["error"].as_str() {
                for line in e.lines() {
                    println!("    {line}");
                }
            }
        }
        let pending_offers: Vec<&str> = data["job"]["steps"]
            .as_array()
            .map(|steps| {
                steps
                    .iter()
                    .filter(|st| st["status"] == json!("pending"))
                    .filter_map(|st| st["label"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if !pending_offers.is_empty() {
            println!(
                "! dependency steps offered, not run: {} — re-run with --install, or use the app panel",
                pending_offers.join(", ")
            );
        }
        if let Some(w) = data["job"]["nodeWarning"].as_str() {
            println!("! {w}");
        }
        if let Some(log) = data["log"].as_array().filter(|l| !l.is_empty()) {
            println!("── job output ──");
            for l in log {
                println!("{}", l.as_str().unwrap_or(""));
            }
        }
    }
    let failed = data["job"]["steps"]
        .as_array()
        .map(|steps| {
            steps.iter().any(|st| {
                matches!(st["status"].as_str().unwrap_or(""), "failed" | "cancelled")
            })
        })
        .unwrap_or(false);
    if failed {
        exit(1);
    }
}

const REPO_USAGE: &str =
    "rex repo <domain> list|status|branches|prs|adopt|link|watch … (or: rex repo tools)";

/// Git/asset assets — wave 1: pure request/response commands. Every call
/// rides the same commands::repo fns the app UI uses (one code path).
fn cmd_repo(words: &[String], json_output: bool) {
    // `rex repo tools` is app-wide, not site-scoped.
    if words.first().map(String::as_str) == Some("tools") {
        let refresh = words.iter().any(|w| w == "--refresh");
        let data = request("repo.tools", json!({ "refresh": refresh }));
        if json_output {
            return print_json(&data);
        }
        if let Some(rows) = data["tools"].as_array() {
            for t in rows {
                if t["ok"] == json!(true) {
                    println!(
                        "{:<9} {:<28} {}",
                        t["name"].as_str().unwrap_or("?"),
                        t["version"].as_str().unwrap_or("?"),
                        t["path"].as_str().unwrap_or(""),
                    );
                } else {
                    println!("{:<9} MISSING", t["name"].as_str().unwrap_or("?"));
                    for line in t["error"].as_str().unwrap_or("").lines() {
                        println!("          {line}");
                    }
                }
            }
        }
        println!("{:<9} bundled composer.phar (runs on each site's PHP)", "composer");
        return;
    }

    let site = find_site(words, REPO_USAGE);
    let id = site["id"].clone();
    let theme = words.iter().any(|w| w == "--theme");
    let sub = words.get(1).map(String::as_str);
    let rest: Vec<&String> =
        words.iter().skip(2).filter(|w| !w.starts_with("--")).collect();
    let dir_arg = |usage: &str| -> String {
        match rest.first() {
            Some(d) => (*d).clone(),
            None => {
                eprintln!("rex: usage: {usage}");
                exit(1);
            }
        }
    };
    match sub {
        Some("list") | None => {
            let with_status = words.iter().any(|w| w == "--status");
            let data = request("repo.list", json!({ "id": id, "status": with_status }));
            if json_output {
                return print_json(&data);
            }
            let Some(rows) = data["assets"].as_array().filter(|a| !a.is_empty()) else {
                return println!(
                    "no git-backed assets (add one in the app, or: rex repo <domain> adopt <dir>)"
                );
            };
            for a in rows {
                let key =
                    format!("{}/{}", a["kind"].as_str().unwrap_or("?"), a["dirName"].as_str().unwrap_or("?"));
                let st = &data["statuses"][&key];
                let live = if st["error"].as_str().is_some() {
                    " (status unavailable)".to_string()
                } else if st.is_object() {
                    let dirty = st["changed"].as_u64().unwrap_or(0) + st["untracked"].as_u64().unwrap_or(0);
                    format!(
                        " {} {}↑{}↓{}",
                        st["branch"].as_str().unwrap_or("detached"),
                        if dirty > 0 { format!("{dirty} dirty ") } else { "clean ".into() },
                        st["ahead"].as_u64().unwrap_or(0),
                        st["behind"].as_u64().unwrap_or(0),
                    )
                } else {
                    String::new()
                };
                println!(
                    "{:<7} {:<28} {:<8} {}{}",
                    a["kind"].as_str().unwrap_or("?"),
                    a["dirName"].as_str().unwrap_or("?"),
                    a["source"].as_str().unwrap_or("?"),
                    a["url"].as_str().filter(|u| !u.is_empty()).unwrap_or("(no remote)"),
                    live,
                );
            }
        }
        Some("status") => {
            let dir = dir_arg("rex repo <domain> status <dir> [--theme]");
            let data = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let head = if data["unborn"] == json!(true) {
                "no commits yet".to_string()
            } else if data["detached"] == json!(true) {
                "detached HEAD".to_string()
            } else {
                data["branch"].as_str().unwrap_or("?").to_string()
            };
            println!("branch     {head}");
            let (ch, un) =
                (data["changed"].as_u64().unwrap_or(0), data["untracked"].as_u64().unwrap_or(0));
            println!(
                "tree       {}",
                if ch + un == 0 {
                    "clean".to_string()
                } else {
                    format!("{ch} changed, {un} untracked")
                }
            );
            match data["upstream"].as_str() {
                Some(up) => println!(
                    "upstream   {up} (↑{} ↓{})",
                    data["ahead"].as_u64().unwrap_or(0),
                    data["behind"].as_u64().unwrap_or(0)
                ),
                None => println!("upstream   (none)"),
            }
            if let Some(r) = data["remote"].as_str() {
                println!("remote     {r}");
            }
            if let Some(t) = data["linkTarget"].as_str() {
                println!("linked →   {t}");
            }
            if let Some(w) = data["lossWarning"].as_str() {
                println!("at risk    {w}");
            }
        }
        Some("branches") => {
            let dir = dir_arg("rex repo <domain> branches <dir> [--theme]");
            let data = request("repo.branches", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let current = data["current"].as_str().unwrap_or("");
            for b in data["local"].as_array().unwrap_or(&vec![]) {
                let name = b.as_str().unwrap_or("?");
                println!("{} {name}", if name == current { "*" } else { " " });
            }
            for b in data["remote"].as_array().unwrap_or(&vec![]) {
                println!("  {}", b.as_str().unwrap_or("?"));
            }
            let tags = data["tags"].as_array().cloned().unwrap_or_default();
            if !tags.is_empty() {
                println!("tags:");
                for t in &tags {
                    println!("  {}", t.as_str().unwrap_or("?"));
                }
            }
        }
        Some("check") => {
            let dir = dir_arg("rex repo <domain> check <dir> [--theme] [--install]");
            let install = words.iter().any(|w| w == "--install");
            let payload = json!({ "id": id, "dir": dir, "theme": theme, "install": install });
            let data = if install {
                request_long("repo.check", payload, "checking + installing what's needed")
            } else {
                request("repo.check", payload)
            };
            print_repo_job(&data, json_output);
        }
        Some("prs") => {
            let dir = dir_arg("rex repo <domain> prs <dir> [--theme]");
            let data = request("repo.prs", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&data);
            }
            let prs = data.as_array().cloned().unwrap_or_default();
            if prs.is_empty() {
                println!("(no PR/MR refs advertised by the remote)");
            }
            for p in &prs {
                let sha = p["sha"].as_str().unwrap_or("?");
                println!(
                    "#{:<6} {:.7}  {}",
                    p["number"].as_u64().unwrap_or(0),
                    sha,
                    p["ref"].as_str().unwrap_or("?"),
                );
            }
        }
        Some("adopt") => {
            let dir = dir_arg("rex repo <domain> adopt <dir> [--theme]");
            request("repo.adopt", json!({ "id": id, "dir": dir, "theme": theme }));
            let st = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            if json_output {
                return print_json(&st);
            }
            println!(
                "adopted {dir} — branch {}, remote {} (metadata only; nothing on disk changed)",
                st["branch"].as_str().unwrap_or("?"),
                st["remote"].as_str().unwrap_or("(none)"),
            );
        }
        Some("link") => {
            let raw = dir_arg("rex repo <domain> link <path> [--name N] [--theme]");
            let target = match std::fs::canonicalize(&raw) {
                Ok(t) => t.to_string_lossy().into_owned(),
                Err(e) => {
                    eprintln!("rex: {raw}: {e}");
                    exit(1);
                }
            };
            let name = words
                .windows(2)
                .find(|w| w[0] == "--name")
                .map(|w| w[1].clone());
            let data = request(
                "repo.link",
                json!({ "id": id, "theme": theme, "target": target, "name": name }),
            );
            if json_output {
                return print_json(&data);
            }
            println!(
                "linked as {} ({})",
                data["dirName"].as_str().unwrap_or("?"),
                if data["isGit"] == json!(true) { "git checkout" } else { "not a git repo" },
            );
            if data["wp"]["kind"] == json!("none") {
                println!("note: no plugin/theme header at the folder root — WordPress won't list it until one exists");
            }
            println!("deleting this asset later removes ONLY the link — the folder stays.");
        }
        Some("watch") => match words.get(2).map(String::as_str) {
            Some("list") | None => {
                let data = request("repo.watch.list", json!({ "id": id }));
                if json_output {
                    return print_json(&data);
                }
                match data["watchers"].as_array().filter(|w| !w.is_empty()) {
                    None => println!("no watchers running"),
                    Some(rows) => {
                        for w in rows {
                            println!(
                                "{:<28} {:<12} {}{}",
                                w["dirName"].as_str().unwrap_or("?"),
                                w["script"].as_str().unwrap_or("?"),
                                w["status"].as_str().unwrap_or("?"),
                                w["exit"].as_i64().map(|c| format!(" (code {c})")).unwrap_or_default(),
                            );
                        }
                    }
                }
            }
            Some("start") => {
                let (Some(dir), Some(script)) = (words.get(3), words.get(4)) else {
                    eprintln!("rex: usage: rex repo <domain> watch start <dir> <script> [--theme]");
                    exit(1);
                };
                let w = request(
                    "repo.watch.start",
                    json!({ "id": id, "dir": dir, "script": script, "theme": theme }),
                );
                if json_output {
                    return print_json(&w);
                }
                println!(
                    "watching {dir} — {script} (runs inside the app; output in the app panel \
                     and logs/repo-*-watch.log; stops when the app quits, never auto-restarts)"
                );
            }
            Some("stop") => {
                let Some(dir) = words.get(3) else {
                    eprintln!("rex: usage: rex repo <domain> watch stop <dir> [--theme]");
                    exit(1);
                };
                request("repo.watch.stop", json!({ "id": id, "dir": dir, "theme": theme }));
                if !json_output {
                    println!("stopped watching {dir}");
                }
            }
            _ => {
                eprintln!("rex: usage: rex repo <domain> watch list|start <dir> <script>|stop <dir>");
                exit(1);
            }
        },
        Some("add") => {
            let url = dir_arg("rex repo <domain> add <url> [--branch B] [--name N] [--theme] [--install]");
            let flag_val = |flag: &str| {
                words.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
            };
            let install = words.iter().any(|w| w == "--install");
            let data = request_long(
                "repo.add",
                json!({
                    "id": id, "theme": theme, "url": url,
                    "branch": flag_val("--branch"), "name": flag_val("--name"),
                    "install": install,
                }),
                &format!("cloning {url}{}", if install { " + installing" } else { "" }),
            );
            print_repo_job(&data, json_output);
        }
        Some(op @ ("pull" | "fetch" | "checkout" | "push")) => {
            let usage = format!("rex repo <domain> {op} <dir> {}[--theme]",
                if op == "checkout" { "<ref> " } else { "" });
            let dir = dir_arg(&usage);
            let target_ref = if op == "checkout" {
                match rest.get(1) {
                    Some(r) => Some((*r).clone()),
                    None => {
                        eprintln!("rex: usage: {usage}");
                        exit(1);
                    }
                }
            } else {
                None
            };
            let install = words.iter().any(|w| w == "--install");
            let data = request_long(
                "repo.op",
                json!({
                    "id": id, "theme": theme, "dir": dir, "op": op,
                    "ref": target_ref, "install": install,
                }),
                &format!("git {op} in {dir}"),
            );
            print_repo_job(&data, json_output);
        }
        Some("run") => {
            let usage = "rex repo <domain> run <dir> <script> [--theme]";
            let dir = dir_arg(usage);
            let Some(script) = rest.get(1) else {
                eprintln!("rex: usage: {usage}");
                exit(1);
            };
            let data = request_long(
                "repo.run",
                json!({ "id": id, "theme": theme, "dir": dir, "script": script }),
                &format!("running {script} in {dir}"),
            );
            print_repo_job(&data, json_output);
        }
        Some("delete") => {
            // Alias over the ALREADY-GUARDED wp delete (the same
            // wp_plugin_delete/wp_theme_delete fns carrying the unlink-only
            // symlink interception) — plus the UI's loss-warning preview.
            let dir = dir_arg("rex repo <domain> delete <dir> [--theme] [--yes]");
            let st = request("repo.status", json!({ "id": id, "dir": dir, "theme": theme }));
            let preview = if st["linkTarget"].as_str().is_some() {
                format!(
                    "LINKED folder — removes only the link; {} stays untouched.",
                    st["linkTarget"].as_str().unwrap_or("your folder")
                )
            } else if let Some(w) = st["lossWarning"].as_str() {
                w.to_string()
            } else {
                "clean and pushed — nothing at risk.".to_string()
            };
            eprintln!("{dir}: {preview}");
            if !words.iter().any(|w| w == "--yes") {
                eprint!("delete this {}? [y/N] ", if theme { "theme" } else { "plugin" });
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err()
                    || !matches!(a.trim(), "y" | "Y" | "yes")
                {
                    eprintln!("aborted");
                    exit(1);
                }
            }
            let key = if theme { "wp.theme.delete" } else { "wp.plugin.delete" };
            request(key, json!({ "id": id, "names": [dir] }));
            if !json_output {
                println!("deleted {dir}");
            }
        }
        _ => {
            eprintln!("rex: usage: {REPO_USAGE}");
            exit(1);
        }
    }
}

// ── shell completions ────────────────────────────────────────────────────────

/// Static word completion (subcommand tree only — domains change too often to
/// bake in; a dynamic version can call `rex site list --json` later).
/// zsh:  rex completions zsh  > ~/.zfunc/_rex   (with ~/.zfunc in $fpath)
/// bash: rex completions bash > /usr/local/etc/bash_completion.d/rex
fn cmd_completions(shell: Option<&str>) {
    const TOP: &str = "status start stop restart site wp repo php db service logs doctor mail tunnel tld blueprints version completions help";
    const SITE: &str = "list create delete info open login logs php xdebug server rename domain move env cert";
    const DB: &str = "export import reset versions browse";
    const PHP: &str = "list default install uninstall settings";
    const WPA: &str = "plugin theme user search-replace cache-flush cron maintenance core";
    const REPO: &str =
        "list status branches prs check adopt link watch add pull fetch checkout push run delete";
    match shell {
        Some("zsh") => println!(
            "#compdef rex\n\
             local -a words2\n\
             case $CURRENT in\n\
             2) compadd {TOP} ;;\n\
             3) case $words[2] in\n\
                site) compadd {SITE} ;;\n\
                db) compadd {DB} ;;\n\
                php) compadd {PHP} ;;\n\
                service) compadd start stop ;;\n\
                mail) compadd list open clear ;;\n\
                tunnel) compadd list start stop ;;\n\
                completions) compadd zsh bash ;;\n\
                repo) compadd tools ;;\n\
                esac ;;\n\
             4) case $words[2] in wp) compadd {WPA} ;; repo) compadd {REPO} ;; esac ;;\n\
             esac"
        ),
        Some("bash") => println!(
            "_rex() {{\n\
             local cur=${{COMP_WORDS[COMP_CWORD]}}\n\
             case $COMP_CWORD in\n\
             1) COMPREPLY=($(compgen -W \"{TOP}\" -- \"$cur\")) ;;\n\
             2) case ${{COMP_WORDS[1]}} in\n\
                site) COMPREPLY=($(compgen -W \"{SITE}\" -- \"$cur\")) ;;\n\
                db) COMPREPLY=($(compgen -W \"{DB}\" -- \"$cur\")) ;;\n\
                php) COMPREPLY=($(compgen -W \"{PHP}\" -- \"$cur\")) ;;\n\
                service) COMPREPLY=($(compgen -W \"start stop\" -- \"$cur\")) ;;\n\
                mail) COMPREPLY=($(compgen -W \"list open clear\" -- \"$cur\")) ;;\n\
                tunnel) COMPREPLY=($(compgen -W \"list start stop\" -- \"$cur\")) ;;\n\
                completions) COMPREPLY=($(compgen -W \"zsh bash\" -- \"$cur\")) ;;\n\
                repo) COMPREPLY=($(compgen -W \"tools\" -- \"$cur\")) ;;\n\
                esac ;;\n\
             3) case ${{COMP_WORDS[1]}} in wp) COMPREPLY=($(compgen -W \"{WPA}\" -- \"$cur\")) ;; repo) COMPREPLY=($(compgen -W \"{REPO}\" -- \"$cur\")) ;; esac ;;\n\
             esac\n\
             }}\n\
             complete -F _rex rex"
        ),
        _ => {
            eprintln!("rex: usage: rex completions zsh|bash");
            exit(1);
        }
    }
}

// ── site settings: server / rename / domain / move / env / cert ─────────────

fn cmd_site_server(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site server <domain> nginx|frankenphp|apache");
    let Some(server) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site server <domain> nginx|frankenphp|apache");
        exit(1);
    };
    let updated = request("site.server", json!({ "id": site["id"], "server": server }));
    if json_output {
        return print_json(&updated);
    }
    print_site_update(&updated);
}

fn cmd_site_rename(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site rename <domain> <name>");
    let name = words[1..].iter().filter(|w| !w.starts_with("--")).cloned().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        eprintln!("rex: usage: rex site rename <domain> <name>");
        exit(1);
    }
    let r = request("site.rename", json!({ "id": site["id"], "name": name }));
    if json_output {
        return print_json(&r);
    }
    println!("✓ {} is now named “{name}”", site["domain"].as_str().unwrap_or("?"));
}

fn cmd_site_domain(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site domain <domain> <new-domain> [--yes]");
    let old = site["domain"].as_str().unwrap_or("?").to_string();
    let Some(new_domain) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site domain <domain> <new-domain> [--yes]");
        exit(1);
    };
    if !words.iter().any(|w| w == "--yes") {
        eprint!(
            "change {old} → {new_domain}? WordPress URLs are rewritten across the \
             database (a backup is taken first). [y/N] "
        );
        let mut a = String::new();
        if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
            eprintln!("aborted (domain unchanged)");
            exit(1);
        }
    }
    let r = request("site.domain", json!({ "id": site["id"], "domain": new_domain }));
    if json_output {
        return print_json(&r);
    }
    println!(
        "✓ {old} → https://{} ({} URL replacement{}{})",
        r["site"]["domain"].as_str().unwrap_or(new_domain),
        r["replacements"].as_u64().unwrap_or(0),
        if r["replacements"] == json!(1) { "" } else { "s" },
        r["backup_path"]
            .as_str()
            .or(r["backupPath"].as_str())
            .map(|p| format!("; backup: {p}"))
            .unwrap_or_default(),
    );
}

fn cmd_site_move(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site move <domain> <dest-parent>");
    let Some(dest) = words.get(1).filter(|w| !w.starts_with("--")) else {
        eprintln!("rex: usage: rex site move <domain> <dest-parent>");
        exit(1);
    };
    let dest = match std::fs::canonicalize(dest) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rex: cannot use {dest}: {e}");
            exit(1);
        }
    };
    let r = request(
        "site.move",
        json!({ "id": site["id"], "destParent": dest.to_string_lossy() }),
    );
    if json_output {
        return print_json(&r);
    }
    println!("✓ moved → {}", r["path"].as_str().unwrap_or("?"));
}

fn cmd_site_env(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site env <domain> [set K=V | unset K]");
    let id = site["id"].clone();
    let fetch = || -> Vec<(String, String)> {
        request("site.env", json!({ "id": id }))["vars"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|e| {
                        Some((e["name"].as_str()?.to_string(), e["value"].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    match words.get(1).map(String::as_str) {
        None => {
            if json_output {
                return print_json(&request("site.env", json!({ "id": id })));
            }
            let vars = fetch();
            if vars.is_empty() {
                return println!("(no env vars)");
            }
            for (k, v) in vars {
                println!("{k}={v}");
            }
        }
        // The backend replaces the whole set — merge client-side.
        Some("set") => {
            let Some((k, v)) = words.get(2).and_then(|kv| kv.split_once('=')) else {
                eprintln!("rex: usage: rex site env <domain> set KEY=value");
                exit(1);
            };
            let mut vars = fetch();
            vars.retain(|(name, _)| name != k);
            vars.push((k.to_string(), v.to_string()));
            let payload: Vec<Value> =
                vars.iter().map(|(n, val)| json!({ "name": n, "value": val })).collect();
            let r = request("site.env.set", json!({ "id": id, "vars": payload }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {k}={v} (site backend reloaded)");
        }
        Some("unset") => {
            let Some(k) = words.get(2) else {
                eprintln!("rex: usage: rex site env <domain> unset KEY");
                exit(1);
            };
            let mut vars = fetch();
            let before = vars.len();
            vars.retain(|(name, _)| name != k);
            if vars.len() == before {
                eprintln!("rex: no env var `{k}` on this site");
                exit(1);
            }
            let payload: Vec<Value> =
                vars.iter().map(|(n, val)| json!({ "name": n, "value": val })).collect();
            let r = request("site.env.set", json!({ "id": id, "vars": payload }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ removed {k}");
        }
        _ => {
            eprintln!("rex: usage: rex site env <domain> [set K=V | unset K]");
            exit(1);
        }
    }
}

fn cmd_site_cert(words: &[String], json_output: bool) {
    let site = find_site(words, "rex site cert <domain> [--regenerate]");
    if words.iter().any(|w| w == "--regenerate") {
        let r = request("site.cert.regenerate", json!({ "id": site["id"] }));
        if json_output {
            return print_json(&r);
        }
        println!("✓ fresh certificate issued (edge reloaded)");
        return;
    }
    let data = request("site.cert", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&data);
    }
    if data.is_null() {
        return println!("no certificate yet (issued on first serve)");
    }
    println!(
        "expires {} ({} days left)\nSANs: {}",
        data["notAfter"].as_str().unwrap_or("?"),
        data["daysLeft"].as_i64().unwrap_or(0),
        data["sans"]
            .as_array()
            .map(|s| s.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
    );
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
    // The commit is the point: "is the running app the code I just changed?"
    // should be one command, not a forensic exercise.
    println!(
        "rexenv {} ({}) · rex {}\n  built {} from {}",
        data["version"].as_str().unwrap_or("?"),
        data["platform"].as_str().unwrap_or("?"),
        env!("CARGO_PKG_VERSION"),
        data["builtAt"].as_str().unwrap_or("?"),
        data["commit"].as_str().unwrap_or("?"),
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
        (Some("search-replace"), from_word) => {
            // Grammar: rex wp <domain> search-replace <from> <to> [--dry-run] [--yes]
            let (Some(from), Some(to)) = (from_word, words.get(3).map(String::as_str)) else {
                eprintln!("rex: usage: rex wp <domain> search-replace <from> <to> [--dry-run] [--yes]");
                exit(1);
            };
            let dry = words.iter().any(|w| w == "--dry-run");
            if !dry && !words.iter().any(|w| w == "--yes") {
                eprint!(
                    "replace `{from}` → `{to}` across the database? (tip: --dry-run first, \
                     `rex db export` for a backup) [y/N] "
                );
                let mut a = String::new();
                if std::io::stdin().read_line(&mut a).is_err() || !matches!(a.trim(), "y" | "Y" | "yes") {
                    eprintln!("aborted (nothing replaced)");
                    exit(1);
                }
            }
            let r = request(
                "wp.search-replace",
                json!({ "id": id, "from": from, "to": to, "dryRun": dry }),
            );
            if json_output {
                return print_json(&r);
            }
            println!(
                "✓ {} replacement{}{}",
                r["replacements"].as_u64().unwrap_or(0),
                if r["replacements"] == json!(1) { "" } else { "s" },
                if dry { " (dry run — nothing written)" } else { "" },
            );
        }
        (Some("cache-flush"), _) => {
            let r = request("wp.cache-flush", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("cache flushed"));
        }
        (Some("cron"), Some("run")) => {
            let r = request("wp.cron-run", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("due events run"));
        }
        (Some("maintenance"), mode) => {
            let on = match mode {
                Some("on") => Some(true),
                Some("off") => Some(false),
                None => None,
                _ => {
                    eprintln!("rex: usage: rex wp <domain> maintenance [on|off]");
                    exit(1);
                }
            };
            let payload = match on {
                Some(on) => json!({ "id": id, "on": on }),
                None => json!({ "id": id }),
            };
            let r = request("wp.maintenance", payload);
            if json_output {
                return print_json(&r);
            }
            println!("maintenance {}", if r["on"] == json!(true) { "ON" } else { "off" });
        }
        (Some("core"), Some("versions")) => {
            let data = request("wp.core-versions", Value::Null);
            if json_output {
                return print_json(&data);
            }
            for v in data["versions"].as_array().map(Vec::as_slice).unwrap_or_default() {
                println!("{:<10} {}", v["version"].as_str().unwrap_or("?"), v["status"].as_str().unwrap_or(""));
            }
        }
        (Some("core"), Some("switch")) => {
            let Some(version) = rest.first() else {
                eprintln!("rex: usage: rex wp <domain> core switch <version>");
                exit(1);
            };
            println!("switching core to {version}… (download + install)");
            let r = request("wp.core-switch", json!({ "id": id, "version": version }));
            if json_output {
                return print_json(&r);
            }
            println!(
                "✓ core is now {}{}",
                r["version"].as_str().unwrap_or(version),
                if r["dbUpdateRequired"] == json!(true) {
                    " — DB update required (open wp-admin once)"
                } else {
                    ""
                },
            );
        }
        (Some("core"), Some("update")) => {
            println!("updating WordPress core… (this can take a minute)");
            let r = request("wp.core-update", json!({ "id": id }));
            if json_output {
                return print_json(&r);
            }
            println!("✓ {}", r["message"].as_str().unwrap_or("core updated"));
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

// ── db reset / versions ──────────────────────────────────────────────────────

fn cmd_db_reset(words: &[String], json_output: bool) {
    let site = find_site(words, "rex db reset <domain>");
    let domain = site["domain"].as_str().unwrap_or("?").to_string();
    // Nuclear: drop + reinstall. Typed confirmation (the UI's model), never
    // just --yes; scripts pass --confirm <domain>.
    let confirmed = flag_value(words, "--confirm").is_some_and(|c| c == domain) || {
        eprint!(
            "RESET {domain}? This DROPS the database and reinstalls WordPress.\n\
             Type the domain to confirm: "
        );
        let mut a = String::new();
        std::io::stdin().read_line(&mut a).is_ok() && a.trim() == domain
    };
    if !confirmed {
        eprintln!("aborted (nothing reset)");
        exit(1);
    }
    let r = request("db.reset", json!({ "id": site["id"] }));
    if json_output {
        return print_json(&r);
    }
    println!("✓ {domain} reset — fresh WordPress install");
}

fn cmd_db_versions(words: &[String], json_output: bool) {
    if let (Some(engine), Some(version)) = (
        words.iter().position(|w| w == "--set").and_then(|i| words.get(i + 1)),
        words.iter().position(|w| w == "--set").and_then(|i| words.get(i + 2)),
    ) {
        let r = request("db.version.set", json!({ "key": engine, "version": version }));
        if json_output {
            return print_json(&r);
        }
        return println!("✓ {engine} → {version} (engine restarted if it was running)");
    }
    let data = request("db.versions", Value::Null);
    if json_output {
        return print_json(&data);
    }
    for e in data["engines"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let key = e["key"].as_str().unwrap_or("?");
        let avail = data["available"][key]
            .as_array()
            .map(|v| v.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        println!(
            "{:<10} {:<9} {:<8} available: {avail}",
            key,
            e["version"].as_str().unwrap_or("?"),
            if e["running"] == json!(true) { "running" } else { "idle" },
        );
    }
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
        // The folder is only ours to delete when we created it — say which.
        let folder = if site["docrootManaged"].as_bool() == Some(false) {
            format!("Your folder at {} is left in place.", site["path"].as_str().unwrap_or("?"))
        } else {
            "This also removes its folder.".to_string()
        };
        eprint!("delete {domain}? This drops its database. {folder} [y/N] ");
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
