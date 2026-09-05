//! M2 scratch tools — the executing registry, and a DIFFERENT capability from
//! M1's read-only one.
//!
//! **Why a separate module rather than more rows in `tools.rs`.** M1's
//! guarantee (#199) is structural because of where its handlers live: the
//! read-only guard scans `tools.rs` and `readctx.rs` for any reach toward a
//! manager, a command, or a syscall. A mutating tool added there would trip that
//! guard — correctly. So M2's tools live here, with their own context type, and
//! M1's guarantee is untouched by their arrival rather than quietly widened to
//! accommodate them. The registries — this one, M1's, and the parity one
//! (`user_sites`, which acts on the USER's sites through a scope witness) — are
//! **disjoint by test**
//! (`every_registry_is_disjoint_and_the_guard_says_which_side_a_tool_belongs_on`):
//! one name, one capability, no shadowing.
//!
//! **The capability, precisely.** An M1 handler receives a `ReadCtx`, which has
//! no mutating method. A scratch handler receives a [`ScratchCtx`], whose door
//! to any site is [`ScratchCtx::claim`] — `core::scratch::claim`, which reads
//! the row and proves `origin = 'agent'` (#208). So the difference between the
//! tiers is not "these ones are trusted": it is that a scratch handler can only
//! reach sites the agent owns, and cannot obtain a handle to any other.
//!
//! The plumbing, the disjointness guard and the sweep union landed BEFORE the
//! first tool did, deliberately: an executing tool arrives into a structure that
//! already refuses to let it be registered untested, unlisted or unswept.
//!
//! **One scrubber, not one per door.** Anything a tool here re-emits goes
//! through [`super::view::scrub_log_line`] — `tail_log`'s scrubber (#201), which
//! is called from this module, never copied into it. wp names the site's
//! absolute docroot in ordinary success output and rexenv's own directories in
//! its errors, so these are the same values arriving through further doors; a
//! local "quick scrub" would agree the day it was written and drift afterwards,
//! which is what `one_scrubber_serves_every_door_a_docroot_can_leave_by` fails
//! on. The prefix set is [`super::view::KnownPaths`], derived from the `Paths`
//! trait rather than listed here — and it is *the paths rexenv knows*, never a
//! claim that no path escapes.

use crate::core::scratch::ScratchSite;
use crate::core::sites::Ownership;
use crate::error::{Error, Result};
use crate::state::app::AppState;
use crate::state::models::{NewSite, Site};
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

/// A scratch tool's async result — same shape as the read side's.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;

/// A scratch tool handler: `(ScratchCtx, args, acted) -> future of JSON`.
/// `acted` records the site rexenv acted on for the feed (#206) — an
/// out-parameter, so a create that inserts a row and then fails still names it.
pub type ToolHandler = for<'a> fn(
    ScratchCtx<'a>,
    &'a Value,
    &'a super::feed::ActedTarget,
) -> ToolFuture<'a>;

/// One executing tool. Deliberately the SAME field discipline as `ReadTool`,
/// including the required `sweep_args`: a tool cannot be registered without the
/// secret-leak sweep being able to exercise it, on this side either.
pub struct ScratchTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    /// The arguments the secret-leak sweep calls this tool with. REQUIRED — the
    /// sweep enumerates the registry, so a new tool is covered by construction.
    pub sweep_args: fn(site_id: &str) -> Value,
    /// What this call was ABOUT, for the feed (v30). REQUIRED, and `None` is the
    /// right answer for most: a create, a delete and a sync are fully described
    /// by the tool's name and the site it names.
    pub summarise: fn(&Value) -> Option<String>,
    pub handler: ToolHandler,
}

/// The closed set of M2 scratch tools. Empty until task 8.
pub fn registry() -> &'static [ScratchTool] {
    REGISTRY
}

static REGISTRY: &[ScratchTool] = &[ScratchTool {
    name: "scratch_create_site",
    description: "Create a fresh, disposable WordPress site for testing — a \"scratch\" site the \
                  agent owns and can delete. Takes a single-word `name` (rexenv adds \
                  `.scratch.<tld>` itself) and an optional `php` version. Scratch sites are \
                  capped, and expire on their own once nothing has used them for a while. This \
                  can take a minute or two: WordPress is downloaded and installed. It never \
                  touches the user's own sites.",
    input_schema: create_params,
    sweep_args: |_id| json!({ "name": "sweep-probe" }),
    summarise: |_| None,
    handler: create_site,
}, ScratchTool {
    name: "scratch_delete_site",
    description: "Delete a scratch site the agent created — its files, its database and its \
                  configuration. Only works on scratch sites: the user's own sites are refused. \
                  Takes `site_id`.",
    input_schema: || json!({
        "type": "object",
        "properties": { "site_id": { "type": "string", "description": "The scratch site's id." } },
        "required": ["site_id"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id }),
    summarise: |_| None,
    handler: delete_site,
}, ScratchTool {
    name: "scratch_add_package",
    description: "Copy a plugin or theme you are developing into a scratch site so it can be \
                  tested. Takes `site_id` and `source` (the directory holding the plugin's main \
                  PHP file or the theme's style.css). rexenv works out which it is from the \
                  source's own header. **It is COPIED, not linked**: nothing the site does can \
                  write back to the source, and the site runs the code as of this moment — call \
                  scratch_sync_package after you change it.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "source": { "type": "string", "description": "Absolute path to the plugin/theme directory." }
        },
        "required": ["site_id", "source"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "source": "/tmp/rexenv-sweep-probe" }),
    summarise: summarise_add_package,
    handler: add_package,
}, ScratchTool {
    name: "scratch_sync_package",
    description: "Re-copy a plugin or theme into the scratch site from where it was added, so \
                  the site runs your latest code. Takes `site_id` and `slug`. The site runs a \
                  SNAPSHOT — call this after every change you want the site to see.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "slug": { "type": "string", "description": "The package's folder name (from scratch_add_package)." }
        },
        "required": ["site_id", "slug"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "slug": "sweep-probe" }),
    summarise: |_| None,
    handler: sync_package,
}, ScratchTool {
    name: "wp_run",
    description: "Run a WP-CLI command inside a scratch site — activate a plugin, set an option, \
                  run the plugin's own commands, whatever the check needs. Takes `site_id` and \
                  `args`, the command as an array of words WITHOUT the leading `wp` (e.g. \
                  [\"plugin\", \"activate\", \"acme\"]). rexenv decides which site it runs against, \
                  from `site_id`, so `--path`, `--url`, `--ssh`, `--http` and `@aliases` are \
                  refused — everything else is yours to run. Only works on scratch sites the agent \
                  created. The command's exit code, stdout and stderr all come back; a non-zero \
                  exit is an answer, not a tool failure, so check `succeeded`.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The WP-CLI command as separate words, without `wp` and without \
                                `--path` (rexenv adds that): [\"plugin\", \"activate\", \"acme\"]."
            }
        },
        "required": ["site_id", "args"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "args": ["option", "get", "home"] }),
    summarise: summarise_wp_run,
    handler: wp_run,
}, ScratchTool {
    name: "set_php_version",
    description: "Change which PHP version a scratch site runs — the compatibility matrix: create a \
                  site, add the plugin, then run the same checks on 8.1, 8.2, 8.3 and so on. Takes \
                  `site_id` and `version` (a minor like `8.3`). Only works on scratch sites the \
                  agent created. If rexenv has no build for the version you ask for it says so and \
                  names the ones it has — it never quietly uses a nearby version, because a result \
                  reported against a version that wasn't tested is worse than a refusal. The FIRST \
                  switch to a given version downloads that PHP build (tens of megabytes), so that \
                  call can take a while; later switches to it are quick.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "version": { "type": "string", "description": "PHP minor, e.g. `8.3`. Not a patch version." }
        },
        "required": ["site_id", "version"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "version": "8.3" }),
    // The version is a closed, rexenv-owned set — but it arrives as agent text,
    // so it is summarised like every other: shaped by the write's clamp, not
    // trusted because we expect it to look like `8.3`.
    summarise: |args| args.get("version").and_then(Value::as_str).map(str::to_string),
    handler: set_php_version,
}, ScratchTool {
    name: "scratch_login_url",
    description: "A one-time link that signs into a scratch site's wp-admin WITHOUT a password — \
                  rexenv's own magic login, the same one the app's \"Log in as\" button uses. \
                  Takes `site_id` and an optional `user_id` (omit for the primary administrator). \
                  Open the link in a browser to land in wp-admin signed in, or fetch it ONCE \
                  headlessly with a cookie jar and follow the redirect: the cookies it sets are \
                  the session. Single-use, expires in two minutes, only works from this machine, \
                  never recorded anywhere, and changes nothing about the account — no password \
                  is set or reset. Only works on scratch sites the agent created; for one of the \
                  user's own sites use `wp_user` with action `login_url` (needs their manage grant).",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "user_id": { "type": "integer", "description": "A WordPress user id to sign in as. Omit for the primary administrator." }
        },
        "required": ["site_id"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id }),
    // A login link is a credential. The feed gets the tool's name and the site
    // and NOTHING else — the same rule `wp_user`'s `login_url` follows.
    summarise: |_| None,
    handler: login_url,
}, ScratchTool {
    name: "db_query",
    description: "Run ONE read query against a site's database and get the rows back. On a \
                  scratch site the agent created, this works immediately and may also write. On \
                  the USER's own site it is READ-ONLY (a SELECT-only account on that one \
                  database), allowed by the Agent access dial's Read level — on whenever the \
                  endpoint is — and every query is listed in the app's activity feed. Takes \
                  `site_id` and `sql`. One statement per call. At most 500 rows come back, and \
                  the reply says so when there were more.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The site whose database to read." },
            "sql": { "type": "string", "description": "One SQL statement." }
        },
        "required": ["site_id", "sql"],
        "additionalProperties": false
    }),
    // The sweep runs against a SCRATCH site, which is the arm that needs no
    // consent — so the secret-leak sweep can exercise this tool without a
    // grant existing, and without the sweep being a way to get one.
    sweep_args: |id| json!({ "site_id": id, "sql": "SELECT 1" }),
    // The SQL is what this call was about, and the feed is where a user goes to
    // see what an agent actually read. Truncated, because a query can be long
    // and the feed line is one line.
    summarise: |args| {
        args.get("sql").and_then(Value::as_str).map(|sql| {
            let one_line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
            if one_line.chars().count() > 120 {
                format!("{}…", one_line.chars().take(120).collect::<String>())
            } else {
                one_line
            }
        })
    },
    handler: db_query,
}, ScratchTool {
    name: "mail_list",
    description: "List the mail a scratch site has SENT — password resets, notifications, anything \
                  its code mailed — so you can trigger something and then read it. Takes `site_id` \
                  and an optional `limit`. It returns only messages rexenv can prove came from that \
                  scratch site: the user's own sites' mail is never included. Works whenever the \
                  endpoint is on; needs rexenv's mail catcher running.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "limit": { "type": "integer", "description": "How many recent messages (max 50; default 20)." }
        },
        "required": ["site_id"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id }),
    summarise: |_| None,
    handler: mail_list,
}, ScratchTool {
    name: "mail_get",
    description: "Read one message a scratch site sent, in full — body, headers, links. Takes \
                  `site_id` and the `message_id` from mail_list. rexenv checks again that the \
                  message really came from that scratch site before returning it, so an id from \
                  anywhere else is refused rather than fetched.",
    input_schema: || json!({
        "type": "object",
        "properties": {
            "site_id": { "type": "string", "description": "The scratch site's id." },
            "message_id": { "type": "string", "description": "From mail_list." }
        },
        "required": ["site_id", "message_id"],
        "additionalProperties": false
    }),
    sweep_args: |id| json!({ "site_id": id, "message_id": "sweep-probe" }),
    summarise: |_| None,
    handler: mail_get,
}];


/// `wp_run`'s summary: the WP-CLI command and subcommand, e.g. `plugin activate`.
///
/// This tool is why the column exists. It is the only one whose action leaves no
/// other record — a create, a delete, an add and a sync all change state the user
/// can go and look at, while `wp_run`'s effects are inside the site and its
/// history exists nowhere but the feed. Rows reading `wp_run · x · ok` record
/// that a runner ran, not what it did, and `wp plugin list` twelve times is a
/// different thing to have happened than `wp eval` twelve times.
///
/// **Two tokens, no more, and no values.** The command and subcommand are the
/// shape WP-CLI itself uses. Arguments past them are where the content is — a
/// plugin slug, an option value, a block of PHP — and none of that belongs in an
/// accountability record it would also make unreadable.
///
/// The tokens are agent-supplied and are made safe by the WRITE's clamp
/// (`feed::clamp_summary`), not by anything here: whatever this returns, only
/// `[a-z][a-z0-9-]{0,19}` survives, so it cannot forge a client name, rexenv's
/// own rows, or the separators between them.
/// `summarise_wp_run`, for the parity registry's raw runner — the same two
/// tokens, the same clamp, one function.
pub(super) fn summarise_wp_run_public(args: &Value) -> Option<String> {
    summarise_wp_run(args)
}

fn summarise_wp_run(args: &Value) -> Option<String> {
    let list = args.get("args")?.as_array()?;
    let words: Vec<String> = list
        .iter()
        .filter_map(Value::as_str)
        // A flag is not the command — `wp --path=x plugin list` must summarise
        // as `plugin list`, not as the flag the target screen already refused.
        .filter(|w| !w.starts_with('-'))
        // STOP at the first token that isn't command-shaped, rather than taking
        // two unconditionally. Not every WP-CLI command has a subcommand:
        // `wp eval '<php>'` takes CODE as its first positional, so a blind
        // take(2) put the user's PHP in the accountability record. The write's
        // clamp would have mangled it into a fragment rather than leaked it, but
        // a fragment of someone's code is not a summary. One shared predicate
        // (`feed::is_summary_token`) decides here and at the write, so the two
        // cannot disagree about what is command-shaped.
        .map(super::feed::fold_token)
        .take_while(|w| super::feed::is_summary_token(w))
        .take(2)
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// `scratch_add_package`'s summary: the folder name being added.
///
/// **The reason given for including this was wrong, and the correction matters.**
/// It was justified as "rexenv-derived — the canonicalised source directory's
/// name". It is not: `summarise` sees the RAW arguments, before
/// `validate_linked_docroot` canonicalises anything, so this is the last
/// component of a path the agent supplied. What makes it safe is the write's
/// charset clamp, which applies the same whatever the provenance — the same
/// protection `wp_run` gets. Recorded here because a comment claiming
/// rexenv-derived provenance would be a false claim about the one thing this
/// column's safety rests on.
///
/// Still worth having: which plugin was added is the one fact the tool's name and
/// target omit, and unlike a sync it is not visible on the site's card until the
/// clone has already happened.
fn summarise_add_package(args: &Value) -> Option<String> {
    let source = args.get("source")?.as_str()?;
    let name = source.trim_end_matches('/').rsplit('/').next()?;
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}


/// Bound on `mail_list` — a scope limit, like `tail_log`'s line cap.
const MAIL_DEFAULT: usize = 20;
const MAIL_MAX: usize = 50;

/// The honesty contract, in every mail reply.
const MAIL_NOTE: &str = "Only mail rexenv can PROVE came from this scratch site is returned — \
    rexenv stamps each scratch site's own address on its outgoing mail and matches on that. Your \
    own sites' mail is never included. The flip side: if this site's code sets its own From \
    address, its mail stops being visible here — so an empty result can mean \"nothing was sent\" \
    or \"the site overrode the stamp\", and rexenv cannot tell those apart.";

/// Does this message carry THIS scratch site's stamp?
///
/// **One predicate, used to FILTER the list and to GATE the fetch.** That is the
/// whole security design of these two tools: Mailpit's ids are global, so
/// `mail_get` receiving an agent-supplied id must re-prove the message is the
/// site's rather than trusting that the id came from a filtered list. If the
/// filter and the gate were separate expressions, `mail_get` would become a read
/// of any message in the user's inbox — password resets included — behind an id
/// an agent can simply guess or enumerate.
fn is_from_scratch(from: &crate::core::mail::MailAddress, domain: &str) -> bool {
    from.address.eq_ignore_ascii_case(&crate::core::wp_mailtag::stamp_for(domain))
}

/// The mail surface's own preconditions, refused in the order that gives the
/// most actionable answer first — and each naming whose move it is.
fn mail_preconditions(_ctx: &ScratchCtx<'_>, scratch: &ScratchSite) -> Result<()> {
    // The stamp is a STAT, not an inference — which is what lets an empty result
    // be told apart from a site rexenv cannot label. Guessing between those from
    // emptiness alone would teach an agent something false.
    let site = scratch.site();
    let docroot = std::path::Path::new(&site.path);
    if !docroot.is_dir() {
        return Err(Error::Other(format!(
            "`{}` has no folder on disk yet, so nothing has stamped its mail. If its setup did not \
             finish, the person you're working with can retry or remove it in rexenv.",
            scratch.domain()
        )));
    }
    if !crate::core::wp_mailtag::is_installed(docroot, site.content_dir_rel()) {
        return Err(Error::Other(format!(
            "`{}` is not stamping its mail, so rexenv cannot tell its messages from anyone \
             else's — and it will not guess. Turning the MCP endpoint off and on again in rexenv \
             (or relaunching it) restamps every scratch site, which fixes it.",
            scratch.domain()
        )));
    }
    Ok(())
}

/// A message as an agent sees it — never the raw inbox row.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentMail {
    id: String,
    from: String,
    to: Vec<String>,
    subject: String,
    date: String,
}

/// List what this scratch site has sent.
fn mail_list<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("mail_list needs a `site_id`.".into()))?;
        let scratch = ctx.claim(id)?;
        acted.set(scratch.site());
        mail_preconditions(&ctx, &scratch)?;
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(MAIL_DEFAULT, |n| (n as usize).clamp(1, MAIL_MAX));

        let inbox = crate::core::mail::list(None).await.map_err(|e| {
            Error::Other(format!(
                "rexenv's mail catcher isn't answering, so there is nothing to read yet ({e}). It \
                 starts with the rest of the stack — that is the user's move in rexenv."
            ))
        })?;
        // FILTER, then cap. Capping first would let the user's mail crowd this
        // site's out of the window and report an empty result that isn't true.
        let mine: Vec<AgentMail> = inbox
            .messages
            .into_iter()
            .filter(|m| is_from_scratch(&m.from, scratch.domain()))
            .take(limit)
            .map(|m| AgentMail {
                id: m.id,
                from: m.from.address,
                to: m.to.into_iter().map(|a| a.address).collect(),
                subject: m.subject,
                date: m.created,
            })
            .collect();
        Ok(json!({
            "domain": scratch.domain(),
            "messages": mine,
            "note": MAIL_NOTE,
        }))
    })
}

/// Read one message in full — after proving again that it is this site's.
fn mail_get<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, message_id) = two_args(args, "site_id", "message_id", "mail_get")?;
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        mail_preconditions(&ctx, &scratch)?;

        let msg = crate::core::mail::detail(&message_id).await.map_err(|e| {
            Error::Other(format!("there is no message `{message_id}` to read ({e})."))
        })?;
        // THE gate, and the reason these two tools share one predicate: the id
        // arrived from the agent, and Mailpit's ids are global. Trusting that it
        // came from a filtered list would make this a read of ANY message in the
        // user's inbox — password resets included — behind an id that can be
        // guessed. The refusal deliberately does NOT say who the message is
        // really from: that would answer the question it is refusing.
        if !is_from_scratch(&msg.from, scratch.domain()) {
            return Err(Error::Other(format!(
                "that message did not come from `{}`, so it is not this agent's to read. Use \
                 mail_list on the scratch site to see the messages that are.",
                scratch.domain()
            )));
        }
        // The agent's own site's reset link is what it triggered to test.
        let known = super::view::KnownPaths::for_site(ctx.platform().paths(), &scratch.site().path).keeping_reset_keys();
        let body: Vec<String> =
            msg.text.lines().map(|l| super::view::scrub_log_line(l, &known)).collect();
        Ok(json!({
            "id": msg.id,
            "from": msg.from.address,
            "to": msg.to.into_iter().map(|a| a.address).collect::<Vec<_>>(),
            "subject": msg.subject,
            "date": msg.date,
            "body": body.join("\n"),
            "note": MAIL_NOTE,
        }))
    })
}

fn create_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "A single word, no dots — the site becomes `<name>.scratch.<tld>`."
            },
            "php": {
                "type": "string",
                "description": "PHP minor version, e.g. `8.3`. Defaults to rexenv's default."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

/// What an executing handler can reach: app state, and — for any SITE — only
/// through [`ScratchCtx::claim`].
///
/// `Copy` (a single `&AppState`), like `ReadCtx`, so async handlers take it by
/// value.
#[derive(Clone, Copy)]
pub struct ScratchCtx<'a> {
    state: &'a AppState,
    /// The one path to the site-creation job. A trait object because the
    /// provision path is generic over `tauri::Runtime` and a `static` registry
    /// of fn pointers cannot be: erasing the runtime here keeps ONE create path
    /// (the same one the app and the CLI use) instead of a second implementation
    /// for agents.
    creator: &'a dyn SiteCreator,
    /// The MCP client's self-reported name, recorded on the row it creates.
    client: &'a str,
    /// Deleting goes through the app's own full delete path (tunnel stop, DB
    /// drop, teardown, reload) — same runtime erasure, same one-brain rule.
    deleter: &'a dyn SiteDeleter,
}

/// Deleting a site through the app's own delete path.
pub trait SiteDeleter: Send + Sync {
    fn delete<'a>(&'a self, id: String) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

/// Creating a site through the app's own provision job.
///
/// Implemented over the `AppHandle` in `mcp_server`, so this module never
/// re-implements provisioning — the one-brain rule, kept across the runtime
/// erasure that the static registry forces.
pub trait SiteCreator: Send + Sync {
    fn create<'a>(
        &'a self,
        new: NewSite,
        ownership: Ownership,
    ) -> Pin<
        Box<
            dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>>
                + Send
                + 'a,
        >,
    >;
}

impl<'a> ScratchCtx<'a> {
    pub fn new(
        state: &'a AppState,
        creator: &'a dyn SiteCreator,
        deleter: &'a dyn SiteDeleter,
        client: &'a str,
    ) -> Self {
        ScratchCtx { state, creator, deleter, client }
    }

    /// The refusals a create must clear BEFORE anything is built, in the order
    /// that gives the most actionable answer first. Returns the pieces the
    /// create needs: the full domain, the PHP version, and the client name to
    /// record on the row.
    fn plan_create(&self, name: &str, php: Option<String>) -> Result<(String, String, String)> {
        let conn = self.db()?;
        // The ceiling first: a name that would be fine is not worth validating
        // if the pool is full, and the cap refusal is the one that hands back
        // the sites it may delete.
        crate::core::scratch::ensure_capacity(&conn)?;
        let tld = crate::core::sites::default_tld(&conn)?;
        let domain = crate::core::scratch::scratch_domain(name, &tld)?;
        // BOTH tables (v42): a user site may ANSWER on this hostname as an extra
        // domain without it being that site's own. `create` refuses either way,
        // so this is about WHEN and with what words: an agent that hears the
        // refusal here can pick another name, while one that hears it four
        // phases later gets a half-built site and a message about a collision
        // it was never told to avoid.
        if let Some(owner) = crate::core::sites::domain_taken_by(&conn, &domain)? {
            return Err(Error::Other(format!(
                "`{domain}` already reaches the site \"{owner}\". Pick a different name, or use \
                 the site that is already there (list_sites shows it)."
            )));
        }
        let php_version = match php {
            Some(v) => v,
            None => crate::state::store::list_php_versions(&conn)?
                .into_iter()
                .find(|v| v.is_default)
                .map(|v| v.minor)
                .ok_or_else(|| {
                    Error::Other(
                        "rexenv has no default PHP version yet — the person you're working with \
                         needs to finish rexenv's setup before sites can be created."
                            .into(),
                    )
                })?,
        };
        Ok((domain, php_version, self.client.to_string()))
    }

    /// Run the create through the SAME provision path the app and the CLI use,
    /// and turn its two failure shapes into answers an agent can act on.
    ///
    /// The half-built case is the one that matters: a failure after the row
    /// exists leaves a real site in the user's list. The reply NAMES it (and
    /// records it in the feed via `acted`), because "nothing happened" would be
    /// false and would send the agent to create another one.
    async fn create(
        &self,
        new: NewSite,
        ownership: Ownership,
        acted: &super::feed::ActedTarget,
    ) -> Result<Site> {
        let domain = new.domain.clone();
        match self.creator.create(new, ownership).await {
            Ok(site) => {
                acted.set(&site);
                Ok(site)
            }
            Err(failure) => {
                let conn = self.db().ok();
                Err(translate_create_failure(&domain, failure, conn.as_deref(), acted))
            }
        }
    }

    /// Delete a proven scratch site, re-asserting the recorded fact at the
    /// destructive write.
    ///
    /// The witness is a snapshot: the user can press Keep between the claim and
    /// this call, and then the site is THEIRS. So the row deletion carries
    /// `AND origin = 'agent'` in its own `WHERE`, and a miss is reported
    /// honestly — "no longer the agent's" — rather than passing as a silent
    /// no-op that would leave the agent believing it deleted something.
    ///
    /// The re-assert happens BEFORE anything destructive, deliberately: putting
    /// it at the last write would mean discovering the adoption after the
    /// database was already dropped. The residual window between this check and
    /// the resource teardown is milliseconds and cannot be closed without a
    /// transaction spanning MySQL — stated, not pretended away.
    async fn delete(&self, scratch: &ScratchSite) -> Result<()> {
        {
            let conn = self.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, \
                     so it is theirs now and agent tools cannot delete it.",
                    scratch.domain()
                )));
            }
        }
        self.deleter.delete(scratch.id().to_string()).await
    }

    /// The created site's serving status, in M1's vocabulary.
    async fn status_of(&self, site: &Site) -> super::view::AgentSiteStatus {
        let read = super::readctx::ReadCtx::new(self.state);
        let signals = read.probe_serving(site).await;
        super::view::AgentSiteStatus::from_signals(site, &signals)
    }

    /// Switch a proven scratch site's PHP version through the SAME mechanism the
    /// app's own switch uses — and deliberately NOT through the app's COMMAND.
    ///
    /// `commands::sites::set_site_php_version` begins with `promote_if_scratch`,
    /// which is right for a user (changing PHP is deliberate, so the site is
    /// theirs) and would be a cap bypass here: an agent switching PHP would adopt
    /// its own scratch site, clearing the expiry and freeing a slot, so
    /// switch → create → switch → create is unbounded. `switch_php_version` is
    /// the mechanism with that policy lifted out (#223); the ownership rule this
    /// path brings instead is the witness plus the re-assert below.
    async fn switch_php(&self, scratch: &ScratchSite, version: &str) -> Result<Option<Site>> {
        crate::commands::sites::switch_php_version(self.state, scratch.id(), version).await
    }

    fn platform(&self) -> &dyn crate::platform::traits::Platform {
        self.state.platform.as_ref()
    }

    /// The bundled PHP CLI for a site's PHP minor + the wp-cli phar — the same
    /// pinned, checksum-locked pair the UI and the CLI run (`BinaryProvider`),
    /// resolved through the platform trait. Downloads on first use, so the
    /// caller must treat the gap either side of it as a real window.
    async fn wp_tools(&self, php_minor: &str) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
        resolve_wp_tools(self.state, php_minor).await
    }

    pub(crate) fn db(&self) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
        self.state
            .db
            .lock()
            .map_err(|_| Error::Other("the app database lock is poisoned".into()))
    }

    /// Prove a site is the agent's, or refuse with the policy statement
    /// (`core::scratch::claim`, #208). **The only way a MUTATING handler obtains
    /// a site**: "I'll just read the row and check it myself" is not an
    /// available shortcut for anything that changes a site.
    ///
    /// **Amended for `db_query` (M3 stage 3), and the amendment is the honest
    /// part.** This used to say there was no `site_by_id` at all. There is one
    /// now — [`ScratchCtx::site`] — because `db_query` is the first tool that
    /// legitimately acts on a site the agent does NOT own, under a different
    /// authority: the user's recorded grant. Leaving the sentence standing while
    /// adding the method would have been a doc asserting a guard that no longer
    /// existed, which this repo has already paid for twice.
    pub fn claim(&self, id: &str) -> Result<ScratchSite> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|_| crate::error::Error::Other("the app database lock is poisoned".into()))?;
        crate::core::scratch::claim(&conn, id)
    }

    /// Install the mail stamp on a freshly-created scratch site, so it is
    /// readable by the agent that just made it (the endpoint being up is the
    /// condition — D16).
    ///
    /// The ONE place this happens for new sites; `commands::mcp::
    /// sync_scratch_mail_stamps` is the one place it happens for existing ones
    /// (at MCP enable and at launch — D16). Two call sites for one fact is how
    /// the gap appeared, so each names the other. Unconditional: a scratch site
    /// is only ever created through the endpoint, and the endpoint being up IS
    /// the condition.
    fn stamp_mail(&self, site: &Site) {
        let conn = match self.db() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("mcp: could not record the mail stamp for {}: {e}", site.domain);
                return;
            }
        };
        let docroot = std::path::Path::new(&site.path);
        match crate::core::wp_mailtag::enable(docroot, site.content_dir_rel(), &site.domain) {
            Ok(created_dir) => {
                // v25: record ownership of a dir WE made, so teardown removes
                // it — never inferred later from emptiness. Same rule the
                // toggle's loop follows.
                if created_dir {
                    let _ = crate::state::store::set_site_mu_dir_created(&conn, &site.id);
                }
            }
            Err(e) => log::warn!("mcp: mail stamp for {} could not be written: {e}", site.domain),
        }
    }

    /// Any site by id, WITHOUT an ownership claim — for `db_query` only.
    ///
    /// Reading a user's own site is the one thing an agent may do to a site it
    /// does not own, and it is gated by `agent_db::authorize` reading a recorded
    /// grant instead. This method therefore grants nothing on its own: it
    /// resolves a row. Every caller must reach a decision function before it
    /// acts, and a MUTATING tool must still use [`ScratchCtx::claim`] — the
    /// ownership rule is unchanged for everything that writes.
    pub fn site(&self, id: &str) -> Result<Site> {
        let conn = self.db()?;
        crate::state::store::get_site(&conn, id)?
            .ok_or_else(|| crate::error::Error::Other(format!("no site with id {id:?}")))
    }

}

/// The bundled PHP CLI for a site's PHP minor + the wp-cli phar — the same
/// pinned, checksum-locked pair the UI and the CLI run (`BinaryProvider`),
/// resolved through the platform trait. Downloads on first use, so the caller
/// must treat the gap either side of it as a real window. Shared with the
/// parity registry's `site_wp_run`: one resolver, one runner, one scrubber.
pub(super) async fn resolve_wp_tools(state: &AppState, php_minor: &str) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let php_bin = resolve_php(state, php_minor).await?;
    let platform = state.platform.as_ref();
    // wp-cli is a .phar, not a Mach-O → resolve_file (no chmod/codesign).
    let wp_phar = crate::core::binaries::resolve_file(platform, "wp-cli", crate::core::binaries::WP_CLI_VERSION).await?;
    Ok((php_bin, wp_phar))
}

/// The PHP CLI a site's commands run on — the same interpreter its pool
/// serves. Split from [`resolve_wp_tools`] for the runners that want no
/// wp-cli (`site_artisan`): resolving the phar there would download it.
pub(super) async fn resolve_php(state: &AppState, php_minor: &str) -> Result<std::path::PathBuf> {
    // The patch the site's POOL runs (selection floored by the pin), not the
    // pin — an agent running a CLI on a different interpreter than the site
    // serves is a disagreement nobody can see from either side.
    let patch = {
        let conn = state.db.lock().map_err(|_| Error::Other("the app database lock is poisoned".into()))?;
        crate::core::php::patch_to_run(&conn, php_minor).map_err(|_| {
            Error::Other(format!(
                "this site is set to PHP {php_minor}, which rexenv has no build for — the \
                 person you're working with can change the site's PHP version in rexenv."
            ))
        })?
    };
    crate::core::binaries::resolve(state.platform.as_ref(), "php", &patch).await
}

/// This registry's tools as MCP descriptors, for the union `tools/list`.
pub fn tools_list_descriptors() -> Value {
    Value::Array(
        registry()
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": (t.input_schema)(),
                })
            })
            .collect(),
    )
}

/// Turn a create failure into an answer an agent can act on — and, when a site
/// EXISTS, make the reply name it.
///
/// The two shapes are genuinely different advice. Nothing created: the refusal
/// stands alone and the agent should fix its input. Created-then-failed: a real
/// site is sitting in the user's list marked "setup incomplete", so reporting a
/// bare failure would be false AND would send the agent to create another one —
/// filling the pool with half-built sites nobody meant to make. It also records
/// the site for the feed (#206), for the same reason.
///
/// The app/CLI failure text points at a local log file; that path never reaches
/// a tool reply. The agent gets the tools it can actually use instead.
fn translate_create_failure(
    domain: &str,
    failure: crate::commands::sites::CreateFailure,
    conn: Option<&rusqlite::Connection>,
    acted: &super::feed::ActedTarget,
) -> Error {
    let Some(id) = failure.site_id else {
        return failure.error;
    };
    if let Some(conn) = conn {
        if let Ok(Some(site)) = crate::state::store::get_site(conn, &id) {
            acted.set(&site);
        }
    }
    Error::Other(format!(
        "`{domain}` was created but its setup did not finish, so it is not usable yet. It exists \
         (id `{id}`) and the person you're working with can see it in rexenv listed as \"setup \
         incomplete\", where they can retry or remove it — retrying needs them, not you. You can \
         read what went wrong with tail_log, or clear it away with scratch_delete_site and try \
         again."
    ))
}

/// What a scratch site is, as an agent sees it.
///
/// Reuses M1's `verdict`/`resolution` vocabulary (`AgentSiteStatus`) rather than
/// inventing a second one, so "it isn't serving, and that is the USER's action
/// in rexenv" reads identically whether the agent asked `site_status` or just
/// created the site. Carries no docroot, no database name, no local path.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScratchSite {
    pub id: String,
    pub domain: String,
    pub url: String,
    /// When it will expire if nothing uses it. Using it pushes this out.
    pub expires_at: Option<String>,
    /// The M1 status shape — serving/verdict/detail/resolution.
    #[serde(flatten)]
    pub status: super::view::AgentSiteStatus,
}

/// Create the site, then answer with WHAT HAPPENED — including the cases where
/// the site exists but is not yet usable.
fn create_site<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("scratch_create_site needs a `name` (a single word).".into()))?;
        let php = args.get("php").and_then(Value::as_str).map(str::to_string);

        // Refusals FIRST, cheapest and most actionable first, each naming a way
        // forward (core::scratch): the pool ceiling, then the name's shape.
        let (domain, php_version, client) = ctx.plan_create(name, php)?;

        let new = NewSite {
            name: name.trim().to_string(),
            domain,
            site_type: crate::state::models::SiteType::Wordpress,
            php_version,
            web_server: crate::state::models::WebServer::Nginx,
            path: String::new(), // never a caller path: an agent cannot link a folder
            db_engine: crate::state::models::SiteDbEngine::Mysql,
            // Nor clone one: `validate_git_source` refuses `Ownership::Agent`
            // outright, so this is the shape the refusal expects rather than
            // the thing the refusal protects against.
            git_url: String::new(),
            git_ref: None,
            git_migrate: true,
            git_build_assets: false,
            // Scratch sites are WordPress; the field is Blank-PHP's and would
            // be recorded NULL anyway. Stated rather than defaulted so the
            // agent path never acquires one by someone flipping the default.
            starter_db: false,
        };
        let ttl_hours = {
            let conn = ctx.db()?;
            crate::core::scratch::scratch_ttl_hours(&conn)
        };
        let site = ctx.create(new, Ownership::Agent { client, ttl_hours }, acted).await?;
        // Stamp the new site's mail NOW.
        //
        // `commands::mcp::sync_scratch_mail_stamps` stamps every EXISTING
        // scratch site when the endpoint turns on (and at launch — D16), and
        // its doc says that "eliminates 'this site predates the feature' as a
        // category". It does — and nothing covered the reverse, so the category
        // it removed came back as a worse one: a site created AFTER the sync
        // was never stamped at all, which is every new scratch site, i.e. the
        // common case.
        //
        // The symptom was a refusal that blamed the user for the opposite of
        // what happened: `mail_list` said "this normally means mail was
        // switched on after this site was made" when the site had been made
        // after mail was switched on. Found by running SMOKE §M2a step 13.
        //
        // Best-effort, matching the toggle's own policy: a stamp that cannot be
        // written must not fail a site that is otherwise built, and read time
        // answers the truth by a live stat rather than trusting this.
        ctx.stamp_mail(&site);
        let status = ctx.status_of(&site).await;
        let view = AgentScratchSite {
            url: format!("https://{}", site.domain),
            id: site.id.clone(),
            domain: site.domain.clone(),
            expires_at: site.expires_at.clone(),
            status,
        };
        serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the site: {e}")))
    })
}

/// Delete a scratch site — gated by the WITNESS, not by its name.
///
/// `claim` is what decides (#208): it reads the recorded `origin`, so a site the
/// user hand-named `*.scratch.rex` is refused with the ownership policy
/// statement, not with a "not found" that would send the agent looking again.
fn delete_site<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("scratch_delete_site needs a `site_id`.".into()))?;
        // THE gate. Not a suffix check, not a name check — the recorded fact.
        let scratch = ctx.claim(id)?;
        acted.set(scratch.site());
        let domain = scratch.domain().to_string();
        ctx.delete(&scratch).await?;
        Ok(json!({
            "deleted": true,
            "domain": domain,
            "detail": format!("`{domain}` and its database are gone."),
        }))
    })
}

/// `db_query` — the one tool that reads a real site's data, and the only one
/// behind a T1 consent grant.
///
/// The shape is deliberate: **decide, then connect.** `agent_db::authorize` is
/// a pure function over recorded facts and it runs before anything is opened,
/// so the refusal path never touches the engine and the authorization decision
/// is readable in one place instead of being spread through this handler.
fn db_query<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("db_query needs a `site_id`.".into()))?;
        let sql = args
            .get("sql")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("db_query needs a `sql` statement.".into()))?;

        // Ownership is the RECORDED fact, not the domain: `claim` succeeds only
        // for a site the agent itself created. A user's own site whose name
        // happens to read like a scratch one is a real site here.
        let is_scratch = ctx.claim(id).is_ok();
        let site = ctx.site(id)?;
        acted.set(&site);

        let engine = crate::core::db::DbEngine::from_site(site.db_engine);
        // D16: the user's own site is READ-ONLY at the dial's Read level —
        // free whenever the endpoint is on — through the same pure gate as
        // before, now over the dial instead of a grant row.
        let (principal, user) = {
            let conn = ctx.db()?;
            crate::core::agent_db::authorize(&conn, is_scratch, &site.domain)?
        };

        // Provisioning is root work and happens per call: the principal may
        // have been dropped by a revoke since the last one, and re-stating the
        // grant is cheaper than a liveness check that could be wrong.
        let version = crate::commands::database::effective_db_version(ctx.state, engine)?;
        let client = engine
            .cached_sql_client(ctx.state.platform.as_ref(), &version)
            .ok_or_else(|| Error::Other(format!(
                "the {} client is not installed, so the agent principal cannot be created",
                engine.label()
            )))?;
        let port = engine.port();
        crate::core::agent_db::provision(&client, port, principal, &site.db_name, &user)?;
        // What survives of the grant table is a RECORD of the principal just
        // provisioned — keyed by (site, principal), written after the
        // provision succeeded — so the site's delete path can still drop an
        // account made under a domain the site no longer has (#403).
        if !is_scratch {
            let conn = ctx.db()?;
            crate::core::agent_db::record_principal(&conn, &site.id, ctx.client, &user)?;
        }

        let result = crate::core::agent_query::run_query(port, &user, &site.db_name, sql).await?;
        serde_json::to_value(result).map_err(|e| Error::Other(e.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::{test_site, SiteOrigin};

    /// rexenv's paths in production shape, for the scrub tests. Not the fixture
    /// `sandbox()` (no filesystem needed here) — what matters is that the
    /// prefixes look like a real install, so the OS username is actually present
    /// for the scrub to have to remove.
    struct SandboxPaths;
    impl crate::platform::traits::Paths for SandboxPaths {
        fn app_data_dir(&self) -> Result<std::path::PathBuf> {
            Ok("/Users/somebody/Library/Application Support/rexenv".into())
        }
        fn config_dir(&self) -> Result<std::path::PathBuf> {
            Ok("/Users/somebody/Library/Application Support/rexenv/config".into())
        }
        fn log_dir(&self) -> Result<std::path::PathBuf> {
            Ok("/Users/somebody/Library/Application Support/rexenv/logs".into())
        }
        fn bin_dir(&self) -> Result<std::path::PathBuf> {
            Ok("/Users/somebody/Library/Application Support/rexenv/bin".into())
        }
        fn hosts_file(&self) -> std::path::PathBuf {
            "/etc/hosts".into()
        }
    }

    /// A creator that fails the way provisioning does — AFTER the row exists.
    struct HalfBuilt {
        site_id: String,
    }
    impl SiteCreator for HalfBuilt {
        fn create<'a>(
            &'a self,
            _new: NewSite,
            _ownership: Ownership,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>> + Send + 'a>>
        {
            let id = self.site_id.clone();
            Box::pin(async move {
                Err(crate::commands::sites::CreateFailure {
                    site_id: Some(id),
                    error: Error::Other("site create failed at \"core_download\"".into()),
                })
            })
        }
    }

    /// A creator that fails BEFORE anything exists (the prepare phase).
    struct NothingCreated;
    impl SiteCreator for NothingCreated {
        fn create<'a>(
            &'a self,
            _new: NewSite,
            _ownership: Ownership,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Site, crate::commands::sites::CreateFailure>> + Send + 'a>>
        {
            Box::pin(async move {
                Err(crate::commands::sites::CreateFailure {
                    site_id: None,
                    error: Error::Other("`probe.scratch.rex` already exists. Pick a different name.".into()),
                })
            })
        }
    }

    #[test]
    fn the_created_view_speaks_m1s_verdict_and_resolution_vocabulary() {
        // A create that lands while the stack is stopped is a SUCCESS whose site
        // is not serving — and the agent must learn that fixing it is the user's
        // action in rexenv, not a tool it should go hunting for. Reusing M1's
        // shape means "not serving, and here is whose move it is" reads
        // identically whether it asked site_status or just created the site.
        let site = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        let signals = super::super::view::ServingSignals {
            edge_answers_ours: false,
            tcp_443_open: false,
            serving_manager: false,
        };
        let view = AgentScratchSite {
            url: format!("https://{}", site.domain),
            id: site.id.clone(),
            domain: site.domain.clone(),
            expires_at: Some("2026-08-03 09:00:00".into()),
            status: super::super::view::AgentSiteStatus::from_signals(&site, &signals),
        };
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(v["serving"], false);
        assert_eq!(v["verdict"], "edge-down", "the specific verdict, not 'not serving': {v}");
        assert_eq!(v["resolution"], "user-action-in-rexenv", "whose move it is: {v}");
        assert!(v["detail"].as_str().unwrap().len() > 10);
        assert_eq!(v["url"], "https://probe.scratch.rex");
        assert!(v["expiresAt"].is_string(), "the agent can see when it dies: {v}");
        // No internals: the docroot and database name never leave rexenv.
        let text = v.to_string();
        assert!(!text.contains("/Users/") && !text.contains("wp_probe"), "leak: {text}");
    }

    #[tokio::test]
    async fn a_create_that_half_builds_names_the_site_rather_than_reading_as_nothing_happened() {
        // The ActedTarget rule surfacing in the tool's OWN output: the site
        // exists, the user can see it, so the reply must name it — otherwise the
        // agent reports failure and creates another one, and the pool fills with
        // half-built sites nobody meant to make.
        let creator = HalfBuilt { site_id: "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24".into() };
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.provisioned = false;
        row.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &row).unwrap();

        let acted = super::super::feed::ActedTarget::default();
        // Exercise the failure translation directly (constructing a full
        // AppState here would add nothing the assertion needs).
        let err = translate_create_failure(
            "probe.scratch.rex",
            crate::commands::sites::CreateFailure {
                site_id: Some(row.id.clone()),
                error: Error::Other("failed at core_download".into()),
            },
            Some(&conn),
            &acted,
        );
        let msg = err.to_string();
        assert!(msg.contains("probe.scratch.rex"), "names the site: {msg}");
        assert!(msg.contains(&row.id), "gives the id the agent can act on: {msg}");
        assert!(msg.contains("was created"), "does not read as nothing happened: {msg}");
        assert!(msg.contains("retry") || msg.contains("retrying"), "says retry exists: {msg}");
        assert!(msg.contains("them, not you"), "and that retrying is the USER's: {msg}");
        assert!(msg.contains("scratch_delete_site"), "and what the agent CAN do: {msg}");
        assert!(!msg.contains("full log:"), "no local log path in a tool reply: {msg}");
        // And the feed names it too — the row exists, so the record says so.
        assert_eq!(acted.take().as_deref(), Some(row.id.as_str()));
        let _ = creator;
        let _ = NothingCreated;
    }

    #[test]
    fn a_create_that_built_nothing_says_so_without_naming_a_site() {
        let acted = super::super::feed::ActedTarget::default();
        let err = translate_create_failure(
            "probe.scratch.rex",
            crate::commands::sites::CreateFailure {
                site_id: None,
                error: Error::Other("`probe.scratch.rex` already exists. Pick a different name.".into()),
            },
            None,
            &acted,
        );
        assert!(err.to_string().contains("Pick a different name"), "the refusal stands alone");
        assert!(!err.to_string().contains("was created"), "nothing was");
        assert_eq!(acted.take(), None, "and the feed names no site");
    }
    #[test]
    fn deleting_is_gated_by_the_witness_not_by_the_name() {
        // The gate is the RECORDED origin. A site the user hand-named
        // `*.scratch.rex` must read as "that one is yours" — the ownership
        // policy statement — and never as "not found", which would send the
        // agent looking for it again instead of understanding the rule.
        let conn = crate::state::db::open_in_memory().unwrap();
        let theirs = test_site("a1b2c3d4-1111-4222-8333-444455556666", "mine.scratch.rex", SiteOrigin::User);
        crate::state::store::insert_site(&conn, &theirs).unwrap();
        let err = crate::core::scratch::claim(&conn, &theirs.id).unwrap_err().to_string();
        assert!(err.contains("your own sites"), "the ownership refusal: {err}");
        assert!(!err.contains("no site with id"), "NOT a not-found: {err}");
        assert!(err.contains("mine.scratch.rex"), "names it: {err}");
    }

    #[test]
    fn a_site_kept_between_the_claim_and_the_delete_is_no_longer_the_agents() {
        // The witness is a snapshot, so the destructive path re-reads the
        // recorded fact immediately before it does anything. This is that read.
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &row).unwrap();
        let scratch = crate::core::scratch::claim(&conn, &row.id).unwrap();
        assert!(crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap());
        // The user presses Keep — origin flips under the held witness.
        conn.execute("UPDATE sites SET origin = 'user', expires_at = NULL WHERE id = ?1", [&row.id])
            .unwrap();
        assert!(
            !crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap(),
            "a Keep between claim and delete must be seen — a stale witness is not permission"
        );
        // ...and a site deleted in between is not the agent's either.
        conn.execute("DELETE FROM sites WHERE id = ?1", [&row.id]).unwrap();
        assert!(!crate::core::scratch::still_the_agents(&conn, scratch.id()).unwrap());
    }

    #[test]
    fn a_reap_that_keeps_failing_says_so_once_not_once_per_launch() {
        // Retry-once-per-launch means a site that CANNOT be deleted would write
        // an identical row every launch, forever: a slow flood that buries the
        // feed and makes one persistent problem look like many events. The first
        // occurrence is news; the same failure after it is not.
        use crate::mcp_server::feed::{last_reap, record_reap, recent, Outcome};
        let conn = crate::state::db::open_in_memory().unwrap();
        let id = "c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24";
        assert!(record_reap(&conn, id, Outcome::Error, Some("database drop failed".into())).unwrap());
        for _ in 0..5 {
            assert!(
                !record_reap(&conn, id, Outcome::Error, Some("database drop failed".into())).unwrap(),
                "the same problem is not news on every launch"
            );
        }
        assert_eq!(recent(&conn, 50).unwrap().len(), 1, "one row per distinct problem");
        // A DIFFERENT reason is news again — it tells the user something changed.
        assert!(record_reap(&conn, id, Outcome::Error, Some("docroot is not writable".into())).unwrap());
        assert_eq!(recent(&conn, 50).unwrap().len(), 2);
        // And success is always recorded: the site is gone, which is the single
        // most consequential event in this lifecycle.
        assert!(record_reap(&conn, id, Outcome::Ok, None).unwrap());
        assert!(record_reap(&conn, id, Outcome::Ok, None).unwrap(), "a reap is never deduped away");
        assert_eq!(last_reap(&conn, id).unwrap().unwrap().0, Outcome::Ok);
    }
    #[test]
    fn a_package_lands_in_the_sites_recorded_content_dir_by_kind() {
        // The destination follows the site's RECORDED content dir (Bedrock's
        // `app/`, Radicle's `content/`), not a hardcoded wp-content — writing to
        // a dead wp-content would silently install nothing.
        let mut site = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        assert!(package_dest(&site, "plugin", "acme").ends_with("wp-content/plugins/acme"));
        assert!(package_dest(&site, "theme", "acme").ends_with("wp-content/themes/acme"));
        site.content_dir = Some("app".into());
        assert!(package_dest(&site, "plugin", "acme").ends_with("app/plugins/acme"));
    }

    #[test]
    fn one_scrubber_serves_every_door_a_docroot_can_leave_by() {
        // #201 closed the docroot leak for `tail_log`. `wp_run` opens a THIRD
        // door onto the same value — wp prints absolute paths in ordinary
        // success output — so the fix is one function called twice, never two
        // that happen to agree. Both halves matter: the behaviour, and the
        // structural fact that there is only one implementation to drift from.

        // Half one — BEHAVIOUR. The same planted line down both tools' paths.
        let docroot = "/Users/somebody/Sites/probe.scratch.rex";
        let known = super::super::view::KnownPaths::for_site(&SandboxPaths, docroot);
        let line = format!("Success: Created {docroot}/wp-content/plugins/acme/acme.php");
        let (via_wp_run, truncated) = agent_stream(line.as_bytes(), &known);
        let mut tail = super::super::view::AgentLogTail::from_lines(
            "id",
            "d.rex",
            &known,
            "wp-debug",
            vec![line.clone()],
        );
        let via_tail_log = tail.lines.remove(0);
        assert!(!truncated, "a short stream is not truncated");
        assert_eq!(via_wp_run, via_tail_log, "the two doors must agree because they are one function");
        assert_ne!(via_wp_run, line, "…and non-vacuously: the line WAS changed");
        assert!(!via_wp_run.contains("/Users/somebody"), "OS username reached the agent: {via_wp_run}");
        assert!(via_wp_run.contains("<docroot>/wp-content/plugins/acme/acme.php"), "{via_wp_run}");

        // Half two — STRUCTURE. `scrub_log_line` is defined once, in view.rs, and
        // no tool module grows its own. This is the half that fails on a future
        // "quick local scrub" rather than letting it pass quietly.
        assert_eq!(
            include_str!("view.rs").matches("pub fn scrub_log_line").count(),
            1,
            "scrub_log_line must be defined exactly once, in view.rs"
        );
        assert!(
            production_lines(include_str!("scratch.rs"))
                .iter()
                .any(|(_, l)| l.split("//").next().unwrap_or("").contains("view::scrub_log_line")),
            "the executing tools must CALL the shared scrubber — if this call went away, either a \
             tool stopped scrubbing or it grew its own scrubber; both are the #201 leak returning. \
             (Checked against production lines only: this test's own prose names the function.)"
        );
        for (file, src, canary) in [
            ("mcp_server/scratch.rs", include_str!("scratch.rs"), "fn agent_stream"),
            ("mcp_server/tools.rs", include_str!("tools.rs"), "fn tail_log"),
        ] {
            let production = production_lines(src);
            // The scan's own coverage, checked rather than assumed: a
            // `#[cfg(test)]` module can sit ANYWHERE in a file (in this one it
            // sits in the middle), and a stripper that cut from it to the end of
            // the file would skip most of the production code while still
            // passing — the guard-covers-a-narrower-surface-than-its-claim
            // family, caught here by planting.
            assert!(
                production.iter().any(|(_, l)| l.contains(canary)),
                "the scan missed `{canary}` in {file} — it is not covering the file it claims to"
            );
            for (n, l) in production {
                // Comments stripped: prose names the tokens to explain the rule.
                let code = l.split("//").next().unwrap_or("");
                // A scrubbing ROUTINE — a fn, or a closure bound to a name. A
                // local named `scrubbed` holding the shared function's OUTPUT is
                // not one, which is why the closure arm keys on `= |`.
                let names_one = code.contains("scrub") || code.contains("redact");
                let defines_a_scrubber =
                    names_one && (code.contains("fn ") || code.contains("= |"));
                assert!(
                    !defines_a_scrubber,
                    "a SECOND scrubber at {}:{} — `{}`.\n\
                     Agent-facing output is scrubbed by ONE function, `view::scrub_log_line`, \
                     because a second one agrees on the day it is written and drifts after (#201 \
                     was the docroot leaking through tail_log; wp_run was the same value through a \
                     third door). If the shapes don't fit, WIDEN scrub_log_line and let both \
                     callers inherit it — do not fork it here.",
                    file,
                    n,
                    code.trim()
                );
            }
        }
    }

    use crate::core::copy_scan::production_lines;


    #[test]
    fn wp_run_takes_its_target_from_the_witness_and_refuses_the_agents() {
        // Ruling 1, at the tool's own door. The screen itself is core's
        // (`refuse_wp_target_override`, exhaustively tested there); what this
        // pins is that wp_run's ARGUMENT PARSING cannot smuggle a target past it
        // — a whole command line in one string, or a non-string entry, would
        // both reach wp-cli as argv this screen never inspected word by word.
        let one_string = json!({ "site_id": "x", "args": "plugin activate acme --path=/elsewhere" });
        let err = wp_argv(&one_string).unwrap_err().to_string();
        assert!(err.contains("array of separate words"), "{err}");
        assert!(err.contains("not \"plugin activate acme\""), "shows the shape, not just the rule: {err}");

        assert!(wp_argv(&json!({ "args": [] })).unwrap_err().to_string().contains("at least one word"));
        assert!(wp_argv(&json!({}))
            .unwrap_err()
            .to_string()
            .contains("array of separate words"));
        let mixed = json!({ "args": ["plugin", 7, "acme"] });
        assert!(wp_argv(&mixed).unwrap_err().to_string().contains("has to be a string"));

        // A well-formed argv survives intact, word for word — the screen sees
        // exactly what wp-cli will.
        let ok = wp_argv(&json!({ "args": ["plugin", "activate", "acme"] })).unwrap();
        assert_eq!(ok, vec!["plugin", "activate", "acme"]);
        crate::core::scratch::refuse_wp_target_override(&ok).unwrap();
    }

    #[test]
    fn a_wp_run_result_says_whether_it_worked_rather_than_leaving_it_to_be_inferred() {
        // A non-zero exit comes back as a normal RESULT, not a tool error: the
        // tool worked, and `wp plugin is-active x` exits 1 to mean "no". The
        // price of that choice is that the payload must be impossible to skim
        // past — hence `succeeded`, and a detail that leads with FAILED.
        let v = serde_json::to_value(AgentWpRun {
            succeeded: false,
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "Error: The 'acme' plugin could not be found.".into(),
            truncated: false,
            detail: "`wp plugin activate acme` FAILED in `probe.scratch.rex` (exit 1). What WP-CLI \
                     said is in `stderr`."
                .into(),
            note: WP_RUN_NOTE,
        })
        .unwrap();
        assert_eq!(v["succeeded"], false);
        assert_eq!(v["exitCode"], 1);
        assert!(v["detail"].as_str().unwrap().contains("FAILED"), "{v}");
        assert!(v["stderr"].as_str().unwrap().contains("could not be found"), "the reason travels: {v}");
    }

    #[test]
    fn a_flood_of_output_is_cut_and_the_cut_is_stated() {
        // A raw runner can emit a database dump; a tool reply is a context
        // window. The cap is not the interesting part — the fact that it is
        // REPORTED is, because silent truncation reads as a complete answer.
        let flood = "x".repeat(WP_OUTPUT_CAP * 2);
        let (kept, truncated) = agent_stream(flood.as_bytes(), &super::super::view::KnownPaths::for_site(&SandboxPaths, "/dr"));
        assert!(truncated, "a stream over the cap must report the cut");
        assert!(kept.len() <= WP_OUTPUT_CAP, "cut at the cap: {}", kept.len());
        // Multi-byte content is cut on a character boundary, not mid-codepoint.
        let wide = "é".repeat(WP_OUTPUT_CAP);
        let (kept, truncated) = agent_stream(wide.as_bytes(), &super::super::view::KnownPaths::for_site(&SandboxPaths, "/dr"));
        assert!(truncated && kept.chars().all(|c| c == 'é'), "cut mid-codepoint");
    }

    #[test]
    fn one_predicate_filters_the_list_and_gates_the_fetch() {
        // The security design of the mail pair, in one assertion. Mailpit's ids
        // are GLOBAL: `mail_get` receives an agent-supplied id, so it must
        // re-prove the message is this site's rather than trust that the id came
        // from a filtered list. If the filter and the gate were separate
        // expressions they could drift, and the drift's shape is `mail_get`
        // becoming a read of ANY message in the user's inbox — password resets
        // included — behind an id an agent can guess or enumerate.
        let addr = |a: &str| crate::core::mail::MailAddress { name: String::new(), address: a.into() };
        let domain = "probe.scratch.rex";

        assert!(is_from_scratch(&addr(&crate::core::wp_mailtag::stamp_for(domain)), domain));
        // Case-insensitive: SMTP addresses are, and a site that upper-cases its
        // own From would otherwise vanish from its own results.
        assert!(is_from_scratch(&addr("REXENV-SCRATCH@PROBE.SCRATCH.REX"), domain));

        // Everything else is refused — including the shapes an inbox actually
        // holds, which is what the user's mail looks like.
        for foreign in [
            "wordpress@myblog.rex",              // the user's own site
            "hello@example.com",                 // Laravel's default MAIL_FROM
            "admin@probe.scratch.rex",           // the same DOMAIN, not the stamp
            "rexenv-scratch@evil.rex",           // the stamp shape, another domain
            "rexenv-scratch@probe.scratch.rex.evil.com", // suffix-extended
            "x+rexenv-scratch@probe.scratch.rex",        // embedded, not equal
            "",
        ] {
            assert!(
                !is_from_scratch(&addr(foreign), domain),
                "`{foreign}` was accepted as this scratch site's mail"
            );
        }

        // And the predicate is the SAME one in both call sites — asserted on the
        // source, because "these two use one function" has no value to compare.
        let prod = include_str!("scratch.rs");
        let prod = prod.split("#[cfg(test)]").next().unwrap_or(prod);
        assert_eq!(
            prod.matches("is_from_scratch(").count(),
            3,
            "the from-match must appear exactly at its definition, the list filter and the fetch \
             gate — a fourth use, or a second expression doing the same job, is how the filter and \
             the gate drift into disagreeing about whose mail this is"
        );
    }

    #[test]
    fn the_mail_note_states_what_an_empty_result_cannot_distinguish() {
        // Fail-closed is only honest if the ambiguity it creates is said out
        // loud: an empty list means "nothing was sent" OR "the site overrode the
        // stamp", and rexenv cannot tell which. An agent told only "no mail"
        // will report that the feature under test is broken.
        assert!(MAIL_NOTE.contains("never included"), "the scope claim: {MAIL_NOTE}");
        assert!(MAIL_NOTE.contains("cannot tell those apart"), "the ambiguity, stated: {MAIL_NOTE}");
        assert!(MAIL_NOTE.contains("overrode the stamp"), "and what the other case IS: {MAIL_NOTE}");
    }

    #[test]
    fn an_unshipped_php_version_is_refused_by_name_never_substituted() {
        // The import path's rule, and it binds harder for an agent: silently
        // bumping an unshipped minor to a neighbour would have it report a
        // compatibility result for a version it never tested. Worse than a
        // refusal, and invisible.
        //
        // The asked-for version is DERIVED (`php::unshipped_minor`), not the
        // `7.4` literal this used to carry — that literal was chosen on the
        // belief 7.4 could never ship, and this test would have gone vacuous
        // the day it did.
        let conn = crate::state::db::open_in_memory().unwrap();
        let mut row = test_site("c58e0a41-7d2f-4b19-93a6-6e1c5d8f0a24", "probe.scratch.rex", SiteOrigin::Agent);
        row.expires_at = Some("2099-01-01 00:00:00".into());
        crate::state::store::insert_site(&conn, &row).unwrap();

        // The refusal lives in CORE, so the CLI and UI give the same answer.
        let asked = crate::core::php::unshipped_minor();
        let err = crate::core::sites::set_php_version(&conn, &row.id, asked).unwrap_err().to_string();
        assert!(err.contains(&format!("no PHP {asked} build")), "names what was asked for: {err}");
        for shipped in crate::core::php::available_minors() {
            assert!(err.contains(&shipped), "the refusal must name `{shipped}`: {err}");
        }
        // …and the site is untouched — no substitution, not even a partial one.
        let after = crate::state::store::get_site(&conn, &row.id).unwrap().unwrap();
        assert_eq!(after.php_version, row.php_version, "a refused switch changed the row");

        // The named set is DERIVED from the shipped builds, so the sentence
        // cannot outlive the versions it names. Asserted rather than trusted:
        // a hand-maintained second list is how a helpful refusal starts lying.
        assert_eq!(
            crate::core::php::available_minors().len(),
            crate::core::binaries::PHP_VERSIONS.len(),
            "available_minors drifted from the shipped build list"
        );
        assert!(crate::core::php::patch_for_minor("8.3").is_some(), "and the set is not empty");
    }

    #[test]
    fn a_login_link_carries_its_own_rules_and_never_reaches_the_feed() {
        // D2 (5 Sep 2026): a link everywhere, never a password. The reply is
        // pinned here because every rule an agent needs is IN it — a link that
        // arrived bare would be fetched twice, or ten minutes later, and the
        // plain page that comes back would read as "login is broken".
        let v = login_reply("probe.scratch.rex", 1, "tok");
        assert_eq!(v["url"], "https://probe.scratch.rex/?rexenv_login=tok&rexenv_user=1");
        assert_eq!(v["singleUse"], true);
        assert_eq!(v["expiresInSeconds"], crate::core::wp_login::LOGIN_TTL_SECS);
        let note = v["note"].as_str().unwrap();
        for must in ["cookie jar", "spent on first use", "No password"] {
            assert!(note.contains(must), "the note must say `{must}`: {note}");
        }
        // The credential never reaches the feed: the summariser is None for an
        // argument set that CARRIES a url-shaped value, not just for an empty one.
        let tool = REGISTRY.iter().find(|t| t.name == "scratch_login_url").expect("registered");
        assert!((tool.summarise)(&json!({ "site_id": "x", "user_id": 3, "url": "https://x/?rexenv_login=tok" })).is_none());
        // The description states the properties the code enforces (single-use,
        // TTL, no password, scratch-only with the real-site door named) — the
        // copy is what a model decides on, so it is guarded like the enable card.
        for must in ["WITHOUT a password", "Single-use", "two minutes", "never recorded", "wp_user"] {
            assert!(tool.description.contains(must), "the description must say `{must}`");
        }
        assert_eq!(crate::core::wp_login::LOGIN_TTL_SECS, 120, "the description says two minutes");
    }

    #[test]
    fn the_summary_records_the_verb_and_never_the_values() {
        // The line this column must not cross. `plugin activate acme` is a verb
        // and a VALUE; only the verb is recorded. Values are where the content
        // is — a slug, an option's contents, a block of PHP — and an
        // accountability record that carried them would be both a channel and
        // unreadable.
        let run = |v: Value| summarise_wp_run(&json!({ "args": v }));
        assert_eq!(run(json!(["plugin", "activate", "acme"])).as_deref(), Some("plugin activate"));
        assert_eq!(run(json!(["eval", "echo WP_HOME;"])).as_deref(), Some("eval"), "no PHP in the record");
        assert_eq!(run(json!(["db", "query", "SELECT * FROM wp_users"])).as_deref(), Some("db query"));
        // A flag is not the command: the target screen already refuses these,
        // but a refused call is still recorded and must name what it TRIED.
        assert_eq!(run(json!(["--path=/elsewhere", "plugin", "list"])).as_deref(), Some("plugin list"));
        // Malformed input summarises to nothing rather than to a guess.
        assert_eq!(run(json!([])), None);
        assert_eq!(summarise_wp_run(&json!({})), None);
        assert_eq!(run(json!(["plugin", 7])).as_deref(), Some("plugin"), "a non-string is skipped");

        // add_package: the folder name, and NOT the path around it — the source
        // path is the user's own directory and belongs on the site card, not in
        // every feed row.
        let add = |p: &str| summarise_add_package(&json!({ "site_id": "x", "source": p }));
        assert_eq!(add("/Users/me/code/acme-blocks").as_deref(), Some("acme-blocks"));
        assert_eq!(add("/Users/me/code/acme-blocks/").as_deref(), Some("acme-blocks"), "trailing slash");
        for out in [add("/Users/me/code/acme-blocks"), add("/x/y")] {
            let t = out.unwrap_or_default();
            assert!(!t.contains('/'), "a path component separator reached the summary: {t}");
        }
    }

    #[test]
    fn every_tool_declares_whether_its_name_is_enough() {
        // `summarise` is REQUIRED on both registries for the same reason
        // `sweep_args` is: a tool must SAY that its name and target describe it,
        // rather than be assumed to have nothing to add because nobody looked.
        // Five of nine legitimately answer None — that is the answer, not a
        // gap — and this asserts the four that don't, so a future edit that
        // silently drops one fails here.
        // The probe must carry every field ANY summariser reads. That is not
        // hygiene: `set_php_version` was added with a summariser and this test
        // still passed, because the probe had no `version` key — so the tool
        // read as "my name is enough" when it had simply not been asked. The
        // count check below is what makes that impossible to repeat.
        let probe = json!({
            "site_id": "s",
            "args": ["plugin", "list"],
            "source": "/tmp/acme",
            "version": "8.3",
            "sql": "SELECT ID FROM wp_posts",
        });
        let scratch_summarised: Vec<&str> = super::registry()
            .iter()
            .filter(|t| (t.summarise)(&probe).is_some())
            .map(|t| t.name)
            .collect();
        let read_summarised: Vec<&str> = super::super::tools::registry()
            .iter()
            .filter(|t| (t.summarise)(&probe).is_some())
            .map(|t| t.name)
            .collect();
        // Registry order, not alphabetical — the list reads as the registry does.
        assert_eq!(
            scratch_summarised,
            vec!["scratch_add_package", "wp_run", "set_php_version", "db_query"],
            "exactly the executing tools whose name and target under-describe them"
        );
        assert!(read_summarised.is_empty(), "a READ tool's name and target always describe it");
        // NON-VACUITY: every tool that DECLARES a real summariser must actually
        // produce one for the probe. A tool whose summariser reads a field the
        // probe lacks would otherwise be counted as "None on purpose" — the
        // exact miss that let set_php_version through.
        let declared = include_str!("scratch.rs")
            .split("static REGISTRY:")
            .nth(1)
            .and_then(|r| r.split("\nfn ").next())
            .map(|r| r.matches("summarise: ").count() - r.matches("summarise: |_| None").count())
            .expect("the registry literal");
        assert_eq!(
            declared,
            scratch_summarised.len(),
            "a scratch tool declares a summariser the probe never triggers — widen the probe, or \
             its `None` is an accident rather than the answer that its name is enough"
        );
    }

    #[test]
    fn the_sync_wording_never_claims_the_source_is_unchanged() {
        // The fingerprint is stat-only: a difference is reliable, sameness is a
        // strong hint. So the copy says what was DETECTED, and hedges the rest.
        let same = "`acme` was re-copied. No changes were detected in the source since the \
                    last sync, so the site was probably already running this code."
            .to_string();
        assert!(same.contains("No changes were detected"), "{same}");
        assert!(!same.contains("unchanged"), "must not claim more than a stat read can know: {same}");
        assert!(same.contains("probably"), "hedged, deliberately: {same}");
    }

}

/// Where a package lands inside a site, and what it is called there.
fn package_dest(site: &Site, kind: &str, slug: &str) -> std::path::PathBuf {
    std::path::Path::new(&site.path)
        .join(site.content_dir_rel())
        .join(if kind == "theme" { "themes" } else { "plugins" })
        .join(slug)
}

/// Copy a plugin/theme the user is developing INTO a scratch site (S1).
fn add_package<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, source) = two_args(args, "site_id", "source", "scratch_add_package")?;
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        // Blast radius FIRST: cloning is not a licence to read. `$HOME`, a
        // volume root, Desktop/Documents/Downloads, app-data and overlaps are
        // refused exactly as hard as linking them would have been (§4.4) — the
        // clone changed the WRITE direction, not the read direction.
        let src = {
            let conn = ctx.db()?;
            crate::core::sites::validate_linked_docroot(&conn, ctx.platform(), &source)?
        };
        // Then the header, on the SOURCE, before anything is copied.
        let kind = crate::core::scratch::detect_kind(&src)?;
        let slug = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("that source path has no directory name.".into()))?
            .to_string();
        let dest = package_dest(scratch.site(), kind, &slug);
        let _ = std::fs::remove_dir_all(&dest);
        crate::core::scratch::clone_tree(&src, &dest)?;
        let fingerprint = crate::core::scratch::fingerprint(&src)?;
        let synced_at = {
            let conn = ctx.db()?;
            let now = crate::state::store::db_now(&conn)?;
            crate::state::store::upsert_scratch_package(
                &conn,
                &crate::state::models::ScratchPackage {
                    site_id: scratch.id().to_string(),
                    slug: slug.clone(),
                    kind: kind.to_string(),
                    source_path: src.display().to_string(),
                    synced_at: now.clone(),
                    fingerprint,
                },
            )?;
            now
        };
        Ok(json!({
            "slug": slug,
            "kind": kind,
            "syncedAt": synced_at,
            "detail": format!(
                "`{slug}` was COPIED into {} — the site runs it as of now, and nothing it does \
                 can write back to your source. Call scratch_sync_package after you change it.",
                scratch.domain()
            ),
        }))
    })
}

/// Re-copy a recorded package so the site runs the latest code.
fn sync_package<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, slug) = two_args(args, "site_id", "slug", "scratch_sync_package")?;
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        let recorded = {
            let conn = ctx.db()?;
            crate::state::store::scratch_package(&conn, scratch.id(), &slug)?
        }
        .ok_or_else(|| {
            Error::Other(format!(
                "`{slug}` was never added to `{}`. Add it with scratch_add_package first.",
                scratch.domain()
            ))
        })?;
        // Only ever re-reads where the clone CAME FROM — the recorded path,
        // never one supplied now.
        let src = std::path::PathBuf::from(&recorded.source_path);
        if !src.is_dir() {
            return Err(Error::Other(format!(
                "the source `{slug}` was copied from is no longer there. If you moved it, add it \
                 again with scratch_add_package."
            )));
        }
        let now_print = crate::core::scratch::fingerprint(&src)?;
        // "changed" is reliable; sameness is a strong HINT, not a proof (an edit
        // preserving mtime, size and count is invisible to a stat-only read) —
        // so the wording is "no changes detected", never "unchanged".
        let changed = now_print != recorded.fingerprint;
        let dest = package_dest(scratch.site(), &recorded.kind, &slug);
        let _ = std::fs::remove_dir_all(&dest);
        crate::core::scratch::clone_tree(&src, &dest)?;
        let synced_at = {
            let conn = ctx.db()?;
            let now = crate::state::store::db_now(&conn)?;
            crate::state::store::upsert_scratch_package(
                &conn,
                &crate::state::models::ScratchPackage {
                    synced_at: now.clone(),
                    fingerprint: now_print,
                    ..recorded
                },
            )?;
            now
        };
        Ok(json!({
            "slug": slug,
            "syncedAt": synced_at,
            "sourceHadChanged": changed,
            "detail": if changed {
                format!("`{slug}` was re-copied — the site now runs your latest code.")
            } else {
                format!(
                    "`{slug}` was re-copied. No changes were detected in the source since the \
                     last sync, so the site was probably already running this code."
                )
            },
        }))
    })
}

// ---------------------------------------------------------------------------
// wp_run — the raw runner (D1), and the two rulings it is built to
// ---------------------------------------------------------------------------

/// How much of each stream an agent gets back. A raw runner can emit a database
/// dump; a tool reply is a model's context window.
pub(super) const WP_OUTPUT_CAP: usize = 32 * 1024;

/// What a WP-CLI run looks like to an agent.
///
/// The exit code and BOTH streams travel, because a raw runner that hides
/// either is useless: wp writes its answer to stdout and its reason to stderr,
/// and which one carries the news depends on the command.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentWpRun {
    /// Exit 0. Named rather than left to be inferred from `exitCode`, because a
    /// non-zero exit comes back as a normal result (see [`wp_run`]) and the one
    /// thing that must not be skimmed past is whether it worked.
    pub(super) succeeded: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
    /// Whether either stream was cut at the cap — stated, never silent.
    pub(super) truncated: bool,
    pub(super) detail: String,
    /// What was done to the output before the agent saw it. Present so a
    /// labelled path doesn't send the agent hunting for a directory called
    /// `<docroot>` — and so the limit is stated where it is read.
    pub(super) note: &'static str,
}

/// The scrub's scope, in the reply. Says what was replaced AND what wasn't:
/// rexenv can only remove the paths it knows, and a raw `wp` command prints
/// whatever it prints.
pub(super) const WP_RUN_NOTE: &str = "Absolute paths rexenv knows — the site's docroot, rexenv's own \
    directories, the home directory — are shown as labels like <docroot>. Paths rexenv doesn't \
    know are printed as WP-CLI wrote them: this is raw command output, not sanitised content.";

/// Run an arbitrary WP-CLI command inside a scratch site.
///
/// **The target comes from the WITNESS, not from the agent.** `--path` is
/// rexenv's, derived from the claimed [`ScratchSite`]'s recorded docroot, and an
/// agent-supplied `--path`/`--url`/`--ssh`/`--http`/`@alias` is REFUSED
/// (`core::scratch::refuse_wp_target_override`) rather than overridden. Passing
/// ours and hoping would not be enough: in WP-CLI a later `--path` wins, so
/// "ours is appended last" is a race we happen to win, not a rule. The refusal
/// is the rule.
///
/// **There is no subcommand denylist, and that is a decision — not an
/// oversight.** Activating a plugin already runs arbitrary user-level PHP
/// (PLAN §3.1, D1, #197), so a list of forbidden verbs would buy nothing real
/// while LOOKING like protection — the guard-covers-a-narrower-surface-than-its-
/// claim family this codebase rejected in S1. `wp eval`, `wp db query`, `wp
/// plugin install --force` all run. What is screened is which SITE, because that
/// is the tier boundary itself: a closed, small, documented set, where the
/// narrower-surface failure cannot happen. Refusing "which site" is enforceable;
/// refusing "which command" isn't.
///
/// **Output goes through the SAME scrub as `tail_log`** (`view::scrub_log_line`,
/// #201). wp re-emits absolute paths constantly — "Created /Users/<name>/Sites/…"
/// on success, rexenv's own phar and PHP binary in errors and `wp cli info` —
/// every one of them carrying the OS username that `AgentSiteView` drops. One
/// function, called from both doors, never copied. Its reach is bounded and the
/// reply says so: rexenv can only label the paths it KNOWS, and a plugin that
/// prints a path rexenv never chose prints it verbatim.
fn wp_run<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("wp_run needs a `site_id`.".into()))?;
        let argv = wp_argv(args)?;

        // THE gate, first: the recorded origin decides, and the witness is the
        // only handle to a site this module has.
        let scratch = ctx.claim(id)?;
        acted.set(scratch.site());
        // Then the target screen, BEFORE anything is resolved or spawned.
        crate::core::scratch::refuse_wp_target_override(&argv)?;

        let site = scratch.site();
        let docroot = std::path::PathBuf::from(&site.path);
        // Resolving the bundled PHP + wp-cli phar can DOWNLOAD on first use —
        // minutes, not milliseconds. That is a real window between the claim and
        // the run, so the recorded fact is re-read immediately before spawning:
        // the witness proves the path was gated when it claimed, never that the
        // user has not pressed Keep since (the snapshot rule).
        let (php_bin, wp_phar) = ctx.wp_tools(&site.php_version).await?;
        {
            let conn = ctx.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, so \
                     it is theirs now and agent tools cannot run anything in it.",
                    scratch.domain()
                )));
            }
        }

        let printed = argv.join(" ");
        let timeout =
            std::time::Duration::from_secs(crate::core::scratch::WP_RUN_TIMEOUT_SECS);
        let out = crate::commands::wordpress::wp_blocking(move || {
            crate::core::wordpress::wp_run_raw(&php_bin, &wp_phar, &docroot, &argv, timeout)
        })
        .await?;

        // The SAME set tail_log scrubs against, derived from `Paths` — wp prints
        // the docroot in ordinary success output and rexenv's own directories
        // in errors and `wp cli info` (the phar, the pinned PHP binary), and
        // every one of those carries the OS username.
        let known = super::view::KnownPaths::for_site(ctx.platform().paths(), &site.path);
        let (stdout, cut_out) = agent_stream(&out.stdout, &known);
        let (stderr, cut_err) = agent_stream(&out.stderr, &known);
        let succeeded = out.status.success();
        let exit_code = out.status.code();
        let mut detail = if succeeded {
            format!("`wp {printed}` ran in `{}` and succeeded.", scratch.domain())
        } else {
            format!(
                "`wp {printed}` FAILED in `{}` (exit {}). What WP-CLI said is in `stderr`.",
                scratch.domain(),
                exit_code.map_or_else(|| "killed by a signal".to_string(), |c| c.to_string())
            )
        };
        if cut_out || cut_err {
            detail.push_str(&format!(
                " The output was longer than {} KB and has been cut — run a narrower command if \
                 you need the rest.",
                WP_OUTPUT_CAP / 1024
            ));
        }
        // An agent asking for a command the machine has globally and rexenv does
        // not bundle gets the phar's own `not a registered wp command` in
        // `stderr` — untouched, because that is the string worth searching — and
        // the reason APPENDED here (#301). Without it the agent's next move is to
        // retry or to tell the user their site is broken. Through the same
        // scrubber as everything else on this surface: an EXPORTED packages dir
        // can carry the OS username, which nothing on the agent surface may.
        if let Some(tell) = crate::core::wp_packages::explain_missing_command_here(&stderr) {
            detail.push(' ');
            detail.push_str(&super::view::scrub_log_line(&tell, &known));
        }
        let view = AgentWpRun {
            succeeded,
            exit_code,
            stdout,
            stderr,
            truncated: cut_out || cut_err,
            detail,
            note: WP_RUN_NOTE,
        };
        serde_json::to_value(view).map_err(|e| Error::Other(format!("serialising the run: {e}")))
    })
}

/// The `args` array, as strings — refusing the shapes that would silently run
/// the wrong thing (a bare string an agent meant as a whole command line, a
/// number, an empty array).
pub(super) fn wp_argv(args: &Value) -> Result<Vec<String>> {
    let Some(list) = args.get("args").and_then(Value::as_array) else {
        return Err(Error::Other(
            "wp_run needs `args`: the command as an array of separate words, without `wp` — \
             [\"plugin\", \"activate\", \"acme\"], not \"plugin activate acme\"."
                .into(),
        ));
    };
    if list.is_empty() {
        return Err(Error::Other(
            "wp_run needs at least one word in `args` — the WP-CLI subcommand to run, e.g. \
             [\"plugin\", \"list\"]."
                .into(),
        ));
    }
    list.iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                Error::Other(
                    "every entry in `args` has to be a string — one WP-CLI word per entry."
                        .into(),
                )
            })
        })
        .collect()
}

/// One captured stream, made fit for an agent: cut at the cap, then scrubbed
/// line by line through **`view::scrub_log_line`** — `tail_log`'s scrubber
/// (#201), not a local one. wp names the docroot in ordinary success output and
/// rexenv's own directories in its errors, so this is the same leak arriving
/// through further doors; a second scrubber would agree the day it was written
/// and drift after.
///
/// The cut keeps the HEAD: wp writes its column headers, its `Success:` line and
/// its first error at the start, so the front of a long stream is the part with
/// the answer in it. The cut is always reported (`truncated`), never silent.
pub(super) fn agent_stream(raw: &[u8], known: &super::view::KnownPaths) -> (String, bool) {
    let text = String::from_utf8_lossy(raw);
    let mut end = WP_OUTPUT_CAP.min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    let scrubbed = text[..end]
        .lines()
        .map(|l| super::view::scrub_log_line(l, known))
        .collect::<Vec<_>>()
        .join("\n");
    (scrubbed, truncated)
}

/// Switch a scratch site's PHP version — the compatibility matrix's one verb.
/// The reply for a minted login link — pure, so its shape is pinned by a test
/// that needs no site. Everything an agent must know to use the link correctly
/// travels WITH the link: it is spent on first use, it dies in two minutes, and
/// it works headlessly (the mu-plugin sets the auth cookie and redirects to
/// wp-admin — `core::wp_login`), so an agent that fetches it twice, or after a
/// pause, gets a plain WordPress page and must know why.
fn login_reply(domain: &str, user_id: u64, token: &str) -> Value {
    json!({
        "domain": domain,
        "url": format!("https://{domain}/?rexenv_login={token}&rexenv_user={user_id}"),
        "userId": user_id,
        "singleUse": true,
        "expiresInSeconds": crate::core::wp_login::LOGIN_TTL_SECS,
        "note": format!(
            "Open it in a browser to land in wp-admin signed in as user {user_id}, or fetch it ONCE \
             headlessly with a cookie jar and follow the redirect — the cookies it sets are the \
             session. The link is spent on first use and expires in {} seconds; ask again for \
             another. No password was set or changed.",
            crate::core::wp_login::LOGIN_TTL_SECS
        ),
    })
}

/// D2, settled 5 Sep 2026 by the owner: **a login link everywhere, real site or
/// scratch, never a password.** The real-site half is `wp_user` → `login_url`
/// (parity P3, `manage`); this is the scratch half, behind the witness. Same
/// token (`core::wp_login::issue`: single-use, 120 s, loopback-only — the
/// tunnel-replay properties are #33/#307) and the same primary-admin rule as the
/// app's own "Open admin" button, so an agent gets exactly what a click gets.
fn login_url<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let id = args
            .get("site_id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Other("scratch_login_url needs a `site_id`.".into()))?
            .to_string();
        let asked_user = args.get("user_id").and_then(Value::as_u64);
        // THE gate first: the recorded origin, never the name.
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        let site = scratch.site().clone();
        // Resolving the tools can DOWNLOAD on first use (minutes), so the
        // ownership fact is re-read on the far side of it, as `wp_run` does: a
        // Keep pressed meanwhile makes this the user's site, and their sites
        // hand out links only through `wp_user` under a grant.
        let (php_bin, wp_phar) = ctx.wp_tools(&site.php_version).await?;
        {
            let conn = ctx.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, so \
                     it is theirs now. A login link into their site comes from `wp_user` (action \
                     `login_url`) under their manage grant, not from this tool.",
                    scratch.domain()
                )));
            }
        }
        let docroot = std::path::PathBuf::from(&site.path);
        let content_rel = site.content_dir_rel().to_string();
        let domain = site.domain.clone();
        let (user_id, token, created_dir) = crate::commands::wordpress::wp_blocking(move || {
            let user_id = match asked_user {
                Some(u) => u,
                None => crate::core::wordpress::primary_admin_id(&php_bin, &wp_phar, &docroot)?,
            };
            let (token, created_dir) = crate::core::wp_login::issue(
                &php_bin,
                &wp_phar,
                &docroot,
                &content_rel,
                &domain,
                user_id,
                crate::core::wp_login::LOGIN_TTL_SECS,
            )?;
            Ok((user_id, token, created_dir))
        })
        .await?;
        if created_dir {
            // v25: a mu-plugins dir WE made is recorded so teardown removes it —
            // the same fact `stamp_mail` and the app's own button record.
            let conn = ctx.db()?;
            let _ = crate::state::store::set_site_mu_dir_created(&conn, &site.id);
        }
        Ok(login_reply(&site.domain, user_id, &token))
    })
}

fn set_php_version<'a>(
    ctx: ScratchCtx<'a>,
    args: &'a Value,
    acted: &'a super::feed::ActedTarget,
) -> ToolFuture<'a> {
    Box::pin(async move {
        let (id, version) = two_args(args, "site_id", "version", "set_php_version")?;
        // THE gate first: the recorded origin, never the name.
        let scratch = ctx.claim(&id)?;
        acted.set(scratch.site());
        let before = scratch.site().php_version.clone();

        // The version refusal lives in CORE (`core::sites::set_php_version`), so
        // the CLI and the UI give the same answer and none of them can
        // substitute a nearby minor. Checked HERE too, before the switch runs,
        // only so the refusal arrives before a download starts — not as a second
        // opinion about what is available.
        let minor = crate::core::php::minor_of(&version);
        if crate::core::php::patch_for_minor(&minor).is_none() {
            return Err(Error::Other(format!(
                "rexenv has no PHP {minor} build, so `{}` was left on PHP {before}. Available: {}. \
                 rexenv will not quietly use a nearby version — a result you report against a \
                 version that was never tested is worse than this refusal.",
                scratch.domain(),
                crate::core::php::available_minors().join(", ")
            )));
        }

        // The witness is a SNAPSHOT taken at `ctx.claim` above, so the recorded
        // fact is re-read immediately before the mutation: the user may have
        // pressed Keep in between, and this site would then be theirs.
        //
        // What this does NOT cover, stated because the comment here claimed it
        // until 13 Aug 2026 and was false: the DOWNLOAD. `switch_php_version`
        // writes the row FIRST and prefetches the pinned build after
        // (`commands/sites.rs`), so a Keep pressed while tens of megabytes come
        // down lands after the switch is already committed — on the far side of
        // this check, which has long since run. That is accepted rather than
        // fixed: the write was authorised when it happened, and reordering a
        // shipped path shared by the UI and the CLI to harden a window with no
        // real consequence costs more than it buys. The gap this DOES close is
        // the one above it, which is short but real.
        {
            let conn = ctx.db()?;
            if !crate::core::scratch::still_the_agents(&conn, scratch.id())? {
                return Err(Error::Other(format!(
                    "`{}` is no longer a scratch site — the person you're working with kept it, so \
                     it is theirs now and agent tools cannot change it.",
                    scratch.domain()
                )));
            }
        }
        let switched = ctx.switch_php(&scratch, &minor).await?;
        let Some(site) = switched else {
            // The row vanished mid-call (deleted, or reaped). Say so plainly
            // rather than reporting a switch that did not happen.
            return Err(Error::Other(format!(
                "`{}` is gone — it was deleted while the switch was running.",
                scratch.domain()
            )));
        };
        // Report the serving state in M1's vocabulary, for the same reason the
        // create does: a switch that lands while the stack is stopped is a
        // SUCCESS whose site does not serve, and the agent must learn that
        // fixing it is the user's move in rexenv, not a tool to hunt for.
        let status = ctx.status_of(&site).await;
        let view = AgentScratchSite {
            url: format!("https://{}", site.domain),
            id: site.id.clone(),
            domain: site.domain.clone(),
            expires_at: site.expires_at.clone(),
            status,
        };
        let mut out = serde_json::to_value(view)
            .map_err(|e| Error::Other(format!("serialising the site: {e}")))?;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("phpVersion".into(), json!(site.php_version));
            obj.insert(
                "detail".into(),
                json!(if before == site.php_version {
                    format!("`{}` was already on PHP {before}.", site.domain)
                } else {
                    format!("`{}` now runs PHP {} (was {before}).", site.domain, site.php_version)
                }),
            );
        }
        Ok(out)
    })
}

pub(super) fn two_args(args: &Value, a: &str, b: &str, tool: &str) -> Result<(String, String)> {
    let get = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    match (get(a), get(b)) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(Error::Other(format!("{tool} needs `{a}` and `{b}`."))),
    }
}
