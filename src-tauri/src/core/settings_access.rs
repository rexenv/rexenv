//! core::settings_access — which settings keys `rex config` may read and write.
//!
//! # Why this is a policy file and not a list in the CLI
//!
//! `commands::settings::set_setting` is a GENERIC key/value writer. It routes
//! exactly two keys through validating setters (`default_tld` → policy gate,
//! `sites_dir` → validation) and everything else falls through to a raw write.
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
    match key {
        // Validated in `set_setting` by a real setter.
        "default_tld" | "sites_dir" => CliAccess::ReadWrite,
        // Preferences that reach the raw setter; see UNVALIDATED_BUT_SAFE.
        "preferred_editor" | "preferred_browser" | "start_services_on_launch" => {
            CliAccess::ReadWrite
        }
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
        "mcp_enabled" | "mcp_mail_enabled" => CliAccess::Denied(
            "the MCP agent socket's enable flag — the app's toggle carries the consent \
             wording that explains what it opens, and a shell write would skip it",
        ),
        _ => CliAccess::Denied(
            "not on the CLI allow-list. Settings are denied unless somebody has ruled \
             on them, because this table holds signed update state and version pins \
             beside the preferences",
        ),
    }
}
