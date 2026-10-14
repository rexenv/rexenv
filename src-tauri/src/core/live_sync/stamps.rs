//! Table STAMPS — the one-line fact "has this table changed since" on either side
//! of a sync (plan §2.6). Live's stamps come from the companion plugin's manifest
//! (`SHOW TABLE STATUS` fields, `docs/rexsync-protocol.md` §4.1); the LOCAL copy's
//! come from here: `CHECKSUM TABLE` per table (exact — it is this machine's own
//! database), and for the options table the same CONTENT stamp the plugin uses,
//! over the rows that do not move on their own.
//!
//! Each side is compared only with ITS OWN earlier stamp (the sync base): a local
//! `ck:` value is never set beside a live `ai:…` value.

use super::pull::LocalSite;
use crate::error::Result;
use std::collections::BTreeMap;

/// Option rows that move on their own — transients, the cron queue, rewrite rules,
/// the plugin's rows — left out of the options table's stamp on BOTH sides. The
/// plugin's copy is `Rexenv_Sync_Reader::VOLATILE_OPTIONS`; a pattern added here is
/// added there in the same commit. LIKE patterns with `_` escaped.
pub const VOLATILE_OPTIONS: [&str; 7] = [
    "\\_transient\\_%",
    "\\_site\\_transient\\_%",
    "cron",
    "rewrite\\_rules",
    "recently\\_activated",
    "rexsync\\_%",
    "action\\_scheduler\\_lock\\_%",
];

/// The options stamp query — `COUNT(*)` and the CRC32 sum of the non-volatile rows
/// of `table`, as one tab-separated row.
pub fn options_stamp_sql(table: &str) -> String {
    let not = VOLATILE_OPTIONS.iter().map(|p| format!("option_name NOT LIKE '{p}'")).collect::<Vec<_>>().join(" AND ");
    format!("SELECT COUNT(*), COALESCE(SUM(CRC32(CONCAT_WS('|', option_name, option_value, autoload))), 0) FROM `{}` WHERE {not}", table.replace('`', "``"))
}

/// The companion plugin's own table in a LOCAL copy (`<prefix>rexsync_nonces`): made
/// when the plugin is activated here, never in live's manifest (the plugin leaves it
/// out), so it must never read as "new here" or be offered to a push — the macOS VM
/// run (14 Oct 2026) listed it ticked in the picker, and live would refuse it
/// ("No such table"). The ONE rule; `local_stamps` and the push plan read it.
pub fn plugin_own_table(prefix: &str, table: &str) -> bool {
    table.strip_prefix(prefix).is_some_and(|bare| bare == "rexsync_nonces")
}

/// Every table of the local site with its stamp, the plugin's own left out. `prefix`
/// names the options table.
pub fn local_stamps(local: &LocalSite<'_>, prefix: &str) -> Result<BTreeMap<String, String>> {
    let options = format!("{prefix}options");
    let mut stamps = crate::core::database::table_stamps(local.client, local.port, local.db_name, Some(&options), Some(&options_stamp_sql(&options)))?;
    stamps.retain(|t, _| !plugin_own_table(prefix, t));
    Ok(stamps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The volatile-row list is the SAME on both sides** (ledger #859): the plugin's
    /// `VOLATILE_OPTIONS` and this one, pattern for pattern. Plant: add a pattern on one
    /// side only → red.
    #[test]
    fn the_plugin_and_rexenv_leave_out_the_same_option_rows() {
        let php = include_str!("../../../../companion/rexenv-sync/includes/class-rexenv-sync-reader.php");
        let line = php.lines().find(|l| l.contains("const VOLATILE_OPTIONS")).expect("the plugin's list");
        let theirs: Vec<String> = line
            .split('\'')
            .skip(1)
            .step_by(2)
            .map(|p| p.replace("\\\\", "\\"))
            .collect();
        let ours: Vec<String> = VOLATILE_OPTIONS.iter().map(|s| s.to_string()).collect();
        assert_eq!(theirs, ours, "the plugin's VOLATILE_OPTIONS and rexenv's differ");
        let sql = options_stamp_sql("wp_options");
        assert!(sql.contains("NOT LIKE '\\_transient\\_%'") && sql.contains("NOT LIKE 'cron'") && sql.contains("CRC32"));
    }

    /// **The plugin's own nonce table in a local copy is nobody's change** (ledger #860):
    /// left out of the local stamps and the picker. Plant: `plugin_own_table` always
    /// false → red.
    #[test]
    fn the_plugins_own_table_is_left_out() {
        assert!(plugin_own_table("wpcs_", "wpcs_rexsync_nonces"));
        assert!(!plugin_own_table("wpcs_", "wpcs_posts") && !plugin_own_table("wp_", "wpcs_rexsync_nonces"));
        let src = include_str!("../../commands/live_sync.rs");
        assert!(src.contains("stamps::plugin_own_table(&m.prefix, name)"), "the push plan no longer leaves the plugin's table out");
    }
}
