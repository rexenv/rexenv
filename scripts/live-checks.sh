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
adminer_check                  network all
adminer_deeplink_check         network all
adminer_login_gate_check       sandbox all
adminer_proxy_check            stack   all
adminer_update_check           network all
adminer_serve_check            service all
agent_db_check                 service all
wp_mail_sink_check             service macos
adopt_check                    demo    all
apache_site_check              sandbox macos
openlitespeed_site_check       sandbox macos,linux
app_bundle_swap_check          sandbox macos
app_relaunch_check             sandbox macos
linux_route_shape_check        sandbox linux
windows_app_bundle_swap_check  sandbox windows
windows_app_relaunch_check     sandbox windows
windows_job_guard_check        sandbox windows
linux_dns_route_check          system  linux
linux_app_swap_check           system  linux
app_swap_probe                 demo    macos
app_update_check               network macos
blueprint_check                network all
browser_detect_check           sandbox all
ca_gen                         system  all
caddy_443                      system  macos
caddy_fetch                    network all
caddy_recovery_demo            system  all
caddy_serve                    service all
cert_trust_prompt_check        system  macos
windows_stale_ca_sweep         system  windows
cli_repo_check                 system  macos
cli_socket_check               stack   macos
cli_wp_install_check           network all
config_rewrite_check           sandbox all
create_site_serve              service macos
db_clone_check                 sandbox all
db_compat_matrix               sandbox all
db_drop_check                  service all
db_dump_check                  sandbox all
db_dump_flags_check            sandbox macos
db_engine_serve                service all
db_restore_check               sandbox all
db_source_check                sandbox macos
db_version_switch_check        network macos
delete_site_serve              service macos
devtools_check                 sandbox all
dist_archive_check             sandbox macos
dotfile_guard_check            sandbox macos
dns_serve                      demo    all
dns_ssl_autostart_check        system  macos
download_progress_check        network all
edge_adopt_reload_check        stack   all
edge_wire_check                sandbox all
fpm_candidate_check            sandbox macos
frankenphp_edge_serve          service macos
frankenphp_fetch               network macos
frankenphp_mail_catch_check    sandbox macos
frankenphp_serve               service macos
frankenphp_subdir_validate     sandbox macos
git_site_clone_check           sandbox all
git_site_provision_check       network all
worktree_git_check             sandbox all
worktree_site_check            network all
worktree_laravel_check         network all
live_sync_plugin_check         network all
live_sync_pull_check           network all
live_sync_pull_into_check      network all
health_watchdog_check          service macos
laravel_postgres_check         network all
linked_site_check              sandbox all
local_import_check             network macos
local_scan_check               sandbox macos
log_tail_check                 service macos
mail_adopt_settings_check      system  macos
mail_api_check                 service all
mail_route_check               service all
mailpit_check                  service all
legacy_pins_check              network macos
legacy_upgrade_check           network macos
macos_floor_check              network macos
manifest_sweep_check           network all
mariadb_bundle_check           network macos
mariadb_site_check             network macos
mcp_control_check              sandbox macos
mcp_mail_check                 service macos
mcp_scratch_check              sandbox macos
mcp_secret_sweep               sandbox all
mcp_socket_check               stack   macos
mcp_user_site_check            sandbox macos
metrics_check                  sandbox all
monitor_coverage_demo          service macos
multisite_check                network all
multisite_wildcard_check       network macos
mysql_serve                    service all
network_check                  network all
nginx_fetch                    network all
nginx_php_serve                sandbox macos
override_fallthrough_check     service all
php_fetch                      network all
php_fpm_serve                  sandbox macos
php_per_site_serve             service all
php_pools_serve                service all
php_switch_serve               service all
php_tools_check                network all
php_versions_check             network macos
php_update_check               network macos
port_check                     sandbox all
postgres_site_db_check         network all
postgres_site_lifecycle_check  network all
postgres_admin_token_check     service all
prefetch_responsiveness_check  network all
priv_check                     system  macos
ready_split_check              service all
redis_bundle_check             network macos
relink_tree_check              sandbox macos
repo_clone_check               network macos
repo_git_ops_check             sandbox all
repo_install_check             network all
repo_link_check                sandbox all
repo_run_all_check             network all
repo_watch_check               network macos
resource_totals_check          stack   macos
retry_recovery_check           sandbox all
robustness_check               network all
seed_and_list                  system  all
server_switch_serve            service macos
service_manager_demo           system  macos
site_cert_gen                  system  all
site_matrix_check              network macos
site_provision_check           network all
site_resources_check           stack   all
site_stop_start_check          sandbox all
sites_folder_check             sandbox all
starter_seed_check             sandbox all
stack_guard_check              stack   all
stack_stop                     system  all
system_setup                   system  all
system_teardown                system  all
teardown_check                 system  macos
terminal_check                 sandbox macos
terminal_site_check            stack   macos
tunnel_check                   network all
tunnel_delete_order_check      network macos
tunnel_exposure_check          network macos
tunnel_guard_check             sandbox macos
tunnel_muplugin_check          sandbox all
tunnel_parent_death_check      sandbox macos
tunnel_sweep                   sandbox macos
valet_import_check             sandbox macos
valet_scan_check               sandbox macos
webview_dialogs_check          sandbox macos
wire_probe_check               stack   all
wp_create_serve                network macos
wp_debug_log_check             demo    all
wp_debug_toggle_check          demo    all
wp_info_check                  network all
wp_dns_check                   sandbox macos
wp_install_serve               network macos
wp_install_stream_check        network macos
wp_login_check                 network all
wp_login_client_ip_check       sandbox all
wp_noise_check                 sandbox all
wp_packages_check              sandbox all
wp_plugins_check               network all
wp_premium_update_check        network all
wp_real443_setup               demo    macos
wp_themes_check                network all
wp_tools_check                 network all
wpcli_check                    sandbox all
wporg_icons_check              network all
xdebug_pool_check              sandbox macos
windows_port_gate_check        sandbox windows
windows_supervision_check      demo    windows
windows_files_check            demo    windows
windows_php_pool_check         demo    windows
windows_nginx_check            demo    windows
windows_php_cli_check          demo    windows
windows_cgi_churn_probe        demo    windows
windows_cgi_breaker_check      demo    windows
windows_wp_site_check          demo    windows
windows_cli_mail_probe         demo    windows
windows_loader_dialog_check    demo    windows
windows_runtime_check          demo    windows
pool_get_values_probe          demo    all
pool_health_check              demo    all
pool_busy_check                demo    all
windows_streamed_step_check    demo    windows
windows_edge_probe             demo    windows
windows_edge_start_check       demo    windows
windows_firefox_profiles_check demo    windows
windows_cert_trust_check       demo    windows
windows_browser_lock_check     demo    windows
windows_dns53_probe            demo    windows
windows_dns_agent_check        demo    windows
windows_dns_agent_task_check   demo    windows
windows_nrpt_route_check       demo    windows
windows_uac_step_check         demo    windows
windows_desktop_probe          demo    windows
windows_cli_pipe_probe         demo    windows
windows_shell_open_check       demo    windows
windows_app_open_check         demo    windows
windows_autostart_check        demo    windows
windows_junction_check         demo    windows
wp_core_zip_check              network all
"

tier_of() {
  echo "$TIERS" | awk -v n="$1" '$1 == n { print $2 }'
}

# Which OS an example can run on: all | macos | windows | linux (the third column).
# `all` = every OS rexenv is built for — macOS, Windows AND Linux. It was spelled `both`
# until 28 Sep 2026, from the days of two OSes; with Linux it read as "two of three".
os_of() {
  echo "$TIERS" | awk -v n="$1" '$1 == n { print $3 }'
}

# THIS host, in the table's vocabulary. Git Bash on the Windows runner reports
# MINGW64_NT-… — W12 runs this script there, so that spelling is the point.
case "$(uname -s)" in
  Darwin) HOST_OS=macos ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT) HOST_OS=windows ;;
  Linux) HOST_OS=linux ;;
  *) HOST_OS=other ;;
esac

# Can this example run HERE? `all` on macOS, Windows and Linux; otherwise the OS (or the
# comma-separated OSes, e.g. `macos,linux` for a server with no Windows build) it names.
runs_here() {
  case ",$(os_of "$1")," in
    ,all,) return 0 ;;
    *",$HOST_OS,"*) return 0 ;;
    *) return 1 ;;
  esac
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
while read -r name tier os; do
  [ -z "$name" ] && continue
  if [ ! -f "examples/$name.rs" ]; then
    echo "STALE tier entry: $name (no such example)" >&2
    missing=1
  fi
  # The OS axis is classification too, so it is enforced the same way: a row
  # without a valid third column would silently run (or silently skip) on a host
  # nobody checked it against.
  if [ "$os" != "all" ]; then
    bad=0
    [ -z "$os" ] && bad=1
    for one in ${os//,/ }; do
      case "$one" in macos|windows|linux) ;; *) bad=1 ;; esac
    done
    if [ "$bad" -eq 1 ]; then
      echo "BAD os column for $name: ${os:-(empty)} — use all, or macos | windows | linux (comma-separated)" >&2
      missing=1
    fi
  fi
done <<<"$TIERS"
[ "$missing" -eq 0 ] || exit 1

cmd="${1:-sandbox}"

if [ "$cmd" = "list" ]; then
  filter="${2:-}"
  echo "$TIERS" | awk -v f="$filter" 'NF == 3 && (f == "" || $2 == f) { printf "  %-32s %-8s %s\n", $1, $2, $3 }'
  exit 0
fi

if [ "$cmd" = "system" ]; then
  name="${2:-}"
  if [ -z "$name" ]; then
    echo "system-tier checks run one at a time: scripts/live-checks.sh system <name>"
    echo "They prompt for admin rights, write real system state, or touch the real app database:"
    echo "$TIERS" | awk 'NF == 3 && $2 == "system" { print "  " $1 "  (" $3 ")" }'
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
  echo "$TIERS" | awk 'NF == 3 && $2 == "demo" { print "  " $1 "  (" $3 ")" }'
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

names="$(echo "$TIERS" | awk -v t="$cmd" 'NF == 3 && $2 == t { print $1 }')"

# Drop what this host cannot run, and COUNT it: a tier that skipped everything
# must not read as a tier that passed everything (W12).
selected=""
skipped=0
for n in $names; do
  if runs_here "$n"; then selected="$selected $n"; else skipped=$((skipped + 1)); fi
done
names="$(echo $selected | tr ' ' '\n' | sed '/^$/d')"
[ "$skipped" -eq 0 ] || echo "live-checks: skipping $skipped check(s) this $HOST_OS host cannot run (tier table's os column)"

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
