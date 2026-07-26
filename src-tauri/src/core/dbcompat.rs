//! core::dbcompat — can this source database be restored into the engine this
//! site uses? (Stage 2 step 4.)
//!
//! A pure function over (source vendor + version, target vendor + version), so
//! the whole matrix is testable without a server anywhere near it. The verdict
//! is computed and SHOWN BEFORE anything runs — a compatibility problem the user
//! meets halfway through a dump is a bug in this design, not bad luck.
//!
//! The three outcomes are three TYPES, not three strings, because the UI must
//! render them differently and code must not be able to confuse them:
//!
//! - [`Verdict::Proceed`] — it runs. It may carry cautions, which are things to
//!   KNOW, never things to decide.
//! - [`Verdict::NeedsOverride`] — refused by default; the user may knowingly
//!   proceed, and the cost of doing so is stated up front.
//! - [`Verdict::Blocked`] — refused with no override, either because it cannot
//!   work or because something must be resolved first. Carries the fix when one
//!   exists, so a block is never a dead end.
//!
//! Every reason is written to teach rather than to stop: the user should
//! finish reading knowing WHY, in the same voice as the rest of the import.

use crate::core::dbsource::Vendor;

/// A dotted version, comparable. `raw` keeps what the server actually said
/// (`8.0.36-28`, `10.11.2-MariaDB`) so messages quote it verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub raw: String,
}

impl Version {
    /// Parse the leading dotted number of a version string. Everything after it
    /// (`-MariaDB`, `-28`, `-log`) is a build suffix we keep but don't compare.
    pub fn parse(raw: &str) -> Option<Version> {
        let head: String =
            raw.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let mut parts = head.split('.').filter(|p| !p.is_empty());
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Some(Version { major, minor, patch, raw: raw.trim().to_string() })
    }

    fn series(&self) -> (u32, u32) {
        (self.major, self.minor)
    }

    /// For prose. The server's own string minus the vendor suffix it already
    /// said — otherwise every message reads "MariaDB 10.6.21-MariaDB". `raw`
    /// keeps the full string for anywhere it should be quoted exactly.
    pub fn pretty(&self) -> &str {
        let lower = self.raw.to_ascii_lowercase();
        match lower.rfind("-mariadb") {
            Some(i) if i + "-mariadb".len() == lower.len() => &self.raw[..i],
            _ => &self.raw,
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.pretty())
    }
}

/// The server we would dump FROM. `vendor: None` means the handshake didn't
/// identify it and nobody has declared it.
#[derive(Debug, Clone)]
pub struct Source {
    pub vendor: Option<Vendor>,
    pub version: Option<Version>,
}

/// The engine this site uses in rexenv.
#[derive(Debug, Clone)]
pub struct Target {
    pub vendor: Vendor,
    pub version: Version,
}

/// Something true about this pairing that the user should know, but that does
/// not require a decision. Cautions never block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Caution {
    /// Dumping a 5.7 server with an 8.x tool emits histogram syntax it can't
    /// read back; the dump disables it.
    ColumnStatistics,
    /// MySQL 8.4 disables `mysql_native_password` by default (9.x removes it),
    /// so accounts in an older dump may restore but not sign in.
    AuthPluginRemoved,
    /// 5.7-era routines and triggers can carry `NO_AUTO_CREATE_USER` in their
    /// `sql_mode`, which 8.x rejects.
    LegacySqlModes,
    /// Crossing a major version: character-set and `sql_mode` defaults moved.
    MajorJump,
    /// A vendor swap: definers, storage engines and SQL dialect differ even
    /// where the dump restores cleanly.
    EngineDrift,
}

impl Caution {
    pub fn message(self) -> &'static str {
        match self {
            Caution::ColumnStatistics => {
                "The dump runs with column statistics disabled — an 8.x dump tool would \
                 otherwise write histogram syntax that a 5.7-era dump can't be read back \
                 with."
            }
            Caution::AuthPluginRemoved => {
                "MySQL 8.4 turns off the old `mysql_native_password` plugin by default. \
                 Database users defined with it restore fine but can't sign in until \
                 their password is set again — your site's own connection is unaffected, \
                 because rexenv restores as root."
            }
            Caution::LegacySqlModes => {
                "Stored routines and triggers written under MySQL 5.7 sometimes carry a \
                 `NO_AUTO_CREATE_USER` sql_mode that 8.x no longer accepts. rexenv \
                 reports any it finds in the dump before restoring."
            }
            Caution::MajorJump => {
                "This crosses a major version, where default character sets and sql_modes \
                 changed. Most sites are unaffected; very old schemas occasionally need a \
                 collation adjusted."
            }
            Caution::EngineDrift => {
                "MySQL and MariaDB have drifted: definers, some storage-engine options and \
                 parts of the SQL dialect differ, so a dump can restore cleanly and still \
                 behave differently."
            }
        }
    }
}

/// The answer. Three distinct states, deliberately not one string with a flag.
#[derive(Debug, Clone)]
pub enum Verdict {
    /// The import can run now.
    Proceed { cautions: Vec<Caution> },
    /// Refused by default. The user may proceed knowingly; `consequence` says
    /// what they are accepting, and `better` names a safer route when one
    /// exists.
    NeedsOverride {
        reason: String,
        consequence: String,
        cautions: Vec<Caution>,
        better: Option<String>,
    },
    /// Refused, with no override. `fix` turns a block into a next step whenever
    /// there is one.
    Blocked { reason: String, fix: Option<String> },
}

impl Verdict {
    /// May the import start without asking anything further?
    pub fn runs_now(&self) -> bool {
        matches!(self, Verdict::Proceed { .. })
    }

    /// Is there a door, if the user chooses to open it?
    pub fn overridable(&self) -> bool {
        matches!(self, Verdict::NeedsOverride { .. })
    }

    /// A short label for the row.
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::Proceed { cautions } if cautions.is_empty() => "compatible",
            Verdict::Proceed { .. } => "compatible, with notes",
            Verdict::NeedsOverride { .. } => "not recommended",
            Verdict::Blocked { .. } => "can't import",
        }
    }

    pub fn cautions(&self) -> &[Caution] {
        match self {
            Verdict::Proceed { cautions } | Verdict::NeedsOverride { cautions, .. } => cautions,
            Verdict::Blocked { .. } => &[],
        }
    }

    /// The whole explanation, in reading order.
    pub fn explain(&self) -> String {
        match self {
            Verdict::Proceed { cautions } => {
                let mut s = String::from("rexenv can import this database.");
                for c in cautions {
                    s.push(' ');
                    s.push_str(c.message());
                }
                s
            }
            Verdict::NeedsOverride { reason, consequence, cautions, better } => {
                let mut s = format!("{reason} {consequence}");
                if let Some(b) = better {
                    s.push(' ');
                    s.push_str(b);
                }
                for c in cautions {
                    s.push(' ');
                    s.push_str(c.message());
                }
                s.push_str(" You can import it anyway if you want to.");
                s
            }
            Verdict::Blocked { reason, fix } => match fix {
                Some(f) => format!("{reason} {f}"),
                None => reason.clone(),
            },
        }
    }
}

/// Judge one source against one target.
pub fn compat(source: &Source, target: &Target) -> Verdict {
    let Some(vendor) = source.vendor else {
        return Verdict::Blocked {
            reason: "rexenv couldn't tell which database server this is — it answered, \
                     but didn't identify itself the way MySQL and MariaDB do."
                .into(),
            fix: Some(
                "Tell rexenv which one it is and it will use the matching tools; the \
                 wrong ones can't even sign in, so it won't guess."
                    .into(),
            ),
        };
    };
    let Some(version) = &source.version else {
        return Verdict::NeedsOverride {
            reason: format!(
                "This server says it's {}, but rexenv couldn't read a version number from it.",
                vendor.label()
            ),
            consequence: "Without one, rexenv can't check whether its dump will restore \
                          into your rexenv database — it may fail part-way, or restore \
                          something subtly different."
                .into(),
            cautions: vec![],
            better: None,
        };
    };

    if vendor != target.vendor {
        return cross_vendor(vendor, version, target);
    }

    // Same vendor: the question is which way the version gap runs.
    let mut cautions = Vec::new();
    if vendor == Vendor::Mysql {
        if version.major == 5 {
            cautions.push(Caution::ColumnStatistics);
            cautions.push(Caution::LegacySqlModes);
            cautions.push(Caution::MajorJump);
        }
        // 8.4 turned the old auth plugin off by default.
        if version.series() < (8, 4) && target.version.series() >= (8, 4) {
            cautions.push(Caution::AuthPluginRemoved);
        }
    } else if version.major < target.version.major {
        cautions.push(Caution::MajorJump);
    }

    // A source NEWER than the target is a downgrade restore: the dump can carry
    // syntax the older server has never heard of.
    if version.series() > target.version.series() {
        return Verdict::NeedsOverride {
            reason: format!(
                "This is {} {}, which is newer than the {} {} rexenv runs for this site.",
                vendor.label(),
                version,
                target.vendor.label(),
                target.version
            ),
            consequence: "Restoring a newer server's dump into an older one is the one \
                          direction that isn't supported by the databases themselves — it \
                          may fail part-way through, leaving a half-restored copy."
                .into(),
            cautions,
            better: newer_target_available(target).map(|v| {
                format!(
                    "rexenv also ships {} {v}; switching this site's engine to it first \
                     avoids the downgrade entirely.",
                    target.vendor.label()
                )
            }),
        };
    }

    // Very old MySQL: reachable, but far enough back that dumps routinely use
    // syntax 8.x rejects.
    if vendor == Vendor::Mysql && version.series() < (5, 7) {
        return Verdict::NeedsOverride {
            reason: format!("This is MySQL {version}, several major versions behind."),
            consequence: "Dumps this old often contain syntax and character sets that \
                          MySQL 8 no longer accepts, so the restore may stop part-way."
                .into(),
            cautions,
            better: None,
        };
    }
    if vendor == Vendor::Mariadb && version.major < 10 {
        return Verdict::NeedsOverride {
            reason: format!("This is MariaDB {version}, several major versions behind."),
            consequence: "Dumps this old often contain syntax and character sets that \
                          current MariaDB no longer accepts, so the restore may stop \
                          part-way."
                .into(),
            cautions,
            better: None,
        };
    }

    Verdict::Proceed { cautions }
}

/// MySQL ↔ MariaDB. They share a protocol and a history, not a dialect — and
/// rexenv ships both, so a same-vendor target is always available.
fn cross_vendor(vendor: Vendor, version: &Version, target: &Target) -> Verdict {
    match vendor {
        // Verified hard failure: real mariadb-dump output carries Aria table
        // options (`PAGE_CHECKSUM=1 TRANSACTIONAL=1`) that MySQL rejects with a
        // syntax error. There is no override for a dump that cannot parse, and
        // the alternative is one setting away.
        Vendor::Mariadb => Verdict::Blocked {
            reason: format!(
                "This is MariaDB {version}, and this site's rexenv database is MySQL {}. \
                 MariaDB dumps carry table options MySQL refuses to read, so the restore \
                 would stop on the first table.",
                target.version
            ),
            fix: Some(
                "rexenv runs MariaDB too — set this site's database engine to MariaDB and \
                 the import becomes a like-for-like copy."
                    .into(),
            ),
        },
        // The softer direction: it usually restores, and then differs.
        Vendor::Mysql => Verdict::NeedsOverride {
            reason: format!(
                "This is MySQL {version}, and this site's rexenv database is MariaDB {}.",
                target.version
            ),
            consequence: "The two have drifted apart: a MySQL dump usually restores into \
                          MariaDB, but definers, some storage-engine options and parts of \
                          the SQL dialect differ, so problems tend to show up later rather \
                          than during the import."
                .into(),
            cautions: vec![Caution::EngineDrift],
            better: Some(
                "rexenv runs MySQL too — setting this site's database engine to MySQL \
                 makes this a like-for-like copy with nothing to convert."
                    .into(),
            ),
        },
    }
}

/// A newer version of the SAME engine that rexenv also ships, if any — used to
/// offer a real way out of a downgrade rather than only a warning.
fn newer_target_available(target: &Target) -> Option<&'static str> {
    let engine = match target.vendor {
        Vendor::Mysql => crate::core::binaries::MYSQL_VERSIONS,
        Vendor::Mariadb => crate::core::binaries::MARIADB_VERSIONS,
    };
    engine
        .iter()
        .filter_map(|v| Version::parse(v).map(|p| (p, *v)))
        .filter(|(p, _)| p.series() > target.version.series())
        .max_by(|a, b| a.0.series().cmp(&b.0.series()))
        .map(|(_, raw)| raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(vendor: Vendor, v: &str) -> Source {
        Source { vendor: Some(vendor), version: Version::parse(v) }
    }
    fn tgt(vendor: Vendor, v: &str) -> Target {
        Target { vendor, version: Version::parse(v).unwrap() }
    }
    /// Everything rexenv can actually be, today.
    fn all_targets() -> Vec<Target> {
        vec![
            tgt(Vendor::Mysql, "8.4.6"),
            tgt(Vendor::Mysql, "8.0.44"),
            tgt(Vendor::Mariadb, "12.3.2"),
            tgt(Vendor::Mariadb, "11.4.12"),
        ]
    }

    #[test]
    fn versions_parse_including_the_shapes_servers_really_send() {
        for (raw, want) in [
            ("8.0.27", (8, 0, 27)),
            ("8.4.6", (8, 4, 6)),
            // A MySQL derivative (Percona) and a MariaDB self-identifier.
            ("8.0.36-28", (8, 0, 36)),
            ("10.11.2-MariaDB", (10, 11, 2)),
            ("12.3.2-MariaDB", (12, 3, 2)),
            // Two-component and suffixed forms.
            ("5.7", (5, 7, 0)),
            ("5.6.51-log", (5, 6, 51)),
        ] {
            let v = Version::parse(raw).unwrap_or_else(|| panic!("{raw} should parse"));
            assert_eq!((v.major, v.minor, v.patch), want, "{raw}");
            assert_eq!(v.raw, raw, "the server's own words are kept verbatim");
        }
        // Prose says "MariaDB 10.11.2", not "MariaDB 10.11.2-MariaDB".
        assert_eq!(Version::parse("10.11.2-MariaDB").unwrap().to_string(), "10.11.2");
        assert_eq!(Version::parse("8.0.36-28").unwrap().to_string(), "8.0.36-28");
        assert!(Version::parse("mysqld").is_none());
        assert!(Version::parse("").is_none());
    }

    #[test]
    fn the_matrix_is_exhaustive_and_every_pairing_has_a_verdict_that_explains_itself() {
        // Every source we can actually meet, against every target we ship.
        let sources: Vec<(&str, Source)> = vec![
            ("mysql 5.6", src(Vendor::Mysql, "5.6.51-log")),
            ("mysql 5.7", src(Vendor::Mysql, "5.7.44")),
            ("mysql 8.0 (DBngin)", src(Vendor::Mysql, "8.0.27")),
            ("mysql 8.0 percona", src(Vendor::Mysql, "8.0.36-28")),
            ("mysql 8.4", src(Vendor::Mysql, "8.4.6")),
            ("mysql 9.x", src(Vendor::Mysql, "9.1.0")),
            ("mariadb 10.6", src(Vendor::Mariadb, "10.6.21-MariaDB")),
            ("mariadb 10.11", src(Vendor::Mariadb, "10.11.2-MariaDB")),
            ("mariadb 11.4", src(Vendor::Mariadb, "11.4.12-MariaDB")),
            ("mariadb 12.3", src(Vendor::Mariadb, "12.3.2-MariaDB")),
            ("mariadb 12.9 (future)", src(Vendor::Mariadb, "12.9.0-MariaDB")),
            ("mariadb 5.5 (ancient)", src(Vendor::Mariadb, "5.5.68-MariaDB")),
            ("unidentified", Source { vendor: None, version: None }),
            (
                "identified, unreadable version",
                Source { vendor: Some(Vendor::Mysql), version: None },
            ),
        ];

        for (name, s) in &sources {
            for t in all_targets() {
                let v = compat(s, &t);
                let text = v.explain();
                assert!(!text.is_empty(), "{name} -> {:?} said nothing", t.version);
                assert!(text.ends_with('.'), "{name}: {text:?} should read as sentences");
                // Anything that refuses or cautions owes the user a reason. A
                // clean pairing does not — "rexenv can import this database."
                // is the whole truth, and padding it would be noise beside the
                // row, which already names the database, server and versions.
                let clean = matches!(&v, Verdict::Proceed { cautions } if cautions.is_empty());
                if !clean {
                    assert!(text.len() > 80, "{name}: {text:?} is too terse to teach anything");
                }
                // Same voice as the rest of the import: explain, never scold.
                for blame in ["invalid", "illegal", "you must", "unsupported configuration"] {
                    assert!(
                        !text.to_lowercase().contains(blame),
                        "{name} -> {}: {text:?} reads as blame ({blame})",
                        t.version
                    );
                }
                // A block is never a dead end unless there genuinely is no way on.
                if let Verdict::Blocked { fix, .. } = &v {
                    assert!(fix.is_some(), "{name} -> {}: blocked with no way forward", t.version);
                }
                // The three states stay distinguishable.
                assert_eq!(v.runs_now(), matches!(v, Verdict::Proceed { .. }));
                assert_eq!(v.overridable(), matches!(v, Verdict::NeedsOverride { .. }));
                assert!(!(v.runs_now() && v.overridable()));
            }
        }
    }

    #[test]
    fn the_live_sample_imports_cleanly_into_either_mysql_we_ship() {
        // DBngin MySQL 8.0.27 — the real source on the dev machine.
        let s = src(Vendor::Mysql, "8.0.27");
        let into_8_0 = compat(&s, &tgt(Vendor::Mysql, "8.0.44"));
        assert!(into_8_0.runs_now());
        assert!(into_8_0.cautions().is_empty(), "same series, nothing to warn about");

        let into_8_4 = compat(&s, &tgt(Vendor::Mysql, "8.4.6"));
        assert!(into_8_4.runs_now(), "8.0 -> 8.4 is an upgrade restore, which is fine");
        assert_eq!(into_8_4.cautions(), &[Caution::AuthPluginRemoved]);
        assert!(into_8_4.explain().contains("restores as root"));
    }

    #[test]
    fn a_5_7_source_proceeds_but_says_what_it_is_doing() {
        let v = compat(&src(Vendor::Mysql, "5.7.44"), &tgt(Vendor::Mysql, "8.4.6"));
        assert!(v.runs_now());
        assert!(v.cautions().contains(&Caution::ColumnStatistics));
        assert!(v.cautions().contains(&Caution::LegacySqlModes));
        assert!(v.cautions().contains(&Caution::AuthPluginRemoved));
        assert_eq!(v.label(), "compatible, with notes");
    }

    #[test]
    fn a_newer_source_needs_an_override_and_is_offered_the_way_out() {
        // MySQL 8.4 into our 8.0.44: a downgrade restore.
        let v = compat(&src(Vendor::Mysql, "8.4.6"), &tgt(Vendor::Mysql, "8.0.44"));
        assert!(v.overridable(), "the user may still choose to try");
        match &v {
            Verdict::NeedsOverride { better, .. } => {
                let b = better.as_deref().expect("we ship 8.4.6 — offer it");
                assert!(b.contains("8.4.6"), "{b}");
            }
            other => panic!("{other:?}"),
        }
        assert!(v.explain().ends_with("You can import it anyway if you want to."));

        // MySQL 9.x, newer than anything we ship: same state, no way out to offer.
        let v9 = compat(&src(Vendor::Mysql, "9.1.0"), &tgt(Vendor::Mysql, "8.4.6"));
        assert!(v9.overridable());
        match v9 {
            Verdict::NeedsOverride { better, .. } => assert!(better.is_none()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_two_cross_vendor_directions_are_deliberately_different() {
        // MariaDB -> MySQL: verified to fail on the first table (Aria options),
        // so there is nothing to override — but the fix is one setting.
        let m2m = compat(&src(Vendor::Mariadb, "11.4.12-MariaDB"), &tgt(Vendor::Mysql, "8.4.6"));
        assert!(!m2m.runs_now() && !m2m.overridable());
        match &m2m {
            Verdict::Blocked { fix, .. } => {
                assert!(fix.as_ref().unwrap().contains("set this site's database engine to MariaDB"))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(m2m.label(), "can't import");

        // MySQL -> MariaDB: usually restores, then differs. Overridable, with
        // the better route named.
        let y2m = compat(&src(Vendor::Mysql, "8.0.27"), &tgt(Vendor::Mariadb, "12.3.2"));
        assert!(y2m.overridable());
        assert_eq!(y2m.cautions(), &[Caution::EngineDrift]);
        assert!(y2m.explain().contains("like-for-like copy"));
    }

    #[test]
    fn an_unidentified_server_is_blocked_with_a_resolution_not_an_override() {
        let v = compat(&Source { vendor: None, version: None }, &tgt(Vendor::Mysql, "8.4.6"));
        // Deliberately NOT overridable: proceeding means picking client tools by
        // coin flip, and the wrong ones cannot authenticate at all.
        assert!(!v.overridable(), "there is nothing here for the user to accept");
        match &v {
            Verdict::Blocked { fix, .. } => {
                assert!(fix.as_ref().unwrap().contains("Tell rexenv which one it is"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mariadb_sources_land_on_our_mariadb_without_drama() {
        for v in ["10.6.21-MariaDB", "10.11.2-MariaDB", "11.4.12-MariaDB"] {
            let verdict = compat(&src(Vendor::Mariadb, v), &tgt(Vendor::Mariadb, "12.3.2"));
            assert!(verdict.runs_now(), "{v} should import into MariaDB 12.3.2");
        }
        // …and a future MariaDB into our current one is still a downgrade.
        assert!(compat(&src(Vendor::Mariadb, "12.9.0-MariaDB"), &tgt(Vendor::Mariadb, "12.3.2"))
            .overridable());
    }

    #[test]
    fn cautions_are_information_and_never_change_whether_it_runs() {
        // A Proceed with cautions still runs; that distinction is the whole
        // reason cautions aren't warnings-that-block.
        let v = compat(&src(Vendor::Mysql, "5.7.44"), &tgt(Vendor::Mysql, "8.0.44"));
        assert!(v.runs_now());
        assert!(!v.cautions().is_empty());
        for c in v.cautions() {
            assert!(c.message().len() > 40);
            assert!(c.message().ends_with('.'));
        }
    }
}
