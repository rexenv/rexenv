/**
 * Typed Tauri IPC bridge. The UI MUST go through these wrappers — never call
 * `invoke` directly from components. Each function maps 1:1 to a Rust command
 * registered in `src-tauri/src/lib.rs`.
 *
 * During early scaffolding the app runs in a plain browser (vite dev) where the
 * Tauri runtime is absent; `isTauri()` lets callers fall back to mock data.
 */
import type { StartupNotice, AdminerStatus, AppInfo, AgentAction, AgentAccess,
  AgentAccessLevel,
  AgentAccessMode, Blueprint, BrowserApp, DbImportJobState, DbImportRecord, RewriteApplied, RewritePreview, RewriteRevertOutcome, LeftoverDump, GitAsset, McpStatus, RepoAssetStatus, RepoBranches, RepoGitOp, RepoJobState, RepoKind, RepoPullRef, RepoStashEntry, WpInstallState, RepoLinkResult, RepoProbeResult, RepoScriptsInfo, RepoToolStatus, RepoWatchState, UnmanagedRepo, CliStatus, DbStatus, DnsStatus, DomainChange, DownloadsSnapshot, EditorApp, EnvVar, FirefoxTrustStatus, GlobalStatus, ImportOutcome, ImportProgress, ImportRequest, ImportResult, ImportScan, LinkedFolderInfo, LogTarget, MailDetail, MailList, MailpitStatus, NewSiteInput, PhpSetting, PhpUpdateOutcome, PhpVersion, PlannedDownload, ServiceInfo, Site, SiteCertInfo, SiteProvisionState, SiteRepoInfo, SiteResources, SiteServing, ResolverPlan, ScratchPackage, TeardownReport, TldPolicy, TunnelInfo, WebServer, WpChecksumCleanup, WpChecksumReport, WpCoreSwitch, WpCoreVersion, WpCronEvent, WpDebugLogStatus, WpInfo, WpInstallInput, WpLanguage, WpNetworkSite, WpOptionsForm, WpOrgPlugin, WpOrgTheme, WpPlugin, WpTheme, WpUpdateProgress, WpUser, UnresolvableTld } from "@/types";
import {
  mockAppInfo,
  mockDatabases,
  mockGlobalStatus,
  mockAddSiteDomain,
  mockAllSiteDomains,
  mockMailDetail,
  mockMailList,
  mockRemoveSiteDomain,
  mockSiteDomains,
  mockPhpSettings,
  mockAdminerStatus,
  mockPhpVersions,
  mockResolverDrift,
  mockServices,
  mockSites,
  mockSitesServing,
} from "@/lib/mock";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Lazily import the Tauri API so a browser-only dev build doesn't crash. */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Native folder picker (tauri-plugin-dialog). Returns the chosen absolute
 *  path, or null if the user cancelled. Browser dev falls back to a plain
 *  prompt (WKWebView lacks window.prompt — real browsers don't). */
export async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  if (!isTauri()) return window.prompt(title, defaultPath ?? "");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false, title, defaultPath });
  return typeof picked === "string" ? picked : null;
}

/** Native file picker limited to `.sql` dumps. Returns the chosen absolute
 *  path, or null if the user cancelled. */
export async function pickSqlFile(title: string): Promise<string | null> {
  if (!isTauri()) return window.prompt(title, "");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    title,
    filters: [{ name: "SQL dump", extensions: ["sql"] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Native file picker limited to `.zip` archives (the plugin/theme upload
 *  flow). Multi-select — wp-cli installs a batch in one job. Returns absolute
 *  paths, empty when the user cancelled. */
export async function pickZipFiles(title: string): Promise<string[]> {
  if (!isTauri()) {
    const typed = window.prompt(title, "");
    return typed ? [typed] : [];
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: true,
    title,
    filters: [{ name: "Zip archive", extensions: ["zip"] }],
  });
  if (typeof picked === "string") return [picked];
  return Array.isArray(picked) ? picked : [];
}

/** App name/version/platform for the About card. Mock fallback outside Tauri. */
export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) return mockAppInfo;
  return invoke<AppInfo>("app_info");
}

/** The fatal startup error, if backend init (DB/CA) failed. Read FIRST so the UI can
 *  show an error screen instead of driving a half-initialized app (which would panic
 *  on AppState commands). Null when init succeeded or outside Tauri. */
export async function initError(): Promise<string | null> {
  if (!isTauri()) return null;
  return invoke<string | null>("init_error");
}

/** Notices the launch sweeps queued before any window existed (a killed
 *  crash-orphan share, a rowless public share). DRAINS: the caller is expected
 *  to show them once. Empty outside Tauri. */
export async function startupNotices(): Promise<StartupNotice[]> {
  if (!isTauri()) return [];
  return invoke<StartupNotice[]>("startup_notices");
}

/** Live global status for the sidebar footer. Mock fallback outside Tauri. */
export async function getGlobalStatus(): Promise<GlobalStatus> {
  if (!isTauri()) return mockGlobalStatus;
  return invoke<GlobalStatus>("global_status");
}

/** List all sites. Falls back to mock data in a plain browser (vite dev). */
export async function listSites(): Promise<Site[]> {
  if (!isTauri()) return mockSites;
  return invoke<Site[]>("list_sites");
}

/** Live per-site serving status (edge up AND the site's own upstream up), keyed by
 *  domain. Non-blocking on the backend; mock fallback outside Tauri. */
export async function getSitesServing(): Promise<SiteServing[]> {
  if (!isTauri()) return mockSitesServing;
  return invoke<SiteServing[]>("sites_serving");
}

/** Honest per-site resources for the Sites page (dedicated CPU/RAM only for
 *  FrankenPHP sites; activity + DB size for shared ones). Empty outside Tauri. */
export async function sitesResources(): Promise<SiteResources[]> {
  if (!isTauri()) return [];
  return invoke<SiteResources[]>("sites_resources");
}

/** Rename a site's display name (domain unchanged). No-op outside Tauri. */
export async function renameSite(id: string, name: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("rename_site", { id, name });
}

/** Change a site's domain — DESTRUCTIVE for WordPress sites (one-way URL
 *  search-replace across the whole database, incl. serialized data). The
 *  backend exports a safety backup to Downloads FIRST and aborts if that
 *  fails; refused on multisite. Folder and database name stay unchanged.
 *  Null outside Tauri. */
export async function changeSiteDomain(id: string, domain: string): Promise<DomainChange | null> {
  if (!isTauri()) return null;
  return invoke<DomainChange>("change_site_domain", { id, domain });
}

/** Move a site's docroot into `destParent` (folder keeps its name). Files are
 *  verified at the destination before the record flips, and the old tree (on a
 *  cross-volume copy) is deleted only after the config reload — the site never
 *  points at a missing path. Null outside Tauri. */
export async function moveSiteDocroot(id: string, destParent: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site>("move_site_docroot", { id, destParent });
}

/** Re-point a site at a docroot the USER moved themselves — `path` is the
 *  folder itself. rexenv touches no file: it records the new location and
 *  reloads the config. This is how a linked/imported folder relocates, since
 *  `moveSiteDocroot` refuses to copy-and-delete a folder we don't own.
 *  Null outside Tauri. */
export async function relinkSiteDocroot(id: string, path: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site>("relink_site_docroot", { id, path });
}

/** Import the selected Valet/Herd sites, one at a time.
 *
 *  Sequential and CONTINUE-ON-FAILURE: each site is independent, so a failure
 *  on one never costs the rest. Every requested domain comes back with exactly
 *  one terminal outcome. Rows stream via `onValetImportRow` as they settle. */
export async function valetImportRun(request: ImportRequest): Promise<ImportResult> {
  return invoke<ImportResult>("valet_import_run", { request });
}

/** Stop after the site currently being imported — never mid-site, which would
 *  leave the half-provisioned state users have to clean up by hand. */
export async function valetImportCancel(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("valet_import_cancel");
}

/** Per-row outcomes as the import settles them. */
export async function onValetImportRow(
  cb: (row: ImportOutcome) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ImportOutcome>("valet-import://row", (e) => cb(e.payload));
}

/** Where the running import is right now — which site, which step, how far.
 *  Fires on every real step boundary of the site's own provision/database job,
 *  plus the shared preparation steps that belong to no single site. */
export async function onValetImportProgress(
  cb: (p: ImportProgress) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ImportProgress>("valet-import://progress", (e) => cb(e.payload));
}

/** Scan for Valet/Herd sites. READ-ONLY: nothing of theirs is written, started
 *  or stopped, and no file inside a project is opened. */
export async function scanValetImport(): Promise<ImportScan> {
  return invoke<ImportScan>("scan_valet_import");
}

/** Start importing a site's database (one at a time; refuses while a provision
 *  job runs for the same site). `confirmOverwrite` is the typed database name,
 *  required only when an unclaimed database of that name already exists. */
export async function dbImportStart(
  siteId: string,
  confirmOverwrite?: string,
): Promise<DbImportJobState> {
  return invoke<DbImportJobState>("db_import_start", { siteId, confirmOverwrite: confirmOverwrite ?? null });
}

/** Latest database-import job state for a site (re-attach on mount). */
export async function dbImportState(siteId: string): Promise<DbImportJobState | null> {
  if (!isTauri()) return null;
  return invoke<DbImportJobState | null>("db_import_state", { siteId });
}

/** Cancel the running database import. During the dump this stops a read;
 *  during the restore, rexenv's own partial copy is dropped (only if it was
 *  created by this import). */
export async function dbImportCancel(id: string): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("db_import_cancel", { id });
}

/** Live job snapshots for a database import. */
export async function onDbImportState(
  id: string,
  cb: (s: DbImportJobState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<DbImportJobState>(`db-import://state/${id}`, (e) => cb(e.payload));
}

/** The settled §9 record for one site — the one fact the UI renders. */
export async function dbImportRecord(siteId: string): Promise<DbImportRecord | null> {
  if (!isTauri()) return null;
  return invoke<DbImportRecord | null>("db_import_record", { siteId });
}

/** All settled records (Sites page badges, one query). */
export async function dbImportRecords(): Promise<DbImportRecord[]> {
  if (!isTauri()) return [];
  return invoke<DbImportRecord[]>("db_import_records");
}

/** The rewrite preview (Stage 3): the diff derived from the exact bytes an
 *  apply would write, plus the whole-file fingerprint apply demands. */
export async function rewritePreview(siteId: string): Promise<RewritePreview> {
  return invoke<RewritePreview>("rewrite_preview", { siteId });
}

/** Apply the previewed rewrite. `fingerprint` is the preview's — apply
 *  refuses if the file changed since the diff was shown. */
export async function rewriteApply(
  siteId: string,
  fingerprint: string,
): Promise<RewriteApplied> {
  return invoke<RewriteApplied>("rewrite_apply", { siteId, fingerprint });
}

/** Revert the rewrite: restore the backup, clear the connected fact. `force`
 *  confirms restoring over a file edited since the rewrite. */
export async function rewriteRevert(
  siteId: string,
  force: boolean,
): Promise<RewriteRevertOutcome> {
  return invoke<RewriteRevertOutcome>("rewrite_revert", { siteId, force });
}

/** Leftover dumps kept by failed imports (their data — listed, never hidden). */
export async function dbImportLeftovers(): Promise<LeftoverDump[]> {
  if (!isTauri()) return [];
  return invoke<LeftoverDump[]>("db_import_leftovers");
}

/** Delete one leftover dump (and its manifest). */
export async function dbImportDeleteLeftover(file: string): Promise<void> {
  return invoke<void>("db_import_delete_leftover", { file });
}

/** Take a TLD's /etc/resolver file over from Valet/Herd, backing theirs up
 *  first. Consent belongs to the UI; this performs what it authorised. */
export async function resolverTakeOver(tld: string): Promise<void> {
  return invoke<void>("resolver_take_over", { tld });
}

/** Give a borrowed resolver file back — the return path for the borrow. */
export async function resolverHandBack(tld: string): Promise<ResolverPlan> {
  return invoke<ResolverPlan>("resolver_hand_back", { tld });
}

/** TLDs we borrowed whose file another tool has since reclaimed. */
export async function resolverDrift(): Promise<string[]> {
  if (!isTauri()) return mockResolverDrift();
  return invoke<string[]>("resolver_drift");
}

/** Inspect a folder the user picked for linking, WITHOUT creating anything:
 *  what we'd serve, as what type, and whether it already holds an app. Runs the
 *  full link preflight, so it REJECTS (throws) with the real reason — too
 *  broad, overlaps another site, inside the sites folder. Detection is pure
 *  filesystem; nothing in the folder is executed. */
export async function inspectLinkedFolder(path: string): Promise<LinkedFolderInfo> {
  return invoke<LinkedFolderInfo>("inspect_linked_folder", { path });
}

/** A site's per-request env vars, name-sorted. Empty outside Tauri. */
export async function listSiteEnv(id: string): Promise<EnvVar[]> {
  if (!isTauri()) return [];
  return invoke<EnvVar[]>("list_site_env", { id });
}

/** Replace a site's env vars (replace-all). The backend validates names
 *  (identifier shape, reserved FastCGI/PHP params, HTTP_ prefix) and values
 *  (no $, {, }, control chars), then regenerates + reloads the server config.
 *  No-op outside Tauri. */
export async function setSiteEnv(id: string, vars: EnvVar[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_site_env", { id, vars });
}

/** TLDs a site answers on that this machine cannot resolve, with the reason.
 *  Empty is the ordinary answer. Empty outside Tauri. */
export async function unresolvableTlds(): Promise<UnresolvableTld[]> {
  if (!isTauri()) return [];
  return invoke<UnresolvableTld[]>("unresolvable_tlds");
}

/** Put back the OS resolver file for a TLD your sites answer on — the fix for
 *  what `unresolvableTlds` reports. Privileged: shows the OS prompt. Refused for
 *  a TLD no site uses, so this cannot point arbitrary names at this machine.
 *  No-op outside Tauri. */
export async function repairResolver(tld: string): Promise<string> {
  if (!isTauri()) return tld;
  return invoke<string>("repair_resolver", { tld });
}

/** Every site's EXTRA domains, keyed by site id — one read for the Sites page.
 *  Sites with none are ABSENT from the map (missing means none; an empty array
 *  would be a second way to say it). Empty outside Tauri. */
export async function allSiteDomains(): Promise<Record<string, string[]>> {
  if (!isTauri()) return mockAllSiteDomains();
  return invoke<Record<string, string[]>>("all_site_domains");
}

/** Every hostname a site answers on — its own domain FIRST, then its extra
 *  domains (v42). Empty outside Tauri. */
export async function siteDomains(id: string): Promise<string[]> {
  if (!isTauri()) return mockSiteDomains(id);
  return invoke<string[]>("site_domains", { id });
}

/** Add an extra domain and START SERVING it: the backend validates the name
 *  against every site's domains AND every other extra domain (one hostname
 *  reaches one site), records it, re-issues the certificate to cover it, and
 *  reloads the web tier. Returns the site's full domain list. Throws with the
 *  colliding site's NAME when the hostname is taken. No-op outside Tauri. */
export async function addSiteDomain(id: string, domain: string): Promise<string[]> {
  if (!isTauri()) return mockAddSiteDomain(id, domain);
  return invoke<string[]>("add_site_domain", { id, domain });
}

/** Remove an extra domain and stop serving it (config rebuilt, cert re-issued
 *  without it). The site's own domain is not removable this way — that is
 *  `changeSiteDomain`. Returns the remaining list. No-op outside Tauri. */
export async function removeSiteDomain(id: string, domain: string): Promise<string[]> {
  if (!isTauri()) return mockRemoveSiteDomain(id, domain);
  return invoke<string[]>("remove_site_domain", { id, domain });
}

/** Read-only info about a site's HTTPS leaf cert (validity, SANs, cert folder).
 *  Null when no cert has been issued yet — or outside Tauri. */
export async function siteCertInfo(id: string): Promise<SiteCertInfo | null> {
  if (!isTauri()) return null;
  return invoke<SiteCertInfo | null>("site_cert_info", { id });
}

/** Re-issue one site's HTTPS leaf cert and force-reload the edge so it's served
 *  immediately. On a failed reload the old cert stays served (error says so).
 *  No-op outside Tauri. */
export async function regenerateSiteCert(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("regenerate_site_cert", { id });
}

/* `createSite` lived here until 21 Aug 2026 — the one-shot wrapper the New Site
 * dialog used before the job-based provision flow replaced it. It had been
 * exempted as UNCALLED ("superseded… the wrapper predates it") for long enough
 * that the exemption stopped being temporary, which is the state the allowlist's
 * own comment says it must never reach: the list "is allowed to SHRINK, never to
 * grow silently". The backend `create_site` command STAYS — `rex site create`
 * calls it through `cli_server` — so what is gone is a frontend door onto a
 * flow the UI no longer uses. Use `siteProvisionJob` below. */

/** Start a STREAMED site-provision job (the New Site card): prepare runs
 *  inline (validation/duplicate/prompt errors reject HERE with nothing
 *  created), then phases stream via `onSiteProvisionState`/`Output`. */
export async function siteProvisionJob(
  input: NewSiteInput,
  wp?: WpInstallInput,
  blueprintId?: string,
): Promise<SiteProvisionState> {
  return invoke<SiteProvisionState>("site_provision_job", { site: input, wp, blueprintId });
}

/** Re-enter provisioning for a "setup incomplete" site (provisioned=false):
 *  prepare's artifacts are re-ENSURED, then the idempotent install steps
 *  re-run. Refused for fully-provisioned sites. */
export async function siteProvisionRetry(siteId: string): Promise<SiteProvisionState> {
  return invoke<SiteProvisionState>("site_provision_retry", { siteId });
}

/** Cancel a running provision — only ever kills wp-cli children the job
 *  itself spawned; a binary download in flight is NEVER aborted (other
 *  consumers may wait on it; it completes into the cache for the retry). */
export async function siteProvisionCancel(id: string): Promise<void> {
  return invoke<void>("site_provision_cancel", { id });
}

/** The most recent provision job (optionally for one domain) — card
 *  re-adoption after the dialog closes or the route remounts. */
export async function siteProvisionActive(
  domain?: string,
): Promise<SiteProvisionState | null> {
  if (!isTauri()) return null;
  return invoke<SiteProvisionState | null>("site_provision_active", { domain: domain ?? null });
}

export async function onSiteProvisionState(
  id: string,
  cb: (state: SiteProvisionState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<SiteProvisionState>(`site-provision://state/${id}`, (e) => cb(e.payload));
}

export async function onSiteProvisionOutput(
  id: string,
  cb: (line: string) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>(`site-provision://output/${id}`, (e) => cb(e.payload));
}

/** Switch a site's PHP version (DB + reload, no rebuild). Returns the updated site. */
export async function setSitePhpVersion(id: string, version: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("set_site_php_version", { id, version });
}

/** Switch a site's web server (Nginx | FrankenPHP). Returns the updated site. */
export async function setSiteWebServer(id: string, server: WebServer): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("set_site_web_server", { id, server });
}

/** Toggle a site's Xdebug (§8.2). Enabling downloads the minor's pinned
 *  xdebug.so on first use and starts the debug pool (load-probe gated);
 *  FrankenPHP sites, and any minor whose registry row reports
 *  `xdebugSupported: false`, are refused by core with the real reason. That
 *  flag — not a version literal repeated here — is what the UI reads. Returns
 *  the updated site. */
export async function setSiteXdebug(id: string, enabled: boolean): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("set_site_xdebug", { id, enabled });
}

/** Delete a site (DB row + cert + docroot). No-op outside Tauri. */
export async function deleteSite(id: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("delete_site", { id });
}

/** **Keep** a scratch site — the user adopting it deliberately. Clears its
 *  expiry and makes it theirs, through the SAME single write every implied
 *  promotion (rename, move, PHP switch…) uses. Idempotent; there is no un-keep.
 *  Returns whether a row actually changed. */
export async function keepSite(id: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("keep_site", { id });
}

/** Every plugin/theme an agent has cloned into a scratch site, across all
 *  sites — one read for the page. Rows whose site is gone are filtered by the
 *  query itself. */
export async function scratchPackages(): Promise<ScratchPackage[]> {
  if (!isTauri()) return [];
  return invoke<ScratchPackage[]>("scratch_packages");
}

/** The scratch reaper's summary for a sweep that DID something — names the
 *  domains it removed and what it left alone. Silent on a quiet launch, so a
 *  user with no scratch sites never learns the reaper exists. The feed rows are
 *  the durable record; dismissing the banner loses nothing. */
export async function onScratchReaped(cb: (summary: string) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("scratch-reaped", (e) => cb(e.payload));
}

/** All registered PHP versions (installed + available). Mock fallback outside Tauri. */
export async function listPhpVersions(): Promise<PhpVersion[]> {
  if (!isTauri()) return mockPhpVersions;
  return invoke<PhpVersion[]>("list_php_versions");
}

/** Refresh the signed update manifest and return the rows it produced.
 *  Best-effort: the caller renders a failure as "couldn't check", never a block. */
export async function phpUpdateCheck(): Promise<PhpVersion[]> {
  if (!isTauri()) return mockPhpVersions;
  return invoke<PhpVersion[]>("php_update_check");
}

/** Move a minor onto a patch, or fail leaving it exactly where it was — the
 *  backend reverts the selection and restarts the pool if it does not come back.
 *
 *  Returns what actually changed: `restarted` is false when no pool was running,
 *  where the choice is saved but nothing is "now on" the new build.
 *
 *  The mock branch is not decoration — without it, the one button this release
 *  exists for throws in every browser-based UI check, which is where the button
 *  is looked at. */
export async function phpUpdateApply(
  minor: string,
  patch: string,
): Promise<PhpUpdateOutcome> {
  if (!isTauri()) return { patch, restarted: true };
  return invoke<PhpUpdateOutcome>("php_update_apply", { minor, patch });
}

/** The Adminer version row: what is staged, what will run, what is offered. */
export async function adminerStatus(): Promise<AdminerStatus> {
  if (!isTauri()) return mockAdminerStatus;
  return invoke<AdminerStatus>("adminer_status");
}

/** Refresh the signed manifest and return the fresh Adminer row.
 *  Best-effort: the caller renders a failure as "couldn't check", never a block. */
export async function adminerUpdateCheck(): Promise<AdminerStatus> {
  if (!isTauri()) return mockAdminerStatus;
  return invoke<AdminerStatus>("adminer_update_check");
}

/** Move Adminer onto `version`, or fail leaving it exactly where it was.
 *
 *  Returns the RE-MEASURED row rather than a success flag, so there is no field
 *  the frontend can assert and the backend can get wrong. */
export async function adminerUpdateApply(version: string): Promise<AdminerStatus> {
  if (!isTauri()) return { staged: version, effective: version, updatable: null };
  return invoke<AdminerStatus>("adminer_update_apply", { version });
}

/** The PHP minor FrankenPHP actually serves (its embedded build, never the
 * site's pool). One backend pin, no frontend copy to drift. */
export async function frankenphpEmbeddedPhp(): Promise<string> {
  if (!isTauri()) return "8.5";
  return invoke<string>("frankenphp_embedded_php");
}

/** Install (enable) or remove (disable) a PHP version. No-op outside Tauri. */
export async function setPhpVersionInstalled(minor: string, installed: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_php_version_installed", { minor, installed });
}

/** Make a PHP version the default for new sites (must be installed). No-op outside Tauri. */
export async function setDefaultPhpVersion(minor: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_default_php_version", { minor });
}

/** The whitelisted ini settings for one PHP minor: every editable key with its
 *  stored value (null = unset, the shown default applies). Mock fallback outside Tauri. */
export async function getPhpSettings(minor: string): Promise<PhpSetting[]> {
  if (!isTauri()) return mockPhpSettings;
  return invoke<PhpSetting[]>("get_php_settings", { minor });
}

/** Replace one PHP minor's ini settings (omitted keys revert to PHP defaults):
 *  backend validates, `php-fpm -t`-gates, persists, restarts that pool, and
 *  reloads nginx so `client_max_body_size` tracks the upload/post sizes. */
export async function applyPhpSettings(
  minor: string,
  settings: { key: string; value: string }[],
): Promise<void> {
  if (!isTauri()) return;
  await invoke("apply_php_settings", { minor, settings });
}

/** Code editors installed on this machine, detection-ordered (first = default
 *  when no preferred_editor setting is stored). Empty outside Tauri. */
export async function listEditors(): Promise<EditorApp[]> {
  if (!isTauri()) return [];
  return invoke<EditorApp[]>("list_editors");
}

/** Open a folder as a PROJECT in a detected editor. Errors when the editor is
 *  gone — the honest no-editor fallback is the caller's job. */
export async function openInEditor(editorId: string, path: string): Promise<void> {
  if (!isTauri()) throw new Error("Opening an editor requires the rexenv desktop app.");
  await invoke("open_in_editor", { editorId, path });
}

/** Web browsers installed on this machine, detection-ordered, with one flagged
 *  `systemDefault` (the OS's https handler). Empty outside Tauri. */
export async function listBrowsers(): Promise<BrowserApp[]> {
  if (!isTauri()) return [];
  return invoke<BrowserApp[]>("list_browsers");
}

/** Open ONE url in a specific browser WITHOUT changing the preference — the
 *  chevron beside "Open in browser". The default moves in Settings only, so a
 *  one-off detour can't silently redirect everything afterwards. Rejects
 *  anything that isn't an http(s) URL (a browser will happily display a local
 *  file). Falls back to a new tab outside Tauri.
 *
 *  `isPrivate` asks for a private/incognito window and is only offered for
 *  browsers whose `supportsPrivate` is true; the backend errors rather than
 *  downgrade to a normal window for the rest. Outside Tauri there is no private
 *  `window.open`, so the dev-server fallback stays an ordinary tab. */
export async function openInBrowser(
  browserId: string,
  url: string,
  isPrivate = false,
): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank");
    return;
  }
  await invoke("open_in_browser", { browserId, url, private: isPrivate });
}

/** Open a path or URL. Paths go to the OS handler (Finder); `http(s)` links go
 *  to the user's `preferred_browser` when one is set and still installed — that
 *  routing lives in the BACKEND so every call site gets it, including the next
 *  one someone adds. Falls back to `window.open` for URLs outside Tauri. */
export async function openExternal(target: string): Promise<void> {
  if (!isTauri()) {
    if (/^https?:\/\//.test(target)) window.open(target, "_blank");
    return;
  }
  await invoke("open_external", { target });
}

/** Reveal a file in the OS file manager with the file selected (macOS:
 *  `open -R`). No-op outside Tauri. */
export async function revealPath(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("reveal_path", { path });
}

/** Which plugin/theme folder a terminal should open in. Only the kind + slug
 *  cross IPC — the backend resolves the directory against the site's recorded
 *  content dir, so no path is ever named from here. */
export interface TerminalAsset {
  kind: "plugin" | "theme";
  /** The asset's folder name (`wp plugin list`'s `name`). */
  name: string;
}

/** Open a PTY shell in a site's docroot (bundled PHP + `wp` on PATH), or in
 *  `asset`'s own folder when one is given. Returns the session id; output
 *  streams via {@link onTerminalOutput}. Desktop-app only. */
export async function openTerminal(
  siteId: string,
  rows: number,
  cols: number,
  asset?: TerminalAsset,
): Promise<string> {
  if (!isTauri()) throw new Error("The terminal requires the rexenv desktop app.");
  return invoke<string>("terminal_open", { siteId, rows, cols, asset: asset ?? null });
}

/** Write input (keystrokes / paste) to a terminal session. */
export async function writeTerminal(id: string, data: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("terminal_write", { id, data });
}

/** Resize a terminal session's PTY. */
export async function resizeTerminal(id: string, rows: number, cols: number): Promise<void> {
  if (!isTauri()) return;
  await invoke("terminal_resize", { id, rows, cols });
}

/** Close a terminal session (kills its shell). */
export async function closeTerminal(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("terminal_close", { id });
}

/** Subscribe to a terminal session's output. Returns an unlisten function.
 *  No-op outside Tauri. */
export async function onTerminalOutput(
  id: string,
  cb: (bytes: Uint8Array) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<number[]>(`terminal://output/${id}`, (e) => cb(new Uint8Array(e.payload)));
}

/** One backend health-watchdog observation (a managed service found dead and
 *  what was done about it). Mirrors the Rust `HealthEvent`. */
export interface HealthEvent {
  service: string;
  action:
    | "restarted"
    | "restart-failed"
    | "gave-up"
    | "edge-down"
    | "edge-restarting"
    | "edge-blocked"
    | "edge-unblocked"
    | "adopted";
  detail: string;
}

/** Subscribe to health-watchdog events (`service-health`): a service died and
 *  was auto-restarted, or needs attention (edge down / gave up). Returns an
 *  unlisten function. No-op outside Tauri. */
export async function onServiceHealth(
  cb: (events: HealthEvent[]) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<HealthEvent[]>("service-health", (e) => cb(e.payload));
}

/** Subscribe to the macOS app menu's "About rexenv" item, which opens the
 *  app's own About screen instead of the native panel (the native one cannot
 *  show the commit, the build date or the licences). Returns an unlisten
 *  function. No-op outside Tauri. */
export async function onAboutMenu(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("menu://about", () => cb());
}

/** Subscribe to the tray menu's routing items (Services, Databases, Mail…).
 *  The payload is the route path; the window is already shown and fronted by
 *  the time this fires. Returns an unlisten function. No-op outside Tauri. */
export async function onTrayRoute(cb: (path: string) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("tray://route", (e) => cb(e.payload));
}

/** Current download-manager state (seed on mount; live updates arrive via
 *  `onDownloadProgress` with the same snapshot shape). Empty outside Tauri. */
export async function downloadsState(): Promise<DownloadsSnapshot> {
  if (!isTauri()) return { batch: null, items: [] };
  return invoke<DownloadsSnapshot>("downloads_state");
}

/** Subscribe to download-manager snapshots (`download-progress`, coalesced to
 *  ≤10/s by the backend). Returns an unlisten function. No-op outside Tauri. */
export async function onDownloadProgress(
  cb: (snapshot: DownloadsSnapshot) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<DownloadsSnapshot>("download-progress", (e) => cb(e.payload));
}

/** Retry ONE failed download (per-item retry in the download panel).
 *  Idempotent — a binary that meanwhile resolved returns instantly. */
export async function retryDownload(name: string, version: string): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("retry_download", { name, version });
}

/** The core binary set (the Start-all plan) with cached flags — row source for
 *  the onboarding Install step. Static-ish mock outside Tauri (vite dev). */
export async function coreBinariesPlan(): Promise<PlannedDownload[]> {
  if (!isTauri()) {
    return [
      { id: "caddy-x", name: "caddy", version: "x", label: "Caddy (edge router)", cached: false },
      { id: "nginx-x", name: "nginx", version: "x", label: "Nginx (web server)", cached: false },
      { id: "mysql-x", name: "mysql", version: "x", label: "MySQL 8.4", cached: false },
      { id: "mailpit-x", name: "mailpit", version: "x", label: "Mailpit (mail catcher)", cached: true },
      { id: "adminer-x", name: "adminer", version: "x", label: "Adminer (DB browser)", cached: true },
      { id: "php-fpm-x", name: "php-fpm", version: "x", label: "PHP 8.3 (FPM)", cached: false },
    ];
  }
  return invoke<PlannedDownload[]>("core_binaries_plan");
}

/** Kick off the core-set prefetch (onboarding auto-download). Fire WITHOUT
 *  awaiting — progress arrives via `onDownloadProgress`, and leaving onboarding
 *  never cancels the downloads (they live in the backend hub). */
export async function prefetchCoreBinaries(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("prefetch_core_binaries");
}

/** Detect whether a site runs WordPress (+ version, multisite). Mock outside Tauri. */
export async function wpInfo(id: string): Promise<WpInfo> {
  if (!isTauri()) {
    const isWp = mockSites.find((s) => s.id === id)?.type === "wordpress";
    return { isWordpress: isWp, version: isWp ? "6.8" : null, multisite: false };
  }
  return invoke<WpInfo>("wp_info", { id });
}

/** A packages dir on this machine that WP-CLI would have loaded into rexenv's
 *  `wp` before the command set was pinned (#228). `null` on almost every
 *  machine — the card renders nothing then. `names` EMPTY means "could not be
 *  named", never "none": the card must not turn that into a count. */
export interface WpCliPackages {
  dir: string;
  names: string[];
}

export async function wpCliPackages(): Promise<WpCliPackages | null> {
  if (!isTauri()) {
    // The dev machine that found #228, so the card is reviewable in the browser.
    return {
      dir: "~/.wp-cli/packages",
      names: ["danielbachhuber/php-compat-command", "wp-cli/dist-archive-command"],
    };
  }
  return invoke<WpCliPackages | null>("wp_cli_packages");
}

/** A foreign proxy already answering :443, for the ONBOARDING warning — `null`
 *  for almost everyone. NOT `edgeAnswersAsOurs`: onboarding runs before any
 *  service starts, so "is ours what answers" is false for every clean first
 *  run. Nothing listening is the ORDINARY case here and returns null — saying
 *  anything about it would invent a problem out of the normal state. */
export interface SetupEdgeConflict {
  holder: string | null;
  app: string | null;
  fix: string | null;
}

export async function setupEdgeConflict(): Promise<SetupEdgeConflict | null> {
  if (!isTauri()) return null;
  return invoke<SetupEdgeConflict | null>("setup_edge_conflict");
}

/** Mailpit mail-catcher status + endpoints. Mock fallback outside Tauri. */
export async function mailpitStatus(): Promise<MailpitStatus> {
  if (!isTauri()) {
    return { running: true, smtpPort: 11025, httpPort: 18025, uiUrl: "http://127.0.0.1:18025" };
  }
  return invoke<MailpitStatus>("mailpit_status");
}

/** The log sources selectable for a site. Mock fallback outside Tauri. */
export async function logTargets(siteId: string): Promise<LogTarget[]> {
  if (!isTauri()) {
    const mock = (key: string, label: string, category: LogTarget["category"]): LogTarget => ({
      key,
      label,
      category,
      path: `/Users/dev/Library/Application Support/dev.rexenv.rexenv/logs/${key}`,
    });
    return [
      mock("nginx-access.log", "Nginx access", "server"),
      mock("nginx-error.log", "Nginx error", "server"),
      mock("php-fpm-8.3.log", "PHP-FPM 8.3", "server"),
      mock("caddy-stdout.log", "Caddy (edge)", "server"),
      mock("mysql-error.log", "MySQL", "database"),
      mock("repo-demo.rex-my-plugin.log", "Git job — my-plugin", "git"),
    ];
  }
  return invoke<LogTarget[]>("log_targets", { siteId });
}

/** Last `lines` lines of a log source (polled to follow). Mock outside Tauri. */
export async function tailLog(key: string, lines: number): Promise<string[]> {
  if (!isTauri()) {
    const now = new Date().toLocaleTimeString();
    return Array.from({ length: 12 }, (_, i) => `${now} [${key}] mock log line ${i + 1}`).slice(-lines);
  }
  return invoke<string[]>("tail_log", { key, lines });
}

/** Truncate a service/DB/Git log to empty (append-mode writers keep going). */
export async function logClear(key: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("log_clear", { key });
}

/** Copy a service/DB/Git log to Downloads; resolves to the saved path. */
export async function logDownload(key: string): Promise<string> {
  if (!isTauri()) return `/Users/dev/Downloads/${key}`;
  return invoke<string>("log_download", { key });
}

/** WordPress debug-log status for a site (WP_DEBUG/WP_DEBUG_LOG, path, size). */
export async function wpDebugLogStatus(siteId: string): Promise<WpDebugLogStatus> {
  if (!isTauri()) {
    return {
      debug: true,
      logEnabled: true,
      path: "/Users/dev/Sites/demo/wp-content/debug.log",
      exists: true,
      sizeBytes: 2048,
      indeterminate: false,
    };
  }
  return invoke<WpDebugLogStatus>("wp_debug_log_status", { siteId });
}

/** Last `lines` lines of a site's WordPress debug.log. Mock outside Tauri. */
export async function wpDebugLogTail(siteId: string, lines: number): Promise<string[]> {
  if (!isTauri()) {
    return [
      "[07-Jul-2026 10:12:03 UTC] PHP Notice: Undefined index 'foo' in functions.php on line 12",
      "[07-Jul-2026 10:12:04 UTC] PHP Deprecated: Function create_function() is deprecated",
    ].slice(-lines);
  }
  return invoke<string[]>("wp_debug_log_tail", { siteId, lines });
}

/** Truncate a site's WordPress debug.log to empty. */
export async function wpDebugLogClear(siteId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_debug_log_clear", { siteId });
}

/** Copy a site's debug.log to Downloads; resolves to the saved path. */
export async function wpDebugLogDownload(siteId: string): Promise<string> {
  if (!isTauri()) return "/Users/dev/Downloads/demo.rex-debug.log";
  return invoke<string>("wp_debug_log_download", { siteId });
}

// Mutable copy of the mock inbox so the dev build's delete / mark-read / clear
// actually change state (the real backend talks to Mailpit's HTTP API).
let mockInbox = mockMailList.messages.map((m) => ({ ...m }));

/** Inbox listing, optionally filtered by a Mailpit search query and/or the
 *  unread filter. Mock outside Tauri.
 *
 *  `unreadOnly` is a flag, not something the caller splices into `query`: the
 *  two compose in the BACKEND (`mail::search_query`), so a filter can never
 *  replace the search and widen the list while the user was narrowing it.
 *  `total`/`unread` stay MAILBOX-WIDE while filtering (Mailpit's own contract),
 *  which is what lets the filter chip keep showing how many unread there are. */
export async function mailpitMessages(query?: string, unreadOnly = false): Promise<MailList> {
  if (!isTauri()) {
    const q = query?.trim().toLowerCase();
    const matched = q
      ? mockInbox.filter(
          (m) =>
            m.subject.toLowerCase().includes(q) ||
            m.from.address.toLowerCase().includes(q) ||
            m.snippet.toLowerCase().includes(q),
        )
      : mockInbox;
    const messages = unreadOnly ? matched.filter((m) => !m.read) : matched;
    return { total: mockInbox.length, unread: mockInbox.filter((m) => !m.read).length, messages };
  }
  return invoke<MailList>("mailpit_messages", { query, unreadOnly });
}

/** Mark EVERY captured message read. No per-id variant on purpose — see
 *  `core/mail.rs::mark_all_read`: the same empty-body shape means "everything"
 *  on Mailpit's delete endpoint, so the all-messages case gets its own name
 *  rather than an id list someone can accidentally pass empty. */
export async function mailpitMarkAllRead(): Promise<void> {
  if (!isTauri()) {
    mockInbox = mockInbox.map((m) => ({ ...m, read: true }));
    return;
  }
  await invoke("mailpit_mark_all_read");
}

/** One message (body + headers) for the preview pane. Mock outside Tauri. */
export async function mailpitMessage(id: string): Promise<MailDetail> {
  if (!isTauri()) return mockMailDetail(id);
  return invoke<MailDetail>("mailpit_message", { id });
}

/** Raw RFC-822 source of a message. Mock outside Tauri. */
export async function mailpitMessageRaw(id: string): Promise<string> {
  if (!isTauri()) {
    const d = mockMailDetail(id);
    return `From: ${d.from.address}\r\nTo: ${d.to.map((t) => t.address).join(", ")}\r\nSubject: ${d.subject}\r\n\r\n${d.text}`;
  }
  return invoke<string>("mailpit_message_raw", { id });
}

/** Delete all captured messages ("Clear all"). Empties the mock inbox off Tauri. */
export async function mailpitClear(): Promise<void> {
  if (!isTauri()) {
    mockInbox = [];
    return;
  }
  await invoke("mailpit_clear");
}

/** Delete specific messages by ID (row delete / bulk selection delete). */
export async function mailpitDelete(ids: string[]): Promise<void> {
  if (ids.length === 0) return; // backend treats empty as an error, never a wipe
  if (!isTauri()) {
    mockInbox = mockInbox.filter((m) => !ids.includes(m.id));
    return;
  }
  await invoke("mailpit_delete", { ids });
}

// ── WordPress Manager — plugins (§6.1) ──────────────────────────────────────

const mockWpPlugins: WpPlugin[] = [
  { name: "akismet", status: "inactive", version: "5.3", update: "available", updateVersion: "5.3.7", title: "Akismet Anti-spam" },
  { name: "hello-dolly", status: "active", version: "1.7.3", update: "none", updateVersion: "", title: "Hello Dolly" },
  { name: "woocommerce", status: "active", version: "9.1.2", update: "none", updateVersion: "", title: "WooCommerce" },
];

/** List a site's plugins (`wp plugin list`). `checkUpdates` opts into the
 *  api.wordpress.org update check — slow (and a hang offline), so list fast
 *  without it and refresh update badges in a background query. Mock fallback
 *  outside Tauri. */
export async function wpPlugins(id: string, checkUpdates = false): Promise<WpPlugin[]> {
  if (!isTauri()) return mockWpPlugins;
  return invoke<WpPlugin[]>("wp_plugins", { id, checkUpdates });
}

/** Live WordPress.org plugin-directory search (Add-plugin flow). Hard 10s
 *  backend timeout; errors surface honestly (the manual slug field still works). */
export async function wpOrgSearchPlugins(query: string): Promise<WpOrgPlugin[]> {
  if (!isTauri()) return [];
  return invoke<WpOrgPlugin[]>("wp_org_search_plugins", { query });
}

/** Live WordPress.org theme-directory search (Add-theme flow). */
export async function wpOrgSearchThemes(query: string): Promise<WpOrgTheme[]> {
  if (!isTauri()) return [];
  return invoke<WpOrgTheme[]>("wp_org_search_themes", { query });
}

/** Icon URLs for installed plugins (plugin-list display) — backend-cached per
 *  app run; slugs wp.org has nothing for map to null (letter-tile fallback in the
 *  UI). A paid plugin (`…-pro`/`…-premium`) is answered with its FREE
 *  counterpart's icon — the artwork wp-admin shows for it too. */
export async function wpOrgPluginIcons(slugs: string[]): Promise<Record<string, string | null>> {
  if (!isTauri() || slugs.length === 0) return {};
  return invoke<Record<string, string | null>>("wp_org_plugin_icons", { slugs });
}

/** Activate plugins (bulk-capable). No-op outside Tauri. */
export async function wpPluginActivate(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_activate", { id, names });
}

/** Deactivate plugins (bulk-capable). No-op outside Tauri. */
export async function wpPluginDeactivate(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_deactivate", { id, names });
}

/** Update plugins (bulk-capable). Resolves when wp-cli exits; progress
 *  arrives meanwhile on `onWpPluginUpdate`. No-op outside Tauri. */
export async function wpPluginUpdate(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_update", { id, names });
}

/** Subscribe to a site's update progress (WP-CLI's own phases) for one noun.
 *  Returns an unlisten fn. */
export async function onWpUpdate(
  channel: "plugins" | "themes" | "core",
  siteId: string,
  cb: (p: WpUpdateProgress) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<WpUpdateProgress>(`wp-update://${channel}/${siteId}`, (e) => cb(e.payload));
}

/** Delete plugins (bulk-capable). No-op outside Tauri. */
export async function wpPluginDelete(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_delete", { id, names });
}

// ── WordPress Manager — themes (§6.2) ───────────────────────────────────────

const mockWpThemes: WpTheme[] = [
  // Titles that are NOT the slug, because that is the shape production has —
  // a fixture where the two match would render identically whether the card
  // reads the title or falls back to the slug.
  { name: "twentytwentyfive", title: "Twenty Twenty-Five", status: "active", version: "1.2", update: "none", updateVersion: "" },
  { name: "twentytwentyfour", title: "Twenty Twenty-Four", status: "inactive", version: "1.3", update: "available", updateVersion: "1.4" },
  // A theme whose header carries no name: the card must fall back to the slug
  // rather than rendering a blank label.
  { name: "custom-child", title: "", status: "inactive", version: "1.6", update: "none", updateVersion: "" },
];

/** The network-enabled subset for the mock. Deliberately NOT all of them and
 *  not none: a fixture where every row is in the same state renders one branch
 *  of the toggle, which is the shape that hides a mis-wired button. */
const MOCK_NETWORK_THEMES = ["twentytwentyfive", "custom-child"];

/** List a site's themes (`wp theme list`), each with its screenshot as a
 *  `data:` URL. `checkUpdates` as in `wpPlugins`. Mock fallback outside Tauri. */
export async function wpThemes(id: string, checkUpdates = false): Promise<WpTheme[]> {
  if (!isTauri()) return mockWpThemes;
  return invoke<WpTheme[]>("wp_themes", { id, checkUpdates });
}

/** Activate a theme (only one live). No-op outside Tauri. */
export async function wpThemeActivate(id: string, name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_activate", { id, name });
}

/** Update themes (bulk-capable). No-op outside Tauri. */
export async function wpThemeUpdate(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_update", { id, names });
}

/** Delete themes (bulk-capable, not the active one). No-op outside Tauri. */
export async function wpThemeDelete(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_delete", { id, names });
}

// ── WordPress Manager — users (§7.1) ────────────────────────────────────────

const mockWpUsers: WpUser[] = [
  { id: 1, login: "admin", email: "admin@acme.rex", roles: "administrator", name: "Admin" },
  { id: 2, login: "editor", email: "editor@acme.rex", roles: "editor", name: "Ed Itor" },
];

/** List a site's WordPress users (`wp user list`). Mock fallback outside Tauri. */
export async function wpUsers(id: string): Promise<WpUser[]> {
  if (!isTauri()) return mockWpUsers;
  return invoke<WpUser[]>("wp_users", { id });
}

/** Create a WordPress user (`wp user create`) with an explicit password.
 *  No-op outside Tauri. */
export async function wpUserCreate(
  id: string,
  login: string,
  email: string,
  role: string,
  password: string,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_user_create", { id, login, email, role, password });
}

/** Change an existing user's password (`wp user update --user_pass`). */
export async function wpUserSetPassword(id: string, userId: number, password: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_user_set_password", { id, userId, password });
}

/** Change a user's role (stock roles only; the primary administrator is
 *  refused by the backend). No-op outside Tauri. */
export async function wpUserSetRole(id: string, userId: number, role: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_user_set_role", { id, userId, role });
}

/** The site's primary administrator id (lowest-ID admin) — its role is locked. */
export async function wpPrimaryAdmin(id: string): Promise<number> {
  if (!isTauri()) return 1;
  return invoke<number>("wp_primary_admin", { id });
}

/** Issue a one-time "Log in as" magic URL for a user. Desktop-app only. */
export async function wpUserLoginUrl(id: string, userId: number): Promise<string> {
  if (!isTauri()) throw new Error('"Log in as" requires the rexenv desktop app.');
  return invoke<string>("wp_user_login_url", { id, userId });
}

/** One-click "Open admin": a one-time magic URL that logs the browser in as
 *  the site's PRIMARY administrator and lands on /wp-admin/. Same hardened
 *  single-use / short-TTL / loopback-only token as `wpUserLoginUrl`. Throws if
 *  the link can't be issued (no admin user, tools missing) — callers fall back
 *  to the plain login page. Desktop-app only. */
export async function wpAdminLoginUrl(id: string): Promise<string> {
  if (!isTauri()) throw new Error('"Open admin" requires the rexenv desktop app.');
  return invoke<string>("wp_admin_login_url", { id });
}

// ── WordPress Manager — tools (§7.2) ────────────────────────────────────────

/** Whether WP_DEBUG is on. Mock fallback outside Tauri. */
export async function wpDebugGet(id: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("wp_debug_get", { id });
}

/** Toggle WP_DEBUG. No-op outside Tauri. */
export async function wpDebugSet(id: string, on: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_debug_set", { id, on });
}

/** Verify core files against wordpress.org checksums. ok=false + warnings is a
 *  normal result (modified/missing/foreign files), not a thrown error. */
export async function wpCoreVerifyChecksums(id: string): Promise<WpChecksumReport> {
  if (!isTauri()) return { ok: true, real: [], benign: [], output: "" };
  return invoke<WpChecksumReport>("wp_core_verify_checksums", { id });
}

/** Installable WordPress releases, newest first (stable-check API — network). */
export async function wpCoreVersions(): Promise<WpCoreVersion[]> {
  if (!isTauri()) return [];
  return invoke<WpCoreVersion[]>("wp_core_versions");
}

/** Switch core to an exact version (downgrade uses --force). Success is gated
 *  on `wp core version` reporting the target; dbUpdateRequired says whether
 *  wp-admin will ask to update the database. */
export async function wpCoreSwitchVersion(id: string, version: string): Promise<WpCoreSwitch> {
  if (!isTauri()) return { version, dbUpdateRequired: false };
  return invoke<WpCoreSwitch>("wp_core_switch_version", { id, version });
}

/** The whitelisted site-options form (values + timezone/role choice lists).
 *  Only the curated scalar whitelist is reachable — dangerous options
 *  (siteurl, home, active_plugins…) don't exist in this API. */
export async function wpOptions(id: string): Promise<WpOptionsForm> {
  if (!isTauri()) return { fields: [], timezones: [], roles: [] };
  return invoke<WpOptionsForm>("wp_options", { id });
}

/** Update one whitelisted option — the backend re-enforces the whitelist by
 *  name and re-validates the value per kind; non-scalar targets are refused. */
export async function wpOptionUpdate(id: string, name: string, value: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_option_update", { id, name, value });
}

/** Delete the checksum panel's benign macOS-noise files (backend re-validates
 *  every path — noise basename, inside-docroot, no symlinks — and skips rather
 *  than aborts), then re-runs verify and returns the fresh report. */
export async function wpChecksumCleanup(id: string, paths: string[]): Promise<WpChecksumCleanup> {
  if (!isTauri())
    return { removed: 0, skipped: [], report: { ok: true, real: [], benign: [], output: "" } };
  return invoke<WpChecksumCleanup>("wp_checksum_cleanup", { id, paths });
}

/** The site's scheduled cron events, soonest first. */
/** Mock cron events, shaped like a real site's rather than a tidy one.
 *
 *  The empty array this used to return meant the cron panel rendered blank in
 *  the dev harness, so no L2 check could ever see it. The rows below carry the
 *  case the args column exists for: `action_scheduler_run_queue` scheduled
 *  TWICE with different args — which is what WooCommerce's Action Scheduler
 *  does — plus a `publish_future_post` carrying a bare post id. Without args
 *  the first two rows are indistinguishable. */
const mockCronEvents: WpCronEvent[] = [
  { hook: "action_scheduler_run_queue", nextRun: "2026-08-23 10:00:00", nextRunRelative: "3 minutes", recurrence: "1 minute", args: '["WP Cron"]' },
  { hook: "action_scheduler_run_queue", nextRun: "2026-08-23 10:00:00", nextRunRelative: "3 minutes", recurrence: "1 minute", args: '["Async Request"]' },
  { hook: "publish_future_post", nextRun: "2026-08-23 14:30:00", nextRunRelative: "4 hours 33 minutes", recurrence: "Non-repeating", args: "[1284]" },
  { hook: "wp_update_themes", nextRun: "2026-08-23 12:00:00", nextRunRelative: "2 hours", recurrence: "12 hours", args: "" },
  { hook: "wp_privacy_delete_old_export_files", nextRun: "2026-08-23 11:00:00", nextRunRelative: "1 hour", recurrence: "1 hour", args: "" },
];

export async function wpCronEvents(id: string): Promise<WpCronEvent[]> {
  if (!isTauri()) return mockCronEvents;
  return invoke<WpCronEvent[]>("wp_cron_events", { id });
}

/** Run all currently-due cron events; returns WP-CLI's summary message. */
export async function wpCronRunDue(id: string): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("wp_cron_run_due", { id });
}

/** Run one hook's scheduled event(s) immediately, due or not. A hook scheduled
 *  more than once runs every instance (WP-CLI has no per-instance id). */
export async function wpCronRunHook(id: string, hook: string): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("wp_cron_run_hook", { id, hook });
}

/** Flush the object cache; returns WP-CLI's confirmation message. */
export async function wpCacheFlush(id: string): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("wp_cache_flush", { id });
}

/** Delete all transients; returns WP-CLI's "N transients deleted" message. */
export async function wpTransientDeleteAll(id: string): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("wp_transient_delete_all", { id });
}

/** The site's current permalink structure ("" = Plain). */
export async function wpPermalinkGet(id: string): Promise<string> {
  if (!isTauri()) return "/%postname%/";
  return invoke<string>("wp_permalink_get", { id });
}

/** Set the permalink structure (backend accepts the stock presets only) and
 *  flush rewrite rules. No-op outside Tauri. */
export async function wpPermalinkSet(id: string, structure: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_permalink_set", { id, structure });
}

/** Available + installed core languages (`wp language core list`) — feeds the
 *  Language picker. Hits api.wordpress.org (~3s; needs network). */
export async function wpLanguages(id: string): Promise<WpLanguage[]> {
  if (!isTauri())
    return [
      { language: "en_US", englishName: "English (United States)", nativeName: "English (United States)", status: "active" },
      { language: "fr_FR", englishName: "French (France)", nativeName: "Français", status: "uninstalled" },
    ];
  return invoke<WpLanguage[]>("wp_languages", { id });
}

/** Switch the site language in one action: install the core pack if missing
 *  (success gated on `is-installed` — install's exit code lies on a failed
 *  download), then activate. Core translations only. No-op outside Tauri. */
export async function wpSwitchLanguage(id: string, locale: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_switch_language", { id, locale });
}

/** The wp-config debug constants toggleable individually (mirrors the Rust
 *  DEBUG_FLAGS whitelist — the backend rejects anything else). */
export type WpDebugFlag = "WP_DEBUG_LOG" | "WP_DEBUG_DISPLAY" | "SCRIPT_DEBUG";

/** Read one boolean wp-config debug constant (unset ⇒ false). */
export async function wpDebugFlagGet(id: string, name: WpDebugFlag): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("wp_debug_flag_get", { id, name });
}

/** Set one boolean wp-config debug constant. No-op outside Tauri. */
export async function wpDebugFlagSet(id: string, name: WpDebugFlag, on: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_debug_flag_set", { id, name, on });
}

/** Whether maintenance mode is active. Mock fallback outside Tauri. */
export async function wpMaintenanceGet(id: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("wp_maintenance_get", { id });
}

/** Toggle maintenance mode (visitors see the "briefly unavailable" page).
 *  No-op outside Tauri. */
export async function wpMaintenanceSet(id: string, on: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_maintenance_set", { id, on });
}

/** Search-replace across the DB; `dryRun` reports the count without changing data.
 *  Returns the number of replacements. Mock returns a sample count outside Tauri. */
export async function wpSearchReplace(id: string, from: string, to: string, dryRun: boolean): Promise<number> {
  if (!isTauri()) return 7;
  return invoke<number>("wp_search_replace", { id, from, to, dryRun });
}

/** Regenerate permalinks (`wp rewrite flush`). No-op outside Tauri. */
export async function wpRewriteFlush(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_rewrite_flush", { id });
}

/** Update WordPress core. Returns WP-CLI output. */
export async function wpCoreUpdate(id: string): Promise<string> {
  if (!isTauri()) return "WordPress is up to date. (mock)";
  return invoke<string>("wp_core_update", { id });
}

/** Re-download core files of the current version. Returns WP-CLI output. */
export async function wpCoreReinstall(id: string): Promise<string> {
  if (!isTauri()) return "Success: WordPress downloaded. (mock)";
  return invoke<string>("wp_core_reinstall", { id });
}

/** Export the site's database to Downloads (bundled mysqldump). Returns the
 *  written path. */
export async function wpDbExport(id: string): Promise<string> {
  if (!isTauri()) return "~/Downloads/mock.rex-db.sql (mock)";
  return invoke<string>("wp_db_export", { id });
}

/** Export site content as WXR XML into Downloads. Returns the written file
 *  paths — wp-cli may split large exports into several files. */
export async function wpContentExport(id: string): Promise<string[]> {
  if (!isTauri()) return ["~/Downloads/mock.WordPress.2026-07-11.000.xml (mock)"];
  return invoke<string[]>("wp_content_export", { id });
}

/** Import a `.sql` dump into the site's database. DESTRUCTIVE — the dump's
 *  tables overwrite existing ones; call only after the typed confirm. */
export async function wpDbImport(id: string, path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_db_import", { id, path });
}

/** Reset a WP site to a clean single-site install: drops + recreates the
 *  database and re-runs the installer with the default local-dev credentials
 *  (admin / admin). Files stay on disk. */
export async function wpSiteReset(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_site_reset", { id });
}

/** Whether the site still accepts the default admin / admin credentials
 *  (backs the tunnel-share warning). */
export async function wpDefaultCreds(id: string): Promise<boolean> {
  if (!isTauri()) return true;
  return invoke<boolean>("wp_default_creds", { id });
}

/** Convert a WP site to multisite ("subdomain" | "subdirectory"). Returns the
 *  updated site. No-op (null) outside Tauri. */
export async function wpMultisiteConvert(id: string, mode: "subdomain" | "subdirectory"): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("wp_multisite_convert", { id, mode });
}

// ── WordPress Manager — network / multisite (§10.3) ─────────────────────────

const mockNetworkSites: WpNetworkSite[] = [
  { id: "1", url: "https://network.rex/", registered: "2026-06-01 10:00:00", deleted: false },
  { id: "2", url: "https://team.network.rex/", registered: "2026-06-10 09:30:00", deleted: false },
];

/** List the network's sub-sites (`wp site list`). Mock fallback outside Tauri. */
export async function wpNetworkSites(id: string): Promise<WpNetworkSite[]> {
  if (!isTauri()) return mockNetworkSites;
  return invoke<WpNetworkSite[]>("wp_network_sites", { id });
}

/** Create a sub-site by slug (`wp site create --slug=`). No-op outside Tauri. */
export async function wpNetworkSiteCreate(id: string, slug: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_network_site_create", { id, slug });
}

/** Delete a sub-site by blog id (`wp site delete`). No-op outside Tauri. */
export async function wpNetworkSiteDelete(id: string, blogId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_network_site_delete", { id, blogId });
}

/** Network-activate plugins (`wp plugin activate … --network`). No-op outside Tauri. */
export async function wpPluginActivateNetwork(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_activate_network", { id, names });
}

/** Network-deactivate plugins (`wp plugin deactivate … --network`). No-op outside Tauri. */
export async function wpPluginDeactivateNetwork(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_deactivate_network", { id, names });
}

/** The stylesheets the network has enabled for its sub-sites.
 *
 *  A separate call because `wp theme list` cannot answer it: theme `status` is
 *  only active/parent/inactive, with no `active-network` the way plugins have.
 *  Outside Tauri the mock enables the first two themes, so the panel renders
 *  both states. */
export async function wpThemesNetworkEnabled(id: string): Promise<string[]> {
  if (!isTauri()) return MOCK_NETWORK_THEMES;
  return invoke<string[]>("wp_themes_network_enabled", { id });
}

/** Network-enable a theme (`wp theme enable <name> --network`). No-op outside Tauri. */
export async function wpThemeEnableNetwork(id: string, name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_enable_network", { id, name });
}

/** Network-disable a theme (`wp theme disable <name> --network`). No-op outside Tauri. */
export async function wpThemeDisableNetwork(id: string, name: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_disable_network", { id, name });
}

/** List the network's super-admins (`wp super-admin list`). Mock fallback outside Tauri. */
export async function wpSuperAdmins(id: string): Promise<string[]> {
  if (!isTauri()) return ["admin"];
  return invoke<string[]>("wp_super_admins", { id });
}

/** Grant super-admin to a user (`wp super-admin add`). No-op outside Tauri. */
export async function wpSuperAdminAdd(id: string, user: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_super_admin_add", { id, user });
}

/** Per-service status + live metrics. Mock fallback outside Tauri. */
export async function servicesStatus(): Promise<ServiceInfo[]> {
  if (!isTauri()) return mockServices;
  return invoke<ServiceInfo[]>("services_status");
}

/** Per-engine database status + live metrics. Mock fallback outside Tauri. */
export async function databasesStatus(): Promise<DbStatus[]> {
  if (!isTauri()) return mockDatabases;
  return invoke<DbStatus[]>("databases_status");
}

/** Start Mailpit alone (Services-row toggle). No-op outside Tauri. */
export async function startMail(): Promise<void> {
  if (!isTauri()) return;
  await invoke("start_mail");
}

/** Stop Mailpit alone. No-op outside Tauri. */
export async function stopMail(): Promise<void> {
  if (!isTauri()) return;
  await invoke("stop_mail");
}

/** Start a database engine by key (e.g. "postgres"). No-op outside Tauri. */
/** Offered versions per engine key (default first) for the Databases picker. */
export async function dbEngineVersions(): Promise<Record<string, string[]>> {
  if (!isTauri()) return { mysql: ["8.4.6"], mariadb: ["12.3.2"], postgres: ["18.6.0"], redis: ["8.8.0"] };
  return invoke<Record<string, string[]>>("db_engine_versions");
}

/** Switch an engine to another offered version (per-series data dirs — the
 *  backend restarts a running engine on the new version). */
export async function setDbEngineVersion(key: string, version: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_db_engine_version", { key, version });
}

export async function startDatabase(key: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("start_database", { key });
}

/** Stop a database engine by key. No-op outside Tauri. */
export async function stopDatabase(key: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("stop_database", { key });
}

/** Start the shared stack (MySQL + php-fpm + nginx + Caddy). */
export async function startServices(): Promise<void> {
  if (!isTauri()) return;
  await invoke("start_services");
}

/** Stop the shared stack. */
export async function stopServices(): Promise<void> {
  if (!isTauri()) return;
  await invoke("stop_services");
}

// ── Public tunnels (§9) ─────────────────────────────────────────────────────

/** Start (or return the existing) public quick tunnel for a site. Desktop-app only. */
export async function startTunnel(id: string): Promise<TunnelInfo> {
  if (!isTauri()) throw new Error("Public sharing requires the rexenv desktop app.");
  return invoke<TunnelInfo>("start_tunnel", { id });
}

/** Stop a site's public tunnel. No-op outside Tauri. */
export async function stopTunnel(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("stop_tunnel", { id });
}

/** All active public tunnels (domain → URL). Mock fallback outside Tauri. */
export async function tunnelsStatus(): Promise<TunnelInfo[]> {
  if (!isTauri()) {
    // Three tunnels, one per HEALTH — the dev shell renders every state the
    // card can be in, and the L2 probe needs the two that are NOT reachable to
    // prove "Live" is earned rather than painted on anything running.
    return [
      {
        domain: "acme.rex",
        url: "https://blue-cat-runs-fast.trycloudflare.com",
        running: true,
        health: "reachable",
      },
      {
        domain: "portfolio.rex",
        url: "https://green-fox-waits.trycloudflare.com",
        running: true,
        health: "unverified",
      },
      {
        domain: "network.rex",
        url: "https://red-owl-fell.trycloudflare.com",
        running: true,
        health: "broken",
      },
    ];
  }
  return invoke<TunnelInfo[]>("tunnels_status");
}

/** Read a setting (or null). */
export async function getSetting(key: string): Promise<string | null> {
  if (!isTauri()) return null;
  return invoke<string | null>("get_setting", { key });
}

/** Insert/update a setting. No-op outside Tauri. */
export async function setSetting(key: string, value: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_setting", { key, value });
}

/** The resolved sites folder (setting or app-data default). */
export async function sitesFolder(): Promise<string> {
  if (!isTauri()) return "~/rexenv/Sites";
  return invoke<string>("sites_folder");
}

/** The default TLD new sites are created under (setting, else "rex"). */
export async function defaultTld(): Promise<string> {
  if (!isTauri()) return "rex";
  return invoke<string>("default_tld");
}

/** Set the default TLD for new sites. The backend refuses blocked TLDs
 *  (.local, .dev, 2-letter, popular gTLDs) with the reason. Returns the
 *  stored (normalized) value. */
export async function setDefaultTld(tld: string): Promise<string> {
  if (!isTauri()) throw new Error("Changing the default TLD requires the rexenv desktop app.");
  return invoke<string>("set_default_tld", { tld });
}

/** Classify a TLD: blocked (with reason) / warn ("may shadow a real TLD") / safe. */
export async function tldPolicy(tld: string): Promise<TldPolicy> {
  // Mock mirrors the backend tiers: RFC-safe TLDs don't warn, everything else does.
  if (!isTauri())
    return { allowed: true, warn: !["test", "localhost", "example", "invalid"].includes(tld), reason: "" };
  return invoke<TldPolicy>("tld_policy", { tld });
}

// ── rex CLI (Settings) ───────────────────────────────────────────────────────

/** `rex` CLI install state. Mock fallback outside Tauri. */
export async function cliStatus(): Promise<CliStatus> {
  if (!isTauri())
    return { available: true, installed: false, current: false, linkPath: "/usr/local/bin/rex", bundledPath: "/Applications/rexenv.app/Contents/MacOS/rex" };
  return invoke<CliStatus>("cli_status");
}

/** Install/refresh the `rex` PATH symlink (may show ONE admin prompt). */
export async function cliInstall(): Promise<CliStatus> {
  return invoke<CliStatus>("cli_install");
}

// ── DNS & SSL + autostart (Settings, §11.1) ─────────────────────────────────

/** Embedded-DNS + OS-resolver health. Mock fallback outside Tauri. */
export async function dnsStatus(): Promise<DnsStatus> {
  if (!isTauri())
    return { running: true, mode: "agent", port: 15353, resolverInstalled: true, resolverPath: "/etc/resolver/rex", caTrusted: true };
  return invoke<DnsStatus>("dns_status");
}

/** Run first-run system setup: install the .rex backbone resolver (admin prompt)
 *  and trust the local CA (keychain dialog). Idempotent. Desktop-app only. */
export async function systemSetup(): Promise<void> {
  if (!isTauri()) throw new Error("System setup requires the rexenv desktop app.");
  await invoke("system_setup");
}

/** Re-trust the local CA in the user keychain (shows the native auth dialog). */
export async function trustLocalCa(): Promise<void> {
  if (!isTauri()) throw new Error("Trusting the local CA requires the rexenv desktop app.");
  await invoke("trust_local_ca");
}

/** Firefox trust state for the Settings SSL card. Mock fallback outside Tauri. */
export async function firefoxTrustStatus(): Promise<FirefoxTrustStatus> {
  if (!isTauri()) return { installed: false, profiles: 0, forced: 0, caPath: "" };
  return invoke<FirefoxTrustStatus>("firefox_trust_status");
}

/** Force Firefox's OS-roots import pref (`security.enterprise_roots.enabled`)
 *  in every profile via user.js — takes effect on Firefox restart. */
export async function trustCaInFirefox(): Promise<FirefoxTrustStatus> {
  if (!isTauri()) throw new Error("Firefox trust requires the rexenv desktop app.");
  return invoke<FirefoxTrustStatus>("trust_ca_in_firefox");
}

/** Regenerate every site's TLS cert (+ Adminer) and reload the edge. Returns the count. */
export async function regenerateCerts(): Promise<number> {
  if (!isTauri()) throw new Error("Regenerating certs requires the rexenv desktop app.");
  return invoke<number>("regenerate_certs");
}

/** Whether rexenv starts on login. Mock returns false outside Tauri. */
export async function autostartStatus(): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("autostart_status");
}

/** Enable/disable "Start rexenv on login". No-op outside Tauri. */
export async function setAutostart(enabled: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("set_autostart", { enabled });
}

/** Reverse rexenv's system changes: stop services, remove every rexenv resolver
 *  file (.rex + any TLDs added on demand), and untrust the local CA (§3.1).
 *  Desktop-app only. */
export async function uninstallSystem(): Promise<TeardownReport> {
  return invoke<TeardownReport>("uninstall_system");
}

// ── Site blueprints (§11.3) ─────────────────────────────────────────────────

const mockBlueprints: Blueprint[] = [
  {
    id: "seed-woocommerce",
    name: "WordPress + WooCommerce",
    spec: {
      siteType: "wordpress", phpVersion: "8.3", webServer: "nginx", multisite: "none",
      plugins: [{ slug: "woocommerce", activate: true }], themes: [], wpDebug: false, language: "",
    },
  },
  {
    id: "seed-multisite",
    name: "WordPress Multisite (subdirectory)",
    spec: {
      siteType: "wordpress", phpVersion: "8.3", webServer: "nginx", multisite: "subdirectory",
      plugins: [], themes: [], wpDebug: true, language: "",
    },
  },
];

/** All site blueprints (newest first). Mock fallback outside Tauri. */
export async function listBlueprints(): Promise<Blueprint[]> {
  if (!isTauri()) return mockBlueprints;
  return invoke<Blueprint[]>("list_blueprints");
}

/** Insert/update a blueprint (upsert by id). No-op outside Tauri. */
export async function saveBlueprint(blueprint: Blueprint): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_blueprint", { blueprint });
}

/** Delete a blueprint by id. No-op outside Tauri. */
export async function deleteBlueprint(id: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_blueprint", { id });
}

// ── Add plugin/theme from Git ─────────────────────────────────────────────────

/** Parse + `git ls-remote` a pasted repo reference: validates the URL and the
 *  user's access BEFORE any clone, and feeds the branch/tag picker. */
export async function repoProbe(url: string): Promise<RepoProbeResult> {
  return invoke<RepoProbeResult>("repo_probe", { url });
}

/** Start the clone→detect job for one repo into a site's wp-content. Returns
 *  the initial snapshot; progress streams via `onRepoJobState`/`onRepoJobOutput`. */
export async function repoAdd(
  siteId: string,
  kind: "plugin" | "theme",
  url: string,
  gitRef: string | null,
  dirName: string | null,
): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_add", { siteId, kind, url, gitRef, dirName });
}

/** Run ONE offered step (composer/install/build). Explicit by design — these
 *  execute the repo's own scripts; nothing runs without this call. */
export async function repoRunStep(jobId: string, stepKey: string): Promise<void> {
  await invoke("repo_run_step", { jobId, stepKey });
}

/** Cancel the job's running step (kills its whole process group; a cancelled
 *  clone removes its partial checkout). */
export async function repoCancel(jobId: string): Promise<void> {
  await invoke("repo_cancel", { jobId });
}

/** Poll a job's snapshot (re-sync after a panel remount). */
export async function repoJobState(jobId: string): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_job_state", { jobId });
}

/** This session's jobs for one site+kind, creation-ordered. The panel uses
 *  this to RECONNECT to a live/unfinished job after a tab-switch remount —
 *  the backend job survives the UI; a blank panel invited a second run. */
export async function repoSiteJobs(
  siteId: string,
  kind: RepoKind,
): Promise<RepoJobState[]> {
  if (!isTauri()) return [];
  return invoke<RepoJobState[]>("repo_site_jobs", { siteId, kind });
}

/** A site's git-sourced plugin/theme dirs (list badges). */
export async function repoAssets(siteId: string): Promise<GitAsset[]> {
  if (!isTauri()) return [];
  return invoke<GitAsset[]>("repo_assets", { siteId });
}

/** git/node availability for the Git add panel. `refresh` re-resolves the
 *  login-shell env (the Re-detect button). */
export async function repoTools(refresh: boolean): Promise<RepoToolStatus[]> {
  return invoke<RepoToolStatus[]>("repo_tools", { refresh });
}

/** Subscribe to one job's state snapshots. Returns an unlisten fn. */
export async function onRepoJobState(
  id: string,
  cb: (state: RepoJobState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<RepoJobState>(`repo-job://state/${id}`, (e) => cb(e.payload));
}

/** Subscribe to one job's streamed log lines. Returns an unlisten fn. */
export async function onRepoJobOutput(
  id: string,
  cb: (line: string) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>(`repo-job://output/${id}`, (e) => cb(e.payload));
}

/** Start a STREAMED install job (live phase lines, attempt cursor, cancel).
 *  `source` picks what `slugs` means and which backend gate runs it: wp.org
 *  slugs, or absolute local `.zip` paths from `pickZipFiles`. Returns the
 *  initial snapshot; progress via `onWpInstallState`/`onWpInstallOutput`. */
export async function wpInstallJob(
  siteId: string,
  kind: "plugin" | "theme",
  slugs: string[],
  activate: boolean,
  source: "wporg" | "zip" = "wporg",
  /** `--force`: unpack over a destination that already exists (wp-admin's
   *  "Replace current with uploaded"). Defaults OFF — an overwrite discards
   *  whatever is in that directory. */
  force = false,
): Promise<WpInstallState> {
  return invoke<WpInstallState>("wp_install_job", { siteId, kind, source, slugs, activate, force });
}

/** Point the embedded Adminer at the app's palette ("dark" | "light" — resolve
 *  "system" first). The console reads it per request, so its own links keep the
 *  scheme; without it the console follows the OS and can sit in the opposite
 *  theme from the app around it. No-op outside Tauri. */
export async function adminerSetTheme(theme: "dark" | "light"): Promise<void> {
  if (!isTauri()) return;
  await invoke("adminer_set_theme", { theme });
}

/** Cancel a running install (kills the wp-cli process group — safe; the UI
 *  states the honest residuals). */
export async function wpInstallCancel(id: string): Promise<void> {
  return invoke<void>("wp_install_cancel", { id });
}

/** The site's most recent install job — card re-adoption after a remount. */
export async function wpInstallActive(
  siteId: string,
  kind: "plugin" | "theme",
): Promise<WpInstallState | null> {
  if (!isTauri()) return null;
  return invoke<WpInstallState | null>("wp_install_active", { siteId, kind });
}

export async function onWpInstallState(
  id: string,
  cb: (state: WpInstallState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<WpInstallState>(`wp-install://state/${id}`, (e) => cb(e.payload));
}

export async function onWpInstallOutput(
  id: string,
  cb: (line: string) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>(`wp-install://output/${id}`, (e) => cb(e.payload));
}

/** Live git status for one managed asset dir (local, fast, runs no repo code). */
export async function repoAssetStatus(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoAssetStatus> {
  return invoke<RepoAssetStatus>("repo_asset_status", { siteId, kind, dirName });
}

/** wp-content dirs that look like git checkouts but have no provenance row
 *  (the quiet "git?" adopt chips). Empty outside Tauri. */
export async function repoUnmanaged(
  siteId: string,
  kind: "plugin" | "theme",
): Promise<UnmanagedRepo[]> {
  if (!isTauri()) return [];
  return invoke<UnmanagedRepo[]>("repo_unmanaged", { siteId, kind });
}

/** Adopt a manually-cloned/linked checkout: records provenance (origin remote
 *  + current branch) — metadata only, nothing on disk changes. */
export async function repoAdopt(
  siteId: string,
  kind: "plugin" | "theme",
  dirName: string,
): Promise<void> {
  await invoke("repo_adopt", { siteId, kind, dirName });
}

/** Start one git op as a streamed job on a managed asset. Same events/cancel/
 *  one-job-per-dir as the add flow; an op that changes lockfiles OFFERS install
 *  steps.
 *
 *  `targetRef` carries the checkout target for `checkout`, and the stash entry
 *  (`stash@{N}`, validated as a REVISION by its own whitelist) for `stash-pop`.
 *  `reset` is unrecoverable for tracked changes — the caller has already
 *  confirmed by the time this runs. */
export async function repoGitOp(
  siteId: string,
  kind: RepoKind,
  dirName: string,
  op: RepoGitOp,
  targetRef: string | null,
): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_git_op", { siteId, kind, dirName, op, targetRef });
}

/** Stash entries for this checkout, newest first — the Restore picker's list.
 *  Read LIVE on every open, never held: git renumbers the list on every pop,
 *  so a cached `stash@{1}` names a different entry than the row showing it. */
export async function repoStashes(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoStashEntry[]> {
  return invoke<RepoStashEntry[]>("repo_stashes", { siteId, kind, dirName });
}

/** Branch names for the checkout dropdown (local + remote-tracking). */
export async function repoBranches(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoBranches> {
  return invoke<RepoBranches>("repo_branches", { siteId, kind, dirName });
}

/** "Run all": run the job's offered dependency steps sequentially (composer →
 *  install → build), stopping at the first failure; never-ran steps become
 *  "skipped". Returns immediately — progress arrives via job events. */
export async function repoRunOfferedSteps(jobId: string): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_run_offered_steps", { jobId });
}

/** Zero-exec dependency check (pure fs reads + stored-fingerprint compare —
 *  runs no repo code). Returns the SETTLED job snapshot; report lines live in
 *  the job's own -check.log slot. */
export async function repoCheck(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_check", { siteId, kind, dirName });
}

/** PR/MR head refs advertised by origin (network — call lazily). */
export async function repoPullRefs(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoPullRef[]> {
  return invoke<RepoPullRef[]>("repo_pull_refs", { siteId, kind, dirName });
}

/** package.json scripts for one asset + the manager that would run them. */
export async function repoScripts(
  siteId: string,
  kind: RepoKind,
  dirName: string,
): Promise<RepoScriptsInfo> {
  return invoke<RepoScriptsInfo>("repo_scripts", { siteId, kind, dirName });
}

/** Run one script ONCE as a streamed job (explicit click — repo code runs). */
export async function repoScriptJob(
  siteId: string,
  kind: RepoKind,
  dirName: string,
  script: string,
): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_script_job", { siteId, kind, dirName, script });
}

/** Build a distributable zip from a git-managed asset into Downloads. Refuses
 *  before starting a job when the checkout has no `.distignore`. */
export async function repoDistArchive(
  siteId: string,
  kind: "plugin" | "theme",
  dirName: string,
): Promise<RepoJobState> {
  return invoke<RepoJobState>("repo_dist_archive", { siteId, kind, dirName });
}

/** Start watching (npm run dev/watch/…): a session process — dies with the
 *  app, never auto-restarts. One watcher per asset dir. */
export async function repoWatchStart(
  siteId: string,
  kind: RepoKind,
  dirName: string,
  script: string,
): Promise<RepoWatchState> {
  return invoke<RepoWatchState>("repo_watch_start", { siteId, kind, dirName, script });
}

/** Stop a watcher (kills its whole process group). */
export async function repoWatchStop(id: string): Promise<void> {
  await invoke("repo_watch_stop", { id });
}

/** Is the SITE's own folder a git checkout? Pure filesystem — no git spawn. */
export async function repoSiteInfo(siteId: string): Promise<SiteRepoInfo> {
  if (!isTauri()) return { present: false, projectRoot: "", clonedFrom: null };
  return invoke<SiteRepoInfo>("repo_site_info", { siteId });
}

/** Watchers — all (footer chip) or one site+kind's (panel). */
export async function repoWatches(
  siteId: string | null,
  kind: RepoKind | null,
): Promise<RepoWatchState[]> {
  if (!isTauri()) return [];
  return invoke<RepoWatchState[]>("repo_watches", { siteId, kind });
}

/** A watcher's in-memory backlog (seeds the pane on remount). */
export async function repoWatchLog(id: string): Promise<string[]> {
  return invoke<string[]>("repo_watch_log", { id });
}

/** Subscribe to one watcher's state changes. Returns an unlisten fn. */
export async function onRepoWatchState(
  id: string,
  cb: (state: RepoWatchState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<RepoWatchState>(`repo-watch://state/${id}`, (e) => cb(e.payload));
}

/** Subscribe to one watcher's streamed output lines. Returns an unlisten fn. */
export async function onRepoWatchOutput(
  id: string,
  cb: (line: string) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>(`repo-watch://output/${id}`, (e) => cb(e.payload));
}

/** Subscribe to the global watcher list (footer chip). Returns an unlisten fn. */
export async function onRepoWatchGlobal(
  cb: (watchers: RepoWatchState[]) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<RepoWatchState[]>("repo-watch-global", (e) => cb(e.payload));
}

/** Symlink an EXISTING local folder into wp-content (source == "linked").
 *  The folder stays where it is; deleting the asset removes ONLY the link. */
export async function repoLink(
  siteId: string,
  kind: "plugin" | "theme",
  dirName: string | null,
  target: string,
): Promise<RepoLinkResult> {
  return invoke<RepoLinkResult>("repo_link", { siteId, kind, dirName, target });
}

// ── MCP: the "AI agents" Settings card + per-site activity ───────────────────

/** The MCP card's whole state in one read: the opt-in toggle, the derived
 *  status line, and the recent activity feed. Off/empty outside Tauri. */
export async function mcpStatus(): Promise<McpStatus> {
  if (!isTauri())
    return { enabled: false, access: { level: "read", mode: null, expiresAt: null, expired: false, label: "Agent access", allows: "", levels: [] }, connectCommand: "claude mcp add rexenv -- rex mcp", activity: { kind: "off" }, recent: [] };
  return invoke<McpStatus>("mcp_status");
}

/** Flip the opt-in toggle. Enabling BINDS the endpoint's socket (and only then
 *  reads on); disabling drops live sessions and unlinks it. Returns fresh status. */
export async function mcpSetEnabled(enable: boolean): Promise<McpStatus> {
  return invoke<McpStatus>("mcp_set_enabled", { enable });
}

/** The activity feed — all recent rows, or just those naming one site (pass a
 *  site id for the per-site SiteDetail section). Newest first. */
export async function agentActivity(siteId: string | null, limit: number): Promise<AgentAction[]> {
  if (!isTauri()) return [];
  return invoke<AgentAction[]>("agent_activity", { siteId, limit });
}

// ── Agent access (D15): the ONE dial for the user's own sites ─────────────────

/** The dial as it stands: level, duration, expiry, and Rust's copy per level. */
export async function agentAccess(): Promise<AgentAccess> {
  if (!isTauri()) return { level: "read", mode: null, expiresAt: null, expired: false, label: "Agent access", allows: "", levels: [] };
  return invoke<AgentAccess>("agent_access_get");
}

/** Turn the dial. A level above Read needs a duration; Read takes none. */
export async function agentAccessSet(level: AgentAccessLevel, mode: AgentAccessMode | null): Promise<AgentAccess> {
  return invoke<AgentAccess>("agent_access_set", { level, mode });
}

/** Clear the feed — the user's own record, theirs to wipe. Returns rows removed. */
export async function agentActivityClear(): Promise<number> {
  return invoke<number>("agent_activity_clear");
}
