//! core::localwp — READ-ONLY discovery of Local (WP Engine, formerly "Local by
//! Flywheel") WordPress sites: the third migration source beside Valet and Herd
//! (`core::valet`), under the same rules. Design record:
//! `docs/archive/PLAN-local-import.md`.
//!
//! **Nothing in this module writes anything, anywhere** (ledger #570). It reads
//! Local's own registry (`sites.json`, in Local's app data) and probes a
//! project only for EXISTENCE — no file inside a user's project is opened.
//! Local's servers are never started or stopped: a Local site's database runs
//! only while that site is started in Local, and that is the user's button.
//!
//! What differs from Valet, and why this is its own module:
//!
//! - Local keeps a REGISTRY, not a symlink farm — one JSON object keyed by site
//!   id with the folder, the domain, the pinned PHP and that site's own mysqld
//!   port. WordPress always lives in `<folder>/app/public`.
//! - Local's domains default to `.local`, which rexenv refuses (`core::tld`:
//!   shadowing it breaks Bonjour). A site on a refused TLD is RE-HOMED onto the
//!   default TLD HERE, before `enrich` or site creation sees the row, so an
//!   importable row can never carry a `.local` domain (ledger #571).
//! - Every Local site runs its own mysqld, and its wp-config says `localhost`
//!   (Local's socket for that site). [`db_source_for`] maps a docroot back to
//!   that server, so a database import never probes `localhost:3306` — which is
//!   nothing, or somebody's Homebrew MySQL holding its own `local` database.

use crate::core::valet::{DiscoveredSite, Origin, SiteStatus, Source, SourceKind};
use std::path::{Path, PathBuf};

/// Local's app-data directory — where `sites.json` and each site's `run/`
/// state live.
pub fn local_home(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Local")
}

/// Where the VM-based "Local by Flywheel" 2.x kept its data. Its sites live
/// inside a VirtualBox machine rather than on the Mac's filesystem, so there is
/// nothing to link — the scan only says so, rather than leaving someone who
/// still has it wondering why their sites are missing.
fn legacy_home(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Local by Flywheel")
}

/// One site as Local's registry describes it. Labels, not facts about what is
/// running: `site-statuses.json` is deliberately not read, for the reason
/// DBngin's plist taught (a file saying "started" about a server that isn't).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSite {
    pub id: String,
    pub name: String,
    /// The site folder with `~` expanded. `None` when the registry records none.
    pub root: Option<PathBuf>,
    /// Their hostname as Local serves it (`ea.local`), normalised. Empty when
    /// the registry has none.
    pub domain: String,
    /// The full PHP version Local pins (`7.3.5`).
    pub php_version: Option<String>,
    /// That site's own mysqld port.
    pub mysql_port: Option<u16>,
    pub mysql_version: Option<String>,
    /// `nginx` or `apache`.
    pub web_server: Option<String>,
    /// `ms-subdomain` / `ms-subdir`; `None` for a single site.
    pub multisite: Option<String>,
}

impl LocalSite {
    /// WordPress lives in `app/public` in every Local site.
    pub fn docroot(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join("app").join("public"))
    }
}

/// Expand Local's `~/Local Sites/x` form against `home`.
pub fn expand_tilde(p: &str, home: &Path) -> PathBuf {
    if p == "~" {
        home.to_path_buf()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(p)
    }
}

/// Parse `sites.json` leniently. `None` only when it isn't a JSON object at
/// all; a site with missing keys still becomes a [`LocalSite`] with `None`s,
/// because a site the user can see in Local must not vanish from the list.
///
/// Two registry generations are read: Local 6+ nests versions and ports under
/// `services.<name>` (`services.mysql.ports.MYSQL = [10003]`), while Local 5
/// kept flat `phpVersion` / `mysqlVersion` / `webServer` / `ports.MYSQL`.
pub fn parse_registry(text: &str, home: &Path) -> Option<Vec<LocalSite>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    let mut out: Vec<LocalSite> = obj
        .iter()
        .map(|(key, s)| {
            let str_at = |ptr: &str| {
                s.pointer(ptr)
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
            };
            let port_at = |ptr: &str| {
                let p = s.pointer(ptr)?;
                let n = p.as_u64().or_else(|| p.as_array()?.first()?.as_u64())?;
                u16::try_from(n).ok()
            };
            let services = s.get("services");
            let web_server = if services.and_then(|x| x.get("nginx")).is_some() {
                Some("nginx".to_string())
            } else if services.and_then(|x| x.get("apache")).is_some() {
                Some("apache".to_string())
            } else {
                str_at("/webServer")
            };
            LocalSite {
                id: str_at("/id").unwrap_or_else(|| key.clone()),
                name: str_at("/name").unwrap_or_else(|| key.clone()),
                root: str_at("/path").map(|p| expand_tilde(&p, home)),
                domain: str_at("/domain")
                    .map(|d| crate::core::sites::normalize_hostname(&d))
                    .unwrap_or_default(),
                php_version: str_at("/services/php/version").or_else(|| str_at("/phpVersion")),
                mysql_port: port_at("/services/mysql/ports/MYSQL").or_else(|| port_at("/ports/MYSQL")),
                mysql_version: str_at("/services/mysql/version").or_else(|| str_at("/mysqlVersion")),
                web_server,
                multisite: str_at("/multiSite"),
            }
        })
        .collect();
    out.sort_by(|a, b| (&a.domain, &a.id).cmp(&(&b.domain, &b.id)));
    Some(out)
}

/// The hostname rexenv serves a Local site under, and the name it had in Local
/// when that differs.
///
/// Only a TLD the POLICY refuses is re-homed (`ea.local` → `ea.<default_tld>`);
/// a name on an allowed TLD keeps its name exactly as a Valet row does, and a
/// name that is malformed in itself stays refused rather than being "fixed"
/// into something the user never chose. `Err` carries the reason for the row.
pub fn rehome_domain(domain: &str, default_tld: &str) -> Result<(String, Option<String>), String> {
    use crate::core::sites::{normalize_hostname, validate_domain};
    let d = normalize_hostname(domain);
    if d.is_empty() {
        return Err("Local's registry records no domain for this site".into());
    }
    if validate_domain(&d).is_ok() {
        return Ok((d, None));
    }
    let (head, tld) = d.rsplit_once('.').unwrap_or((d.as_str(), ""));
    if !tld.is_empty() && crate::core::tld::classify(tld).allowed {
        // The TLD is fine, so the NAME is what's wrong — say that.
        return Err(validate_domain(&d).err().map(|e| e.to_string()).unwrap_or_default());
    }
    let target = format!("{head}.{default_tld}");
    validate_domain(&target).map_err(|e| e.to_string())?;
    Ok((target, Some(d)))
}

/// Read Local on this machine, if it is here. `None` when neither the registry
/// nor the old VM-based Local exists — an absent tool is not a source.
pub fn discover(home: &Path, default_tld: &str) -> Option<(Source, Vec<DiscoveredSite>)> {
    let dir = local_home(home);
    let legacy = legacy_home(home).is_dir();
    if !dir.join("sites.json").is_file() && !legacy {
        return None;
    }
    Some(scan_registry(&dir, home, default_tld, legacy))
}

/// Read ONE registry rooted at `dir`. Split out so tests can build fixture
/// trees; [`discover`] is just this plus the presence check.
pub fn scan_registry(
    dir: &Path,
    home: &Path,
    default_tld: &str,
    legacy_present: bool,
) -> (Source, Vec<DiscoveredSite>) {
    let mut notes = Vec::new();
    if legacy_present {
        notes.push(
            "Local by Flywheel 2.x (the version that ran sites in a virtual machine) is also \
             on this machine — its sites live inside that VM, so there is no folder rexenv can \
             link. Sites from current Local are listed below."
                .to_string(),
        );
    }
    let registry = dir.join("sites.json");
    let sites = match std::fs::read_to_string(&registry) {
        Ok(text) => parse_registry(&text, home).unwrap_or_else(|| {
            notes.push("couldn't understand Local's sites.json — no sites read from it".into());
            Vec::new()
        }),
        Err(_) if !registry.exists() => Vec::new(),
        Err(e) => {
            notes.push(format!("couldn't read Local's sites.json ({e})"));
            Vec::new()
        }
    };

    let rows: Vec<DiscoveredSite> = sites.iter().map(|s| row(s, default_tld)).collect();
    let renamed = rows.iter().filter(|r| r.renamed_from.is_some()).count();
    if renamed > 0 {
        notes.push(format!(
            "rexenv can't serve .local — macOS uses it for Bonjour, so taking it over breaks \
             printers and AirDrop. {renamed} site{} will be served on .{default_tld} instead, \
             and each copied database has its URLs updated to match (rexenv's copy only — \
             Local's database is never written).",
            if renamed == 1 { "" } else { "s" }
        ));
    }

    let source = Source {
        kind: SourceKind::Local,
        home: dir.display().to_string(),
        tld: "local".into(),
        loopback: "127.0.0.1".into(),
        parked: Vec::new(),
        notes,
    };
    (source, rows)
}

/// One migration row for a registered site. Existence probes only.
fn row(s: &LocalSite, default_tld: &str) -> DiscoveredSite {
    let rehomed = rehome_domain(&s.domain, default_tld);
    let (domain, renamed_from) = match &rehomed {
        Ok((d, from)) => (d.clone(), from.clone()),
        Err(_) if s.domain.is_empty() => (s.name.clone(), None),
        Err(_) => (s.domain.clone(), None),
    };
    let docroot = s.docroot();
    let status = match (&s.root, &docroot) {
        (None, _) | (_, None) => {
            SiteStatus::Unsupported("Local's registry records no folder for this site".into())
        }
        (Some(root), _) if !root.is_dir() => {
            SiteStatus::Unsupported(format!("its folder is missing ({})", root.display()))
        }
        (Some(root), Some(doc)) if !doc.is_dir() => {
            SiteStatus::Unsupported(format!("there's no app/public folder in {}", root.display()))
        }
        (_, Some(doc)) if crate::core::phpconf::wp_config_path(doc).is_none() => {
            SiteStatus::Unsupported(format!(
                "there's no wp-config.php in {} — WordPress was never set up there",
                doc.display()
            ))
        }
        _ if s.multisite.is_some() => SiteStatus::Unsupported(format!(
            "a WordPress multisite network ({}) — importing networks from Local isn't supported \
             yet; single sites import normally",
            match s.multisite.as_deref() {
                Some("ms-subdomain") => "subdomains",
                Some("ms-subdir") => "subdirectories",
                other => other.unwrap_or("network"),
            }
        )),
        _ => match &rehomed {
            Ok(_) => SiteStatus::Importable,
            Err(why) => SiteStatus::Unsupported(why.clone()),
        },
    };
    DiscoveredSite {
        source: SourceKind::Local,
        name: s.name.clone(),
        domain,
        origin: Origin::Local { id: s.id.clone() },
        path: docroot.filter(|d| d.is_dir()).map(|d| d.display().to_string()),
        php_minor: s.php_version.as_deref().map(crate::core::php::minor_of),
        secured: false,
        proxy_to: None,
        also_in: None,
        status,
        renamed_from,
    }
}

/// Whether a site config's database host means "Local's socket for this site"
/// — the ONLY case in which the registry may replace what the config says
/// (ledger #572).
///
/// Local writes `define( 'DB_HOST', 'localhost' )`, and PHP reads a bare
/// `localhost` as the unix socket its php.ini names. Read literally that is
/// `localhost:3306`, which is not where the data is. Anything more specific —
/// an IP, another host, an explicit port — is a choice somebody made (a Local
/// site pointed at DBngin on purpose), and the config wins. A bare `localhost`
/// parses to the default port, so `localhost:3306` spelled out is
/// indistinguishable from it; that is the same server the socket form reaches
/// in every setup that has one there, so treating it the same is safe.
pub fn overrides_config(config_host: &str, config_port: u16) -> bool {
    config_host.trim().eq_ignore_ascii_case("localhost") && config_port == 3306
}

/// Whether a site's config reaches ONLY Local's own server — which rexenv
/// cannot reach — so that under rexenv, until the connection is rewritten, the
/// site reads NO database at all (ledger #575).
///
/// The reason it matters: a Valet or Herd import keeps reading its old server
/// (DBngin, Homebrew) until it is connected, and the interim copy says so. A
/// Local site's `localhost` lands on rexenv's own socket instead
/// (`mysqli.default_socket`), where Local's `root`/`root` is refused — so that
/// same sentence would be false, and the site shows WordPress's database error.
/// Measured 12 Sep 2026 on two real Local imports: both served 500s.
pub fn config_reaches_only_local(home: &Path, config_host: &str, config_port: u16, docroot: &Path) -> bool {
    overrides_config(config_host, config_port) && db_source_for(home, docroot).is_some()
}

/// A Local site's own database server, as the registry places it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDb {
    pub site_name: String,
    /// Their hostname (`ea.local`) — what the copy's URLs say.
    pub domain: String,
    /// TCP port — the fallback route when no socket file exists. Local's mysqld
    /// answers a TCP connect from 127.0.0.1 with ERR 1130 before its handshake,
    /// so with a socket present nothing is sent here at all.
    pub port: u16,
    /// The socket WordPress itself signs in through (Local's `[client]` group
    /// and generated php.ini both point there).
    pub socket: PathBuf,
    pub version: Option<String>,
}

/// The Local site whose `app/public` IS `docroot`, if any, with its database
/// server's address. Re-read from the registry on every call — nothing about
/// their setup is cached, exactly as credentials are re-read from wp-config.
///
/// Matching is by canonical path, so a docroot reached through a symlink still
/// matches. A registry id that isn't a plain token is refused rather than
/// joined onto a path: `sites.json` is someone else's file.
pub fn db_source_for(home: &Path, docroot: &Path) -> Option<LocalDb> {
    let dir = local_home(home);
    let text = std::fs::read_to_string(dir.join("sites.json")).ok()?;
    let want = canon(docroot);
    let site = parse_registry(&text, home)?
        .into_iter()
        .find(|s| s.docroot().is_some_and(|d| canon(&d) == want))?;
    if site.id.is_empty()
        || !site.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(LocalDb {
        port: site.mysql_port?,
        // Observed live 11 Sep 2026 (Local 10.1.2, a started site): the socket
        // is here, and exists only while that site runs.
        socket: dir.join("run").join(&site.id).join("mysql").join("mysqld.sock"),
        site_name: site.name,
        domain: site.domain,
        version: site.mysql_version,
    })
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A fixture HOME shaped like the real install this was built against:
    /// Local's app data with a `sites.json`, and `~/Local Sites/<name>/app/public`.
    struct Home(PathBuf);

    impl Home {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rexenv-localwp-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(local_home(&dir)).unwrap();
            Self(dir)
        }
        fn registry(self, json: &str) -> Self {
            std::fs::write(local_home(&self.0).join("sites.json"), json).unwrap();
            self
        }
        fn wordpress(self, name: &str) -> Self {
            let doc = self.0.join("Local Sites").join(name).join("app/public");
            std::fs::create_dir_all(&doc).unwrap();
            std::fs::write(doc.join("wp-config.php"), "<?php define( 'DB_HOST', 'localhost' );\n")
                .unwrap();
            self
        }
        fn bare_folder(self, name: &str) -> Self {
            std::fs::create_dir_all(self.0.join("Local Sites").join(name).join("app/public")).unwrap();
            self
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The real registry's shape (Local 6.3.1), values replaced.
    fn site_json(id: &str, name: &str, domain: &str, php: &str, port: u16, multi: &str) -> String {
        format!(
            r#""{id}": {{"id":"{id}","name":"{name}","path":"~/Local Sites/{name}","domain":"{domain}",
               "mysql":{{"database":"local","user":"root","password":"root"}},
               "services":{{"php":{{"version":"{php}","ports":{{"cgi":[10002]}}}},
                            "mysql":{{"name":"mysql","version":"8.0.35","ports":{{"MYSQL":[{port}]}}}},
                            "nginx":{{"version":"1.26.1","ports":{{"HTTP":[10004]}}}}}},
               "multiSite":"{multi}"}}"#
        )
    }

    fn fingerprint(dir: &Path) -> BTreeMap<String, (u64, Option<std::time::SystemTime>)> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let md = std::fs::symlink_metadata(e.path()).unwrap();
                if md.is_dir() {
                    stack.push(e.path());
                }
                out.insert(e.path().display().to_string(), (md.len(), md.modified().ok()));
            }
        }
        out
    }

    #[test]
    fn both_registry_generations_parse_and_junk_is_not_a_registry() {
        let home = Path::new("/Users/dev");
        let new = format!("{{{}}}", site_json("0Yh5N8r16", "ea", "EA.local.", "7.3.5", 10003, ""));
        let sites = parse_registry(&new, home).unwrap();
        assert_eq!(sites.len(), 1);
        let s = &sites[0];
        assert_eq!(s.root.as_deref(), Some(Path::new("/Users/dev/Local Sites/ea")), "~ expands");
        assert_eq!(s.domain, "ea.local", "normalised: case and trailing dot");
        assert_eq!(s.php_version.as_deref(), Some("7.3.5"));
        assert_eq!(s.mysql_port, Some(10003));
        assert_eq!(s.web_server.as_deref(), Some("nginx"));
        assert_eq!(s.multisite, None, "an empty multiSite is a single site");
        assert_eq!(s.docroot().unwrap(), Path::new("/Users/dev/Local Sites/ea/app/public"));

        // Local 5's flat keys, and a port that is a bare number.
        let old = r#"{"abc":{"name":"shop","path":"/Users/dev/Sites/shop","domain":"shop.test",
                      "phpVersion":"7.4.1","mysqlVersion":"5.7.28","webServer":"apache",
                      "ports":{"MYSQL":10011},"multiSite":"ms-subdir"}}"#;
        let s = &parse_registry(old, home).unwrap()[0];
        assert_eq!((s.id.as_str(), s.mysql_port, s.web_server.as_deref()), ("abc", Some(10011), Some("apache")));
        assert_eq!(s.multisite.as_deref(), Some("ms-subdir"));

        // A site missing everything still becomes a row-able entry, never a panic.
        let bare = parse_registry(r#"{"x":{}}"#, home).unwrap();
        assert_eq!((bare[0].name.as_str(), bare[0].root.as_ref()), ("x", None));

        assert!(parse_registry("not json", home).is_none());
        assert!(parse_registry("[1,2]", home).is_none(), "an array is not Local's registry");
    }

    /// `.local` is refused by policy, so it is re-homed onto the default TLD;
    /// an allowed TLD keeps its name; a malformed name is NOT "fixed".
    #[test]
    fn only_a_refused_tld_is_rehomed() {
        assert_eq!(rehome_domain("ea.local", "rex").unwrap(), ("ea.rex".into(), Some("ea.local".into())));
        assert_eq!(
            rehome_domain("sub.site.local", "test").unwrap(),
            ("sub.site.test".into(), Some("sub.site.local".into()))
        );
        assert_eq!(rehome_domain("ea", "rex").unwrap(), ("ea.rex".into(), Some("ea".into())));
        assert_eq!(rehome_domain("shop.test", "rex").unwrap(), ("shop.test".into(), None), "allowed TLD kept");
        let bad = rehome_domain("my_site.test", "rex").unwrap_err();
        assert!(bad.contains("my_site.test"), "the name is what's wrong, and the reason says so: {bad}");
        assert!(rehome_domain("my_site.local", "rex").is_err(), "re-homing never launders a bad label");
        assert!(rehome_domain("", "rex").is_err());
    }

    /// Every kind of registry entry is a row, the good one importable with
    /// WordPress's folder as its path — and no importable row carries a domain
    /// site creation would refuse (the `.local` guarantee, ledger #571).
    #[test]
    fn scan_surfaces_every_kind_of_site_and_rehomes_before_anyone_sees_it() {
        let registry = format!(
            "{{{},{},{},{}}}",
            site_json("aaa", "ea", "ea.local", "8.1.29", 10003, ""),
            site_json("bbb", "multi", "multi.local", "8.1.29", 10009, "ms-subdomain"),
            site_json("ccc", "gone", "gone.local", "8.2.1", 10013, ""),
            site_json("ddd", "empty", "empty.test", "8.2.1", 10017, ""),
        );
        let home = Home::new("scan").registry(&registry).wordpress("ea").wordpress("multi").bare_folder("empty");
        let before = fingerprint(&home.0);
        let (source, rows) = discover(&home.0, "rex").expect("a registry is a source");
        assert_eq!(before, fingerprint(&home.0), "the scan WROTE something (ledger #570)");

        assert_eq!(source.kind, SourceKind::Local);
        assert_eq!(rows.len(), 4, "every registered site is a row: {rows:?}");
        let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap_or_else(|| panic!("{n} missing"));

        let ea = by("ea");
        assert_eq!(ea.status, SiteStatus::Importable);
        assert_eq!((ea.domain.as_str(), ea.renamed_from.as_deref()), ("ea.rex", Some("ea.local")));
        assert!(ea.path.as_deref().unwrap().ends_with("Local Sites/ea/app/public"), "{:?}", ea.path);
        assert_eq!(ea.php_minor.as_deref(), Some("8.1"));
        assert_eq!(ea.origin, Origin::Local { id: "aaa".into() });

        let reason = |n: &str| match &by(n).status {
            SiteStatus::Unsupported(r) => r.clone(),
            s => panic!("{n}: expected unsupported, got {s:?}"),
        };
        assert!(reason("multi").contains("multisite"), "{}", reason("multi"));
        assert!(reason("gone").contains("folder is missing"), "{}", reason("gone"));
        assert!(reason("empty").contains("wp-config.php"), "{}", reason("empty"));

        for r in rows.iter().filter(|r| r.status == SiteStatus::Importable) {
            assert!(
                crate::core::sites::validate_domain(&r.domain).is_ok(),
                "an importable row carries a domain site creation refuses: {}",
                r.domain
            );
        }
        assert!(
            source.notes.iter().any(|n| n.contains(".local") && n.contains(".rex")),
            "re-homing is announced, not silent: {:?}",
            source.notes
        );
    }

    #[test]
    fn no_registry_is_no_source_and_a_broken_one_is_a_note() {
        let home = Home::new("absent");
        std::fs::remove_dir_all(local_home(&home.0)).unwrap();
        assert!(discover(&home.0, "rex").is_none(), "Local isn't here — not a source");

        let home = Home::new("junk").registry("{ this is not json");
        let (source, rows) = discover(&home.0, "rex").unwrap();
        assert!(rows.is_empty());
        assert!(source.notes.iter().any(|n| n.contains("couldn't understand")), "{:?}", source.notes);
    }

    /// Only the socket-meaning `localhost` lets the registry override the
    /// config; every explicit choice wins (ledger #572).
    #[test]
    fn only_a_bare_localhost_is_replaced_by_the_registry() {
        assert!(overrides_config("localhost", 3306));
        assert!(overrides_config(" LocalHost ", 3306));
        assert!(!overrides_config("127.0.0.1", 3306), "an IP is a TCP choice");
        assert!(!overrides_config("localhost", 10003), "an explicit port is a choice");
        assert!(!overrides_config("db.internal", 3306));
        assert!(!overrides_config("", 3306));
    }

    /// Only a registered Local docroot whose config says a bare `localhost`
    /// reaches nothing under rexenv (ledger #575) — an explicit host, or a
    /// folder Local doesn't know, is a server rexenv can still reach.
    #[test]
    fn only_a_local_sites_socket_config_reaches_nothing_under_rexenv() {
        let registry = format!("{{{}}}", site_json("aaa", "ea", "ea.local", "8.2.1", 10003, ""));
        let home = Home::new("reach").registry(&registry).wordpress("ea");
        let doc = home.0.join("Local Sites/ea/app/public");
        assert!(config_reaches_only_local(&home.0, "localhost", 3306, &doc));
        assert!(!config_reaches_only_local(&home.0, "127.0.0.1", 3306, &doc), "an explicit TCP host is reachable");
        assert!(!config_reaches_only_local(&home.0, "localhost", 3306, &home.0.join("elsewhere")), "not a Local site");
    }

    /// The database lookup matches a docroot to ITS Local site — through a
    /// symlink too — and refuses a registry id it would have to join onto a path.
    #[test]
    fn the_database_is_found_by_docroot_and_a_hostile_id_is_refused() {
        let registry = format!(
            "{{{},{}}}",
            site_json("0Yh5N8r16", "ea", "ea.local", "8.1.29", 10003, ""),
            site_json("../../etc", "evil", "evil.local", "8.1.29", 10005, ""),
        );
        let home = Home::new("db").registry(&registry).wordpress("ea").wordpress("evil");
        let doc = home.0.join("Local Sites/ea/app/public");

        let db = db_source_for(&home.0, &doc).expect("ea's docroot is a Local site");
        assert_eq!((db.port, db.domain.as_str(), db.site_name.as_str()), (10003, "ea.local", "ea"));
        assert_eq!(db.socket, local_home(&home.0).join("run/0Yh5N8r16/mysql/mysqld.sock"));

        let link = home.0.join("linked-docroot");
        std::os::unix::fs::symlink(&doc, &link).unwrap();
        assert_eq!(db_source_for(&home.0, &link).map(|d| d.port), Some(10003), "a symlinked docroot is the same site");

        assert!(db_source_for(&home.0, &home.0.join("Local Sites/ea")).is_none(), "the site ROOT isn't the docroot");
        assert!(db_source_for(&home.0, Path::new("/somewhere/else")).is_none());
        assert!(
            db_source_for(&home.0, &home.0.join("Local Sites/evil/app/public")).is_none(),
            "an id with path separators must never be joined onto run/"
        );
    }
}
