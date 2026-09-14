//! Windows' route for a TLD: an NRPT rule (W6 S4, ledger #618). The pure half — which rules are ours,
//! which TLDs another tool routes, the rexenv OPS that change them, and the PowerShell those ops become —
//! compiled into the macOS test build so it runs in `verify.sh`. `WindowsDns` reads the rules from the
//! registry and hands them here.
//!
//! **Ops, not scripts, cross the elevation boundary** (owner's ruling, 15 Sep 2026, ledger #619): what
//! `WindowsDns` hands `PrivilegeManager` is `nrpt-install test`, never PowerShell. The elevated
//! `rexenv.exe --elevated-step` parses the ops, refuses anything else or any invalid TLD label, and only
//! then builds the PowerShell itself — so no other process can use a "rexenv" UAC prompt to run a script
//! of its own.
//!
//! Measured on the Dell (15 Sep 2026, `scripts/probes/windows-nrpt.ps1`): a rule takes effect at once,
//! with no cache flush; each is a key under `DnsPolicyConfig` whose `Name` is a `REG_MULTI_SZ` of
//! namespaces, `GenericDNSServers` a `;`-joined `REG_SZ`, `Comment` and `DisplayName` `REG_SZ`; the desktop
//! user reads them without elevation. **One rule may name several namespaces** — so taking one TLD over
//! must not drop the others, and a rule that named several is put back into the same rule.

use crate::platform::traits::ResolverOwner;
use std::path::PathBuf;

/// The comment that marks a rule as rexenv's.
pub(crate) const OUR_COMMENT: &str = "rexenv";
/// The only server rexenv's rule names: its resolver on loopback (port 53 — NRPT has no port field).
pub(crate) const OUR_SERVER: &str = "127.0.0.1";

/// One NRPT rule as the registry holds it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NrptRule {
    /// The rule's key — a GUID like `{423F9788-…}`, what `Set-`/`Remove-DnsClientNrptRule -Name` take.
    pub key: String,
    pub namespaces: Vec<String>,
    pub servers: Vec<String>,
    pub comment: String,
    pub display_name: String,
    /// Delivered by Group Policy (the `Policies` key): never ours, never changed by rexenv.
    pub policy: bool,
}

fn namespace(tld: &str) -> String {
    format!(".{tld}")
}

fn names(rule: &NrptRule, tld: &str) -> bool {
    let ns = namespace(tld);
    rule.namespaces.iter().any(|n| n.eq_ignore_ascii_case(&ns))
}

/// rexenv's exact signature: a local rule with our comment, this one namespace, and our one server.
fn is_ours_for(rule: &NrptRule, tld: &str) -> bool {
    !rule.policy
        && rule.comment == OUR_COMMENT
        && rule.namespaces.len() == 1
        && names(rule, tld)
        && rule.servers.len() == 1
        && rule.servers[0] == OUR_SERVER
}

/// Who owns `tld`'s route among `rules`. Ours only when every rule naming the TLD is exactly ours; any
/// other rule naming it makes it Foreign, with those rules as the content a takeover backs up.
pub(crate) fn owner(rules: &[NrptRule], tld: &str) -> ResolverOwner {
    let naming: Vec<&NrptRule> = rules.iter().filter(|r| names(r, tld)).collect();
    if naming.is_empty() {
        return ResolverOwner::Absent;
    }
    let foreign: Vec<&NrptRule> = naming.iter().copied().filter(|r| !is_ours_for(r, tld)).collect();
    if foreign.is_empty() {
        return ResolverOwner::Ours;
    }
    ResolverOwner::Foreign { content: serde_json::to_string_pretty(&foreign).ok() }
}

fn label_of(namespace: &str) -> Option<String> {
    let label = namespace.strip_prefix('.')?.to_ascii_lowercase();
    crate::core::tld::is_valid_label(&label).then_some(label)
}

/// TLDs rexenv routes: one per rule of ours, valid labels only, sorted.
pub(crate) fn our_tlds(rules: &[NrptRule]) -> Vec<String> {
    let mut tlds: Vec<String> = rules
        .iter()
        .filter_map(|r| r.namespaces.first().and_then(|n| label_of(n)).filter(|t| is_ours_for(r, t)))
        .collect();
    tlds.sort();
    tlds.dedup();
    tlds
}

/// TLDs another rule routes — every valid-label namespace of a rule that is not ours, sorted.
pub(crate) fn foreign_tlds(rules: &[NrptRule]) -> Vec<String> {
    let mut tlds: Vec<String> = rules
        .iter()
        .filter(|r| !r.namespaces.first().and_then(|n| label_of(n)).is_some_and(|t| is_ours_for(r, &t)))
        .flat_map(|r| r.namespaces.iter().filter_map(|n| label_of(n)))
        .collect();
    tlds.sort();
    tlds.dedup();
    tlds
}

/// One privileged change rexenv makes to NRPT rules — what crosses the elevation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// Make each TLD's route ours (`install_script`).
    Install(String),
    /// Remove our rules for these TLDs (`uninstall_script`).
    Remove(Vec<String>),
    /// Put back the rules rexenv backed up for these TLDs, from rexenv's own backup directory.
    Restore(Vec<String>),
}

const INSTALL: &str = "nrpt-install";
const REMOVE: &str = "nrpt-remove";
const RESTORE: &str = "nrpt-restore";

pub(crate) fn install_op(tld: &str) -> String {
    format!("{INSTALL} {tld}")
}

pub(crate) fn remove_op(tlds: &[String]) -> String {
    std::iter::once(REMOVE.to_string()).chain(tlds.iter().cloned()).collect::<Vec<_>>().join(" ")
}

pub(crate) fn restore_op(tlds: &[String]) -> String {
    std::iter::once(RESTORE.to_string()).chain(tlds.iter().cloned()).collect::<Vec<_>>().join(" ")
}

/// Parse the ops the core handed `PrivilegeManager` — one per line, or joined with ` ; ` as the core joins
/// privileged steps. Every word after the verb must be a valid TLD label; anything else — another verb, a
/// shell word, an empty op — refuses the whole batch.
pub(crate) fn parse_ops(text: &str) -> Result<Vec<Op>, String> {
    let mut ops = Vec::new();
    for line in text.split(" ; ").flat_map(|chunk| chunk.lines()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some((verb, tlds)) = words.split_first() else { continue };
        // The verb first: a foreign command is named as one, not as a bad TLD (measured 15 Sep 2026 — the
        // Dell's check read "`-Recurse` is not a TLD rexenv routes" for `Remove-Item -Recurse …`).
        if ![INSTALL, REMOVE, RESTORE].contains(verb) {
            return Err(format!("`{line}` is not a privileged step rexenv can run on Windows"));
        }
        if let Some(bad) = tlds.iter().find(|t| !crate::core::tld::is_valid_label(t)) {
            return Err(format!("`{bad}` is not a TLD rexenv routes"));
        }
        let tlds: Vec<String> = tlds.iter().map(|t| t.to_string()).collect();
        match *verb {
            INSTALL if tlds.len() == 1 => ops.push(Op::Install(tlds[0].clone())),
            REMOVE => ops.push(Op::Remove(tlds)),
            RESTORE => ops.push(Op::Restore(tlds)),
            _ => return Err(format!("`{line}` is not a privileged step rexenv can run on Windows")),
        }
    }
    if ops.is_empty() {
        return Err("no privileged step to run".into());
    }
    Ok(ops)
}

/// The PowerShell for `ops`, restores reading rexenv's own backups under `backup_dir`.
pub(crate) fn script_for(ops: &[Op], backup_dir: &std::path::Path) -> String {
    let mut s = String::new();
    for op in ops {
        s.push_str(&match op {
            Op::Install(tld) => install_script(tld),
            Op::Remove(tlds) => uninstall_script(tlds),
            Op::Restore(tlds) => {
                restore_script(&tlds.iter().map(|t| (t.clone(), backup_dir.join(t))).collect::<Vec<_>>())
            }
        });
    }
    s
}

/// `s` as a PowerShell single-quoted string.
pub(crate) fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Make `tld`'s route ours: every local rule naming it loses that namespace (a rule left with none is
/// removed — the others it named keep routing), then rexenv's rule is added. `tld` is an `[a-z]{1,63}`
/// label by construction; it is quoted anyway.
pub(crate) fn install_script(tld: &str) -> String {
    let ns = ps_quote(&namespace(tld));
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $ns = {ns}\n\
         foreach ($r in @(Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains $ns }})) {{\n\
         \x20 $rest = @($r.Namespace | Where-Object {{ $_ -ne $ns }})\n\
         \x20 if ($rest.Count -eq 0) {{ Remove-DnsClientNrptRule -Name $r.Name -Force }} else {{ Set-DnsClientNrptRule -Name $r.Name -Namespace $rest }}\n\
         }}\n\
         Add-DnsClientNrptRule -Namespace $ns -NameServers {server} -Comment {comment} -DisplayName {display} | Out-Null\n",
        server = ps_quote(OUR_SERVER),
        comment = ps_quote(OUR_COMMENT),
        display = ps_quote(&format!("rexenv .{tld}")),
    )
}

/// Remove rexenv's rules for `tlds` — only rules carrying our signature's comment and naming exactly the
/// TLD; nobody else's rule is touched.
pub(crate) fn uninstall_script(tlds: &[String]) -> String {
    let mut s = String::from("$ErrorActionPreference = 'Stop'\n");
    for tld in tlds {
        s.push_str(&format!(
            "foreach ($r in @(Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq {comment} -and @($_.Namespace).Count -eq 1 -and $_.Namespace -contains {ns} }})) {{ Remove-DnsClientNrptRule -Name $r.Name -Force }}\n",
            comment = ps_quote(OUR_COMMENT),
            ns = ps_quote(&namespace(tld)),
        ));
    }
    s
}

/// Put borrowed routes back from our backups: rexenv's rule for the TLD goes, then each saved rule is
/// restored — into the SAME rule when it still exists (its namespaces set back, which returns the TLD a
/// shared rule had lost), or added again when it is gone. The backup is READ by the script
/// (`ConvertFrom-Json` on the file); its contents never enter the script text.
pub(crate) fn restore_script(restores: &[(String, PathBuf)]) -> String {
    let mut s = uninstall_script(&restores.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>());
    for (tld, backup) in restores {
        s.push_str(&format!(
            "foreach ($saved in @(Get-Content -Raw -LiteralPath {path} | ConvertFrom-Json)) {{\n\
             \x20 if (-not (@($saved.namespaces) -contains {ns})) {{ throw {refusal} }}\n\
             \x20 if (Get-DnsClientNrptRule | Where-Object {{ $_.Name -eq $saved.key }}) {{\n\
             \x20   Set-DnsClientNrptRule -Name $saved.key -Namespace @($saved.namespaces)\n\
             \x20 }} else {{\n\
             \x20   $add = @{{ Namespace = @($saved.namespaces); NameServers = @($saved.servers) }}\n\
             \x20   if ($saved.comment) {{ $add.Comment = $saved.comment }}\n\
             \x20   if ($saved.display_name) {{ $add.DisplayName = $saved.display_name }}\n\
             \x20   Add-DnsClientNrptRule @add | Out-Null\n\
             \x20 }}\n\
             }}\n",
            path = ps_quote(&backup.display().to_string()),
            ns = ps_quote(&namespace(tld)),
            refusal = ps_quote(&format!("the backup for .{tld} does not route .{tld}; rexenv will not restore it")),
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(key: &str, namespaces: &[&str], servers: &[&str], comment: &str, policy: bool) -> NrptRule {
        NrptRule {
            key: key.into(),
            namespaces: namespaces.iter().map(|s| s.to_string()).collect(),
            servers: servers.iter().map(|s| s.to_string()).collect(),
            comment: comment.into(),
            display_name: String::new(),
            policy,
        }
    }

    /// The Dell's shapes: ours on `.rex`, another tool's on `.test` + `.example` with two servers.
    #[test]
    fn a_rule_is_ours_only_by_the_exact_signature_and_any_other_rule_makes_the_tld_foreign() {
        let ours = rule("{A}", &[".rex"], &["127.0.0.1"], "rexenv", false);
        let theirs = rule("{B}", &[".test", ".example"], &["127.0.0.1", "::1"], "herd", false);
        let rules = vec![ours.clone(), theirs.clone()];
        assert_eq!(owner(&rules, "rex"), ResolverOwner::Ours);
        assert_eq!(owner(&rules, "dev"), ResolverOwner::Absent);
        match owner(&rules, "test") {
            ResolverOwner::Foreign { content: Some(c) } => {
                let saved: Vec<NrptRule> = serde_json::from_str(&c).expect("the backup parses");
                assert_eq!(saved, vec![theirs.clone()], "the whole rule is what a takeover backs up");
            }
            other => panic!("expected foreign with content, got {other:?}"),
        }
        // Case never makes a rule foreign or ours by accident.
        assert_eq!(owner(&[rule("{A}", &[".REX"], &["127.0.0.1"], "rexenv", false)], "rex"), ResolverOwner::Ours);
        // Near-misses are foreign: another comment, a second server, a second namespace, a policy rule.
        for near in [
            rule("{C}", &[".rex"], &["127.0.0.1"], "valet", false),
            rule("{C}", &[".rex"], &["127.0.0.1", "::1"], "rexenv", false),
            rule("{C}", &[".rex", ".dev"], &["127.0.0.1"], "rexenv", false),
            rule("{C}", &[".rex"], &["127.0.0.1"], "rexenv", true),
        ] {
            assert!(matches!(owner(std::slice::from_ref(&near), "rex"), ResolverOwner::Foreign { .. }), "{near:?}");
        }
        // Ours AND another rule on the same TLD: foreign — somebody else routes it too.
        assert!(matches!(owner(&[ours, rule("{D}", &[".rex"], &["10.0.0.1"], "", false)], "rex"), ResolverOwner::Foreign { .. }));
    }

    #[test]
    fn the_tld_lists_split_ours_from_theirs_and_keep_only_valid_labels() {
        let rules = vec![
            rule("{A}", &[".rex"], &["127.0.0.1"], "rexenv", false),
            rule("{B}", &[".test", ".example", "host.corp.example", ".Bad1"], &["127.0.0.1"], "herd", false),
            rule("{C}", &[".dev"], &["127.0.0.1"], "rexenv", true),
        ];
        assert_eq!(our_tlds(&rules), vec!["rex"]);
        assert_eq!(foreign_tlds(&rules), vec!["dev", "example", "test"], "an FQDN and an invalid label are dropped");
    }

    /// Taking `.test` from a rule that also names `.example` removes only `.test` from it; a rule naming
    /// only `.test` is removed; then ours is added — and every value is quoted.
    #[test]
    fn install_takes_only_its_namespace_from_a_shared_rule() {
        let s = install_script("test");
        assert!(s.starts_with("$ErrorActionPreference = 'Stop'"), "{s}");
        assert!(s.contains("$ns = '.test'"));
        assert!(s.contains("$rest = @($r.Namespace | Where-Object { $_ -ne $ns })"));
        assert!(s.contains("Set-DnsClientNrptRule -Name $r.Name -Namespace $rest"));
        assert!(s.contains("Remove-DnsClientNrptRule -Name $r.Name -Force"));
        assert!(s.contains("Add-DnsClientNrptRule -Namespace $ns -NameServers '127.0.0.1' -Comment 'rexenv' -DisplayName 'rexenv .test'"));
    }

    #[test]
    fn uninstall_removes_only_rules_with_our_comment_and_one_namespace() {
        let s = uninstall_script(&["rex".into(), "test".into()]);
        assert_eq!(s.matches("Remove-DnsClientNrptRule").count(), 2);
        assert!(s.contains("$_.Comment -eq 'rexenv' -and @($_.Namespace).Count -eq 1 -and $_.Namespace -contains '.rex'"));
        assert!(s.contains("-contains '.test'"));
        assert!(!s.contains("Set-DnsClientNrptRule"), "uninstall never edits another rule");
    }

    /// The backup is read by the script, never pasted into it — a path with an apostrophe stays one
    /// quoted string; the saved rule goes back into the same rule when it still exists.
    #[test]
    fn restore_reads_the_backup_file_and_returns_a_namespace_to_the_rule_it_left() {
        let s = restore_script(&[("test".into(), PathBuf::from(r"C:\Users\O'Neil\rexenv\resolver-backups\test"))]);
        assert!(s.contains(r"Get-Content -Raw -LiteralPath 'C:\Users\O''Neil\rexenv\resolver-backups\test' | ConvertFrom-Json"), "{s}");
        assert!(s.contains("Set-DnsClientNrptRule -Name $saved.key -Namespace @($saved.namespaces)"));
        assert!(s.contains("Add-DnsClientNrptRule @add"));
        assert!(s.contains("-contains '.test'"), "ours is removed first");
        assert!(!s.contains("herd"), "no backup content in the script");
        // A backup that does not route the TLD it is filed under is refused, not restored.
        assert!(s.contains("if (-not (@($saved.namespaces) -contains '.test')) { throw 'the backup for .test does not route .test; rexenv will not restore it' }"));
    }

    /// Ledger #619 — only rexenv's own ops parse, joined as the core joins privileged steps; any other
    /// verb, a shell word in a TLD's place, or nothing at all refuses the whole batch.
    #[test]
    fn only_rexenv_ops_with_valid_tlds_parse() {
        let joined = [install_op("test"), remove_op(&["rex".into(), "dev".into()]), restore_op(&["test".into()])].join(" ; ");
        assert_eq!(
            parse_ops(&joined).unwrap(),
            vec![Op::Install("test".into()), Op::Remove(vec!["rex".into(), "dev".into()]), Op::Restore(vec!["test".into()])]
        );
        let foreign = parse_ops("Remove-Item -Recurse C:\\rexenv").unwrap_err();
        assert!(foreign.contains("not a privileged step"), "a foreign command is named as one: {foreign}");
        for bad in [
            "Remove-Item -Recurse C:\\Windows",
            "nrpt-install test; calc",
            "nrpt-install TEST",
            "nrpt-install te$t",
            "nrpt-install a b",
            "nrpt-install",
            "nrpt-remove ../x",
            "",
        ] {
            assert!(parse_ops(bad).is_err(), "{bad:?} parsed");
        }
        let dir = std::path::Path::new("rexenv backups");
        let script = script_for(&parse_ops(&restore_op(&["test".into()])).unwrap(), dir);
        assert!(script.contains(&format!("-LiteralPath {}", ps_quote(&dir.join("test").display().to_string()))), "{script}");
    }
}
