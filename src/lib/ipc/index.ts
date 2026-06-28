/**
 * Typed Tauri IPC bridge. The UI MUST go through these wrappers — never call
 * `invoke` directly from components. Each function maps 1:1 to a Rust command
 * registered in `src-tauri/src/lib.rs`.
 *
 * During early scaffolding the app runs in a plain browser (vite dev) where the
 * Tauri runtime is absent; `isTauri()` lets callers fall back to mock data.
 */
import type { AppInfo, DbStatus, GlobalStatus, MailDetail, MailList, MailpitStatus, NewSiteInput, PhpVersion, ServiceInfo, Site, WebServer, WpInfo, WpInstallInput } from "@/types";
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
