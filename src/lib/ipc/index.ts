/**
 * Typed Tauri IPC bridge. The UI MUST go through these wrappers — never call
 * `invoke` directly from components. Each function maps 1:1 to a Rust command
 * registered in `src-tauri/src/lib.rs`.
 *
 * During early scaffolding the app runs in a plain browser (vite dev) where the
 * Tauri runtime is absent; `isTauri()` lets callers fall back to mock data.
 */
import type { AppInfo, Blueprint, DbStatus, DnsStatus, DownloadsSnapshot, GlobalStatus, LogTarget, MailDetail, MailList, MailpitStatus, NewSiteInput, PhpVersion, PlannedDownload, ServiceInfo, Site, SiteCertInfo, SiteResources, SiteServing, TunnelInfo, WebServer, WpChecksumReport, WpCronEvent, WpDebugLogStatus, WpInfo, WpInstallInput, WpNetworkSite, WpPlugin, WpTheme, WpUser } from "@/types";
import {
  mockAppInfo,
  mockDatabases,
  mockGlobalStatus,
  mockMailDetail,
  mockMailList,
  mockPhpVersions,
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

/** Create a site (provision + WordPress one-click install when type=wordpress +
 *  bring up if the stack is running). A `blueprintId` applies that preset's
 *  plugins/themes/multisite after install (§11.3). No-op outside Tauri. */
export async function createSite(
  input: NewSiteInput,
  wp?: WpInstallInput,
  blueprintId?: string,
): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("create_site", { site: input, wp, blueprintId });
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

/** Delete a site (DB row + cert + docroot). No-op outside Tauri. */
export async function deleteSite(id: string): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("delete_site", { id });
}

/** All registered PHP versions (installed + available). Mock fallback outside Tauri. */
export async function listPhpVersions(): Promise<PhpVersion[]> {
  if (!isTauri()) return mockPhpVersions;
  return invoke<PhpVersion[]>("list_php_versions");
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

/** Open a path or URL in the OS default handler (Finder / browser). Falls back
 *  to `window.open` for URLs outside Tauri. */
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

/** Open a PTY shell in a site's docroot (bundled PHP + `wp` on PATH). Returns the
 *  session id; output streams via {@link onTerminalOutput}. Desktop-app only. */
export async function openTerminal(siteId: string, rows: number, cols: number): Promise<string> {
  if (!isTauri()) throw new Error("The terminal requires the rexenv desktop app.");
  return invoke<string>("terminal_open", { siteId, rows, cols });
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
  action: "restarted" | "restart-failed" | "gave-up" | "edge-down" | "adopted";
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
    return [
      { key: "nginx-access.log", label: "Nginx access" },
      { key: "nginx-error.log", label: "Nginx error" },
      { key: "php-fpm-8.3.log", label: "PHP-FPM 8.3" },
      { key: "caddy-stdout.log", label: "Caddy (edge)" },
      { key: "mysql-error.log", label: "MySQL" },
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

/** WordPress debug-log status for a site (WP_DEBUG/WP_DEBUG_LOG, path, size). */
export async function wpDebugLogStatus(siteId: string): Promise<WpDebugLogStatus> {
  if (!isTauri()) {
    return {
      debug: true,
      logEnabled: true,
      path: "/Users/dev/Sites/demo/wp-content/debug.log",
      exists: true,
      sizeBytes: 2048,
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
  if (!isTauri()) return "/Users/dev/Downloads/demo.test-debug.log";
  return invoke<string>("wp_debug_log_download", { siteId });
}

// Mutable copy of the mock inbox so the dev build's delete / mark-read / clear
// actually change state (the real backend talks to Mailpit's HTTP API).
let mockInbox = mockMailList.messages.map((m) => ({ ...m }));

/** Inbox listing, optionally filtered by a Mailpit search query. Mock outside Tauri. */
export async function mailpitMessages(query?: string): Promise<MailList> {
  if (!isTauri()) {
    const q = query?.trim().toLowerCase();
    const messages = q
      ? mockInbox.filter(
          (m) =>
            m.subject.toLowerCase().includes(q) ||
            m.from.address.toLowerCase().includes(q) ||
            m.snippet.toLowerCase().includes(q),
        )
      : mockInbox;
    return { total: messages.length, unread: messages.filter((m) => !m.read).length, messages };
  }
  return invoke<MailList>("mailpit_messages", { query });
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

// ── WordPress Manager — plugins (§6.1) ──────────────────────────────────────

const mockWpPlugins: WpPlugin[] = [
  { name: "akismet", status: "inactive", version: "5.3", update: "available" },
  { name: "hello-dolly", status: "active", version: "1.7.3", update: "none" },
  { name: "woocommerce", status: "active", version: "9.1.2", update: "none" },
];

/** List a site's plugins (`wp plugin list`). `checkUpdates` opts into the
 *  api.wordpress.org update check — slow (and a hang offline), so list fast
 *  without it and refresh update badges in a background query. Mock fallback
 *  outside Tauri. */
export async function wpPlugins(id: string, checkUpdates = false): Promise<WpPlugin[]> {
  if (!isTauri()) return mockWpPlugins;
  return invoke<WpPlugin[]>("wp_plugins", { id, checkUpdates });
}

/** Install a plugin by slug (optionally activate). No-op outside Tauri. */
export async function wpPluginInstall(id: string, slug: string, activate: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_install", { id, slug, activate });
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

/** Update plugins (bulk-capable). No-op outside Tauri. */
export async function wpPluginUpdate(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_update", { id, names });
}

/** Delete plugins (bulk-capable). No-op outside Tauri. */
export async function wpPluginDelete(id: string, names: string[]): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_plugin_delete", { id, names });
}

// ── WordPress Manager — themes (§6.2) ───────────────────────────────────────

const mockWpThemes: WpTheme[] = [
  { name: "twentytwentyfive", status: "active", version: "1.2", update: "none" },
  { name: "twentytwentyfour", status: "inactive", version: "1.3", update: "available" },
  { name: "twentytwentythree", status: "inactive", version: "1.6", update: "none" },
];

/** List a site's themes (`wp theme list`), each with its screenshot as a
 *  `data:` URL. `checkUpdates` as in `wpPlugins`. Mock fallback outside Tauri. */
export async function wpThemes(id: string, checkUpdates = false): Promise<WpTheme[]> {
  if (!isTauri()) return mockWpThemes;
  return invoke<WpTheme[]>("wp_themes", { id, checkUpdates });
}

/** Install a theme by slug (optionally activate). No-op outside Tauri. */
export async function wpThemeInstall(id: string, slug: string, activate: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_theme_install", { id, slug, activate });
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
  { id: 1, login: "admin", email: "admin@acme.test", roles: "administrator", name: "Admin" },
  { id: 2, login: "editor", email: "editor@acme.test", roles: "editor", name: "Ed Itor" },
];

/** List a site's WordPress users (`wp user list`). Mock fallback outside Tauri. */
export async function wpUsers(id: string): Promise<WpUser[]> {
  if (!isTauri()) return mockWpUsers;
  return invoke<WpUser[]>("wp_users", { id });
}

/** Create a WordPress user (`wp user create`). No-op outside Tauri. */
export async function wpUserCreate(id: string, login: string, email: string, role: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("wp_user_create", { id, login, email, role });
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

/** The site's scheduled cron events, soonest first. */
export async function wpCronEvents(id: string): Promise<WpCronEvent[]> {
  if (!isTauri()) return [];
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
  if (!isTauri()) return "~/Downloads/mock.test-db.sql (mock)";
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
  { id: "1", url: "https://network.test/", registered: "2026-06-01 10:00:00", deleted: false },
  { id: "2", url: "https://team.network.test/", registered: "2026-06-10 09:30:00", deleted: false },
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

/** Start a database engine by key (e.g. "postgres"). No-op outside Tauri. */
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
    return [{ domain: "acme.test", url: "https://blue-cat-runs-fast.trycloudflare.com", running: true }];
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

// ── DNS & SSL + autostart (Settings, §11.1) ─────────────────────────────────

/** Embedded-DNS + OS-resolver health. Mock fallback outside Tauri. */
export async function dnsStatus(): Promise<DnsStatus> {
  if (!isTauri())
    return { running: true, port: 15353, resolverInstalled: true, resolverPath: "/etc/resolver/test", caTrusted: true };
  return invoke<DnsStatus>("dns_status");
}

/** Run first-run system setup: install the .test resolver (admin prompt) and trust
 *  the local CA (keychain dialog). Idempotent. Desktop-app only. */
export async function systemSetup(): Promise<void> {
  if (!isTauri()) throw new Error("System setup requires the rexenv desktop app.");
  await invoke("system_setup");
}

/** Re-trust the local CA in the user keychain (shows the native auth dialog). */
export async function trustLocalCa(): Promise<void> {
  if (!isTauri()) throw new Error("Trusting the local CA requires the rexenv desktop app.");
  await invoke("trust_local_ca");
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

/** Reverse rexenv's system changes: stop services, remove the .test resolver, and
 *  untrust the local CA (§3.1). Desktop-app only. */
export async function uninstallSystem(): Promise<void> {
  if (!isTauri()) throw new Error("Removing system changes requires the rexenv desktop app.");
  await invoke("uninstall_system");
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
