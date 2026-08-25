//! The scratch-mail tell — the stamp that lets rexenv tell an agent's own mail
//! apart from the user's (MCP M2b, PLAN §3.5 / D4).
//!
//! # Why a stamp exists at all
//!
//! Mailpit's inbox is GLOBAL — one inbox for every site rexenv serves, with no
//! per-site tagging. So "show the agent its own mail" needs a fact that says
//! which site a message came from, and the plan's first draft assumed the
//! recipient address carried it. **It does not**: WordPress mails the *admin*
//! address, and Laravel's default `MAIL_FROM` is `hello@example.com` — neither
//! mentions the site. Filtering on those would be a guard checked against a
//! field that doesn't hold the fact.
//!
//! So the tell is one rexenv CREATES: a mu-plugin, installed only in scratch
//! sites, that forces `From` to an address derived from that site's own domain.
//!
//! # Fail-closed, and which way that fails
//!
//! A message without the stamp is invisible to the agent. That direction is the
//! whole point: the failure mode is *the agent misses its own mail*, never *the
//! agent reads the user's*. A scratch site's own code can filter `wp_mail_from`
//! after us and win — WordPress runs the last filter added — in which case that
//! site's mail simply stops being visible. Still the safe direction, and it is
//! why the tool's reply and the toggle's copy both say so rather than leaving a
//! user to infer it from silence.
//!
//! # One definition of the stamp
//!
//! [`stamp_for`] is used to RENDER the mu-plugin and to MATCH in the mail
//! filter. That is deliberate and it is the whole reason this function exists
//! rather than two format strings: a writer and a reader that each spell the
//! address themselves agree on the day they are written and drift afterwards,
//! and the drift is silent — the agent just stops seeing its mail, which is
//! indistinguishable from the fail-closed case above.
//!
//! # What it is written INTO
//!
//! `content_rel`, the site's RECORDED content dir (v24) — never a hardcoded
//! `wp-content`. On a Bedrock scratch site the content dir is `app/`, and a stamp
//! written to a dead `wp-content/` would never load: the agent would see no mail,
//! with nothing anywhere saying why. Same defect `package_dest` avoids (#217).

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Placeholder the stamp address is baked into.
const FROM_PLACEHOLDER: &str = "__REXENV_SCRATCH_FROM__";

/// The auto-managed mu-plugin.
///
/// It sets `From` and nothing else. A stamp that also touched `To`, the subject
/// or the body would be changing the mail the developer is trying to test, and
/// the point is to make their mail findable, not different.
///
/// `PHP_INT_MAX` priority so a site's own `wp_mail_from` filter, added later,
/// still wins — which is the fail-closed direction, deliberately not fought.
const MU_PLUGIN_TEMPLATE: &str = r#"<?php
/* Plugin Name: rexenv scratch mail tag
 * Description: Auto-managed by rexenv. Stamps this disposable site's own address on outgoing mail so an AI agent can find its own messages — and only its own. Safe to delete.
 */
add_filter('wp_mail_from', static function ($from) {
    $stamp = '__REXENV_SCRATCH_FROM__';
    return $stamp === '' ? $from : $stamp;
}, PHP_INT_MAX);
"#;

/// The address rexenv stamps on a scratch site's outgoing mail.
///
/// **THE definition — used to render the plugin AND to match in the filter.**
/// See the module doc: two copies agree on the day they are written.
pub fn stamp_for(domain: &str) -> String {
    format!("rexenv-scratch@{domain}")
}

/// Where the stamp lives inside a site.
fn mu_plugin_path(docroot: &Path, content_rel: &str) -> PathBuf {
    docroot.join(content_rel).join("mu-plugins").join("rexenv-scratch-mail.php")
}

/// The domain is baked into single-quoted PHP source, so reject anything beyond
/// a plain host before it gets there — defense in depth behind
/// `sites::validate_domain`, exactly as `wp_tunnel::validate_origin` is.
fn validate_domain(domain: &str) -> Result<()> {
    if domain.is_empty()
        || !domain
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(Error::Other(format!(
            "a scratch domain may contain only a–z, 0–9, '.', '-': {domain}"
        )));
    }
    Ok(())
}

/// Install (or refresh) the stamp. Returns whether it CREATED the `mu-plugins`
/// directory, so the caller can record ownership (v25 `mu_dir_created`) —
/// teardown removes a dir we made, never one inferred from emptiness.
///
/// Idempotent: identical content is not rewritten.
pub fn enable(docroot: &Path, content_rel: &str, domain: &str) -> Result<bool> {
    validate_domain(domain)?;
    let rendered = MU_PLUGIN_TEMPLATE.replace(FROM_PLACEHOLDER, &stamp_for(domain));
    let path = mu_plugin_path(docroot, content_rel);
    let mut created_dir = false;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&rendered) {
        if let Some(parent) = path.parent() {
            created_dir = !parent.exists();
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, rendered)?;
    }
    Ok(created_dir)
}

/// Remove the stamp. Sweeps EVERY known content-dir layout rather than taking
/// one, for `wp_tunnel::disable`'s reason: a removal caller may hold only a
/// recorded docroot, and deleting a filename that is exactly ours from a layout
/// dir that never had it changes nothing.
pub fn disable(docroot: &Path) -> Result<()> {
    for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
        match std::fs::remove_file(mu_plugin_path(docroot, layout)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Is the stamp installed for this site right now?
///
/// A STAT, not an inference — which is what lets `mail_list` tell "no mail from
/// this site" apart from "rexenv can't stamp this site's mail". Those send an
/// agent to different next steps, and guessing between them from an empty result
/// is how a tool teaches an agent something false.
pub fn is_installed(docroot: &Path, content_rel: &str) -> bool {
    mu_plugin_path(docroot, content_rel).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Both ways a scratch site can come to exist must stamp it.**
    ///
    /// The stamp has exactly two moments: the toggle flipping ON (which
    /// retro-stamps every existing scratch site) and a site being CREATED while
    /// it is already on. Only the first was implemented. Its own doc claimed the
    /// retro-stamp "eliminates 'this site predates the feature' as a category" —
    /// true, and it replaced that category with a strictly worse one, because a
    /// site created after the toggle is EVERY new scratch site on a machine
    /// with mail enabled.
    ///
    /// The symptom was a refusal that blamed the user for the opposite of what
    /// happened: `mail_list` said "this normally means mail was switched on
    /// after this site was made" about a site made after mail was switched on.
    /// Found by running SMOKE §M2a step 13 against the packaged app, 25 Aug
    /// 2026 — no unit test could see it, because each half was correct alone.
    ///
    /// A source guard: the property is "these two call sites both exist", and
    /// the thing that went wrong was one of them never being written.
    #[test]
    fn a_scratch_site_is_stamped_both_when_the_toggle_flips_and_when_it_is_created() {
        let toggle = include_str!("../commands/mcp.rs");
        let create = include_str!("../mcp_server/scratch.rs");
        assert!(
            toggle.contains("wp_mailtag::enable"),
            "the toggle stopped retro-stamping existing scratch sites — sites made BEFORE mail \
             was switched on become permanently unreadable to the agent"
        );
        // The CALL, not the definition. The first version of this assertion
        // looked for the bare name and a plant that deleted the call while
        // leaving the helper behind PASSED — a guard that proves a function
        // exists rather than that anything invokes it. Dead code satisfies the
        // weak form; only a call site satisfies this one.
        assert!(
            create.contains("ctx.stamp_mail_if_enabled(&site)"),
            "scratch creation stopped stamping — every site made WHILE mail is on becomes \
             unreadable, and `mail_list` blames the user for the opposite of what happened"
        );
        // Both must also record a mu-dir they created, or teardown leaves it.
        for (what, src) in [("the toggle", toggle), ("scratch creation", create)] {
            assert!(
                src.contains("set_site_mu_dir_created"),
                "{what} writes the stamp without recording that it may have created the \
                 mu-plugins directory — teardown infers nothing from emptiness (v25), so the \
                 directory would be left behind"
            );
        }
    }


    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexenv-mailtag-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_stamp_has_one_definition_shared_by_the_writer_and_the_reader() {
        // The property this module exists for. If the rendered plugin and the
        // filter's expectation could disagree, the agent would silently stop
        // seeing its own mail — indistinguishable from the fail-closed case,
        // so nothing would look wrong.
        let dir = tmp("shared");
        let domain = "probe.scratch.rex";
        enable(&dir, "wp-content", domain).unwrap();
        let written =
            std::fs::read_to_string(dir.join("wp-content/mu-plugins/rexenv-scratch-mail.php")).unwrap();
        assert!(
            written.contains(&stamp_for(domain)),
            "the rendered plugin does not carry the stamp the filter will match on:\n{written}"
        );
        assert!(!written.contains(FROM_PLACEHOLDER), "placeholder left unreplaced");
        // And it stamps From ONLY — a tag that rewrote the recipient or the body
        // would be changing the mail under test, not labelling it.
        assert!(written.contains("wp_mail_from"), "{written}");
        for untouched in ["wp_mail_from_name", "wp_mail_content_type", "'To'", "subject"] {
            assert!(!written.contains(untouched), "the stamp touches `{untouched}`:\n{written}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_writes_to_the_recorded_content_dir_and_sweeps_every_layout() {
        // Bedrock's content dir is `app/`. A stamp written to a hardcoded
        // `wp-content/` there would never load, and the agent would see no mail
        // with nothing saying why — #217's defect, in a place where the symptom
        // is silence rather than an error.
        let dir = tmp("layouts");
        enable(&dir, "app", "bedrock.scratch.rex").unwrap();
        assert!(dir.join("app/mu-plugins/rexenv-scratch-mail.php").is_file());
        assert!(!dir.join("wp-content/mu-plugins/rexenv-scratch-mail.php").exists());
        assert!(is_installed(&dir, "app"), "the stat must find it where it was written");
        assert!(!is_installed(&dir, "wp-content"), "and not where it wasn't");

        // Removal sweeps all layouts, because a remover may hold only a docroot.
        enable(&dir, "wp-content", "bedrock.scratch.rex").unwrap();
        disable(&dir).unwrap();
        for layout in crate::core::sites::CONTENT_DIR_LAYOUTS {
            assert!(
                !dir.join(layout).join("mu-plugins/rexenv-scratch-mail.php").exists(),
                "`{layout}` kept the stamp"
            );
        }
        disable(&dir).unwrap(); // idempotent — removing what isn't there is fine
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enable_reports_a_dir_it_created_so_teardown_removes_only_ours() {
        // v25's rule: teardown removes a mu-plugins dir rexenv MADE, never one
        // inferred from being empty — a user's own empty mu-plugins dir is
        // theirs. So `enable` has to report the fact rather than the caller
        // guessing it afterwards, by which point it is unknowable.
        let dir = tmp("dirowner");
        assert!(enable(&dir, "wp-content", "a.scratch.rex").unwrap(), "we created mu-plugins");
        assert!(
            !enable(&dir, "wp-content", "a.scratch.rex").unwrap(),
            "second call created nothing — and rewrote nothing (idempotent)"
        );
        // A dir that already existed is not ours, even on the first write.
        let other = tmp("dirowner2");
        std::fs::create_dir_all(other.join("wp-content/mu-plugins")).unwrap();
        assert!(!enable(&other, "wp-content", "b.scratch.rex").unwrap(), "not ours to remove");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn a_domain_that_could_escape_the_php_string_is_refused() {
        // The domain reaches single-quoted PHP source. Sites' domains are
        // validated upstream, so this is defense in depth — the same layer
        // `wp_tunnel::validate_origin` is, and worth having for the same reason.
        let dir = tmp("inject");
        for bad in ["a'.$x.'b.rex", "a b.rex", "A.rex", "a;rex", "", "a/../b"] {
            assert!(enable(&dir, "wp-content", bad).is_err(), "`{bad}` was accepted");
        }
        assert!(enable(&dir, "wp-content", "ok-1.scratch.rex").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
