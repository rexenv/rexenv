//! Push a local site TO its live one (plan §2.5, §2.6 — L10). The local site is
//! never rewritten: its database is copied, the COPY's URLs are moved to the
//! live host, and the copy is what gets dumped. On the live side everything
//! lands in a quarantine and shadow tables until ONE swap (plugin #832).
//!
//! Conflicts are the plugin's verdict against the BASE rexenv recorded at the
//! last pull/push: tables and files live changed since. They stop the push
//! unless the caller named each in `override` — rexenv never merges; a conflict
//! the caller named in `skip` is left out of the push instead (per-item choice,
//! 15 Oct 2026). A table the pull ANONYMISED here is refused outright (#879).
//!
//! Resume (15 Oct 2026, `progress.rs`): a push that died after `begin` leaves the open
//! push on live and a record here; the next push of the same (or a narrower) set asks
//! the plugin to continue it, and sends only the tables it does not hold whole and the
//! files it holds short — provided nothing changed here since.

use super::base::SyncBase;
use super::client::{Client, Conflicts};
use super::progress;
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

/// Is `table` (full name, with `prefix`) one live owns by default? On a network a
/// sub-site's table carries its blog id after the prefix (`wp_2_comments`): the id is
/// stepped over, so every sub-site's comments and orders count as live-owned too (S5).
pub fn live_owned(prefix: &str, table: &str) -> bool {
    let bare = bare_name(prefix, table);
    LIVE_OWNED.iter().any(|p| bare == *p || bare.starts_with(&format!("{p}_")))
}

/// The table name without the site prefix and, on a network, without a sub-site's
/// `<id>_` — `wp_2_comments` → `comments`, `wp_comments` → `comments`. Pure.
pub fn bare_name<'a>(prefix: &str, table: &'a str) -> &'a str {
    let bare = table.strip_prefix(prefix).unwrap_or(table);
    match bare.split_once('_') {
        Some((id, rest)) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty() => rest,
        _ => bare,
    }
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
    /// Tables and files to leave OUT of this push — conflicts the person chose to keep
    /// live's version of.
    pub skip: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PushReport {
    pub tables: Vec<String>,
    /// Tables unchanged here since the base, left out (when `tables` was `None`).
    pub tables_unchanged: usize,
    pub files: usize,
    /// Tables and files the interrupted push had already put on live whole, not sent again.
    pub tables_resumed: usize,
    pub files_resumed: usize,
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
    // Per-item choice: what the person chose to keep live's version of is left out.
    let before_skip = tables.len() + files.len();
    let tables: Vec<String> = tables.into_iter().filter(|t| !options.skip.contains(t)).collect();
    files.retain(|f| !options.skip.contains(f));
    let skipped = before_skip - tables.len() - files.len();
    // Never a scrubbed table (#879): the local rows are anonymised copies of live's.
    if let Some(t) = tables.iter().find(|t| base.anonymised.contains(t)) {
        return Err(Error::Other(format!(
            "{t} holds anonymised data here (anonymise on pull is on for {}) — it is never pushed. Turn anonymising off and pull again to push it.",
            local.domain
        ))
        .into());
    }
    if tables.is_empty() && files.is_empty() {
        return Err(Error::Other(if skipped > 0 {
            "everything that changed was left out — nothing to push.".to_string()
        } else {
            format!("nothing has changed on {} since the last sync — there is nothing to push.", local.domain)
        })
        .into());
    }
    on_line(&format!(
        "push: {} tables{}, {} of {} files changed since the last sync{}, to {}",
        tables.len(),
        if tables_unchanged > 0 { format!(" ({tables_unchanged} unchanged here, left out)") } else { String::new() },
        files.len(),
        total_files,
        if skipped > 0 { format!(", {skipped} kept as live has them") } else { String::new() },
        m.site_url
    ));
    // An interrupted push to continue (`progress.rs`), when the plugin can.
    let record_file = progress::push_file(local.scratch, local.db_name);
    let open: Option<progress::PushProgress> = progress::read(&record_file);
    let plugin_resumes = super::client::plugin_version(&m.plugin) >= Some(super::client::RESUME_PLUGIN);
    let resume_id: Option<String> = open.as_ref().filter(|r| plugin_resumes && progress::push_resumable(r, &tables, &files)).map(|r| r.push_id.clone());
    if let Some(r) = open.as_ref().filter(|_| resume_id.is_none()) {
        // Not reusable (another set, an older plugin): the plugin's next `begin` drops it.
        on_line(&format!("  an interrupted push ({}) is on live and cannot be continued for this set — it is dropped", &r.push_id[..r.push_id.len().min(8)]));
        progress::forget(&record_file);
    }

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
        // A network's copy moves network-aware (S5): the local wp-config says so.
        let network = crate::core::sites::network_mode_on_disk(local.docroot).is_some();
        let n = wordpress::rehome_urls_on_copy(local.platform, local.php, local.wp_phar, local.docroot, local.scratch, local.port, &copy, local.domain, &live_host, network)?;
        on_line(&format!("URLs: https://{} → https://{live_host} in the copy ({n} replacements)", local.domain));
        Ok(())
    })();
    if let Err(e) = staged {
        let _ = local.engine.drop_database(local.client, local.port, &copy);
        return Err(e.into());
    }

    // ── begin: the plugin's conflict verdict ─────────────────────────────
    let mut begun_try = client.push_begin(&tables, &files, base, &options.override_items, resume_id.as_deref()).await;
    if resume_id.is_some() && matches!(&begun_try, Err(e) if e.to_string().contains("no longer has that push")) {
        // Live dropped it (the record was stale): a fresh one.
        progress::forget(&record_file);
        begun_try = client.push_begin(&tables, &files, base, &options.override_items, None).await;
    }
    let begun = match begun_try {
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
    // What the open push already holds whole, and has not changed here since: skipped.
    let local_stamps_now = super::stamps::local_stamps(local, &m.prefix)?;
    let local_files_now: BTreeMap<String, (u64, i64)> = super::pull::walk_local(&content)?.into_iter().map(|(p, s, m)| (p, (s, m))).collect();
    let (skip_tables, skip_files) = match (&open, begun.resumed) {
        (Some(rec), true) => (progress::push_tables_to_skip(rec, &begun.db_done, &local_stamps_now), progress::push_files_to_skip(rec, &begun.received, &local_files_now)),
        _ => (Vec::new(), Vec::new()),
    };
    if begun.resumed {
        on_line(&format!("  continuing the interrupted push: {} and {} already on live, not sent again", super::count(skip_tables.len(), "table", "tables"), super::count(skip_files.len(), "file", "files")));
    }
    // The record for a resume of THIS push, written before the first byte goes.
    let record = progress::PushProgress {
        push_id: push_id.clone(),
        tables: tables.clone(),
        files: files.clone(),
        local_tables: tables.iter().filter_map(|t| local_stamps_now.get(t).map(|s| (t.clone(), s.clone()))).collect(),
        local_files: files.iter().filter_map(|f| local_files_now.get(f).map(|(_, m)| (f.clone(), *m))).collect(),
    };
    progress::write(&record_file, &record)?;
    let sent: Result<()> = async {
        for rel in files.iter().filter(|f| !skip_files.contains(f)) {
            let mut p = content.clone();
            for seg in rel.split('/') {
                p.push(seg);
            }
            client.push_file(&push_id, rel, &std::fs::read(&p)?).await?;
        }
        if !files.is_empty() {
            on_line(&format!("  {} in the quarantine", super::count(files.len() - skip_files.len(), "file", "files")));
        }
        for t in tables.iter().filter(|t| !skip_tables.contains(t)) {
            let f = local.scratch.join(format!("push-{t}.sql"));
            local.engine.dump_tables_to_file(&local.dump, local.port, &copy, &[t.as_str()], &f)?;
            let sql = std::fs::read_to_string(&f)?;
            let _ = std::fs::remove_file(&f);
            // A chunk the host refuses as too large (413: `post_max_size` 8M on the cPanel
            // host, with the JSON's escaping on top of 4 MB) halves the chunk size and sends
            // the table again from its first chunk — which drops the shadow table (#892).
            let mut max = 4 * 1024 * 1024;
            let pieces = loop {
                let pieces = chunks(&sql, max);
                let mut outcome = Ok(());
                for (i, piece) in pieces.iter().enumerate() {
                    if let Err(e) = client.push_sql(&push_id, t, piece, i + 1, pieces.len()).await {
                        outcome = Err(e);
                        break;
                    }
                }
                match outcome {
                    Ok(()) => break pieces,
                    Err(e) if max > 64 * 1024 && e.to_string().contains("refused a request that large") => {
                        max /= 2;
                        on_line(&format!("  {t} — the site refused a {} KB chunk; sending it in {} KB chunks", max * 2 / 1024, max / 1024));
                    }
                    Err(e) => return Err(e),
                }
            };
            on_line(&format!("  {t} — {} in the shadow table", super::count(pieces.len(), "chunk", "chunks")));
        }
        Ok(())
    }
    .await;
    let _ = local.engine.drop_database(local.client, local.port, &copy);
    if let Err(e) = sent {
        // The open push stays on live, the record here: the next push of this set
        // continues from what landed (a plugin that cannot resume gets it aborted then).
        if !plugin_resumes {
            let _ = client.push_abort(&push_id).await;
            progress::forget(&record_file);
            return Err(e.into());
        }
        return Err(Error::Other(format!("{e} — the next push continues from what already reached the site")).into());
    }

    // ── the swap, then the proof ─────────────────────────────────────────
    let backup_id = client.push_swap(&push_id).await?;
    progress::forget(&record_file);
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
        anonymised: base.anonymised.clone(),
    };
    // The files just pushed now carry LIVE mtimes we did not read; re-list them.
    if options.files {
        for f in client.list_files(&super::pull::DEFAULT_EXCLUDES, None).await? {
            new_base.files.insert(f.path, format!("{}:{}", f.size, f.mtime));
        }
    }
    Ok(PushReport { tables, tables_unchanged, files: files.len(), tables_resumed: skip_tables.len(), files_resumed: skip_files.len(), backup_id, base: new_base })
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
        // A network's sub-site tables (S5): the blog id is stepped over.
        assert!(live_owned("wpms_", "wpms_2_comments") && live_owned("wpms_", "wpms_12_wc_orders") && !live_owned("wpms_", "wpms_2_posts"));
        assert_eq!(bare_name("wpms_", "wpms_2_posts"), "posts");
        assert_eq!(bare_name("wpms_", "wpms_posts"), "posts");
        assert_eq!(bare_name("wpms_", "wpms_x_posts"), "x_posts");
        let sql = "A;\nB;\nCCCC;\n";
        assert_eq!(chunks(sql, 6), vec!["A;\nB;\n", "CCCC;\n"]);
        assert_eq!(chunks(sql, 100), vec![sql]);
    }

    /// **A table the pull anonymised is never pushed, and an item in `skip` is left out
    /// before the plugin is asked** (ledger #879). TEXT: the refusal reads `base.anonymised`
    /// and comes before `push_begin`; the skip filter too. Plant: move the refusal after
    /// `push_begin(` → red.
    #[test]
    fn anonymised_tables_are_refused_and_skips_applied_before_begin() {
        let src = include_str!("push.rs");
        let body = &src[src.find("pub async fn push_from(").expect("push_from")..];
        let refuse = body.find("base.anonymised.contains(t)").expect("the anonymised refusal");
        let skip = body.find("!options.skip.contains(t)").expect("the skip filter");
        let begin = body.find("client.push_begin(").expect("begin");
        assert!(refuse < begin && skip < begin, "refusal/skip after begin");
        assert!(body.contains("it is never pushed"));
        // The new base carries the list on.
        assert!(body.contains("anonymised: base.anonymised.clone()"));
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
