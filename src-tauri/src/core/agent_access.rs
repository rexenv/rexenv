//! core::agent_access — the ONE dial that says how far an agent may go on the
//! user's own sites (D15 in `PLAN-mcp-parity.md`).
//!
//! Replaces the per-site × per-scope × per-client grants of P1 for everything
//! except publishing a site (`share`, which keeps a person's click — see
//! [`crate::core::agent_grants`]). Three levels, ordered:
//!
//! - **Read** — the default whenever MCP is on. Every read tool, every site.
//!   Reads need no door: the socket is `0600` to this user, and what a read
//!   returns is what the app's own screens show.
//! - **Changes** — every `manage` action (PHP/server/xdebug, plugin and theme
//!   activation, options, restart, dry runs, blueprints) and the `system` ops,
//!   for which the macOS password dialog IS the consent — rexenv no longer
//!   asks twice.
//! - **Full** — `destroy` and `run`: delete, reset, a live search-replace, a
//!   database import, raw `wp`/artisan, `composer_link`, migration writes.
//!   `run` sits here and not in Changes on purpose: what it hands over is not
//!   data but a shell as the user over their real projects.
//!
//! A level above Read carries a **duration**: this session (gone at the next
//! launch, like the old session grants), 7 days (an expiry stamp), or always
//! (a durable setting). Global — never per site, never per client: the threat
//! model is a misbehaving model, not a stranger, and a per-client row was the
//! knob nobody wanted. Every call still lands in the activity feed.
//!
//! **Why not "no sensitive data, so allow everything":** accepted for Read,
//! rejected for Run. The owner's brief said a local dev tool holds nothing
//! sensitive; a `run` grant is not about data.

use crate::core::agent_grants::Scope;
use crate::error::{Error, Result};
use crate::state::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub const LEVEL_KEY: &str = "agent_access_level";
pub const MODE_KEY: &str = "agent_access_mode";
pub const EXPIRES_KEY: &str = "agent_access_expires_at";

/// The dial's label in Settings — the ONE constant the refusal text names.
pub const DIAL_LABEL: &str = "Agent access";

/// How long a level above Read lasts.
pub const DAYS: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    Read,
    Changes,
    Full,
}

impl AccessLevel {
    pub fn as_db(self) -> &'static str {
        match self {
            AccessLevel::Read => "read",
            AccessLevel::Changes => "changes",
            AccessLevel::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(AccessLevel::Read),
            "changes" => Some(AccessLevel::Changes),
            "full" => Some(AccessLevel::Full),
            _ => None,
        }
    }

    /// The level a scope needs. The tools' per-action `Scope` ranking is the
    /// input; this is the whole mapping, and it is a `match` so a sixth scope
    /// cannot land in a level by default.
    pub fn needed_for(scope: Scope) -> Self {
        match scope {
            Scope::Read => AccessLevel::Read,
            Scope::Manage | Scope::System => AccessLevel::Changes,
            Scope::Destroy | Scope::Run => AccessLevel::Full,
        }
    }

    /// What the level hands over, in the card's own words — served from Rust
    /// so the prompt, the refusal and the card cannot drift (#404).
    pub fn what_it_allows(self) -> &'static str {
        match self {
            AccessLevel::Read => "look at any of your sites — status, content, users, logs, every site's mail (password-reset links included) and their databases, read-only (password hashes and API keys are in there) — and create disposable sites of its own; it cannot change anything you made",
            AccessLevel::Changes => "change how any of your sites is served and what is installed in it — PHP version, web server, Xdebug, plugins and themes on or off, options, restarts, dry runs, blueprints — and start or stop rexenv's stack (macOS still asks for your password)",
            AccessLevel::Full => "do everything Changes allows, and also delete or reset a site and its database, run a live search-replace or a database import, publish a site to the internet for up to an hour (anyone with the link reaches it until rexenv stops the share), and run commands and code of its choosing in any of your sites, as you",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Session,
    Days,
    Always,
}

impl Mode {
    pub fn as_db(self) -> &'static str {
        match self {
            Mode::Session => "session",
            Mode::Days => "days",
            Mode::Always => "always",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "session" => Some(Mode::Session),
            "days" => Some(Mode::Days),
            "always" => Some(Mode::Always),
            _ => None,
        }
    }
}

/// The dial as it stands RIGHT NOW: an expired 7-day setting reads as Read
/// (the row is left for the card to show as expired, never rewritten here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccess {
    pub level: AccessLevel,
    /// `None` at Read — a duration is a property of a level above it.
    pub mode: Option<Mode>,
    /// The stamp a 7-day setting expires at (UTC, the db's clock).
    pub expires_at: Option<String>,
    /// A 7-day setting whose stamp has passed: the card says so instead of
    /// silently showing Read.
    pub expired: bool,
    pub label: &'static str,
    pub allows: &'static str,
    /// Every level with its sentence, so the card renders the choice from
    /// Rust's words and holds no copy of its own.
    pub levels: Vec<LevelCopy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelCopy {
    pub level: AccessLevel,
    pub allows: &'static str,
}

pub const LEVELS: [AccessLevel; 3] = [AccessLevel::Read, AccessLevel::Changes, AccessLevel::Full];

pub fn current(conn: &Connection) -> Result<AgentAccess> {
    let level = store::get_setting(conn, LEVEL_KEY)?.as_deref().and_then(AccessLevel::parse).unwrap_or(AccessLevel::Read);
    let mode = store::get_setting(conn, MODE_KEY)?.as_deref().and_then(Mode::parse);
    let expires_at = store::get_setting(conn, EXPIRES_KEY)?;
    if level == AccessLevel::Read {
        return Ok(view(AccessLevel::Read, None, None, false));
    }
    if mode == Some(Mode::Days) {
        let now = store::db_now(conn)?;
        if let Some(until) = &expires_at {
            if *until <= now {
                return Ok(view(AccessLevel::Read, mode, expires_at, true));
            }
        }
    }
    Ok(view(level, mode, expires_at, false))
}

fn view(level: AccessLevel, mode: Option<Mode>, expires_at: Option<String>, expired: bool) -> AgentAccess {
    AgentAccess {
        level,
        mode,
        expires_at,
        expired,
        label: DIAL_LABEL,
        allows: level.what_it_allows(),
        levels: LEVELS.iter().map(|l| LevelCopy { level: *l, allows: l.what_it_allows() }).collect(),
    }
}

/// Set the dial. Read clears the duration; a level above Read needs one.
pub fn set(conn: &Connection, level: AccessLevel, mode: Option<Mode>) -> Result<AgentAccess> {
    match (level, mode) {
        (AccessLevel::Read, _) => {
            store::set_setting(conn, LEVEL_KEY, "read")?;
            store::set_setting(conn, MODE_KEY, "")?;
            store::set_setting(conn, EXPIRES_KEY, "")?;
        }
        (_, None) => {
            return Err(Error::Other(format!(
                "`{DIAL_LABEL}` above Read needs a duration — this session, {DAYS} days, or always."
            )));
        }
        (_, Some(m)) => {
            store::set_setting(conn, LEVEL_KEY, level.as_db())?;
            store::set_setting(conn, MODE_KEY, m.as_db())?;
            let until = match m {
                Mode::Days => conn.query_row(
                    "SELECT strftime('%Y-%m-%d %H:%M:%S', 'now', ?1)",
                    [format!("+{DAYS} days")],
                    |r| r.get::<_, String>(0),
                )?,
                _ => String::new(),
            };
            store::set_setting(conn, EXPIRES_KEY, &until)?;
        }
    }
    current(conn)
}

/// What launch does: a level set "for this session" dies with the session,
/// exactly as the old session grants did. Returns whether anything ended.
pub fn end_session_at_launch(conn: &Connection) -> Result<bool> {
    let mode = store::get_setting(conn, MODE_KEY)?.as_deref().and_then(Mode::parse);
    let level = store::get_setting(conn, LEVEL_KEY)?.as_deref().and_then(AccessLevel::parse).unwrap_or(AccessLevel::Read);
    if mode == Some(Mode::Session) && level != AccessLevel::Read {
        set(conn, AccessLevel::Read, None)?;
        return Ok(true);
    }
    Ok(false)
}

/// The gate: does the dial, right now, allow `scope`?
pub fn allows(conn: &Connection, scope: Scope) -> Result<bool> {
    Ok(current(conn)?.level >= AccessLevel::needed_for(scope))
}

/// The refusal an agent reads when the dial is below what a call needs — names
/// the dial, the level, and where it is set, never a site the agent could not
/// otherwise see.
pub fn refusal(what: &str, scope: Scope, now: &AgentAccess) -> String {
    let needed = AccessLevel::needed_for(scope);
    let standing = if now.expired {
        format!("the {DAYS}-day setting has expired, so it is back at Read")
    } else {
        format!("it is at {}", cap(now.level.as_db()))
    };
    format!(
        "{what} needs `{}` permission — `{DIAL_LABEL}` at {} or above in rexenv — and {standing}. The person you're \
         working with can change it under Settings → \"AI agents (MCP)\" → \"{DIAL_LABEL}\" — for \
         this session, for {DAYS} days, or always. That is their decision, not something an agent \
         can change; with it at {}, an agent can {}.",
        scope.as_db(),
        cap(needed.as_db()),
        cap(needed.as_db()),
        needed.what_it_allows()
    )
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        crate::state::db::open_in_memory().unwrap()
    }

    /// **The dial defaults to Read, every scope maps to exactly one level by a
    /// `match`, a level above Read needs a duration, "session" dies at launch,
    /// "7 days" carries a stamp and reads as Read (flagged expired) once it
    /// passes, and "always" survives both.**
    #[test]
    fn the_dial_defaults_to_read_and_each_duration_ends_the_way_it_says() {
        let c = conn();
        let now = current(&c).unwrap();
        assert_eq!((now.level, now.mode, now.expired), (AccessLevel::Read, None, false));
        assert!(allows(&c, Scope::Read).unwrap() && !allows(&c, Scope::Manage).unwrap());

        assert_eq!(AccessLevel::needed_for(Scope::Read), AccessLevel::Read);
        assert_eq!(AccessLevel::needed_for(Scope::Manage), AccessLevel::Changes);
        assert_eq!(AccessLevel::needed_for(Scope::System), AccessLevel::Changes);
        assert_eq!(AccessLevel::needed_for(Scope::Destroy), AccessLevel::Full);
        assert_eq!(AccessLevel::needed_for(Scope::Run), AccessLevel::Full);

        let err = set(&c, AccessLevel::Full, None).unwrap_err().to_string();
        assert!(err.contains("needs a duration"), "{err}");

        // Session: on, then gone at launch.
        set(&c, AccessLevel::Changes, Some(Mode::Session)).unwrap();
        assert!(allows(&c, Scope::Manage).unwrap() && !allows(&c, Scope::Destroy).unwrap());
        assert!(end_session_at_launch(&c).unwrap());
        assert_eq!(current(&c).unwrap().level, AccessLevel::Read);
        assert!(!end_session_at_launch(&c).unwrap(), "nothing to end twice");

        // 7 days: a stamp in the future; expired once it passes, and the card sees why.
        let a = set(&c, AccessLevel::Full, Some(Mode::Days)).unwrap();
        assert!(a.expires_at.as_deref().unwrap() > store::db_now(&c).unwrap().as_str());
        assert!(allows(&c, Scope::Run).unwrap());
        store::set_setting(&c, EXPIRES_KEY, "2020-01-01 00:00:00").unwrap();
        let a = current(&c).unwrap();
        assert_eq!((a.level, a.expired), (AccessLevel::Read, true));
        assert!(!allows(&c, Scope::Run).unwrap());
        assert!(!end_session_at_launch(&c).unwrap(), "a 7-day setting is not a session's");

        // Always: survives a launch.
        set(&c, AccessLevel::Full, Some(Mode::Always)).unwrap();
        assert!(!end_session_at_launch(&c).unwrap());
        assert!(allows(&c, Scope::Destroy).unwrap());

        // Back to Read clears the duration.
        let a = set(&c, AccessLevel::Read, None).unwrap();
        assert_eq!((a.mode, a.expires_at), (None, None));
    }

    /// **The refusal names the dial, the level needed, the level standing,
    /// where to change it, and what the needed level hands over.**
    #[test]
    fn the_refusal_names_the_dial_and_both_levels() {
        let c = conn();
        let r = refusal("deleting `shop.rex`", Scope::Destroy, &current(&c).unwrap());
        for must in ["`Agent access`", "Full", "it is at Read", "Settings → \"AI agents (MCP)\"", "as you"] {
            assert!(r.contains(must), "missing {must:?} in: {r}");
        }
        set(&c, AccessLevel::Full, Some(Mode::Days)).unwrap();
        store::set_setting(&c, EXPIRES_KEY, "2020-01-01 00:00:00").unwrap();
        let r = refusal("x", Scope::Manage, &current(&c).unwrap());
        assert!(r.contains("has expired"), "{r}");
    }
}
