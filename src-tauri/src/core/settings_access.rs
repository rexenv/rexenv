//! core::settings_access — which settings keys `rex config` may read and write.
//!
//! # Why this is a policy file and not a list in the CLI
//!
//! `commands::settings::set_setting` is a GENERIC key/value writer. It routes
//! the keys in [`GATED_SETTERS`] through their validating setters (`default_tld`
//! → policy gate, `sites_dir` → validation) and everything else falls through to
//! a raw write.
//! That is fine for the app, where the only writers are the controls on the
//! Settings screen. It is not fine for a shell.
//!
//! The settings table holds more than preferences. It holds the SIGNED PHP
//! update manifest, its signature, and `php_update_manifest_serial` — which is
//! monotonic rollback protection: `core::updates` refuses a document whose
//! serial is not greater than the highest ever accepted. A raw `rex config set`
//! would let a one-liner reset that high-water mark, which is the difference
//! between a convenience command and a way to re-open a replayed manifest.
//!
//! So: **deny by default**. A key nobody has ruled on is refused, not written.
//! A key added next month is refused until somebody decides, rather than
//! silently becoming writable because the CLI enumerates the table.
//!
//! The decisions live HERE, in core, because two things must agree about them:
//! the CLI dispatch and the guard that checks the CLI cannot write round a
//! validating setter. A list in `cli_server.rs` would be a second copy of a
//! security boundary — the shape this tree keeps finding as the actual defect.

/// A validating setter: the ONE door a gated key's value may come through.
/// Both of today's have this shape (`&Connection, &str -> stored value`), and a
/// third one must too — that is what makes the registry below possible.
pub type ValidatingSetter = fn(&rusqlite::Connection, &str) -> crate::error::Result<String>;

/// **The gated keys, declared beside their setters.** `set_setting` DISPATCHES
/// through this — it holds no per-key `if` — so adding a row here is the whole
/// act of gating a key, and there is no second place to remember.
///
/// Why a registry and not a match: the match was two hardcoded arms and the
/// guard that checked it was two hardcoded names, so a THIRD gated key was
/// settable straight past its rule with nothing failing (`docs/TODO.md`, the
/// guard-covers-claimed-surface family, ledger #344). Nothing derivable
/// distinguishes "has a validating setter" from any other `pub fn`, so the
/// fact is declared once, here, and every consumer reads it: the dispatch,
/// `cli_access` below, and the guard.
pub const GATED_SETTERS: &[(&str, ValidatingSetter)] = &[
    (crate::core::sites::DEFAULT_TLD_KEY, crate::core::sites::set_default_tld),
    (crate::core::sites::SITES_DIR_KEY, crate::core::sites::set_sites_dir),
    (crate::core::scratch::SCRATCH_CAP_KEY, crate::core::scratch::set_scratch_cap),
    (crate::core::scratch::SCRATCH_TTL_KEY, crate::core::scratch::set_scratch_ttl_hours),
];

/// The validating setter for `key`, if it is gated.
pub fn gated_setter(key: &str) -> Option<ValidatingSetter> {
    GATED_SETTERS.iter().find(|(k, _)| *k == key).map(|(_, f)| *f)
}

/// What `rex config` may do with one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAccess {
    /// Readable and writable.
    ReadWrite,
    /// Readable; writes refused, with the reason.
    ReadOnly(&'static str),
    /// Neither, with the reason.
    Denied(&'static str),
}

/// Writable keys that reach the RAW setter — i.e. have no validating route in
/// `set_setting` — each with the reason that is acceptable.
///
/// The guard reads this: a key may be `ReadWrite` only if it is routed through
/// a validating setter OR named here. Adding a writable key without either is
/// what the guard exists to stop.
pub const UNVALIDATED_BUT_SAFE: &[(&str, &str)] = &[
    (
        "preferred_editor",
        "an editor ID matched against a known table at the point of use — an unknown \
         value yields `unknown editor: …`, never an executed string",
    ),
    (
        "preferred_browser",
        "same shape as preferred_editor: an ID looked up in the BROWSERS table, never \
         a command line",
    ),
    (
        "start_services_on_launch",
        "a boolean read once at launch; a junk value reads as false, which is the safe \
         direction (nothing starts)",
    ),
    (
        "app_update_auto_check",
        "a boolean read before any update I/O. Junk reads as ON, the OPPOSITE of \
         start_services_on_launch and deliberately: there the safe direction is that \
         nothing starts, here it is that the user still hears about a security release. \
         Only the exact string `false` turns it off",
    ),
    (
        "app_update_skipped",
        "a version string compared LIVE against the offer — junk simply never equals a \
         version, so the worst a bad value does is nothing",
    ),
];

/// The ruling for `key`. Unknown keys are DENIED — that is the default, and it
/// is the point.
pub fn cli_access(key: &str) -> CliAccess {
    // Per-engine version pins: `db_version_mysql`, `db_version_mariadb`, …
    if key.starts_with("db_version_") {
        return CliAccess::ReadOnly(
            "a pinned engine version — it decides which binary gets staged and run. \
             Change it on the Databases screen, which checks the version is one rexenv \
             ships",
        );
    }
    // Gated keys are writable BY DEFINITION of being gated — read off the
    // registry, not restated here, so the two can never disagree.
    if gated_setter(key).is_some() {
        return CliAccess::ReadWrite;
    }
    match key {
        // Preferences that reach the raw setter; see UNVALIDATED_BUT_SAFE.
        "preferred_editor" | "preferred_browser" | "start_services_on_launch"
        | "app_update_auto_check" | "app_update_skipped" => CliAccess::ReadWrite,
        // The signed update chain. The serial is the one that matters most:
        // resetting it re-opens a replayed older manifest.
        "php_update_manifest" | "php_update_manifest_sig" => CliAccess::Denied(
            "part of the SIGNED PHP update chain — a manifest and its signature are \
             verified together and are not yours or mine to hand-edit",
        ),
        "php_update_manifest_serial" => CliAccess::Denied(
            "the update chain's rollback protection: rexenv refuses any manifest whose \
             serial is not greater than the highest already accepted, and writing this \
             key resets that high-water mark",
        ),
        // The APP's own update chain (`core::app_update`). Same shape, same
        // reasons, and a strictly larger grant: these bytes are the process that
        // enforces every rule on this page.
        // Per document since 28 Sep 2026 (`app_update::doc_key`): the bare names are the first
        // document's, `<name>:<document>` the others'. Same chain, same refusal.
        k if k == "app_update_release"
            || k == "app_update_release_sig"
            || k.starts_with("app_update_release:")
            || k.starts_with("app_update_release_sig:") =>
        CliAccess::Denied(
            "part of the SIGNED app update chain — the descriptor and its signature are \
             verified together, and they name the bytes rexenv would replace ITSELF with",
        ),
        k if k == "app_update_release_serial" || k.starts_with("app_update_release_serial:") => CliAccess::Denied(
            "the app update chain's rollback protection — writing this key re-opens a \
             replayed older release, which is how a host would hold rexenv on a \
             superseded build",
        ),
        "app_update_check" => CliAccess::ReadOnly(
            "a cache of the last update check, rewritten on the next one; worth reading \
             to see when it last ran, not worth hand-writing",
        ),
        "app_update_notice" => CliAccess::ReadOnly(
            "what the previous process recorded before it swapped the bundle; the next \
             launch consumes it once and reports the version it reads from ITSELF",
        ),
        // Pins and caches: worth reading, not worth writing from a shell.
        "adminer_version" => CliAccess::ReadOnly(
            "a pinned version — it decides which Adminer gets staged. The Databases \
             screen's update flow checks the download before it moves the pin",
        ),
        "php_upstream_check" => CliAccess::ReadOnly(
            "a cache of the last upstream check; rexenv rewrites it on the next check, \
             so a hand-written value is both untrusted and short-lived",
        ),
        // Agent control surface: the app's toggle carries the consent copy that
        // explains what enabling it exposes, and a CLI flag would route round it.
        "mcp_enabled" => CliAccess::Denied(
            "the MCP agent socket's enable flag — the app's toggle carries the consent \
             wording that explains what it opens, and a shell write would skip it",
        ),
        // The Agent access dial (D15): what an agent may do to the user's own
        // sites. An agent widening its own access is the thing the dial exists
        // to prevent, and the card's copy says what each level hands over.
        "agent_access_level" | "agent_access_mode" | "agent_access_expires_at" => CliAccess::Denied(
            "the Agent access dial — the card in Settings → AI agents (MCP) carries the \
             sentence that says what each level hands over, and neither a shell write \
             nor an agent may turn it",
        ),
        _ => CliAccess::Denied(
            "not on the CLI allow-list. Settings are denied unless somebody has ruled \
             on them, because this table holds signed update state and version pins \
             beside the preferences",
        ),
    }
}
