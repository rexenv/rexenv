/**
 * Typed Tauri IPC bridge. The UI MUST go through these wrappers — never call
 * `invoke` directly from components. Each function maps 1:1 to a Rust command
 * registered in `src-tauri/src/lib.rs`.
 *
 * During early scaffolding the app runs in a plain browser (vite dev) where the
 * Tauri runtime is absent; `isTauri()` lets callers fall back to mock data.
 */
import type { AppInfo, DbStatus, GlobalStatus, PhpVersion, ServiceInfo, Site } from "@/types";
import {
  mockDatabases,
  mockGlobalStatus,
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

/** Switch a site's PHP version (DB + reload, no rebuild). Returns the updated site. */
export async function setSitePhpVersion(id: string, version: string): Promise<Site | null> {
  if (!isTauri()) return null;
  return invoke<Site | null>("set_site_php_version", { id, version });
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
