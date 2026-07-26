//! Live check for SOURCE DATABASE DISCOVERY (Stage 2 step 3). Run:
//! `cargo run --example db_source_check`
//!
//! Read-only in the strongest sense available: it opens config files, and it
//! makes TCP connections that read the server's opening greeting and then hang
//! up. It never authenticates, never sends a byte to any server, and never
//! starts or stops anything. Nothing is written anywhere.
//!
//! What it proves against the real machine:
//!   1. DBngin's plist contributes a LABEL, and its `Status` field is reported
//!      as a claim beside the live truth — on this machine it says "started"
//!      for an engine that isn't listening;
//!   2. every server in `servers` answered a connection, and every labelled
//!      candidate that didn't answer is in `silent` instead;
//!   3. the pre-auth handshake identifies the vendor of whatever IS running.

use rexenv_lib::core::dbsource::{self, HintSource, Identity, Probe};

fn main() {
    let home = directories::BaseDirs::new().expect("home").home_dir().to_path_buf();
    let mut ok = true;

    println!("=== 1. what the config files CLAIM (labels only) ===");
    let dbngin = dbsource::dbngin_hints(&home);
    if dbngin.is_empty() {
        println!("  DBngin: no engine list (not installed, or a binary plist)");
    }
    for (host, port, hint) in &dbngin {
        println!(
            "  {host}:{port} — {} (its own Status field says: {})",
            hint.label,
            hint.claimed_status.as_deref().unwrap_or("nothing")
        );
    }
    let herd = dbsource::herd_hints(&home);
    println!(
        "  Herd services: {}",
        if herd.is_empty() { "none (free Herd has no services config)" } else { "present" }
    );

    println!("\n=== 2. what is ACTUALLY listening ===");
    let found = dbsource::discover(&home, &[]);
    for s in &found.servers {
        let id = match &s.identity {
            Identity::Handshake { vendor, version } => {
                format!("{} {version} (from its own handshake, no login)", vendor.label())
            }
            Identity::Declared { vendor, .. } => format!("{} (declared)", vendor.label()),
            Identity::Unknown { note } => {
                format!("listening, vendor unknown{}", note.as_deref().map(|n| format!(" — {n}")).unwrap_or_default())
            }
        };
        println!("  {}:{} — {id}", s.host, s.port);
    }
    if found.servers.is_empty() {
        println!("  (nothing listening on any candidate port)");
    }

    println!("\n=== 3. claimed but NOT listening ===");
    for c in &found.silent {
        let claim = c
            .hints
            .iter()
            .find_map(|h| h.claimed_status.clone())
            .unwrap_or_else(|| "no status".into());
        println!("  {}:{} — {} — its config says \"{claim}\", nothing answered", c.host, c.port, c.label());
    }
    if found.silent.is_empty() {
        println!("  (none)");
    }

    println!("\n=== 4. the invariant: a server is present only because it ANSWERED ===");
    for s in &found.servers {
        let re = dbsource::probe(&s.host, s.port);
        let listening = matches!(re, Probe::Listening(_));
        println!("  re-probe {}:{} -> listening={listening}", s.host, s.port);
        ok &= listening;
    }
    // Anything DBngin named that is in `servers` must NOT be there on the
    // strength of the plist: this asserts the pairing directly.
    for (_, port, _) in &dbngin {
        let in_servers = found.servers.iter().any(|s| s.port == *port);
        let in_silent = found.silent.iter().any(|c| c.port == *port);
        let answered = matches!(dbsource::probe("127.0.0.1", *port), Probe::Listening(_));
        println!(
            "  DBngin port {port}: answered={answered} -> servers={in_servers} silent={in_silent}"
        );
        ok &= in_servers == answered;
        ok &= in_silent == !answered;
    }

    println!("\n=== 5. a site's own config outranks a generic guess ===");
    let cands = dbsource::candidates(&home, &[("127.0.0.1".into(), 3306)]);
    let c = cands.iter().find(|c| c.port == 3306).expect("3306 is a candidate");
    let from_site = c.hints.iter().any(|h| h.source == HintSource::SiteConfig);
    println!("  3306 carries a site-config hint: {from_site}");
    ok &= from_site;

    if ok {
        println!(
            "\nOK — labels come from files, presence comes from listeners, and the vendor \
             comes from the wire without logging in."
        );
    } else {
        eprintln!("\nFAILED — see above.");
        std::process::exit(1);
    }
}
