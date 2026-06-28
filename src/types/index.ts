/**
 * Shared TS types — these mirror the Rust structs exposed over IPC.
 * Keep field names in sync with `src-tauri/src/state/models.rs`.
 */

export type ServiceStatus = "running" | "stopped" | "starting" | "error";

export type WebServer = "nginx" | "apache" | "frankenphp" | "openlitespeed";

export type SiteType = "wordpress" | "laravel" | "php";

export interface Site {
  id: string;
  name: string;
  domain: string; // e.g. "mysite.test"
  type: SiteType;
  status: ServiceStatus;
  phpVersion: string; // e.g. "8.3"
  webServer: WebServer;
  ssl: boolean;
  path: string;
  createdAt: string; // SQLite datetime, mirrors the Rust Site struct
}

/** Input for creating a site (mirrors the Rust NewSite). */
export interface NewSiteInput {
  name: string;
  domain: string;
  type: SiteType;
  phpVersion: string;
  webServer: WebServer;
  path: string; // empty → core computes the docroot under the sites dir
}

/** WordPress one-click install fields (type=wordpress). Empty fields default
 *  server-side (title→name, admin→admin, email→admin@domain, language→en_US). */
export interface WpInstallInput {
  title?: string;
  adminUser?: string;
  adminEmail?: string;
  adminPassword?: string;
  language?: string; // WP locale, e.g. "fr_FR"; "" → en_US
}

export interface GlobalStatus {
  /** "All running" | "Partial" | "Stopped" derived from service states */
  summary: "all" | "partial" | "stopped";
  running: number;
  total: number;
  cpuPercent: number;
  ramMb: number;
  ramTotalMb: number;
}

/** A PHP version in the registry (mirrors the Rust PhpVersion). */
export interface PhpVersion {
  minor: string; // "8.3" — the key + what Site.phpVersion references
  patch: string; // "8.3.31"
  fpmPort: number;
  installed: boolean;
  isDefault: boolean;
}

export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
}

/** One shared service's status + live metrics (mirrors the Rust ServiceStatus DTO). */
export interface ServiceInfo {
  name: string;
  running: boolean;
  pid: number | null;
  port: number;
  cpuPercent: number;
  ramMb: number;
}

/** One database engine's status + live metrics (mirrors the Rust DbStatus DTO). */
export interface DbStatus {
  key: string; // "mysql" | "postgres" | …
  label: string; // "MySQL" | "PostgreSQL"
  port: number;
  version: string; // pinned version, e.g. "8.4.6"
  running: boolean;
  pid: number | null;
  cpuPercent: number;
  ramMb: number;
}

/** WordPress detection for a site's docroot (mirrors the Rust WpInfo DTO). */
export interface WpInfo {
  isWordpress: boolean;
  version: string | null; // wp core version, when WordPress
  multisite: boolean;
}

/** Mailpit mail-catcher health + endpoints (mirrors the Rust MailpitStatus DTO). */
export interface MailpitStatus {
  running: boolean;
  smtpPort: number;
  httpPort: number;
  uiUrl: string;
}

/** An email address (display name may be empty). */
export interface MailAddress {
  name: string;
  address: string;
}

/** One captured message in the inbox list (mirrors the Rust MailSummary DTO). */
export interface MailSummary {
  id: string;
  from: MailAddress;
  to: MailAddress[];
  subject: string;
  created: string; // ISO timestamp
  read: boolean;
  snippet: string;
}

/** The inbox listing (counts + a page of messages). */
export interface MailList {
  total: number;
  unread: number;
  messages: MailSummary[];
}

/** One header row (repeated values joined). */
export interface MailHeader {
  name: string;
  value: string;
}

/** A full message for the preview pane (mirrors the Rust MailDetail DTO). */
export interface MailDetail {
  id: string;
  from: MailAddress;
  to: MailAddress[];
  cc: MailAddress[];
  subject: string;
  date: string;
  text: string;
  html: string;
  headers: MailHeader[];
}
