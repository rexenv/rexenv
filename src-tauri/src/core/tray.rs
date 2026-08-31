//! The menu-bar menu, as data.
//!
//! The tray menu has rules in it — what the status line says, when Start all is
//! dead, whether a Sites submenu exists at all, which item carries a checkmark.
//! Rendering a menu does not. So the rules live HERE as a pure
//! `TrayModel -> MenuSpec` function with no Tauri types anywhere near it, and
//! `lib.rs` only walks the spec and builds the real menu from it. That split is
//! the whole reason this file exists: a menu built inline in a Tauri callback
//! is provable only by clicking it, and clicking is the one thing this project
//! cannot automate on the developer's machine.
//!
//! **This module NEVER computes whether something is running.** It is handed a
//! summary and formats it. The count and the "all/partial/stopped" verdict come
//! from `commands::system::summarize` — the same function the sidebar footer
//! uses — because a tray that decided for itself would be a SECOND answer to
//! "is the stack up", sitting a centimetre from the first one and free to
//! disagree with it. Same reason the status line names services exactly as the
//! Services screen names them (`Caddy`, not `Edge`): one vocabulary.
//!
//! See `docs/PLAN-menubar-tray.md` §3 for the three rules the tray inherits.

/// How many sites the **Sites ›** submenu lists before it stops. The menu is a
/// shortcut, not the Sites screen — a developer with 40 sites gets a menu the
/// height of the display and no way to find anything in it. **Open rexenv** is
/// the answer for the rest, and the submenu says so when it truncates.
pub const MAX_SITES: usize = 8;

/// A site as the menu needs it: enough to show a row and open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraySite {
    /// Full domain, e.g. `blog.rex` — also the action payload.
    pub domain: String,
}

/// Everything the menu is allowed to know. Assembled by the caller from the
/// SAME sources the UI reads; nothing in here is measured by this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    /// `"all" | "partial" | "stopped"` — straight from
    /// `commands::system::summarize`, not recomputed. An unknown value is
    /// treated as "partial" rather than panicking: a wrong-but-cautious label
    /// beats a menu bar with no menu in it.
    pub summary: &'static str,
    /// Running / total, on the footer's counting rule (optional engines count
    /// only while running, so a never-started Postgres cannot pin the menu at
    /// "partial" forever).
    pub running: u32,
    pub total: u32,
    /// Most-recent-first, already capped or not — `build` caps it.
    pub sites: Vec<TraySite>,
    /// The `mcp_enabled` setting, for the checkmark.
    pub mcp_on: bool,
    /// True when the services lock was busy and this model repeats the previous
    /// snapshot. The menu must NOT block the menu bar waiting for a lock
    /// (`PLAN-menubar-tray.md` §3 rule 2), so it shows the last thing it knew —
    /// and says so, because a stale number presented as current is the honest-UI
    /// failure this project keeps a ledger about.
    pub stale: bool,
}

/// Which screen a routing item lands on. A closed set rather than a string so a
/// typo is a compile error — the frontend's routes are not something this file
/// can check at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayRoute {
    Sites,
    Services,
    Databases,
    Mail,
    Tunnels,
}

impl TrayRoute {
    /// The frontend path this route opens. Matches `src/routes/` 1:1.
    pub fn path(self) -> &'static str {
        match self {
            TrayRoute::Sites => "/sites",
            TrayRoute::Services => "/services",
            TrayRoute::Databases => "/databases",
            TrayRoute::Mail => "/mail",
            TrayRoute::Tunnels => "/tunnels",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            TrayRoute::Sites => "sites",
            TrayRoute::Services => "services",
            TrayRoute::Databases => "databases",
            TrayRoute::Mail => "mail",
            TrayRoute::Tunnels => "tunnels",
        }
    }

    fn from_slug(s: &str) -> Option<Self> {
        Some(match s {
            "sites" => TrayRoute::Sites,
            "services" => TrayRoute::Services,
            "databases" => TrayRoute::Databases,
            "mail" => TrayRoute::Mail,
            "tunnels" => TrayRoute::Tunnels,
            _ => return None,
        })
    }
}

/// What clicking an item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    /// Show the window and bring the app to the front.
    Open,
    /// The one way out of the app (`RunEvent::ExitRequested` holds the gate).
    Quit,
    StartAll,
    StopAll,
    /// Open `https://<domain>` through the browser preference.
    OpenSite(String),
    /// Show the window on a route.
    Route(TrayRoute),
    ToggleMcp,
}

impl TrayAction {
    /// The string id Tauri hands back on click. **The click handler's only
    /// input**, so it round-trips: `parse(a.id()) == Some(a)` for every action,
    /// proven below. A site domain rides in the id because the alternative —
    /// an index into a list rebuilt every few seconds — opens whatever site
    /// slid into position 3 between the render and the click.
    pub fn id(&self) -> String {
        match self {
            TrayAction::Open => "tray:open".into(),
            TrayAction::Quit => "tray:quit".into(),
            TrayAction::StartAll => "tray:start-all".into(),
            TrayAction::StopAll => "tray:stop-all".into(),
            TrayAction::ToggleMcp => "tray:mcp".into(),
            TrayAction::OpenSite(d) => format!("tray:site:{d}"),
            TrayAction::Route(r) => format!("tray:route:{}", r.slug()),
        }
    }

    /// Parse an id back. `None` for anything unknown — an id this build does
    /// not recognise must do NOTHING, never fall through to a default action.
    pub fn parse(id: &str) -> Option<Self> {
        let rest = id.strip_prefix("tray:")?;
        Some(match rest {
            "open" => TrayAction::Open,
            "quit" => TrayAction::Quit,
            "start-all" => TrayAction::StartAll,
            "stop-all" => TrayAction::StopAll,
            "mcp" => TrayAction::ToggleMcp,
            other => {
                if let Some(domain) = other.strip_prefix("site:") {
                    // An empty domain would build `https://` and open nothing.
                    if domain.is_empty() {
                        return None;
                    }
                    TrayAction::OpenSite(domain.to_string())
                } else {
                    TrayAction::Route(TrayRoute::from_slug(other.strip_prefix("route:")?)?)
                }
            }
        })
    }
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// Text that is not clickable — the status line, and the "…and N more"
    /// hint. Rendered as a disabled item so it reads as information rather
    /// than as something broken.
    Label(String),
    Separator,
    Item {
        title: String,
        action: TrayAction,
        /// `false` renders greyed. Used instead of hiding, so the menu does not
        /// change shape between two states a user is comparing.
        enabled: bool,
        /// `Some` makes it a checkmark item.
        checked: Option<bool>,
    },
    Submenu {
        title: String,
        entries: Vec<MenuEntry>,
    },
}

/// The whole menu, top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub entries: Vec<MenuEntry>,
}

impl MenuSpec {
    /// Every action id in the spec, submenus included — used by the tests to
    /// hold the uniqueness rule, and by the click handler to reject an id that
    /// is not in the menu it just rendered.
    pub fn ids(&self) -> Vec<String> {
        fn walk(entries: &[MenuEntry], out: &mut Vec<String>) {
            for e in entries {
                match e {
                    MenuEntry::Item { action, .. } => out.push(action.id()),
                    MenuEntry::Submenu { entries, .. } => walk(entries, out),
                    MenuEntry::Label(_) | MenuEntry::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.entries, &mut out);
        out
    }
}

/// The status line: what the stack is doing, in one disabled row.
///
/// Deliberately the footer's three words plus the count, not a list of service
/// names: with seven PHP pools installed, a name list is a paragraph, and the
/// question a menu-bar glance asks is "is it up", not "which of the twelve".
fn status_line(m: &TrayModel) -> String {
    let base = match m.summary {
        "stopped" => "Stopped".to_string(),
        "all" => format!("All running · {} services", m.total),
        // Anything unrecognised lands here WITH its numbers, which stay true
        // whatever the verdict string says.
        _ => format!("Partial · {} of {} running", m.running, m.total),
    };
    // A stale snapshot is labelled, never silently shown as current.
    if m.stale {
        format!("{base} · updating…")
    } else {
        base
    }
}

/// The menu before the app has state: **Open rexenv** and **Quit rexenv**, and
/// nothing that would be a claim.
///
/// The status item is installed early — it is the app's only presence, and an
/// app that shows nothing in the menu bar while it opens databases and adopts
/// services looks like an app that failed to start. But everything else in the
/// menu describes state that does not exist yet, so this menu OMITS it rather
/// than showing zeros: "Stopped · 0 of 0" would be a measurement nobody took.
/// The first tick replaces this with the real menu.
pub fn bootstrap() -> MenuSpec {
    MenuSpec {
        entries: vec![
            MenuEntry::Item {
                title: "Open rexenv".into(),
                action: TrayAction::Open,
                enabled: true,
                checked: None,
            },
            MenuEntry::Item {
                title: "Quit rexenv".into(),
                action: TrayAction::Quit,
                enabled: true,
                checked: None,
            },
        ],
    }
}

/// Build the menu for a model. Pure: same model in, same menu out.
pub fn build(m: &TrayModel) -> MenuSpec {
    let mut entries = vec![MenuEntry::Label(status_line(m)), MenuEntry::Separator];

    // Start all is dead when everything already runs; Stop all when nothing
    // does. Greyed rather than hidden — the menu keeps its shape, so the eye
    // does not have to re-find items between two glances a second apart.
    entries.push(MenuEntry::Item {
        title: "Start all".into(),
        action: TrayAction::StartAll,
        enabled: m.summary != "all",
        checked: None,
    });
    entries.push(MenuEntry::Item {
        title: "Stop all".into(),
        action: TrayAction::StopAll,
        enabled: m.running > 0,
        checked: None,
    });
    entries.push(MenuEntry::Separator);

    // Sites. A submenu that exists but is empty is a dead end, so with no sites
    // the menu says so in a disabled row instead — and still offers the screen.
    if m.sites.is_empty() {
        entries.push(MenuEntry::Label("No sites yet".into()));
    } else {
        let mut sub: Vec<MenuEntry> = m
            .sites
            .iter()
            .take(MAX_SITES)
            .map(|s| MenuEntry::Item {
                title: s.domain.clone(),
                action: TrayAction::OpenSite(s.domain.clone()),
                enabled: true,
                checked: None,
            })
            .collect();
        if m.sites.len() > MAX_SITES {
            // Says what it is hiding. A truncated list that pretends to be the
            // whole list is how a developer concludes a site is gone.
            sub.push(MenuEntry::Separator);
            sub.push(MenuEntry::Label(format!(
                "…and {} more — open rexenv",
                m.sites.len() - MAX_SITES
            )));
        }
        entries.push(MenuEntry::Submenu { title: "Sites".into(), entries: sub });
    }

    for (title, route) in [
        ("All sites…", TrayRoute::Sites),
        ("Services", TrayRoute::Services),
        ("Databases", TrayRoute::Databases),
        ("Mail", TrayRoute::Mail),
        ("Tunnels", TrayRoute::Tunnels),
    ] {
        entries.push(MenuEntry::Item {
            title: title.into(),
            action: TrayAction::Route(route),
            enabled: true,
            checked: None,
        });
    }

    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item {
        title: "MCP server".into(),
        action: TrayAction::ToggleMcp,
        enabled: true,
        checked: Some(m.mcp_on),
    });
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Item {
        title: "Open rexenv".into(),
        action: TrayAction::Open,
        enabled: true,
        checked: None,
    });
    entries.push(MenuEntry::Item {
        title: "Quit rexenv".into(),
        action: TrayAction::Quit,
        enabled: true,
        checked: None,
    });

    MenuSpec { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TrayModel {
        TrayModel {
            summary: "all",
            running: 12,
            total: 12,
            sites: vec![TraySite { domain: "blog.rex".into() }],
            mcp_on: false,
            stale: false,
        }
    }

    fn item(spec: &MenuSpec, action: &TrayAction) -> (String, bool, Option<bool>) {
        fn find(entries: &[MenuEntry], want: &TrayAction) -> Option<(String, bool, Option<bool>)> {
            for e in entries {
                match e {
                    MenuEntry::Item { title, action, enabled, checked } if action == want => {
                        return Some((title.clone(), *enabled, *checked));
                    }
                    MenuEntry::Submenu { entries, .. } => {
                        if let Some(hit) = find(entries, want) {
                            return Some(hit);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        find(&spec.entries, action).unwrap_or_else(|| panic!("no item for {action:?}"))
    }

    fn labels(spec: &MenuSpec) -> Vec<String> {
        fn walk(entries: &[MenuEntry], out: &mut Vec<String>) {
            for e in entries {
                match e {
                    MenuEntry::Label(t) => out.push(t.clone()),
                    MenuEntry::Submenu { entries, .. } => walk(entries, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&spec.entries, &mut out);
        out
    }

    /// The click handler's ONLY input is the id string, so an action that does
    /// not survive the round trip is an item that fires the wrong thing — or
    /// nothing. Covers the two carrying a payload (site, route), which are the
    /// ones a naive `format!` breaks.
    #[test]
    fn every_action_survives_the_round_trip_through_its_id() {
        let all = [
            TrayAction::Open,
            TrayAction::Quit,
            TrayAction::StartAll,
            TrayAction::StopAll,
            TrayAction::ToggleMcp,
            TrayAction::OpenSite("blog.rex".into()),
            TrayAction::OpenSite("my.long.sub.domain.test".into()),
            TrayAction::Route(TrayRoute::Sites),
            TrayAction::Route(TrayRoute::Services),
            TrayAction::Route(TrayRoute::Databases),
            TrayAction::Route(TrayRoute::Mail),
            TrayAction::Route(TrayRoute::Tunnels),
        ];
        for a in all {
            assert_eq!(TrayAction::parse(&a.id()).as_ref(), Some(&a), "round trip: {a:?}");
        }
    }

    /// An id this build does not know must do NOTHING. The dangerous shape is
    /// not the garbage string — it is the WELL-FORMED one with an empty or
    /// unknown payload, which a lenient parser turns into "open https://" or a
    /// route that does not exist.
    #[test]
    fn an_unknown_or_empty_id_parses_to_nothing_rather_than_a_default() {
        for bad in [
            "",
            "open",              // missing the namespace
            "tray:",             // namespace only
            "tray:nope",
            "tray:site:",        // empty domain → https:// → opens nothing
            "tray:route:",       // empty route
            "tray:route:settings", // a real screen, but not one the tray offers
            "TRAY:OPEN",         // ids are not case-folded
        ] {
            assert_eq!(TrayAction::parse(bad), None, "must not parse: {bad:?}");
        }
    }

    /// Two items sharing an id means one of them fires the other's action —
    /// and with the site domain in the id, the shape that would collide is two
    /// sites with the same domain, which the DB permits far more easily than it
    /// sounds (a soft-deleted row, an import).
    #[test]
    fn no_two_items_in_a_menu_share_an_id() {
        let mut m = model();
        m.sites = (0..MAX_SITES + 3).map(|i| TraySite { domain: format!("s{i}.rex") }).collect();
        let ids = build(&m).ids();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate ids in {ids:?}");
    }

    /// The status line is the one thing a glance reads, so each state says a
    /// different thing — and the numbers come from the model, never from a
    /// count taken here.
    #[test]
    fn the_status_line_says_what_the_footer_says() {
        let mut m = model();
        assert_eq!(status_line(&m), "All running · 12 services");

        m.summary = "partial";
        m.running = 4;
        assert_eq!(status_line(&m), "Partial · 4 of 12 running");

        m.summary = "stopped";
        m.running = 0;
        assert_eq!(status_line(&m), "Stopped");

        // An unrecognised verdict must not panic and must not claim "all":
        // it falls back to the numbers, which stay true whatever the string is.
        m.summary = "something-new";
        m.running = 7;
        assert_eq!(status_line(&m), "Partial · 7 of 12 running");
    }

    /// Rule 2 of the plan: a busy services lock never blocks the menu bar, so
    /// the menu repeats the last snapshot — and must SAY it is repeating it.
    /// A stale count shown as current is the honest-UI failure this project
    /// keeps a ledger about.
    #[test]
    fn a_stale_snapshot_is_labelled_and_still_shows_its_numbers() {
        let mut m = model();
        m.stale = true;
        let line = status_line(&m);
        assert!(line.contains("updating…"), "stale line must say so: {line}");
        assert!(line.contains("All running"), "stale line still shows what it knew: {line}");
    }

    /// Start all with everything up, and Stop all with everything down, are
    /// buttons that can only be wrong. Greyed rather than missing, so the menu
    /// keeps its shape between two glances.
    #[test]
    fn start_and_stop_are_disabled_exactly_where_they_would_be_meaningless() {
        let mut m = model(); // "all"
        let spec = build(&m);
        assert!(!item(&spec, &TrayAction::StartAll).1, "Start all is pointless when all run");
        assert!(item(&spec, &TrayAction::StopAll).1);

        m.summary = "stopped";
        m.running = 0;
        let spec = build(&m);
        assert!(item(&spec, &TrayAction::StartAll).1);
        assert!(!item(&spec, &TrayAction::StopAll).1, "Stop all is pointless when none run");

        m.summary = "partial";
        m.running = 4;
        let spec = build(&m);
        assert!(item(&spec, &TrayAction::StartAll).1, "partial must offer both");
        assert!(item(&spec, &TrayAction::StopAll).1);
    }

    /// A submenu that opens onto nothing is a dead end. With no sites the menu
    /// says so in a disabled row — and the routes stay, so the answer to "I
    /// have no sites" is still one click away.
    #[test]
    fn an_empty_site_list_becomes_a_sentence_not_an_empty_submenu() {
        let mut m = model();
        m.sites.clear();
        let spec = build(&m);
        assert!(
            !spec.entries.iter().any(|e| matches!(e, MenuEntry::Submenu { .. })),
            "no submenu when there is nothing to put in it"
        );
        assert!(labels(&spec).iter().any(|l| l == "No sites yet"));
        // The escape hatch is still there.
        item(&spec, &TrayAction::Route(TrayRoute::Sites));
    }

    /// The cap exists so a developer with forty sites gets a menu, not a wall.
    /// The truncation must ANNOUNCE itself: a short list that looks complete is
    /// how someone concludes a site has disappeared.
    #[test]
    fn a_long_site_list_is_capped_and_says_how_many_it_hid() {
        let mut m = model();
        m.sites = (0..MAX_SITES + 5).map(|i| TraySite { domain: format!("s{i}.rex") }).collect();
        let spec = build(&m);
        let site_items = spec.ids().iter().filter(|i| i.starts_with("tray:site:")).count();
        assert_eq!(site_items, MAX_SITES);
        assert!(
            labels(&spec).iter().any(|l| l.contains("and 5 more")),
            "the menu must say what it hid: {:?}",
            labels(&spec)
        );
        // The FIRST sites survive — the caller hands them most-recent-first, so
        // taking from the front is what makes the cap mean "recent".
        assert!(spec.ids().contains(&"tray:site:s0.rex".to_string()));
        assert!(!spec.ids().contains(&format!("tray:site:s{}.rex", MAX_SITES + 4)));
    }

    /// The checkmark is bound to the setting, both ways — a toggle that only
    /// ever renders one state is the bug that makes people click twice.
    #[test]
    fn the_mcp_item_carries_the_setting_as_a_checkmark() {
        let mut m = model();
        assert_eq!(item(&build(&m), &TrayAction::ToggleMcp).2, Some(false));
        m.mcp_on = true;
        assert_eq!(item(&build(&m), &TrayAction::ToggleMcp).2, Some(true));
    }

    /// The startup menu must not MEASURE anything. It exists because the status
    /// item is installed before the app has state (the panic that taught this:
    /// `state() called before manage()`), and the tempting fix — a model full of
    /// zeros — would put "Stopped · 0 of 0 running" in the menu bar of an app
    /// whose services are, in fact, running and being adopted at that moment.
    #[test]
    fn the_startup_menu_offers_only_what_it_can_honestly_offer() {
        let spec = bootstrap();
        assert_eq!(spec.ids(), vec!["tray:open".to_string(), "tray:quit".to_string()]);
        // No labels at all: every label in this menu would be a claim about
        // state nobody has read yet.
        assert!(
            !spec.entries.iter().any(|e| matches!(e, MenuEntry::Label(_))),
            "the startup menu states nothing: {:?}",
            spec.entries
        );
    }

    /// Open and Quit are the two items Phase A shipped and the two the menu can
    /// never lose: Quit is the ONLY way out of an app with no dock icon, and
    /// Open the only way back to a hidden window.
    #[test]
    fn open_and_quit_are_present_and_always_enabled() {
        let mut m = model();
        m.sites.clear();
        m.summary = "stopped";
        m.running = 0;
        let spec = build(&m);
        assert!(item(&spec, &TrayAction::Open).1);
        assert!(item(&spec, &TrayAction::Quit).1);
    }

    /// Every route the menu offers maps to a real `src/routes/` path. Cheap
    /// guard against a slug typo that renders a menu item landing nowhere.
    #[test]
    fn every_route_item_has_a_path_the_frontend_serves() {
        let spec = build(&model());
        let routes: Vec<String> = spec
            .ids()
            .into_iter()
            .filter(|i| i.starts_with("tray:route:"))
            .collect();
        assert_eq!(routes.len(), 5, "all five routes are offered: {routes:?}");
        for id in routes {
            let Some(TrayAction::Route(r)) = TrayAction::parse(&id) else {
                panic!("route id did not parse: {id}");
            };
            assert!(r.path().starts_with('/'), "{id} → {}", r.path());
        }
    }
}
