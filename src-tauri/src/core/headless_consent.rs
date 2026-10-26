//! core::headless_consent — the MCP opt-in and the Agent access dial on a box with no Settings
//! screen (docs/PLAN-headless-web.md D7, task T3).
//!
//! **Why this exists at all.** Both settings are GUI consents by rule: `rex config` and the MCP
//! `settings` tool refuse them (`settings_access`, ledger #203/#397), because the sentence beside
//! the toggle IS the consent and a shell write would skip it. A headless server has no toggle, so
//! issue #7's `rex mcp` could never be turned on there (T0). D7's answer: the administrator's own
//! one-time step, `sudo rexenv --headless-setup`, asks the same two questions in a terminal with
//! the SAME sentences — `mcp_server::ENABLE_CONSENT` and `AccessLevel::what_it_allows`, never a copy
//! — or takes `--enable-mcp` / `--agent-access <level>` typed by that administrator for an
//! unattended install. The flags ARE the consent.
//!
//! **Why only root reaches the write.** The threat the dial exists for is an agent widening its own
//! access, and an agent runs as the user. So nothing the user's own processes can invoke writes these
//! keys: `rex config` still refuses them, and the ONE caller of [`apply`] is the setup, after its
//! `geteuid() == 0` check, in a child that has dropped to the user only to write the user's own
//! database (`platform/linux/headless.rs`, ledger #920). There is no argv flag for "just write it".
//!
//! Common code: the questions, the answers and the write are the same on every OS; only Linux has a
//! headless setup yet (D3), and macOS and Windows refuse `--headless-setup` before any of this runs.

use crate::core::agent_access::{self, AccessLevel, Mode, DIAL_LABEL, LEVELS};
use crate::error::Result;
use crate::mcp_server::{enable_consent_text, MCP_ENABLED_KEY};
use crate::state::store;
use rusqlite::Connection;

/// `--enable-mcp`: the administrator's typed consent to the endpoint.
pub const ENABLE_MCP_FLAG: &str = "--enable-mcp";
/// `--agent-access <read|changes|full>`: the administrator's typed consent to a level.
pub const AGENT_ACCESS_FLAG: &str = "--agent-access";

/// What the administrator consented to. `None` = not asked, left as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Consent {
    pub mcp: Option<bool>,
    pub access: Option<AccessLevel>,
}

impl Consent {
    /// Whether anything is to be written.
    pub fn is_empty(&self) -> bool {
        self.mcp.is_none() && self.access.is_none()
    }
}

/// What a headless server lacks that the paragraph mentions: the card's activity list. Said, so
/// "listed below" is not read as a promise the terminal keeps.
pub const NO_CARD_NOTE: &str = "(On a headless server there is no Settings card yet: the activity list it mentions is \
recorded the same way, but has no screen to show it until rexenv's web UI ships.)";

/// The first question: the card's own paragraph, then the toggle's label as a question.
pub fn mcp_question() -> String {
    format!("{}\n{NO_CARD_NOTE}\n\nEnable the MCP endpoint? [y/N] ", enable_consent_text())
}

/// The second question: every level with the card's own sentence for it.
pub fn access_question() -> String {
    let mut q = format!("{DIAL_LABEL} — how far an agent may go on your sites:\n");
    for l in LEVELS {
        q.push_str(&format!("  {:<8} {}\n", l.as_db(), l.what_it_allows()));
    }
    q.push_str(&format!(
        "A level above read stays until you change it (re-run the setup with {AGENT_ACCESS_FLAG} read).\n\
         {DIAL_LABEL} [read/changes/full] (Enter = read): "
    ));
    q
}

/// A yes/no answer; Enter is No (the toggle's default is off). `None` = ask again.
pub fn parse_yes(answer: &str) -> Option<bool> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "" | "n" | "no" => Some(false),
        _ => None,
    }
}

/// A level typed at the prompt or after `--agent-access`; Enter at the prompt is Read.
pub fn parse_level(answer: &str) -> Option<AccessLevel> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => Some(AccessLevel::Read),
        other => AccessLevel::parse(other),
    }
}

/// The refusal for a level that is not one.
pub fn bad_level(value: &str) -> String {
    format!("{AGENT_ACCESS_FLAG} takes read, changes or full — not {value:?}")
}

/// Write what was consented to into the user's own database: the toggle's key and the dial,
/// exactly as the card's controls write them. A level above Read is set for ALWAYS — there is no
/// session to end and no one to see a 7-day expiry. Returns one line per change, for the terminal.
///
/// The ONE non-GUI writer of these keys (ledger #920): called only by the root setup.
pub fn apply(conn: &Connection, consent: &Consent) -> Result<Vec<String>> {
    let mut said = Vec::new();
    if let Some(on) = consent.mcp {
        store::set_setting(conn, MCP_ENABLED_KEY, if on { "true" } else { "false" })?;
        said.push(if on {
            "MCP endpoint: on — it opens at rexenv's next launch; connect an agent with `rex mcp`.".to_string()
        } else {
            "MCP endpoint: off.".to_string()
        });
    }
    if let Some(level) = consent.access {
        let mode = (level != AccessLevel::Read).then_some(Mode::Always);
        let now = agent_access::set(conn, level, mode)?;
        said.push(format!(
            "{DIAL_LABEL}: {}{} — an agent may {}.",
            now.level.as_db(),
            if mode.is_some() { ", until you change it" } else { "" },
            now.allows
        ));
    }
    // The card's "standing" line, in the terminal: the endpoint off with the dial left up.
    if consent.mcp == Some(false) && consent.access.is_none() {
        let now = agent_access::current(conn)?;
        if now.level != AccessLevel::Read {
            said.push(format!(
                "{DIAL_LABEL} is still set to {} and applies as soon as the endpoint is on ({AGENT_ACCESS_FLAG} read lowers it).",
                now.level.as_db()
            ));
        }
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_asks_with_the_cards_own_sentences() {
        let q = mcp_question();
        assert!(q.starts_with(&enable_consent_text()), "the question must open with the card's paragraph");
        assert!(q.contains("every site's mail") && q.contains("as you"), "{q}");
        assert!(q.trim_end().ends_with("[y/N]"), "Enter must read as the toggle's default, off");
        let a = access_question();
        for l in LEVELS {
            assert!(a.contains(l.what_it_allows()), "the {l:?} sentence must be Rust's own");
        }
        // No copy of either lives in this file: the words come from their owners.
        let src = include_str!("headless_consent.rs");
        let src = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(!src.contains("password-reset") && !src.contains("hands over"), "a copied sentence");
    }

    #[test]
    fn answers_default_to_the_safe_side_and_junk_asks_again() {
        assert_eq!(parse_yes(""), Some(false));
        assert_eq!(parse_yes(" Y \n"), Some(true));
        assert_eq!(parse_yes("yes"), Some(true));
        assert_eq!(parse_yes("sure"), None);
        assert_eq!(parse_level(""), Some(AccessLevel::Read));
        assert_eq!(parse_level("FULL\n"), Some(AccessLevel::Full));
        assert_eq!(parse_level("admin"), None);
        assert!(bad_level("admin").contains("read, changes or full"));
    }

    #[test]
    fn apply_writes_exactly_what_the_cards_controls_write() {
        let conn = crate::state::db::open_in_memory().unwrap();
        // Nothing consented → nothing written.
        assert!(apply(&conn, &Consent::default()).unwrap().is_empty());
        assert_eq!(store::get_setting(&conn, MCP_ENABLED_KEY).unwrap(), None);
        apply(&conn, &Consent { mcp: Some(true), access: Some(AccessLevel::Changes) }).unwrap();
        assert_eq!(store::get_setting(&conn, MCP_ENABLED_KEY).unwrap().as_deref(), Some("true"));
        let now = agent_access::current(&conn).unwrap();
        assert_eq!((now.level, now.mode), (AccessLevel::Changes, Some(Mode::Always)));
        // Back down: Read clears the duration, off is written as the toggle writes it.
        let said = apply(&conn, &Consent { mcp: Some(false), access: Some(AccessLevel::Read) }).unwrap();
        assert_eq!(said.len(), 2);
        assert_eq!(store::get_setting(&conn, MCP_ENABLED_KEY).unwrap().as_deref(), Some("false"));
        assert_eq!(agent_access::current(&conn).unwrap().mode, None);
        // Off with the dial left up says so, as the card's standing line does.
        agent_access::set(&conn, AccessLevel::Full, Some(Mode::Always)).unwrap();
        let said = apply(&conn, &Consent { mcp: Some(false), access: None }).unwrap();
        assert!(said[1].contains("still set to full"), "{said:?}");
        // The CLI still may not write either: this door is not `rex config`.
        for key in [MCP_ENABLED_KEY, agent_access::LEVEL_KEY, agent_access::MODE_KEY] {
            assert!(matches!(crate::core::settings_access::cli_access(key), crate::core::settings_access::CliAccess::Denied(_)), "{key}");
        }
    }

    /// **Only the root setup writes these keys outside the GUI** (ledger #920). `apply` has one
    /// caller in the tree, and it sits in the Linux setup after the root check and inside the
    /// child that dropped to the user. A second caller — a `rex` verb, an argv mode the user's own
    /// processes could run, an MCP tool — is how an agent would widen its own access.
    #[test]
    fn only_the_root_setup_calls_apply() {
        fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).expect("read src").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, root, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    if rel != "core/headless_consent.rs" && src.contains("headless_consent::apply(") {
                        out.push(rel);
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut callers = Vec::new();
        walk(&root, &root, &mut callers);
        // Not only `src/`: the CLI crate and the examples are callers too if they ever call it.
        for extra in ["examples", "../cli/src"] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(extra);
            if dir.is_dir() {
                walk(&dir, &dir, &mut callers);
            }
        }
        assert_eq!(callers, ["platform/linux/headless.rs"], "apply must have exactly one caller");
        let door = include_str!("../platform/linux/headless.rs");
        assert_eq!(door.matches("headless_consent::apply(").count(), 1);
        let setup = &door[door.find("fn setup(argv: &[String])").expect("the setup")..];
        let root = setup.find("if unsafe { libc::geteuid() } != 0 {").expect("the root check");
        let drop = setup.find("write_as_user(&account, ").expect("the drop to the user");
        assert!(root < drop, "the root check must come before the write");
        let child = &door[door.find("fn write_as_user(").expect("the child")..];
        let child = &child[..child.find("\n}\n").unwrap()];
        // The drop lives in `become_user`, shared with the fresh-server preparation's child (#923).
        let uid = child.find("if !become_user(account, &name) {").expect("the uid drop");
        let write = child.find("headless_consent::apply(").expect("the write, inside the child");
        assert!(uid < write, "the write must run after the child dropped to the user");
        let drop_fn = &door[door.find("fn become_user(").expect("the drop")..];
        let drop_fn = &drop_fn[..drop_fn.find("\n}\n").unwrap()];
        assert!(drop_fn.contains("libc::setuid(account.uid) == 0") && drop_fn.contains("libc::setuid(0) != 0"), "the drop, proven");
    }
}
