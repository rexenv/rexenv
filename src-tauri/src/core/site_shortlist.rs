//! Which sites the person keeps at hand, in the order every surface shows them
//! (`docs/PLAN-sites-ux.md` T5, ledger #966).
//!
//! Four places list "the sites you are working on": the site switcher, the Sites
//! list's Pinned group, the sidebar, and the tray's Sites submenu. If each ordered
//! them itself there would be four orders — and the tray, which is Rust, could
//! not share the frontend's. So the order is computed here, once, from the
//! recorded facts (`state::store::site_prefs`), and every surface renders it.
//!
//! Pure: sites and prefs in, ids out. No clock, no database, no OS.

use crate::error::{Error, Result};
use crate::state::models::{Site, SitePref};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

/// How many recently opened sites the shortlist carries. Five: enough to cover
/// "the few I am on this week", few enough that the list is still a shortlist.
pub const RECENT_LIMIT: usize = 5;

/// The shortlist, as ids. `opened_at` carries every recorded open, so the Sites
/// list can sort ALL sites by it — the two short lists above it are the capped
/// view, this is the whole fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SiteShortlist {
    /// Pinned sites, by name (case-insensitively, then by domain so two sites
    /// with one name keep a stable order).
    pub pinned: Vec<String>,
    /// The most recently opened sites that are NOT pinned, newest first, at most
    /// [`RECENT_LIMIT`]. A pinned site is already at hand; listing it twice
    /// would spend a recent slot on saying so.
    pub recent: Vec<String>,
    /// site id → when its page was last opened (`datetime('now')` text, UTC).
    pub opened_at: HashMap<String, String>,
}

/// Build the shortlist. A pref whose site is gone is ignored (the table cascades,
/// so this is for a caller holding a list read a moment before a delete).
pub fn shortlist(sites: &[Site], prefs: &[SitePref]) -> SiteShortlist {
    let by_id: HashMap<&str, &Site> = sites.iter().map(|s| (s.id.as_str(), s)).collect();
    let key = |s: &Site| (s.name.to_lowercase(), s.domain.clone());

    let mut pinned: Vec<&Site> = prefs
        .iter()
        .filter(|p| p.pinned)
        .filter_map(|p| by_id.get(p.site_id.as_str()).copied())
        .collect();
    pinned.sort_by_key(|s| key(s));

    let mut recent: Vec<(&str, &Site)> = prefs
        .iter()
        .filter(|p| !p.pinned)
        .filter_map(|p| {
            let at = p.last_opened_at.as_deref()?;
            Some((at, by_id.get(p.site_id.as_str()).copied()?))
        })
        .collect();
    // Newest first. `datetime('now')` text sorts as time; a tie (two opens in one
    // second) falls back to the name so the order cannot flicker between reads.
    recent.sort_by(|a, b| b.0.cmp(a.0).then_with(|| key(a.1).cmp(&key(b.1))));

    SiteShortlist {
        pinned: pinned.iter().map(|s| s.id.clone()).collect(),
        recent: recent.iter().take(RECENT_LIMIT).map(|(_, s)| s.id.clone()).collect(),
        opened_at: prefs
            .iter()
            .filter(|p| by_id.contains_key(p.site_id.as_str()))
            .filter_map(|p| Some((p.site_id.clone(), p.last_opened_at.clone()?)))
            .collect(),
    }
}

/// The shortlist for the sites in the database right now.
pub fn read(conn: &Connection) -> Result<SiteShortlist> {
    let sites = crate::state::store::list_sites(conn)?;
    let prefs = crate::state::store::site_prefs(conn)?;
    Ok(shortlist(&sites, &prefs))
}

/// Why a scratch site cannot be pinned, in the words the UI shows.
pub const SCRATCH_PIN_REFUSAL: &str =
    "This is a scratch site an agent made — rexenv removes it when it expires, pinned or not. \
     Keep it first (row menu → Keep this site), then pin it.";

/// Pin or unpin a site.
///
/// **A scratch site is never pinned.** A pin says "I keep this one at hand", and
/// the reaper deletes a scratch site when it expires whatever is said about it —
/// so a pinned scratch site is a promise in the sidebar that vanishes overnight.
/// Pinning must not quietly adopt it either: adoption is Keep, a decision the
/// person makes on purpose (#213). So the pin is refused, and says where Keep is.
/// Unpinning is always allowed — it can only remove a promise.
pub fn set_pinned(conn: &Connection, id: &str, pinned: bool) -> Result<()> {
    let site = crate::state::store::get_site(conn, id)?
        .ok_or_else(|| Error::Other("That site no longer exists.".into()))?;
    if pinned && site.is_scratch() {
        return Err(Error::Other(SCRATCH_PIN_REFUSAL.into()));
    }
    crate::state::store::set_site_pinned(conn, id, pinned)?;
    Ok(())
}

/// Order the tray's sites: pinned, then recently opened, then the rest in the
/// order given. The tray caps its submenu and says what it hid — with this, what
/// it shows first is what the person is working on, not the first eight by date.
pub fn at_hand_first<T>(items: Vec<(String, T)>, list: &SiteShortlist) -> Vec<T> {
    let rank = |id: &str| -> (u8, usize) {
        if let Some(i) = list.pinned.iter().position(|p| p == id) {
            (0, i)
        } else if let Some(i) = list.recent.iter().position(|p| p == id) {
            (1, i)
        } else {
            (2, 0)
        }
    };
    let mut indexed: Vec<(usize, (String, T))> = items.into_iter().enumerate().collect();
    indexed.sort_by_key(|(i, (id, _))| {
        let (group, pos) = rank(id);
        (group, pos, *i)
    });
    indexed.into_iter().map(|(_, (_, t))| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::SiteOrigin;

    fn site(id: &str, name: &str) -> Site {
        let domain = format!("{}.rex", name.to_lowercase().replace(' ', "-"));
        let mut s = crate::state::models::test_site(id, &domain, SiteOrigin::User);
        s.name = name.into();
        s
    }

    fn pref(id: &str, pinned: bool, at: Option<&str>) -> SitePref {
        SitePref { site_id: id.into(), pinned, last_opened_at: at.map(str::to_string) }
    }

    #[test]
    fn pinned_by_name_then_recent_newest_first_and_a_pinned_site_is_not_also_recent() {
        let sites = vec![site("a", "zeta"), site("b", "Alpha"), site("c", "mid"), site("d", "old"), site("e", "never")];
        let prefs = vec![
            pref("a", true, Some("2026-11-14 10:00:00")),
            pref("b", true, None),
            pref("c", false, Some("2026-11-14 09:00:00")),
            pref("d", false, Some("2026-11-01 09:00:00")),
        ];
        let list = shortlist(&sites, &prefs);
        assert_eq!(list.pinned, ["b", "a"], "pinned sorts by name, case-insensitively");
        assert_eq!(list.recent, ["c", "d"], "recent is newest first and leaves out the pinned one");
        assert_eq!(list.opened_at.len(), 3, "every recorded open is carried, pinned or not");
        assert!(!list.opened_at.contains_key("e"));
    }

    #[test]
    fn recent_is_capped_and_a_pref_for_a_gone_site_is_ignored() {
        let sites: Vec<Site> = (0..9).map(|i| site(&format!("s{i}"), &format!("site {i}"))).collect();
        let mut prefs: Vec<SitePref> =
            (0..9).map(|i| pref(&format!("s{i}"), false, Some(&format!("2026-11-14 10:00:0{i}")))).collect();
        prefs.push(pref("gone", true, Some("2026-11-14 11:00:00")));
        let list = shortlist(&sites, &prefs);
        assert_eq!(list.recent.len(), RECENT_LIMIT);
        assert_eq!(list.recent[0], "s8", "the newest open leads");
        assert!(list.pinned.is_empty(), "a pin for a site that is gone was listed");
        assert!(!list.opened_at.contains_key("gone"));
    }

    #[test]
    fn two_opens_in_one_second_keep_a_stable_order() {
        let sites = vec![site("x", "beta"), site("y", "alpha")];
        let at = Some("2026-11-14 10:00:00");
        let one = shortlist(&sites, &[pref("x", false, at), pref("y", false, at)]);
        let two = shortlist(&sites, &[pref("y", false, at), pref("x", false, at)]);
        assert_eq!(one.recent, two.recent);
        assert_eq!(one.recent, ["y", "x"]);
    }

    #[test]
    fn a_scratch_site_is_refused_a_pin_and_says_where_keep_is() {
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut scratch = site("11111111-1111-4111-8111-111111111111", "probe");
        scratch.origin = SiteOrigin::Agent;
        scratch.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &scratch).unwrap();
        let mut own = site("22222222-2222-4222-8222-222222222222", "shop");
        own.origin = SiteOrigin::User;
        crate::state::store::insert_site(&conn, &own).unwrap();

        let err = set_pinned(&conn, &scratch.id, true).unwrap_err().to_string();
        assert!(err.contains("Keep"), "the refusal does not say what to do: {err}");
        assert!(read(&conn).unwrap().pinned.is_empty(), "the refused pin was written");
        // Unpinning one is not refused — it can only remove a promise.
        set_pinned(&conn, &scratch.id, false).unwrap();

        set_pinned(&conn, &own.id, true).unwrap();
        assert_eq!(read(&conn).unwrap().pinned, [own.id.clone()]);
        // Kept (the origin flips to the user's): now it may be pinned.
        crate::state::store::keep_site(&conn, &scratch.id).unwrap();
        set_pinned(&conn, &scratch.id, true).unwrap();
        assert_eq!(read(&conn).unwrap().pinned.len(), 2);

        assert!(set_pinned(&conn, "no-such-site", true).is_err());
    }

    #[test]
    fn the_tray_lists_pinned_then_recent_then_the_rest_in_their_own_order() {
        let list = SiteShortlist {
            pinned: vec!["p2".into(), "p1".into()],
            recent: vec!["r1".into()],
            opened_at: HashMap::new(),
        };
        let items = ["x", "r1", "p1", "y", "p2"].map(|id| (id.to_string(), id)).to_vec();
        assert_eq!(at_hand_first(items, &list), ["p2", "p1", "r1", "x", "y"]);
        // Nothing pinned or opened: the given order, untouched.
        let items = ["b", "a"].map(|id| (id.to_string(), id)).to_vec();
        assert_eq!(at_hand_first(items, &SiteShortlist::default()), ["b", "a"]);
    }
}
