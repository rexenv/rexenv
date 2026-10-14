//! Push a local site TO its live one (plan §2.5, §2.6 — L10). The local site is
//! never rewritten: its database is copied, the COPY's URLs are moved to the
//! live host, and the copy is what gets dumped. On the live side everything
//! lands in a quarantine and shadow tables until ONE swap (plugin #832).
//!
//! Conflicts are the plugin's verdict against the BASE rexenv recorded at the
//! last pull/push: tables and files live changed since. They stop the push
//! unless the caller named each in `override` — rexenv never merges.

use super::base::SyncBase;
use super::client::{Client, Conflicts};
use super::pull::LocalSite;
use crate::core::wordpress;
use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Tables only live writes (plan §2.6): unticked by default on a push, because
/// what live wrote there since the last pull would be lost. Suffix patterns on
/// the table name without its prefix. ONE list, here.
pub const LIVE_OWNED: [&str; 12] = [
    "users", "usermeta", "comments", "commentmeta",
    "wc_orders", "wc_orders_meta", "wc_order_stats", "wc_order_product_lookup", "woocommerce_sessions", "woocommerce_order_items",
    "gf_entry", "wpforms_entries",
];

/// Is `table` (full name, with `prefix`) one live owns by default?
pub fn live_owned(prefix: &str, table: &str) -> bool {
    let bare = table.strip_prefix(prefix).unwrap_or(table);
    LIVE_OWNED.iter().any(|p| bare == *p || bare.starts_with(&format!("{p}_")))
}

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    /// Tables to push — full names. `None` = every table CHANGED HERE since the base
    /// (by `stamps::local_stamps` against `base.local_tables`) except the live-owned;
    /// with a base that has no local stamps, every table except the live-owned.
    pub tables: Option<Vec<String>>,
    /// Push files too (changed against the base, or all when there is no base).
    pub files: bool,
    /// Conflicts the person chose to overwrite anyway.
    pub override_items: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PushReport {
    pub tables: Vec<String>,
    /// Tables unchanged here since the base, left out (when `tables` was `None`).
    pub tables_unchanged: usize,
    pub files: usize,
    pub backup_id: String,
    pub base: SyncBase,
}

/// Which of `local_now`'s tables changed since the base's local stamps — `None`
/// when the base carries none (an old base: nothing can be said). Pure.
pub fn changed_locally(base: &SyncBase, local_now: &BTreeMap<String, String>) -> Option<Vec<String>> {
    if base.local_tables.is_empty() {
        return None;
    }
    Some(local_now.iter().filter(|(t, s)| base.local_tables.get(*t) != Some(*s)).map(|(t, _)| t.clone()).collect())
}

/// What stopped a push before anything on live changed.
#[derive(Debug)]
pub enum PushStop {
    /// Live changed since the base; nothing was sent. The person picks.
    Conflicts(Vec<String>),
    Failed(Error),
}

impl From<Error> for PushStop {
    fn from(e: Error) -> Self {
        PushStop::Failed(e)
    }
}

impl From<std::io::Error> for PushStop {
    fn from(e: std::io::Error) -> Self {
        PushStop::Failed(e.into())
    }
}

/// Split a dump at statement ends into pieces of at most `max` bytes.
pub fn chunks(sql: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for stmt in sql.split_inclusive(";\n") {
        if !cur.is_empty() && cur.len() + stmt.len() > max {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(stmt);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Push `local` to its live site. `prior` is the base from the last pull/push
/// (None = never synced: every file counts as new, and every table a conflict
/// the plugin cannot judge — the caller should have warned).
pub async fn push_from(
    client: &Client,
    local: &LocalSite<'_>,
    prior: Option<&SyncBase>,
    options: &PushOptions,
    on_line: &mut (dyn FnMut(&str) + Send),
) -> std::result::Result<PushReport, PushStop> {
    if !local.engine.is_mysql_family() {
        return Err(Error::Other("only a MySQL or MariaDB site can be pushed to a live WordPress".into()).into());
    }
    let m = client.manifest().await?;
    let empty = SyncBase::default();
    let base = prior.unwrap_or(&empty);
    let local_tables = local.engine.list_tables(local.client, local.port, local.db_name)?;
    let mut tables_unchanged = 0;
    let tables: Vec<String> = match &options.tables {
        Some(t) => t.clone(),
        None => {
            let candidates: Vec<String> = m.tables.iter().map(|t| t.name.clone()).filter(|t| !live_owned(&m.prefix, t)).collect();
            // Only what changed HERE since the base (14 Oct 2026) — a table the local
            // copy did not touch has nothing to send. An old base without local stamps
            // cannot tell, and sends them all as before.
            match changed_locally(base, &super::stamps::local_stamps(local, &m.prefix)?) {
                Some(changed) => {
                    let picked: Vec<String> = candidates.iter().filter(|t| changed.contains(t)).cloned().collect();
                    tables_unchanged = candidates.len() - picked.len();
                    picked
                }
                None => candidates,
            }
        }
    };
    for t in &tables {
        if !local_tables.contains(t) {
            return Err(Error::Other(format!("{} has no table {t} to push.", local.domain)).into());
        }
    }
    // Files: changed against the base, every file when there is no base.
    let content = local.docroot.join("wp-content");
    let mut files: Vec<String> = Vec::new();
    let mut total_files = 0;
    if options.files {
        for (rel, _size, mtime) in super::pull::walk_local(&content)? {
            total_files += 1;
            // Changed LOCALLY since the last sync (`SyncBase::file_changed_here`: the
            // recorded local mtime, or "after `at`" on an older base). The base's own
            // file stamps are LIVE's — the plugin's conflict check reads those; a local
            // file's mtime never matches them.
            if prior.is_none() || base.file_changed_here(&rel, mtime) {
                files.push(rel);
            }
        }
    }
    if tables.is_empty() && files.is_empty() {
        return Err(Error::Other(format!("nothing has changed on {} since the last sync — there is nothing to push.", local.domain)).into());
    }
    on_line(&format!(
        "push: {} tables{}, {} of {} files changed since the last sync, to {}",
        tables.len(),
        if tables_unchanged > 0 { format!(" ({tables_unchanged} unchanged here, left out)") } else { String::new() },
        files.len(),
        total_files,
        m.site_url
    ));

    // ── the local copy, moved to the live host ───────────────────────────
    let copy = format!("{}_push", local.db_name);
    crate::core::database::validate_db_name(&copy)?;
    local.engine.drop_database(local.client, local.port, &copy)?;
    local.engine.create_database(local.client, local.port, &copy)?;
    let staged: Result<()> = (|| {
        let dump_file = local.scratch.join("push-local.sql");
        std::fs::create_dir_all(local.scratch)?;
        // The WHOLE local database into the copy, not only the chosen tables: the
        // URL move boots WordPress against the copy, and a copy without `wp_options`
        // cannot boot ("Error establishing a database connection" — the first
        // one-table push, 10 Oct 2026). Only the chosen tables are dumped for upload.
        local.engine.dump_tables_to_file(&local.dump, local.port, local.db_name, &[], &dump_file)?;
        local.engine.import_from_file(local.client, local.port, &copy, &dump_file)?;
        let _ = std::fs::remove_file(&dump_file);
        let live_host = super::pull::host_of(&m.site_url);
        let n = wordpress::rehome_urls_on_copy(local.platform, local.php, local.wp_phar, local.docroot, local.scratch, local.port, &copy, local.domain, &live_host, false)?;
        on_line(&format!("URLs: https://{} → https://{live_host} in the copy ({n} replacements)", local.domain));
        Ok(())
    })();
    if let Err(e) = staged {
        let _ = local.engine.drop_database(local.client, local.port, &copy);
        return Err(e.into());
    }

    // ── begin: the plugin's conflict verdict ─────────────────────────────
    let begun = match client.push_begin(&tables, &files, base, &options.override_items).await {
        Ok(Ok(b)) => b,
        Ok(Err(Conflicts(list))) => {
            let _ = local.engine.drop_database(local.client, local.port, &copy);
            // Only names rexenv asked about: the list is the site's to send, and a
            // site could pad it to nudge an override (#837). Anything else is a
            // refusal, not a choice to offer.
            let asked: Vec<&String> = tables.iter().chain(files.iter()).collect();
            let (ours, theirs): (Vec<String>, Vec<String>) = list.into_iter().partition(|c| asked.contains(&c));
            if ours.is_empty() {
                return Err(PushStop::Failed(Error::Other(format!("the site named conflicts rexenv did not ask about ({}) — nothing was sent.", theirs.join(", ")))));
            }
            return Err(PushStop::Conflicts(ours));
        }
        Err(e) => {
            let _ = local.engine.drop_database(local.client, local.port, &copy);
            return Err(e.into());
        }
    };
    let push_id = begun.push_id.clone();
    let sent: Result<()> = async {
        for rel in &files {
            let mut p = content.clone();
            for seg in rel.split('/') {
                p.push(seg);
            }
            client.push_file(&push_id, rel, &std::fs::read(&p)?).await?;
        }
        if !files.is_empty() {
            on_line(&format!("  {} in the quarantine", super::count(files.len(), "file", "files")));
        }
        for t in &tables {
            let f = local.scratch.join(format!("push-{t}.sql"));
            local.engine.dump_tables_to_file(&local.dump, local.port, &copy, &[t.as_str()], &f)?;
            let sql = std::fs::read_to_string(&f)?;
            let _ = std::fs::remove_file(&f);
            let pieces = chunks(&sql, 4 * 1024 * 1024);
            for piece in &pieces {
                client.push_sql(&push_id, t, piece).await?;
            }
            on_line(&format!("  {t} — {} in the shadow table", super::count(pieces.len(), "chunk", "chunks")));
        }
        Ok(())
    }
    .await;
    let _ = local.engine.drop_database(local.client, local.port, &copy);
    if let Err(e) = sent {
        let _ = client.push_abort(&push_id).await;
        return Err(e.into());
    }

    // ── the swap, then the proof ─────────────────────────────────────────
    let backup_id = client.push_swap(&push_id).await?;
    on_line(&format!("swapped in; the previous live tables are the site's backup {backup_id}"));
    let after = client.manifest().await?;
    let mut new_base = SyncBase {
        tables: after.tables.iter().map(|t| (t.name.clone(), t.checksum.clone())).collect(),
        files: base.files.clone(),
        at: super::base::now(),
        // The local copy is what was just sent: its stamps now are the new local base.
        local_tables: super::stamps::local_stamps(local, &m.prefix)?,
        local_files: super::pull::walk_local(&content)?.into_iter().map(|(p, _, m)| (p, m)).collect(),
        kind: "push".into(),
        // What a rollback of THIS push puts back as the local half (#862).
        before_push: Some(Box::new(base.local_half())),
    };
    // The files just pushed now carry LIVE mtimes we did not read; re-list them.
    if options.files {
        for f in client.list_files(&super::pull::DEFAULT_EXCLUDES, None).await? {
            new_base.files.insert(f.path, format!("{}:{}", f.size, f.mtime));
        }
    }
    Ok(PushReport { tables, tables_unchanged, files: files.len(), backup_id, base: new_base })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_owned_tables_and_chunking() {
        assert!(live_owned("wp_", "wp_users"));
        assert!(live_owned("wp_", "wp_wc_orders_meta"));
        assert!(live_owned("wp_", "wp_woocommerce_sessions"));
        assert!(!live_owned("wp_", "wp_posts"));
        assert!(!live_owned("wp_", "wp_userless"), "a prefix of a name is not the name");
        let sql = "A;\nB;\nCCCC;\n";
        assert_eq!(chunks(sql, 6), vec!["A;\nB;\n", "CCCC;\n"]);
        assert_eq!(chunks(sql, 100), vec![sql]);
    }

    /// **A push with no table list sends only the tables changed HERE since the base**
    /// (ledger #860): same stamp → left out; a base without local stamps → `None`
    /// (every table, as before). Plant: compare against `base.tables` → the live stamp
    /// never equals a local one and every table is "changed".
    #[test]
    fn the_default_push_is_what_changed_here() {
        let base = SyncBase {
            local_tables: [("wp_posts", "ck:1"), ("wp_options", "opt:3:9")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            tables: [("wp_posts", "ai:7"), ("wp_options", "ai:8")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            ..Default::default()
        };
        let now: BTreeMap<String, String> = [("wp_posts", "ck:2"), ("wp_options", "opt:3:9"), ("wp_new", "ck:0")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(changed_locally(&base, &now), Some(vec!["wp_new".to_string(), "wp_posts".to_string()]));
        assert_eq!(changed_locally(&SyncBase::default(), &now), None);
    }
}
