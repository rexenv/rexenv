#!/bin/bash
# Tiered runner for the live checks (src-tauri/examples) — L1 of the layer
# model (docs/TESTING.md §4). Same discipline as verify.sh:
# exit codes are LOAD-BEARING, no pipes between a check and its verdict, and
# the green verdict comes ONLY from this script's own final line.
#
# Usage:
#   scripts/live-checks.sh              # the sandbox tier (safe anytime)
#   scripts/live-checks.sh <tier>      # sandbox | service | network | stack
#   scripts/live-checks.sh system <name>   # system-tier checks run ONE at a
#                                          # time, deliberately, never in bulk
#   scripts/live-checks.sh list [tier] # show the classification
#
# Tiers (an example's strongest requirement decides):
#   sandbox — safe with the stack RUNNING: sandboxed paths, fixture ports, no
#             prompts, offline on a warm binary cache.
#   service — spawns real services on ports the running stack may hold: STOP
#             THE STACK first. No prompts, warm-cache offline.
#   network — needs the internet (wp.org, ghcr, Cloudflare, GitHub); also
#             assumes the stack is stopped unless the example says otherwise.
#   stack   — needs the user's stack RUNNING **and the app QUIT**: services outlive
#             the app by design, and `cli_socket_check`/`mcp_socket_check` refuse
#             while the app holds its sockets. Stating only "stack RUNNING" made
#             this tier unsatisfiable-by-reading, and it went un-run long enough for
#             three of its checks to rot (ledger #388-#390, 24 Aug 2026).
#   system  — admin prompts, root state, LaunchAgents, the REAL app database,
#             or real system teardown. Run singly with eyes open.
#   demo    — takes args, needs a second process, or demonstrates rather than
#             asserts. Not runnable in bulk; run by hand per its doc header.
#
# EVERY example must be classified here — an unlisted example fails the run
# (that is the enforcement that new examples declare a tier).
set -euo pipefail

# ── The verdict must not be pipeable away ─────────────────────────────────────
# This script's exit code IS the verdict, and a pipe swallows it:
#   ./scripts/live-checks.sh ... | tail -3 && git commit
# runs the commit on `tail`'s status, not ours. That trap has been documented
# since 28 Jul 2026 ("verify-script-not-piped-checks") and was walked into again
# on 3 Aug by the person who documented it, landing a commit on a RED tier. So
# it is enforced here rather than remembered — the same reasoning as the
# unforgeable verdict line itself.
#
# A FILE redirect is fine (it keeps every line and the exit code) and so is a
# terminal. Only a PIPE is refused, because only a pipe both truncates the
# output and replaces the status.
#
# HONEST LIMIT — this closes one half, not both. It makes a piped run refuse to
# produce a verdict at all, so an `&& git commit` can never chain off a FALSE
# GREEN. It does NOT stop the chain: `script | tail && git commit` still reaches
# the commit, now after a loud refusal instead of a red verdict. Structurally
# binding the commit path needs a recorded-verdict receipt the hook checks
# (docs/TODO.md, "verdict receipt"); that is deliberately not bolted on mid-
# release.
if [ -p /dev/stdout ] && [ "${REXENV_ALLOW_PIPE:-0}" != "1" ]; then
  cat >&2 <<'PIPEMSG'
live-checks.sh: refusing to run with stdout piped.

  A pipe replaces this script's exit code with the last command's, so an
  `&& git commit` after it commits on a verdict that was never checked.

  Redirect to a file instead — it keeps everything, including the status:
      ./scripts/live-checks.sh ... > /tmp/out.log 2>&1; echo "exit=$?"
      tail -40 /tmp/out.log

  If you genuinely need a pipe and have handled the status yourself
  (`set -o pipefail`), re-run with REXENV_ALLOW_PIPE=1.
PIPEMSG
  exit 2
fi

cd "$(dirname "$0")/../src-tauri"

TIERS="
adminer_check network
adminer_deeplink_check network
adminer_login_gate_check sandbox
adminer_proxy_check stack
adminer_update_check network
adminer_serve_check service
agent_db_check service
wp_mail_sink_check service
adopt_check demo
apache_site_check sandbox
app_bundle_swap_check sandbox
app_relaunch_check sandbox
app_swap_probe demo
app_update_check network
blueprint_check network
browser_detect_check sandbox
ca_gen system
caddy_443 system
caddy_fetch network
caddy_recovery_demo system
caddy_serve service
cli_repo_check system
cli_socket_check stack
cli_wp_install_check network
config_rewrite_check sandbox
create_site_serve service
db_compat_matrix sandbox
db_drop_check service
db_dump_check sandbox
db_dump_flags_check sandbox
db_engine_serve service
db_restore_check sandbox
db_source_check sandbox
db_version_switch_check network
delete_site_serve service
devtools_check sandbox
dist_archive_check sandbox
dotfile_guard_check sandbox
dns_serve demo
dns_ssl_autostart_check system
download_progress_check network
edge_adopt_reload_check stack
edge_wire_check sandbox
fpm_candidate_check sandbox
frankenphp_edge_serve service
frankenphp_fetch network
frankenphp_mail_catch_check sandbox
frankenphp_serve service
frankenphp_subdir_validate sandbox
git_site_clone_check sandbox
git_site_provision_check network
health_watchdog_check service
laravel_postgres_check network
linked_site_check sandbox
log_tail_check service
mail_adopt_settings_check system
mail_api_check service
mail_route_check service
mailpit_check service
macos_floor_check network
manifest_sweep_check network
mariadb_bundle_check network
mariadb_site_check network
mcp_control_check sandbox
mcp_mail_check service
mcp_scratch_check sandbox
mcp_secret_sweep sandbox
mcp_socket_check stack
mcp_user_site_check sandbox
metrics_check sandbox
monitor_coverage_demo service
multisite_check network
multisite_wildcard_check network
mysql_serve service
network_check network
nginx_fetch network
nginx_php_serve sandbox
override_fallthrough_check service
php_fetch network
php_fpm_serve sandbox
php_per_site_serve service
php_pools_serve service
php_switch_serve service
php_tools_check network
php_versions_check network
php_update_check network
port_check sandbox
postgres_site_db_check network
postgres_site_lifecycle_check network
prefetch_responsiveness_check network
priv_check system
ready_split_check service
redis_bundle_check network
relink_tree_check sandbox
repo_clone_check network
repo_git_ops_check sandbox
repo_install_check network
repo_link_check sandbox
repo_run_all_check network
repo_watch_check network
resource_totals_check stack
retry_recovery_check sandbox
robustness_check network
seed_and_list system
server_switch_serve service
service_manager_demo system
site_cert_gen system
site_provision_check network
site_resources_check stack
site_stop_start_check sandbox
sites_folder_check sandbox
starter_seed_check sandbox
stack_guard_check stack
stack_stop system
system_setup system
system_teardown system
teardown_check system
terminal_check sandbox
terminal_site_check stack
tunnel_check network
tunnel_delete_order_check network
tunnel_exposure_check network
tunnel_guard_check sandbox
tunnel_muplugin_check sandbox
tunnel_parent_death_check sandbox
tunnel_sweep sandbox
valet_import_check sandbox
valet_scan_check sandbox
webview_dialogs_check sandbox
wire_probe_check stack
wp_create_serve network
wp_debug_log_check demo
wp_debug_toggle_check demo
wp_info_check network
wp_dns_check sandbox
wp_install_serve network
wp_install_stream_check network
wp_login_check network
wp_login_client_ip_check sandbox
wp_noise_check sandbox
wp_packages_check sandbox
wp_plugins_check network
wp_premium_update_check network
wp_real443_setup demo
wp_themes_check network
wp_tools_check network
wpcli_check sandbox
wporg_icons_check network
xdebug_pool_check sandbox
"

tier_of() {
  echo "$TIERS" | awk -v n="$1" '$1 == n { print $2 }'
}

# Enforcement: every example on disk is classified, every classified name exists.
missing=0
for f in examples/*.rs; do
  name="$(basename "$f" .rs)"
  [ "$name" = "common" ] && continue
  if [ -z "$(tier_of "$name")" ]; then
    echo "UNCLASSIFIED example: $name — add it to the tier table in $0" >&2
    missing=1
  fi
done
while read -r name tier; do
  [ -z "$name" ] && continue
  if [ ! -f "examples/$name.rs" ]; then
    echo "STALE tier entry: $name (no such example)" >&2
    missing=1
  fi
done <<<"$TIERS"
[ "$missing" -eq 0 ] || exit 1

cmd="${1:-sandbox}"

if [ "$cmd" = "list" ]; then
  filter="${2:-}"
  echo "$TIERS" | awk -v f="$filter" 'NF == 2 && (f == "" || $2 == f) { printf "  %-32s %s\n", $1, $2 }'
  exit 0
fi

if [ "$cmd" = "system" ]; then
  name="${2:-}"
  if [ -z "$name" ]; then
    echo "system-tier checks run one at a time: scripts/live-checks.sh system <name>"
    echo "They prompt for admin rights, write real system state, or touch the real app database:"
    echo "$TIERS" | awk 'NF == 2 && $2 == "system" { print "  " $1 }'
    exit 1
  fi
  if [ "$(tier_of "$name")" != "system" ]; then
    echo "$name is not a system-tier check (tier: $(tier_of "$name"))" >&2
    exit 1
  fi
  exec cargo run --example "$name"
fi

if [ "$cmd" = "demo" ]; then
  echo "demo-tier examples take args or a second process; run per their doc headers:"
  echo "$TIERS" | awk 'NF == 2 && $2 == "demo" { print "  " $1 }'
  exit 0
fi

case "$cmd" in sandbox | service | network | stack) ;; *)
  echo "unknown tier: $cmd (sandbox | service | network | stack | system | demo | list)" >&2
  exit 1
  ;;
esac

# ── Tier preconditions, ENFORCED ─────────────────────────────────────────────
#
# These were two `echo NOTE:` lines, and a note is not a control. Running the
# service tier against a LIVE stack on 23 Aug 2026 produced thirteen failures,
# eleven of them loud and correct — and one that was not: `delete_site_serve`'s
# nginx could not take :18088 because the user's had it, `await_listening(18088)`
# then passed against the USER's nginx, and the example printed
# `del.test -> HTTP 200` before dying later on an unrelated unwrap. A fixture
# reported that its precondition held while reading a server it does not own.
#
# Per-example `require_ports_free` is the other half and 19 of the 23
# service-tier examples still lack it. This is the half that cannot be forgotten
# when someone adds the 24th: one check, before any example starts, for the whole
# tier.
#
# **No override, deliberately.** An env escape hatch here would recreate exactly
# the note this replaces. The fix is `rex stop`, which takes seconds.
#
# Only rexenv's OWN fixed service ports are probed — not :443. The edge is
# designed to outlive the app, other tools shadow-bind it (Herd does), and "some
# Caddy is up" is not the same claim as "rexenv's stack is running". Ports that
# belong to somebody else are the per-example guard's job.
stack_ports_up() {
  local up=""
  local p
  for p in 18088 18025 13306 13307 9774 9780 9781 9782 9783 9784 9785; do
    if nc -z 127.0.0.1 "$p" >/dev/null 2>&1; then up="$up $p"; fi
  done
  echo "$up"
}

if [ "$cmd" = "service" ] || [ "$cmd" = "network" ]; then
  busy="$(stack_ports_up)"
  if [ -n "$busy" ]; then
    cat >&2 <<STACKMSG
live-checks.sh: refusing to run the $cmd tier — the rexenv stack is RUNNING.

  Answering on:$busy

  These examples bring up their OWN services on these exact ports. Beside a live
  stack they do not collide, they JOIN: a readiness gate that connects is
  satisfied by your server, and an example can then report on a stack it does
  not own. That is not hypothetical — it is why this check exists.

  Stop the stack, then run this again:
      rex stop        # or the Stop button in rexenv
STACKMSG
    exit 1
  fi
fi
if [ "$cmd" = "stack" ]; then
  if [ -z "$(stack_ports_up)" ]; then
    cat >&2 <<'STACKDOWN'
live-checks.sh: refusing to run the stack tier — the rexenv stack is NOT running.

  These examples probe adoption, wiring and resource use of a LIVE stack. With
  nothing up they would assert against absence and could only pass vacuously.

  Start the stack, then run this again:
      rex start       # or the Start button in rexenv
STACKDOWN
    exit 1
  fi
fi

# Build everything first so per-example runs are launch-only. The BIN too:
# `tunnel_parent_death_check` re-executes `target/debug/rexenv` and REFUSES
# when it is missing or stale, and `--examples` alone never produces it — on a
# fresh target dir the sandbox tier failed the 0.5.0 release gate (5 Sep 2026)
# on a precondition, not a proof. Selecting the bin here makes the tier
# self-sufficient; the example's own staleness check still stands.
cargo build --examples --bin rexenv

names="$(echo "$TIERS" | awk -v t="$cmd" 'NF == 2 && $2 == t { print $1 }')"

# Keep every example's output, and REPLAY a failure's tail next to the verdict.
#
# Why this exists: the verdict is printed at the end, and a failing example's
# own output is thousands of lines above it. A caller that reads the tail — a
# human, or `verify-full.sh` piping into one — sees "FAILED — apache_site_check"
# and no reason, and the honest next move (re-run it) DESTROYS the evidence if
# the failure was transient. That happened twice in one session, both times
# under CPU contention, and both times the output was gone before anyone could
# read it. So the runner keeps it rather than relying on someone remembering to
# capture it (docs/TODO.md, "live-check transients").
logdir="$(mktemp -d "${TMPDIR:-/tmp}/rexenv-live-checks-XXXXXX")"
failed=""
for name in $names; do
  echo
  echo "── $name ──────────────────────────────────────────"
  # pipefail (set above) makes the pipeline carry cargo's exit code, not tee's.
  if ! cargo run -q --example "$name" 2>&1 | tee "$logdir/$name.log"; then
    failed="$failed $name"
  fi
done

echo
if [ -n "$failed" ]; then
  echo "live-checks($cmd): FAILED —$failed"
  for name in $failed; do
    echo
    echo "──── $name — last 40 lines (full output: $logdir/$name.log) ────"
    tail -40 "$logdir/$name.log"
  done
  echo
  echo "live-checks($cmd): FAILED —$failed  (output kept in $logdir)"
  exit 1
fi
rm -rf "$logdir"
echo "live-checks($cmd): all green"
