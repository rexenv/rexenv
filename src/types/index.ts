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
  multisite: MultisiteMode; // WordPress multisite mode (§10.1)
}

/** WordPress multisite mode (mirrors the Rust MultisiteMode). */
export type MultisiteMode = "none" | "subdomain" | "subdirectory";

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

/** Live per-site serving status (mirrors the Rust SiteServing). `serving` is true
 *  only when the edge is up AND the site's own upstream is up. Keyed by domain. */
export interface SiteServing {
  domain: string;
  serving: boolean;
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
  /** Human-readable OS + CPU, e.g. "macOS · Apple silicon" (from the build target). */
  platform: string;
}

/** Which group a service belongs to on the Services screen. */
export type ServiceKind = "php" | "database" | "mail" | "web";

/** One shared service's status + live metrics (mirrors the Rust ServiceStatus DTO). */
export interface ServiceInfo {
  name: string;
  running: boolean;
  pid: number | null;
  port: number;
  cpuPercent: number;
  ramMb: number;
  // Optional UI hints — the Rust DTO may not send these yet; the frontend
  // derives `kind` from the name as a fallback (see serviceKind).
  kind?: ServiceKind;
  version?: string;
  isDefault?: boolean;
  isRouter?: boolean;
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

/** A per-site public tunnel (mirrors the Rust TunnelInfo DTO). */
export interface TunnelInfo {
  domain: string;
  url: string; // public https://<id>.trycloudflare.com
  running: boolean;
}

/** A WordPress plugin row (mirrors the Rust WpPlugin DTO / `wp plugin list`). */
export interface WpPlugin {
  name: string;
  status: string; // active | inactive | active-network | must-use | dropin
  version: string;
  update: string; // none | available | …
}

/** A WordPress theme row (mirrors the Rust WpTheme DTO / `wp theme list`). */
export interface WpTheme {
  name: string;
  status: string; // active | inactive | parent
  version: string;
  update: string; // none | available | …
}

/** A WordPress user row (mirrors the Rust WpUser DTO / `wp user list`). */
export interface WpUser {
  id: number;
  login: string;
  email: string;
  roles: string; // comma-separated
  name: string;
}

/** One plugin/theme in a blueprint (slug + activate-on-install). §11.3 */
export interface BlueprintItem {
  slug: string;
  activate: boolean;
}

/** A blueprint's reusable recipe (mirrors the Rust BlueprintSpec). §11.3 */
export interface BlueprintSpec {
  siteType: SiteType;
  phpVersion: string;
  webServer: WebServer;
  multisite: MultisiteMode;
  plugins: BlueprintItem[];
  themes: BlueprintItem[];
  wpDebug: boolean;
  language: string;
}

/** A named, reusable site preset (mirrors the Rust Blueprint). §11.3 */
export interface Blueprint {
  id: string;
  name: string;
  spec: BlueprintSpec;
}

/** Embedded-DNS + OS-resolver health for Settings (mirrors the Rust DnsStatus DTO). */
export interface DnsStatus {
  running: boolean; // the embedded resolver is bound on its loopback port
  port: number;
  resolverInstalled: boolean; // /etc/resolver/test present
  resolverPath: string;
  caTrusted: boolean; // local CA trusted for THIS user (per-user, unlike the resolver)
}

/** A network sub-site row (mirrors the Rust WpNetworkSite DTO / `wp site list`). */
export interface WpNetworkSite {
  id: string; // blog_id (1 = main site)
  url: string; // full sub-site URL
  registered: string;
  deleted: boolean; // soft-deleted / archived
}

/** Mailpit mail-catcher health + endpoints (mirrors the Rust MailpitStatus DTO). */
export interface MailpitStatus {
  running: boolean;
  smtpPort: number;
  httpPort: number;
  uiUrl: string;
}

/** A selectable log source for the Logs viewer (mirrors the Rust LogTarget DTO). */
export interface LogTarget {
  key: string; // file name within the log dir
  label: string;
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
