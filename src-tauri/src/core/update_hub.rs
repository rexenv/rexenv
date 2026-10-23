//! core::update_hub — ONE inventory of everything newer than what this install runs.
//!
//! `docs/PLAN-unified-updates.md` T1. Before this, four families answered "is there
//! something newer?" on four screens with four vocabularies: the app on its About card,
//! PHP in Settings → PHP, Adminer and phpMyAdmin on the Databases page — and only the
//! app's answer ever reached the chrome (pills, toast, tray). The hub does not add a
//! fifth answer; it reads the four existing ones and lines them up in one shape, so the
//! pill, the toast, the tray item, `rex status`, the MCP read and Settings → Updates all
//! render the same list.
//!
//! The rules it keeps (plan §2):
//! - **Computed live, never stored.** Every call re-reads the facts; a skip is a VERSION
//!   compared against the live offer, so skipping 8.3.28 cannot hide 8.3.29. The app's skip
//!   is the key `core::app_update` already owns, read through [`UpdateKind::App`].
//! - **It does not install.** Each offer still installs through its family's own command
//!   (`php_update_apply`, `adminer_update_apply`, `phpmyadmin_update_apply`,
//!   `app_update_apply`); the hub only says what is there and what pressing Install costs.
//! - **Sentences live here**, beside the rule, and the page renders what it is sent.
//! - **The app is LAST.** The order of [`UpdateInventory::offers`] IS the Update-all order
//!   (T5): pool swaps and console restages report their measured effect before the one
//!   step that ends in a restart prompt.
//!
//! `inventory` is a pure function over facts the command layer gathers, so every rule is
//! an L0 test without a platform, a database of PHP rows or a signed manifest.

use crate::core::app_update::AppUpdateState;
use crate::error::Result;
use crate::state::models::PhpVersionView;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Which thing the offer is about. `kind` is the wire tag; a PHP offer carries its minor
/// because each minor is its own pool and its own install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateKind {
    App,
    Php { minor: String },
    Adminer,
    PhpMyAdmin,
}

impl UpdateKind {
    /// The settings key that holds this kind's skipped version. The app's is the key
    /// `core::app_update` has always used, read and written through its own functions —
    /// an alias, not a migration, so a skip recorded by 0.9.0 still counts.
    fn skip_key(&self) -> Option<String> {
        match self {
            UpdateKind::App => None,
            UpdateKind::Php { minor } => Some(format!("update_skipped:php:{minor}")),
            UpdateKind::Adminer => Some("update_skipped:adminer".into()),
            UpdateKind::PhpMyAdmin => Some("update_skipped:phpmyadmin".into()),
        }
    }

    /// What the row is called, in the one vocabulary every surface uses.
    pub fn label(&self) -> String {
        match self {
            UpdateKind::App => "rexenv".into(),
            UpdateKind::Php { minor } => format!("PHP {minor}"),
            UpdateKind::Adminer => "Adminer".into(),
            UpdateKind::PhpMyAdmin => "phpMyAdmin".into(),
        }
    }
}

/// One thing that is newer than what runs here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOffer {
    pub kind: UpdateKind,
    /// "PHP 8.3", "Adminer", "phpMyAdmin", "rexenv".
    pub label: String,
    /// What runs now — the EFFECTIVE version (the selection floored by the pin), never the pin.
    pub installed: String,
    /// What the signed document offers.
    pub offered: String,
    pub size_bytes: Option<u64>,
    /// What the user must know before pressing Install, when there is something to know —
    /// the app's consent sentence; PHP's "this build cannot reach PostgreSQL" cost. `None`
    /// when the install costs nothing the user has to be told.
    pub consent: Option<String>,
    /// What pressing Install DOES, as the measured-effect vocabulary the toasts already use.
    pub effect: String,
    /// This exact version was set aside. Listed anyway, so the row can offer Undo; the
    /// pill and the toast count only offers that are not.
    pub skipped: bool,
}

/// Everything Settings → Updates, the pills, the toast and `rex status` render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInventory {
    /// In Update-all order: PHP minors (ascending), Adminer, phpMyAdmin, the app LAST.
    pub offers: Vec<UpdateOffer>,
    /// An app version already swapped onto disk that this process is not running — the
    /// pill's "Restart to finish" wins over every offer while this is set.
    pub restart_pending: Option<String>,
    /// When a signed document was last ACCEPTED — either one; the one poller fetches both
    /// (`store_checked`). Falls back to the app's own record for installs that upgraded
    /// before the hub existed. Never advanced by a failure.
    pub checked_at: Option<String>,
    /// The auto-check setting, as it is right now (one setting for every kind).
    pub auto_check: bool,
    /// What went wrong with the LAST interactive check, when something did — "couldn't
    /// reach" and "refused what it published" are different facts and the page says which.
    /// `None` from [`inventory`]; only `updates_check_now` sets it, for that one answer.
    pub check_problem: Option<String>,
}

impl UpdateInventory {
    /// What the pill and the toast count: offers the user has not set aside.
    pub fn pending(&self) -> Vec<&UpdateOffer> {
        self.offers.iter().filter(|o| !o.skipped).collect()
    }
}

/// The facts the command layer gathers; the hub judges them.
pub struct Facts<'a> {
    pub app: &'a AppUpdateState,
    /// Whether the app is a Homebrew install — the app's consent sentence differs.
    pub homebrew: bool,
    pub php: &'a [PhpVersionView],
    /// `(effective, updatable)` for Adminer, as `AdminerStatus` carries them.
    pub adminer: (&'a str, Option<&'a str>),
    pub phpmyadmin: (&'a str, Option<&'a str>),
}

/// Line up every live offer in one shape. Pure: reads the skip keys, writes nothing.
pub fn inventory(conn: &Connection, facts: &Facts<'_>) -> UpdateInventory {
    let mut offers = Vec::new();
    for row in facts.php.iter().filter(|r| r.installed && r.unavailable_reason.is_none()) {
        if let Some(target) = row.updatable.as_deref() {
            let kind = UpdateKind::Php { minor: row.minor.clone() };
            offers.push(UpdateOffer {
                label: kind.label(),
                skipped: skipped_version(conn, &kind).as_deref() == Some(target),
                installed: row.patch.clone(),
                offered: target.to_string(),
                size_bytes: None,
                consent: row.update_cost.clone(),
                effect: php_effect(&row.minor, &row.patch, target),
                kind,
            });
        }
    }
    for (kind, (have, updatable)) in [
        (UpdateKind::Adminer, facts.adminer),
        (UpdateKind::PhpMyAdmin, facts.phpmyadmin),
    ] {
        if let Some(target) = updatable {
            offers.push(UpdateOffer {
                label: kind.label(),
                skipped: skipped_version(conn, &kind).as_deref() == Some(target),
                installed: have.to_string(),
                offered: target.to_string(),
                size_bytes: None,
                consent: None,
                effect: console_effect(&kind, target),
                kind,
            });
        }
    }
    // The app LAST (plan §3.1): its install ends in a restart prompt.
    if let Some(offer) = &facts.app.offered {
        offers.push(UpdateOffer {
            kind: UpdateKind::App,
            label: UpdateKind::App.label(),
            installed: facts.app.running.clone(),
            offered: offer.version.clone(),
            size_bytes: Some(offer.size_bytes),
            consent: Some(crate::core::app_update::consent_sentence(offer, facts.homebrew)),
            effect: app_effect(&offer.version),
            skipped: false,
        });
    } else if let Some(v) = &facts.app.skipped {
        // Set aside, and still listed so the row can offer Undo. `core::app_update::state`
        // withdraws the offer for a skipped version, so the details are the skip's.
        offers.push(UpdateOffer {
            kind: UpdateKind::App,
            label: UpdateKind::App.label(),
            installed: facts.app.running.clone(),
            offered: v.clone(),
            size_bytes: None,
            consent: None,
            effect: app_effect(v),
            skipped: true,
        });
    }
    UpdateInventory {
        offers,
        restart_pending: facts.app.installed_pending.clone(),
        checked_at: crate::state::store::get_setting(conn, CHECKED_KEY)
            .ok()
            .flatten()
            .or_else(|| facts.app.checked_at.clone()),
        auto_check: facts.app.auto_check,
        check_problem: None,
    }
}

/// When either signed document was last accepted (`db_now` format).
const CHECKED_KEY: &str = "updates_checked_at";

/// Record that a document was accepted just now. Written on the success path ONLY, so
/// `checked N ago` can never age a failure into a success (the app's `store_check` rule).
pub fn store_checked(conn: &Connection) -> Result<String> {
    let now = crate::state::store::db_now(conn)?;
    crate::state::store::set_setting(conn, CHECKED_KEY, &now)?;
    Ok(now)
}

/// The sentence for an interactive check whose fetched document was REFUSED — a stale
/// serial, a bad signature — as opposed to one that could not be fetched.
pub fn refusal_sentence(what: &str, reason: &str) -> String {
    format!(
        "The update server answered, but rexenv refused the {what} it published: {reason}. \
         Nothing here changes until a document this copy accepts is published."
    )
}

/// The sentence for a check that could not reach the server at all.
pub fn unreachable_sentence(reason: &str) -> String {
    format!("Couldn't reach the update server just now ({reason}). The last answer stands.")
}

/// The version set aside for `kind`, if any — the app's through its own key.
pub fn skipped_version(conn: &Connection, kind: &UpdateKind) -> Option<String> {
    match kind.skip_key() {
        None => crate::core::app_update::skipped_version(conn),
        Some(key) => crate::state::store::get_setting(conn, &key).ok().flatten(),
    }
}

/// Set `kind`'s offered `version` aside (`Some`) or take the skip back (`None`). Stored as a
/// VERSION, compared live by [`inventory`] — a later release offers itself again.
pub fn set_skipped(conn: &Connection, kind: &UpdateKind, version: Option<&str>) -> Result<()> {
    match kind.skip_key() {
        None => crate::core::app_update::set_skipped(conn, version),
        Some(key) => match version {
            Some(v) => crate::state::store::set_setting(conn, &key, v),
            None => crate::state::store::delete_setting(conn, &key),
        },
    }
}

/// The ONE wording for what a PHP install does — the Settings row's button title and the
/// `php_update_apply` toast already say the halves of it; the page says it once, up front.
pub fn php_effect(minor: &str, current: &str, target: &str) -> String {
    format!(
        "Downloads PHP {target} and restarts the {minor} pool onto it — put back on {current} if \
         the pool does not come up. Sites on {minor} answer again within a second."
    )
}

pub fn console_effect(kind: &UpdateKind, target: &str) -> String {
    match kind {
        UpdateKind::Adminer => format!(
            "Downloads Adminer {target}, checks it still binds to rexenv's login gate and frame \
             protections, and restages the console onto it. Nothing else restarts."
        ),
        UpdateKind::PhpMyAdmin => format!(
            "Downloads phpMyAdmin {target}, verifies the tree, and restages the console onto it. \
             Nothing else restarts."
        ),
        _ => unreachable!("console_effect is for the two consoles"),
    }
}

pub fn app_effect(version: &str) -> String {
    format!(
        "Downloads rexenv {version}, verifies it, replaces the running copy, then asks before \
         restarting. Your sites, databases and DNS keep running either way."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::app_update::Offer;

    fn conn() -> Connection {
        crate::state::db::open_in_memory().unwrap()
    }

    fn app(offered: Option<&str>, skipped: Option<&str>) -> AppUpdateState {
        AppUpdateState {
            running: "0.9.0".into(),
            enabled: true,
            auto_check: true,
            offered: offered.map(|v| Offer {
                version: v.into(),
                url: "https://github.com/rexenv/rexenv/releases/download/v0.9.1/rexenv.app.tar.gz".into(),
                sha256: "00".repeat(32),
                size_bytes: 31_000_000,
                notes: String::new(),
                published_at: "2026-10-23".into(),
            }),
            no_offer_reason: None,
            checked_at: Some("2026-10-23 10:00:00".into()),
            skipped: skipped.map(String::from),
            installed_pending: None,
            restart_declined: false,
            check_refusal: None,
        }
    }

    fn php(minor: &str, patch: &str, updatable: Option<&str>) -> PhpVersionView {
        PhpVersionView {
            minor: minor.into(),
            unavailable_reason: None,
            patch: patch.into(),
            serving: None,
            upstream: None,
            updatable: updatable.map(String::from),
            update_cost: None,
            upstream_checked_at: None,
            fpm_port: 9083,
            installed: true,
            is_default: false,
            xdebug_supported: true,
            xdebug_unavailable_reason: None,
            xdebug_version: None,
            eol_since: None,
            postgres_supported: true,
        }
    }

    /// Every kind in ONE list, the app LAST, nothing invented for a family with nothing newer.
    #[test]
    fn the_inventory_lines_up_every_live_offer_with_the_app_last() {
        let c = conn();
        let a = app(Some("0.9.1"), None);
        let rows = [php("8.2", "8.2.35", None), php("8.3", "8.3.27", Some("8.3.28"))];
        let inv = inventory(&c, &Facts { app: &a, homebrew: false, php: &rows, adminer: ("6.1.1", Some("6.1.2")), phpmyadmin: ("5.2.3", None) });
        let kinds: Vec<&UpdateKind> = inv.offers.iter().map(|o| &o.kind).collect();
        assert_eq!(kinds, vec![&UpdateKind::Php { minor: "8.3".into() }, &UpdateKind::Adminer, &UpdateKind::App]);
        assert_eq!(inv.offers[0].installed, "8.3.27");
        assert_eq!(inv.offers[0].offered, "8.3.28");
        assert_eq!(inv.offers[0].label, "PHP 8.3");
        assert!(inv.offers[2].consent.as_deref().unwrap().contains("0.9.1"), "the app row carries Rust's consent sentence");
        assert_eq!(inv.offers[2].size_bytes, Some(31_000_000));
        assert_eq!(inv.pending().len(), 3);
    }

    /// A skip is a VERSION per kind: it hides exactly that offer from the count, keeps the row
    /// (for Undo), leaves every other kind alone, and a newer version offers itself again.
    #[test]
    fn a_skip_is_one_version_of_one_kind_compared_live() {
        let c = conn();
        let a = app(Some("0.9.1"), None);
        let rows = [php("8.3", "8.3.27", Some("8.3.28"))];
        let php83 = UpdateKind::Php { minor: "8.3".into() };
        set_skipped(&c, &php83, Some("8.3.28")).unwrap();
        let facts = Facts { app: &a, homebrew: false, php: &rows, adminer: ("6.1.1", Some("6.1.2")), phpmyadmin: ("5.2.3", None) };
        let inv = inventory(&c, &facts);
        assert_eq!(inv.offers.len(), 3, "a skipped offer stays LISTED so the row can offer Undo");
        assert!(inv.offers[0].skipped);
        assert!(!inv.offers[1].skipped, "skipping PHP 8.3 must not touch Adminer");
        assert_eq!(inv.pending().len(), 2);
        // The next patch is not the skipped one: offered again.
        let rows = [php("8.3", "8.3.27", Some("8.3.29"))];
        let inv = inventory(&c, &Facts { php: &rows, ..facts });
        assert!(!inv.offers[0].skipped, "a stored flag would hide 8.3.29 too; a version does not");
        // Undo.
        set_skipped(&c, &php83, None).unwrap();
        assert_eq!(skipped_version(&c, &php83), None);
    }

    /// The app's skip is `core::app_update`'s own key — a skip 0.9.0 recorded is still a skip.
    #[test]
    fn the_app_skip_is_the_key_app_update_already_owns() {
        let c = conn();
        set_skipped(&c, &UpdateKind::App, Some("0.9.1")).unwrap();
        assert_eq!(crate::core::app_update::skipped_version(&c).as_deref(), Some("0.9.1"));
        // `state()` withdraws a skipped offer; the hub still lists it, set aside.
        let a = app(None, Some("0.9.1"));
        let inv = inventory(&c, &Facts { app: &a, homebrew: false, php: &[], adminer: ("6.1.1", None), phpmyadmin: ("5.2.3", None) });
        assert_eq!(inv.offers.len(), 1);
        assert!(inv.offers[0].skipped);
        assert_eq!(inv.offers[0].offered, "0.9.1");
        assert!(inv.pending().is_empty(), "a set-aside app release is not counted on the pill");
    }

    /// `checked_at` is the hub's own record once either document has been accepted, and the
    /// app's record before that — never a failure's timestamp, because only success writes it.
    #[test]
    fn checked_at_is_the_one_record_for_both_documents_and_falls_back_to_the_apps() {
        let c = conn();
        let a = app(None, None);
        let facts = Facts { app: &a, homebrew: false, php: &[], adminer: ("6.1.1", None), phpmyadmin: ("5.2.3", None) };
        assert_eq!(inventory(&c, &facts).checked_at.as_deref(), Some("2026-10-23 10:00:00"), "before the hub ever recorded a check, the app's record is the answer");
        let now = store_checked(&c).unwrap();
        assert_eq!(inventory(&c, &facts).checked_at, Some(now), "once recorded, the hub's own stamp wins");
        assert!(inventory(&c, &facts).check_problem.is_none(), "a read never carries a problem; only an interactive check does");
    }

    /// A minor this OS has no build of, or one not installed, offers nothing even if the
    /// catalog has a row — the Settings row disables Install for it, and so must the hub.
    #[test]
    fn an_unavailable_or_uninstalled_minor_is_not_an_offer() {
        let c = conn();
        let a = app(None, None);
        let mut gone = php("7.4", "7.4.33", Some("7.4.34"));
        gone.unavailable_reason = Some("no build for this OS".into());
        let mut off = php("8.1", "8.1.34", Some("8.1.35"));
        off.installed = false;
        let inv = inventory(&c, &Facts { app: &a, homebrew: false, php: &[gone, off], adminer: ("6.1.1", None), phpmyadmin: ("5.2.3", None) });
        assert!(inv.offers.is_empty(), "{:?}", inv.offers);
    }
}
