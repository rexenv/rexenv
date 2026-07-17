//! Git-repo sources for "add plugin/theme from Git": parse every common way a
//! developer pastes a repository — https, ssh://, scp-like `git@host:path`,
//! `owner/repo` shorthand, and forge web URLs with a `/tree/<branch>` suffix —
//! into one normalized clone URL + derived target folder name.
//!
//! PURE string work: no network, no fs. The ls-remote probe and the clone job
//! build on these (later phases). Anything that reaches `git` argv comes out
//! of here validated (M7 class — pasted text becomes a path + argv element).

use crate::error::{Error, Result};

/// A parsed, normalized repository source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSource {
    /// Normalized URL handed to `git ls-remote`/`git clone` verbatim.
    pub url: String,
    /// Host, lowercased (display + the shorthand disclosure line).
    pub host: String,
    /// Default target folder name: last path segment minus `.git`.
    pub dir_name: String,
    /// Branch/tag CANDIDATE extracted from a pasted forge web URL
    /// (`github.com/o/r/tree/<ref>`, GitLab `/-/tree/<ref>`). Branch names may
    /// contain slashes, so a `/tree/feat/x/docs` tail is ambiguous — the UI
    /// matches this against the probe's real refs and silently drops a miss.
    pub ref_candidate: Option<String>,
}

fn other(msg: impl Into<String>) -> Error {
    Error::Other(msg.into())
}

/// Parse user input into a [`RepoSource`]. Errors are user-facing.
pub fn parse_source(raw: &str) -> Result<RepoSource> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(other(
            "Paste a git repository URL — https://host/owner/repo, \
             git@host:owner/repo.git, or owner/repo (GitHub).",
        ));
    }
    if s.chars().any(char::is_whitespace) {
        return Err(other("That doesn't look like a git URL (it contains spaces)."));
    }
    // npm-style `git+https://…` prefix — devs paste these from package.json.
    let s = s.strip_prefix("git+").unwrap_or(s);
    if s.starts_with("git://") {
        return Err(other(
            "The git:// protocol is unencrypted and no longer served by major \
             forges — paste the https:// or git@ form of this URL.",
        ));
    }
    if is_archive(s) {
        return Err(other(
            "That's an archive, not a repository. Paste the repository URL — \
             wp.org plugins install from the search tab.",
        ));
    }
    if let Some((scheme, rest)) = s.split_once("://") {
        return match scheme {
            "https" | "http" => parse_http_url(scheme, rest),
            "ssh" => parse_ssh_url(rest),
            _ => Err(other(format!(
                "Unsupported scheme \"{scheme}://\" — use https://, ssh://, or \
                 the git@host:path form."
            ))),
        };
    }
    if let Some(src) = parse_scp_like(s)? {
        return Ok(src);
    }
    if let Some(src) = parse_shorthand(s)? {
        return Ok(src);
    }
    Err(other(
        "Unrecognized repository reference. Accepted forms: \
         https://host/owner/repo, git@host:owner/repo.git, \
         ssh://git@host/owner/repo, or owner/repo (GitHub).",
    ))
}

fn is_archive(s: &str) -> bool {
    let path = s.split(['#', '?']).next().unwrap_or(s);
    [".zip", ".tar.gz", ".tgz", ".tar.bz2", ".tar.xz"]
        .iter()
        .any(|ext| path.to_ascii_lowercase().ends_with(ext))
}

/// `https://host/…` (any forge, self-hosted included). GitHub/GitLab web-UI
/// suffixes are understood: `/tree/<ref>` becomes the ref candidate; other
/// routes (`/blob/…`, `/pull/…`, GitLab `/-/…`) truncate to the repository.
fn parse_http_url(scheme: &str, rest: &str) -> Result<RepoSource> {
    // A pasted web URL often carries `#readme` or GitLab's `?ref_type=heads`.
    let rest = rest.split(['#', '?']).next().unwrap_or(rest);
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| other("That URL has no repository path (expected host/owner/repo)."))?;
    let host = host.to_ascii_lowercase();
    if host.is_empty() {
        return Err(other("That URL has no host."));
    }
    let mut segs: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut candidate = None;
    if host == "github.com" {
        if segs.len() < 2 {
            return Err(other("A GitHub URL needs owner/repo."));
        }
        if segs.len() > 2 {
            if segs[2] == "tree" && segs.len() > 3 {
                candidate = Some(segs[3..].join("/"));
            }
            segs.truncate(2); // /blob/…, /pull/…, /releases/… → the repo
        }
    } else if let Some(pos) = segs.iter().position(|p| *p == "-") {
        // GitLab route marker: /group[/subgroup]/repo/-/tree/<ref>[/…]
        if segs.get(pos + 1) == Some(&"tree") && segs.len() > pos + 2 {
            candidate = Some(segs[pos + 2..].join("/"));
        }
        segs.truncate(pos);
    }
    let last = segs
        .last()
        .ok_or_else(|| other("That URL has no repository path (expected host/owner/repo)."))?;
    let dir_name = derive_dir_name(last)?;
    Ok(RepoSource {
        url: format!("{scheme}://{host}/{}", segs.join("/")),
        host,
        dir_name,
        ref_candidate: candidate,
    })
}

/// `ssh://git@host[:port]/path/repo.git`.
fn parse_ssh_url(rest: &str) -> Result<RepoSource> {
    let rest = rest.split(['#', '?']).next().unwrap_or(rest).trim_end_matches('/');
    let (userhost, path) = rest
        .split_once('/')
        .ok_or_else(|| other("An ssh:// URL needs a path (ssh://git@host/owner/repo)."))?;
    let host = userhost
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(userhost)
        .split(':') // strip an explicit port for display
        .next()
        .unwrap_or(userhost)
        .to_ascii_lowercase();
    let last = path
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| other("An ssh:// URL needs a repository name."))?;
    Ok(RepoSource {
        url: format!("ssh://{userhost}/{path}"),
        host,
        dir_name: derive_dir_name(last)?,
        ref_candidate: None,
    })
}

/// scp-like `user@host:path` (the form forges label "SSH"). Returns Ok(None)
/// when the shape doesn't apply so parsing can fall through.
fn parse_scp_like(s: &str) -> Result<Option<RepoSource>> {
    let Some((userhost, path)) = s.split_once(':') else {
        return Ok(None);
    };
    let Some((user, host)) = userhost.split_once('@') else {
        return Ok(None);
    };
    if user.is_empty()
        || host.is_empty()
        || path.is_empty()
        || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Ok(None);
    }
    let path = path.trim_end_matches('/');
    let last = path
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| other("That git@ URL needs a repository name after the colon."))?;
    Ok(Some(RepoSource {
        url: format!("{user}@{}:{path}", host.to_ascii_lowercase()),
        host: host.to_ascii_lowercase(),
        dir_name: derive_dir_name(last)?,
        ref_candidate: None,
    }))
}

/// `owner/repo` shorthand → GitHub over https. The owner side must be
/// dot-free so `example.com/foo` never false-positives as shorthand.
fn parse_shorthand(s: &str) -> Result<Option<RepoSource>> {
    let Some((owner, repo)) = s.split_once('/') else {
        return Ok(None);
    };
    let owner_ok = !owner.is_empty()
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !repo.is_empty()
        && !repo.contains('/')
        && repo.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !owner_ok || !repo_ok {
        return Ok(None);
    }
    Ok(Some(RepoSource {
        url: format!("https://github.com/{owner}/{repo}.git"),
        host: "github.com".into(),
        dir_name: derive_dir_name(repo)?,
        ref_candidate: None,
    }))
}

/// Target folder name from the repo's last path segment, `.git` stripped.
/// Becomes a directory under wp-content — validated here, once, before it can
/// touch a path (M7 class). Leading dots are refused outright: WordPress
/// ignores hidden plugin/theme folders, and the vhost templates 404 them.
fn derive_dir_name(seg: &str) -> Result<String> {
    let name = seg.strip_suffix(".git").unwrap_or(seg);
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(other(format!(
            "Can't use \"{seg}\" as a folder name under wp-content — it must \
             start with a letter or digit and contain only letters, digits, \
             dots, dashes, or underscores."
        )));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> RepoSource {
        parse_source(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn https_forms_normalize_and_derive_the_folder() {
        for input in [
            "https://github.com/acme/my-plugin",
            "https://github.com/acme/my-plugin.git",
            "https://github.com/acme/my-plugin/",
            "  https://github.com/acme/my-plugin  ",
            "git+https://github.com/acme/my-plugin.git",
        ] {
            let src = parse(input);
            assert_eq!(src.host, "github.com", "{input}");
            assert_eq!(src.dir_name, "my-plugin", "{input}");
            assert!(src.url.starts_with("https://github.com/acme/my-plugin"), "{input}");
            assert_eq!(src.ref_candidate, None, "{input}");
        }
        // Host case folds; self-hosted https passes through untouched.
        assert_eq!(parse("https://GitHub.com/a/b").host, "github.com");
        let corp = parse("http://git.corp.internal/team/widget.git");
        assert_eq!(corp.url, "http://git.corp.internal/team/widget.git");
        assert_eq!(corp.dir_name, "widget");
    }

    #[test]
    fn github_web_suffixes_truncate_and_tree_preselects_the_ref() {
        let tree = parse("https://github.com/acme/my-plugin/tree/develop");
        assert_eq!(tree.url, "https://github.com/acme/my-plugin");
        assert_eq!(tree.ref_candidate.as_deref(), Some("develop"));
        // Slashed branch names arrive joined — a candidate, not a promise.
        let feat = parse("https://github.com/acme/my-plugin/tree/feat/fast-build");
        assert_eq!(feat.ref_candidate.as_deref(), Some("feat/fast-build"));
        // Fragment from a copied web URL is noise.
        assert_eq!(parse("https://github.com/acme/p#readme").url, "https://github.com/acme/p");
        // Other routes: repo only, no candidate.
        for input in [
            "https://github.com/acme/my-plugin/pull/12",
            "https://github.com/acme/my-plugin/blob/main/src/index.js",
            "https://github.com/acme/my-plugin/releases/tag/v1.2.0",
        ] {
            let src = parse(input);
            assert_eq!(src.url, "https://github.com/acme/my-plugin", "{input}");
            assert_eq!(src.ref_candidate, None, "{input}");
        }
    }

    #[test]
    fn gitlab_route_marker_handles_subgroups_and_query_noise() {
        let src = parse("https://gitlab.com/group/sub/widget/-/tree/main?ref_type=heads");
        assert_eq!(src.url, "https://gitlab.com/group/sub/widget");
        assert_eq!(src.dir_name, "widget");
        assert_eq!(src.ref_candidate.as_deref(), Some("main"));
        // Non-tree GitLab routes truncate to the repo.
        let blob = parse("https://gitlab.com/g/widget/-/blob/main/readme.md");
        assert_eq!(blob.url, "https://gitlab.com/g/widget");
        assert_eq!(blob.ref_candidate, None);
    }

    #[test]
    fn ssh_and_scp_forms_pass_through_for_private_repos() {
        let scp = parse("git@github.com:acme/my-plugin.git");
        assert_eq!(scp.url, "git@github.com:acme/my-plugin.git");
        assert_eq!(scp.host, "github.com");
        assert_eq!(scp.dir_name, "my-plugin");
        // Absolute path on a bare server is valid scp syntax.
        assert_eq!(parse("git@build.corp:/srv/git/tool.git").dir_name, "tool");
        let ssh = parse("ssh://git@gitlab.com/group/widget.git");
        assert_eq!(ssh.url, "ssh://git@gitlab.com/group/widget.git");
        assert_eq!(ssh.host, "gitlab.com");
        assert_eq!(ssh.dir_name, "widget");
        // Explicit port stays in the clone URL, not the display host.
        let port = parse("ssh://git@git.corp:2222/team/widget.git");
        assert_eq!(port.url, "ssh://git@git.corp:2222/team/widget.git");
        assert_eq!(port.host, "git.corp");
    }

    #[test]
    fn shorthand_is_github_but_never_swallows_a_domain() {
        let src = parse("acme/my-plugin");
        assert_eq!(src.url, "https://github.com/acme/my-plugin.git");
        assert_eq!(src.host, "github.com");
        // A dotted left side is a host someone forgot the scheme for — refuse
        // rather than silently cloning from github.com/example.com/foo.
        assert!(parse_source("example.com/foo").is_err());
    }

    #[test]
    fn rejects_are_specific() {
        for (input, needle) in [
            ("", "Paste a git repository URL"),
            ("not a url at all", "contains spaces"),
            ("git://github.com/a/b.git", "git://"),
            ("https://github.com/a/b/archive/main.zip", "archive"),
            ("https://example.com/repo.tar.gz", "archive"),
            ("ftp://host/a/b", "Unsupported scheme"),
            ("just-a-word", "Unrecognized"),
            ("https://github.com/onlyowner", "owner/repo"),
        ] {
            let err = parse_source(input).unwrap_err().to_string();
            assert!(err.contains(needle), "{input}: {err}");
        }
        // Dot-leading repo name: hidden under wp-content AND 404'd by the
        // vhost dotfile guard — refused with the reason.
        assert!(parse_source("https://github.com/acme/.dotplugin")
            .unwrap_err()
            .to_string()
            .contains("folder name"));
    }
}
