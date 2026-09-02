//! core::valet — READ-ONLY discovery of Laravel Valet / Herd environments.
//!
//! **Nothing in this module writes anything, anywhere.** It reads their config,
//! their symlink farm and their per-site nginx confs, and it never opens a file
//! *inside* a user's project (classification is `core::sites::detect_project`,
//! which does existence probes only). Their services are never started or
//! stopped, and their files are never modified — the user must be able to go
//! back to Herd at any moment.
//!
//! Discovery is deliberately tolerant. Real installations are messy: the dev
//! machine this was built against has 29 sites across two trees of which 13 are
//! dangling symlinks, two confs with no site folder at all, a conf on a TLD its
//! own config doesn't mention, a parked path listed twice differing only by a
//! trailing slash, and BOTH isolation-marker formats side by side. Every one of
//! those is surfaced as a row or a note; none of them is a crash, and none is
//! silently dropped — a site the user can see in Herd but not here would make
//! them distrust the whole list.
//!
//! This module is pure filesystem, with no database access, so the scan is
//! unit-testable against fixture trees. Status that depends on rexenv's own
//! state — already imported, overlaps an existing site, PHP version we don't
//! ship — is decided by the caller, which has the connection.

use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Which tool a discovered site came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Valet,
    Herd,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Valet => "Valet",
            SourceKind::Herd => "Herd",
        }
    }
}

/// One discovered environment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub kind: SourceKind,
    pub home: String,
    /// From `config.json`; `tld` else the pre-v2.1 `domain` key else `test`.
    pub tld: String,
    pub loopback: String,
    /// Parked directories, canonicalized and deduped.
    pub parked: Vec<String>,
    /// Anything odd we want the user to see rather than wonder about.
    pub notes: Vec<String>,
}

/// How a site is registered with Valet/Herd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum Origin {
    /// A symlink in `<home>/Sites` — the target is where the code lives.
    Linked { target: String },
    /// A directory inside a parked path.
    Parked,
    /// A per-site nginx conf with no site behind it.
    ConfigOnly,
}

/// Why a row can't be imported as-is. The caller adds the reasons that need
/// rexenv's own state (already imported, overlapping, unavailable PHP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "reason")]
pub enum SiteStatus {
    Importable,
    NeedsAttention(String),
    Unsupported(String),
    /// rexenv already serves this domain. Shown so the list reconciles with
    /// what they see in Valet/Herd, but never offered again.
    AlreadyImported,
}

/// One row of the migration list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSite {
    pub source: SourceKind,
    /// The Valet/Herd site name (the symlink or directory name).
    pub name: String,
    /// Full hostname it is served as.
    pub domain: String,
    pub origin: Origin,
    /// Where the code actually is — `None` when the target is missing.
    pub path: Option<String>,
    /// PHP minor from `# ISOLATED_PHP_VERSION`, in whichever format they wrote
    /// it. `None` = they use their global PHP, so we pick.
    pub php_minor: Option<String>,
    /// They issued a certificate for it (we always issue our own).
    pub secured: bool,
    /// A `valet proxy` entry rather than a site.
    pub proxy_to: Option<String>,
    /// The same domain also exists in the other tool.
    pub also_in: Option<SourceKind>,
    pub status: SiteStatus,
}

/// The whole read-only picture.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub sources: Vec<Source>,
    pub sites: Vec<DiscoveredSite>,
}

/// Default Valet home, current then legacy (`~/.valet` before v2.1).
pub fn valet_homes(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".config/valet"), home.join(".valet")]
}

/// Herd keeps a Valet-shaped tree of its own.
pub fn herd_home(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Herd/config/valet")
}

/// Read both environments. Herd wins a domain collision: it auto-migrates Valet
/// on first launch, so its copy is the superset — the Valet row is folded in as
/// an `also_in` note rather than shown twice.
pub fn discover(home: &Path) -> Discovery {
    let mut out = Discovery::default();
    let valet = valet_homes(home).into_iter().find(|p| p.join("config.json").is_file());
    let herd = Some(herd_home(home)).filter(|p| p.join("config.json").is_file());

    // Herd first so it wins the dedupe.
    for (kind, dir) in [(SourceKind::Herd, herd), (SourceKind::Valet, valet)] {
        let Some(dir) = dir else { continue };
        let (source, sites) = scan_source(kind, &dir);
        out.sources.push(source);
        for mut site in sites {
            if let Some(existing) = out.sites.iter_mut().find(|s| s.domain == site.domain) {
                existing.also_in = Some(site.source);
                continue;
            }
            site.also_in = None;
            out.sites.push(site);
        }
    }
    out.sites.sort_by(|a, b| a.domain.cmp(&b.domain));
    out
}

/// Read ONE environment rooted at `dir`. Split out so tests can build fixture
/// trees; `discover` is just this plus the dedupe.
pub fn scan_source(kind: SourceKind, dir: &Path) -> (Source, Vec<DiscoveredSite>) {
    let mut notes = Vec::new();
    let raw = std::fs::read_to_string(dir.join("config.json"));
    let cfg = match &raw {
        Ok(text) => parse_config(text).unwrap_or_else(|| {
            notes.push(format!(
                "couldn't understand {}'s config.json — no sites read from it",
                kind.label()
            ));
            Config::default()
        }),
        Err(e) => {
            notes.push(format!("couldn't read {}'s config.json ({e})", kind.label()));
            Config::default()
        }
    };

    // Report the catch-all before anything else, because it is the one part of
    // their setup that will NOT be reproduced — every other row here either
    // imports or says why it cannot, and this one would just quietly stop
    // happening.
    if let Some(default_site) = &cfg.default_site {
        notes.push(format!(
            "{} serves {default_site} for any unmatched *.{} hostname (its `default` \
             setting). rexenv has no catch-all: after migrating, a hostname you have not \
             created will not resolve to it. Import that project as its own site if you \
             need it.",
            kind.label(),
            cfg.tld
        ));
    }

    let sites_dir = dir.join("Sites");
    let mut sites: Vec<DiscoveredSite> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1) Linked sites. Links win over parked, as in Valet.
    match std::fs::read_dir(&sites_dir) {
        Ok(entries) => {
            let mut unreadable = 0;
            for e in entries {
                let Ok(e) = e else {
                    unreadable += 1;
                    continue;
                };
                let Ok(name) = e.file_name().into_string() else {
                    unreadable += 1;
                    continue;
                };
                let link = e.path();
                let target = std::fs::read_link(&link)
                    .map(|t| if t.is_absolute() { t } else { sites_dir.join(t) })
                    .unwrap_or_else(|_| link.clone());
                seen.insert(name.clone());
                sites.push(row(
                    kind,
                    &name,
                    &cfg.tld,
                    Origin::Linked { target: target.display().to_string() },
                    target.is_dir().then(|| target.clone()),
                    dir,
                ));
            }
            if unreadable > 0 {
                notes.push(format!("{unreadable} unreadable entries in {}", sites_dir.display()));
            }
        }
        Err(_) if sites_dir.exists() => {
            notes.push(format!("couldn't read {}", sites_dir.display()))
        }
        Err(_) => {}
    }

    // 2) Parked directories. The Sites dir is itself listed in `paths` once a
    //    site has been linked — skip it so links aren't counted twice.
    let canon_sites = sites_dir.canonicalize().unwrap_or_else(|_| sites_dir.clone());
    for parked in &cfg.parked {
        let p = PathBuf::from(parked);
        if p.canonicalize().unwrap_or_else(|_| p.clone()) == canon_sites {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&p) else {
            notes.push(format!("parked folder {} isn't readable", p.display()));
            continue;
        };
        let mut count = 0;
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let Ok(name) = e.file_name().into_string() else { continue };
            if name.starts_with('.') || !seen.insert(name.clone()) {
                continue;
            }
            count += 1;
            sites.push(row(kind, &name, &cfg.tld, Origin::Parked, Some(e.path()), dir));
        }
        if count == 0 {
            notes.push(format!("parked folder {} has no sites in it", p.display()));
        }
    }

    // 3) Per-site confs with nothing behind them. Listed rather than dropped so
    //    the count reconciles with what the user sees in Valet/Herd.
    if let Ok(entries) = std::fs::read_dir(dir.join("Nginx")) {
        let known: HashSet<String> = sites.iter().map(|s| s.domain.clone()).collect();
        for e in entries.flatten() {
            let Ok(fname) = e.file_name().into_string() else { continue };
            if fname.starts_with('.') || known.contains(&fname) {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            let mut r = DiscoveredSite {
                source: kind,
                name: fname.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or(fname.clone()),
                domain: fname.clone(),
                origin: Origin::ConfigOnly,
                path: None,
                php_minor: parse_isolated_php(&text),
                secured: cert_exists(dir, &fname),
                proxy_to: parse_proxy_target(&text),
                also_in: None,
                status: SiteStatus::Unsupported(String::new()),
            };
            r.status = SiteStatus::Unsupported(match &r.proxy_to {
                Some(to) => format!("a {} proxy to {to}, not a site", kind.label()),
                None => "leftover config with no site folder".into(),
            });
            sites.push(r);
        }
    }

    let source = Source {
        kind,
        home: dir.display().to_string(),
        tld: cfg.tld,
        loopback: cfg.loopback,
        parked: cfg.parked,
        notes,
    };
    (source, sites)
}

/// Build one row for a linked/parked site, reading its per-site conf if any.
fn row(
    kind: SourceKind,
    name: &str,
    tld: &str,
    origin: Origin,
    path: Option<PathBuf>,
    home: &Path,
) -> DiscoveredSite {
    let domain = format!("{name}.{tld}");
    let conf = std::fs::read_to_string(home.join("Nginx").join(&domain)).unwrap_or_default();
    let proxy_to = parse_proxy_target(&conf);
    let missing_target = match &origin {
        Origin::Linked { target } => path.is_none().then(|| target.clone()),
        _ => None,
    };
    let status = if let Some(to) = &proxy_to {
        SiteStatus::Unsupported(format!("a {} proxy to {to}, not a site", kind.label()))
    } else if let Some(target) = missing_target {
        SiteStatus::Unsupported(format!("its folder is missing ({target})"))
    } else if path.is_none() {
        SiteStatus::Unsupported("its folder is missing".into())
    } else if !crate::core::tld::is_valid_label(&name.to_lowercase())
        && !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        SiteStatus::Unsupported(format!("\"{name}\" isn't a valid hostname"))
    } else {
        SiteStatus::Importable
    };
    DiscoveredSite {
        source: kind,
        name: name.to_string(),
        domain: domain.clone(),
        origin,
        path: path.map(|p| p.display().to_string()),
        php_minor: parse_isolated_php(&conf),
        secured: cert_exists(home, &domain),
        proxy_to,
        also_in: None,
        status,
    }
}

fn cert_exists(home: &Path, domain: &str) -> bool {
    home.join("Certificates").join(format!("{domain}.crt")).is_file()
}

/// The bits of `config.json` we use.
#[derive(Debug, Clone)]
struct Config {
    tld: String,
    loopback: String,
    parked: Vec<String>,
    /// Valet's `default` key: the project served for ANY unmatched hostname
    /// under their TLD. rexenv has no catch-all — an unknown host reaches
    /// nginx's default server, which is deliberately not a site — so this
    /// cannot be imported, only REPORTED. Silence would be the bad outcome: a
    /// user whose `foo.test` typo used to land on a working site would see it
    /// stop working after migrating and have nothing to connect it to.
    default_site: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tld: "test".into(),
            loopback: "127.0.0.1".into(),
            parked: Vec::new(),
            default_site: None,
        }
    }
}

/// Parse their config leniently.
///
/// `tld` was called `domain` before Valet 2.1 (which also moved the home
/// directory and changed the default from `.dev` to `.test`), so both keys are
/// accepted. Parked paths are deduped after normalising trailing slashes —
/// Herd really does list the same folder twice, differing only by one.
fn parse_config(text: &str) -> Option<Config> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let tld = v["tld"]
        .as_str()
        .or_else(|| v["domain"].as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("test")
        .trim()
        .trim_start_matches('.')
        .to_string();
    let loopback = v["loopback"].as_str().unwrap_or("127.0.0.1").to_string();
    let mut parked = Vec::new();
    let mut seen = HashSet::new();
    if let Some(arr) = v["paths"].as_array() {
        for p in arr.iter().filter_map(|p| p.as_str()) {
            let norm = p.trim_end_matches('/').to_string();
            if !norm.is_empty() && seen.insert(norm.clone()) {
                parked.push(norm);
            }
        }
    }
    // Their `default` is a PATH to the project, and an empty string means
    // "unset" in their own config (Valet writes `""` when you clear it), which
    // is why this filters rather than just mapping.
    let default_site = v["default"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(Config { tld, loopback, parked, default_site })
}

/// The PHP minor a per-site conf is isolated to, in any of the formats the
/// tools actually write.
///
/// Valet writes `php@8.1` on a fresh isolate but rewrites it as bare digits
/// (`81`) when securing/unsecuring, and Herd writes `8.3`. All three occur on
/// one machine, so all three are accepted: take the digits and split after the
/// major. Scanned anywhere in the file, not just line 1 — that is what Valet's
/// own `isolated` listing does.
pub fn parse_isolated_php(conf: &str) -> Option<String> {
    let raw = conf
        .lines()
        .find_map(|l| l.trim().strip_prefix("# ISOLATED_PHP_VERSION="))?
        .trim();
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 2 {
        return None;
    }
    let (major, minor) = digits.split_at(1);
    Some(format!("{major}.{minor}"))
}

/// The upstream a `valet proxy` entry points at, if this conf is one.
///
/// Mirrors Valet's own recognition: a conf with a `proxy_pass` and no fastcgi
/// backend is a proxy, not a site. The stub marker on line 1 is a corroborating
/// signal, not a requirement — securing a proxy rewrites the file.
pub fn parse_proxy_target(conf: &str) -> Option<String> {
    // Match the directive ANYWHERE in the line, not just at its start: it is
    // usually on its own line but can share one with the enclosing `location`
    // block, and a formatting difference must not turn a proxy into a
    // seemingly-importable site.
    let after = conf.lines().find(|l| l.contains("proxy_pass"))?.split("proxy_pass").nth(1)?;
    let to = after.split_whitespace().next()?.trim_end_matches(';');
    (!to.is_empty()).then(|| to.to_string())
}

/// Group discovered sites by the TLD they are served on — each needs its own
/// `/etc/resolver` file, and a conf can be on a TLD the config never mentions.
pub fn tlds_in_use(sites: &[DiscoveredSite]) -> Vec<String> {
    let mut set: BTreeMap<String, ()> = BTreeMap::new();
    for s in sites {
        if let Some((_, tld)) = s.domain.rsplit_once('.') {
            set.insert(tld.to_string(), ());
        }
    }
    set.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fixture environment. Real trees are messy; these tests encode
    /// exactly the mess found on the dev machine.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rexenv-valet-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for sub in ["Sites", "Nginx", "Certificates"] {
                std::fs::create_dir_all(dir.join(sub)).unwrap();
            }
            Self(dir)
        }
        fn config(self, json: &str) -> Self {
            std::fs::write(self.0.join("config.json"), json).unwrap();
            self
        }
        fn project(self, name: &str) -> Self {
            let p = self.0.join("projects").join(name);
            std::fs::create_dir_all(&p).unwrap();
            std::os::unix::fs::symlink(&p, self.0.join("Sites").join(name)).unwrap();
            self
        }
        fn dangling(self, name: &str, target: &str) -> Self {
            std::os::unix::fs::symlink(target, self.0.join("Sites").join(name)).unwrap();
            self
        }
        fn conf(self, fqdn: &str, body: &str) -> Self {
            std::fs::write(self.0.join("Nginx").join(fqdn), body).unwrap();
            self
        }
        fn cert(self, fqdn: &str) -> Self {
            std::fs::write(self.0.join("Certificates").join(format!("{fqdn}.crt")), "x").unwrap();
            self
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn isolation_marker_accepts_every_format_the_tools_write() {
        // All three occur on one real machine: Valet writes php@8.1 on isolate
        // but bare digits after secure/unsecure, and Herd writes 8.3.
        for (raw, want) in [
            ("# ISOLATED_PHP_VERSION=php@8.1\n", "8.1"),
            ("# ISOLATED_PHP_VERSION=8.4\n", "8.4"),
            ("# ISOLATED_PHP_VERSION=82\n", "8.2"),
            ("# ISOLATED_PHP_VERSION=7.4\n", "7.4"),
            ("server {\n}\n# ISOLATED_PHP_VERSION=83\n", "8.3"),
        ] {
            assert_eq!(parse_isolated_php(raw).as_deref(), Some(want), "{raw:?}");
        }
        assert_eq!(parse_isolated_php("server { }\n"), None, "no marker = their global PHP");
        assert_eq!(parse_isolated_php("# ISOLATED_PHP_VERSION=\n"), None);
    }

    #[test]
    fn config_tolerates_the_old_key_and_duplicate_parked_paths() {
        // Pre-2.1 Valet called it `domain`.
        let old = parse_config(r#"{"domain":"dev","paths":[]}"#).unwrap();
        assert_eq!(old.tld, "dev");
        // Herd lists the same folder twice, differing only by a trailing slash.
        let herd = parse_config(
            r#"{"tld":"test","loopback":"127.0.0.1","paths":["/a/Sites","/a/Sites/","/b"]}"#,
        )
        .unwrap();
        assert_eq!(herd.parked, vec!["/a/Sites", "/b"], "duplicate parked path is noise");
        // Junk doesn't panic; it just isn't a config.
        assert!(parse_config("not json at all").is_none());
        // Missing keys fall back rather than failing.
        let bare = parse_config("{}").unwrap();
        assert_eq!((bare.tld.as_str(), bare.loopback.as_str()), ("test", "127.0.0.1"));
    }

    /// Valet's `default` (catch-all) is the one part of their setup rexenv will
    /// NOT reproduce, so the scan must SAY so.
    ///
    /// Everything else in the migration list either imports or carries a reason
    /// it cannot. A catch-all that simply stops happening is the worst shape of
    /// all: after migrating, a hostname the user never created used to land on a
    /// working project and now resolves nowhere, with nothing anywhere
    /// connecting that to the move.
    #[test]
    fn the_catch_all_site_is_reported_because_it_cannot_be_imported() {
        let fx = Fixture::new("default-key").config(
            r#"{"tld":"test","loopback":"127.0.0.1","paths":[],"default":"/Users/dev/fallback"}"#,
        );
        let (source, _sites) = scan_source(SourceKind::Valet, &fx.0);
        let note = source
            .notes
            .iter()
            .find(|n| n.contains("/Users/dev/fallback"))
            .unwrap_or_else(|| panic!("the catch-all was not reported: {:?}", source.notes));
        assert!(
            note.contains("unmatched") && note.contains("test"),
            "the note must say WHAT stops working (unmatched hostnames on their TLD): {note}"
        );

        // Unset is the normal case and must stay silent — Valet writes an empty
        // string when the key is cleared, and a note about "no catch-all" would
        // be a warning about the ordinary state, which teaches people to skip
        // the notes.
        for cfg in [r#"{"tld":"test","paths":[]}"#, r#"{"tld":"test","paths":[],"default":""}"#] {
            let fx = Fixture::new("default-unset").config(cfg);
            let (source, _) = scan_source(SourceKind::Valet, &fx.0);
            assert!(
                !source.notes.iter().any(|n| n.contains("catch-all") || n.contains("unmatched")),
                "an unset catch-all produced a note: {:?}",
                source.notes
            );
        }
    }

    #[test]
    fn scan_surfaces_every_kind_of_mess_without_dropping_anything() {
        let fx = Fixture::new("mess").config(
            r#"{"tld":"test","loopback":"127.0.0.1","paths":["/nonexistent-parked"]}"#,
        );
        let fx = fx
            .project("good")
            .dangling("broken", "/definitely/not/here")
            .conf("good.test", "# ISOLATED_PHP_VERSION=8.2\nserver { }\n")
            .cert("good.test")
            .conf("orphan.test", "server { }\n")
            .conf("api.test", "# valet stub: proxy.valet.conf\nlocation / { proxy_pass http://127.0.0.1:3000; }\n");

        let (source, sites) = scan_source(SourceKind::Valet, &fx.0);
        assert_eq!(source.tld, "test");
        let by = |d: &str| sites.iter().find(|s| s.domain == d).unwrap_or_else(|| panic!("{d} missing"));

        // A healthy linked site: importable, its PHP read, its cert noticed.
        let good = by("good.test");
        assert_eq!(good.status, SiteStatus::Importable);
        assert_eq!(good.php_minor.as_deref(), Some("8.2"));
        assert!(good.secured);
        assert!(good.path.is_some());

        // A dangling symlink is a ROW naming the missing target — never dropped,
        // because the user can still see it in Valet.
        let broken = by("broken.test");
        assert!(broken.path.is_none());
        match &broken.status {
            SiteStatus::Unsupported(r) => assert!(r.contains("/definitely/not/here"), "{r}"),
            s => panic!("expected unsupported, got {s:?}"),
        }

        // A conf with no site behind it, and a proxy, are distinct reasons.
        match &by("orphan.test").status {
            SiteStatus::Unsupported(r) => assert!(r.contains("no site folder"), "{r}"),
            s => panic!("{s:?}"),
        }
        let proxy = by("api.test");
        assert_eq!(proxy.proxy_to.as_deref(), Some("http://127.0.0.1:3000"));
        match &proxy.status {
            SiteStatus::Unsupported(r) => assert!(r.contains("proxy"), "{r}"),
            s => panic!("{s:?}"),
        }

        // A parked path that doesn't exist is a note, not a failure.
        assert!(source.notes.iter().any(|n| n.contains("nonexistent-parked")), "{:?}", source.notes);
        assert_eq!(sites.len(), 4, "every discovered thing is a row");
    }

    #[test]
    fn a_conf_on_another_tld_still_contributes_its_tld() {
        // A real install carried a `.dev` conf inside a `.test`-TLD config.
        let fx = Fixture::new("tlds").config(r#"{"tld":"test","paths":[]}"#);
        let fx = fx.project("app").conf("legacy.dev", "server { }\n");
        let (_, sites) = scan_source(SourceKind::Valet, &fx.0);
        assert_eq!(tlds_in_use(&sites), vec!["dev".to_string(), "test".to_string()]);
    }

    #[test]
    fn herd_wins_a_duplicate_domain_and_valet_is_noted() {
        let valet = Fixture::new("dedupe-v").config(r#"{"tld":"test","paths":[]}"#).project("shared");
        let herd = Fixture::new("dedupe-h").config(r#"{"tld":"test","paths":[]}"#).project("shared");
        let (_, vsites) = scan_source(SourceKind::Valet, &valet.0);
        let (_, hsites) = scan_source(SourceKind::Herd, &herd.0);

        // `discover`'s fold, applied to the two fixture results.
        let mut merged: Vec<DiscoveredSite> = Vec::new();
        for mut s in hsites.into_iter().chain(vsites) {
            if let Some(e) = merged.iter_mut().find(|m| m.domain == s.domain) {
                e.also_in = Some(s.source);
                continue;
            }
            s.also_in = None;
            merged.push(s);
        }
        assert_eq!(merged.len(), 1, "one row per domain");
        assert_eq!(merged[0].source, SourceKind::Herd, "Herd's copy wins");
        assert_eq!(merged[0].also_in, Some(SourceKind::Valet));
    }
}
