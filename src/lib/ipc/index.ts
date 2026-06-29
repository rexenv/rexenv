/**
 * Typed Tauri IPC bridge. The UI MUST go through these wrappers — never call
 * `invoke` directly from components. Each function maps 1:1 to a Rust command
 * registered in `src-tauri/src/lib.rs`.
 *
 * During early scaffolding the app runs in a plain browser (vite dev) where the
 * Tauri runtime is absent; `isTauri()` lets callers fall back to mock data.
 */
import type { AppInfo, DbStatus, GlobalStatus, LogTarget, MailDetail, MailList, MailpitStatus, NewSiteInput, PhpVersion, ServiceInfo, Site, TunnelInfo, WebServer, WpInfo, WpInstallInput, WpNetworkSite, WpPlugin, WpTheme, WpUser } from "@/types";
import {
  mockDatabases,
  mockGlobalStatus,
  mockMailDetail,
  mockMailList,
  mockPhpVersions,
  mockServices,
  mockSites,
} from "@/lib/mock";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Lazily import the Tauri API so a browser-only dev build doesn't crash. */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Round-trip smoke test for the IPC bridge (task 0.5). */
export async function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
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

/** Mark a site running. No-op outside Tauri. */
export async function startSite(id: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("start_site", { id });
}

/** Mark a site stopped. No-op outside Tauri. */
export async function stopSite(id: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("stop_site", { id });
}

/** Create a site (provision + WordPress one-click install when type=wordpress +
 *  bring up if the stack is running). No-op outside Tauri. */
export async function createSite(input: NewSiteInput, wp?: WpInstallInput): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("create_site", { site: input, wp });
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

/** Open a path or URL in the OS default handler (Finder / browser). Falls back
 *  to `window.open` for URLs outside Tauri. */
export async function openExternal(target: string): Promise<void> {
  if (!isTauri()) {
    if (/^https?:\/\//.test(target)) window.open(target, "_blank");
    return;
  }
  await invoke("open_external", { target });
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
    return { running: true, smtpPort: 1025, httpPort: 8025, uiUrl: "http://127.0.0.1:8025" };
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

/** Inbox listing, optionally filtered by a Mailpit search query. Mock outside Tauri. */
export async function mailpitMessages(query?: string): Promise<MailList> {
  if (!isTauri()) {
    const q = query?.trim().toLowerCase();
    if (!q) return mockMailList;
    const messages = mockMailList.messages.filter(
      (m) =>
        m.subject.toLowerCase().includes(q) ||
        m.from.address.toLowerCase().includes(q) ||
        m.snippet.toLowerCase().includes(q),
    );
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

/** Delete all captured messages ("Clear all"). No-op outside Tauri. */
export async function mailpitClear(): Promise<void> {
  if (!isTauri()) return;
  await invoke("mailpit_clear");
}

// ── WordPress Manager — plugins (§6.1) ──────────────────────────────────────

const mockWpPlugins: WpPlugin[] = [
  { name: "akismet", status: "inactive", version: "5.3", update: "available" },
  { name: "hello-dolly", status: "active", version: "1.7.3", update: "none" },
  { name: "woocommerce", status: "active", version: "9.1.2", update: "none" },
];

/** List a site's plugins (`wp plugin list`). Mock fallback outside Tauri. */
export async function wpPlugins(id: string): Promise<WpPlugin[]> {
  if (!isTauri()) return mockWpPlugins;
  return invoke<WpPlugin[]>("wp_plugins", { id });
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

/** List a site's themes (`wp theme list`). Mock fallback outside Tauri. */
export async function wpThemes(id: string): Promise<WpTheme[]> {
  if (!isTauri()) return mockWpThemes;
  return invoke<WpTheme[]>("wp_themes", { id });
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

/** Issue a one-time "Log in as" magic URL for a user. Desktop-app only. */
export async function wpUserLoginUrl(id: string, userId: number): Promise<string> {
  if (!isTauri()) throw new Error('"Log in as" requires the rexenv desktop app.');
  return invoke<string>("wp_user_login_url", { id, userId });
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
  if (!isTauri()) return "~/Library/Application Support/dev.rexenv.app/sites";
  return invoke<string>("sites_folder");
}
