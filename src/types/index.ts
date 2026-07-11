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

/** Read-only identity of a site's HTTPS leaf cert (mirrors the Rust SiteCertInfo).
 *  Dates are RFC 3339 UTC; `daysLeft` goes negative once expired. */
export interface SiteCertInfo {
  notBefore: string;
  notAfter: string;
  daysLeft: number;
  sans: string[];
  certDir: string;
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
  /** rexenv's OWN total: sum of per-core % across every supervised process
   *  tree (Activity-Monitor style — can exceed 100). NOT machine-wide usage. */
  cpuPercent: number;
  /** Logical cores — divide cpuPercent by this for a 0-100 machine share. */
  cpuCores: number;
  /** rexenv's OWN total RAM (all supervised process trees), MB. */
  ramMb: number;
  /** The machine's total RAM, MB — meter denominator only. */
  ramTotalMb: number;
}

/** Honest per-site resources (mirrors the Rust SiteResources). A site is not a
 *  process: only FrankenPHP-override sites (`dedicated`) have real CPU/RAM;
 *  shared nginx+pool sites get ACTIVITY (last-60s requests/bytes) + DB size —
 *  never a fabricated per-site CPU/RAM. */
export interface SiteResources {
  id: string;
  domain: string;
  dedicated: boolean;
  cpuPercent: number | null;
  ramMb: number | null;
  requestsPerMin: number | null;
  bytesPerMin: number | null;
  dbSizeBytes: number | null;
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

/** A core language row (mirrors the Rust WpLanguage DTO / `wp language core list`). */
export interface WpLanguage {
  language: string; // locale code, e.g. fr_FR
  englishName: string;
  nativeName: string;
  status: string; // active | installed | uninstalled
}

/** A WordPress theme row (mirrors the Rust WpTheme DTO / `wp theme list`). */
export interface WpTheme {
  name: string;
  status: string; // active | inactive | parent
  version: string;
  update: string; // none | available | …
  /** The theme's screenshot.* preview as a `data:` URL; null when it has none. */
  screenshot?: string | null;
}

/** A WordPress user row (mirrors the Rust WpUser DTO / `wp user list`). */
export interface WpUser {
  id: number;
  login: string;
  email: string;
  roles: string; // comma-separated
  name: string;
}

/** Result of `wp core verify-checksums` (mirrors the Rust WpChecksumReport).
 *  A failed verification is a normal result, split into real issues
 *  (modified/missing/foreign core files) vs benign OS clutter (.DS_Store etc.,
 *  "should not exist" findings only). */
export interface WpChecksumReport {
  /** Raw wp-cli exit verdict. Extra "should not exist" files do NOT fail it
   *  (exit 0 + Success line); only modified/missing core files do. Never
   *  drive a pass decision from `ok` alone — `real` is the signal. */
  ok: boolean;
  real: string[];
  benign: string[];
  output: string;
}

/** One scheduled cron event (mirrors the Rust WpCronEvent / `wp cron event list`). */
export interface WpCronEvent {
  hook: string;
  nextRun: string; // GMT timestamp, e.g. "2026-07-11 12:00:00"
  nextRunRelative: string; // e.g. "11 hours 4 minutes"
  recurrence: string; // "1 hour", "1 day", … or "Non-repeating"
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

/** WordPress debug-log status (mirrors the Rust WpDebugLogStatus DTO). */
export interface WpDebugLogStatus {
  debug: boolean; // WP_DEBUG constant
  logEnabled: boolean; // WP_DEBUG_LOG truthy or a custom path
  path: string; // resolved debug.log path
  exists: boolean;
  sizeBytes: number;
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

/** Where one binary download is in its life (mirrors Rust `downloads::Phase`).
 *  `preparing` = post-download extract/relink/codesign; `cached` = was already
 *  on disk when the action planned its batch. */
export type DownloadPhase =
  | "pending"
  | "downloading"
  | "preparing"
  | "done"
  | "cached"
  | "failed";

/** One binary's download state (mirrors the Rust `downloads::ItemSnapshot`). */
export interface DownloadItem {
  id: string;
  /** Manifest name + pinned version — pass back to `retryDownload`. */
  name: string;
  version: string;
  label: string;
  phase: DownloadPhase;
  downloadedBytes: number;
  /** null = server sent no Content-Length → render indeterminate. */
  totalBytes: number | null;
  bytesPerSec: number | null;
  error: string | null;
}

/** The active action's batch: `done`/`total` count only items that actually
 *  needed downloading (cached rows are listed but not counted). */
export interface DownloadBatch {
  action: string;
  done: number;
  total: number;
}

/** Full download-manager state — the `download-progress` event payload and the
 *  `downloads_state` seed share this shape (snapshot, not delta). */
export interface DownloadsSnapshot {
  batch: DownloadBatch | null;
  items: DownloadItem[];
}

/** One planned core binary (onboarding Install rows): static row source with a
 *  cached flag; live progress overlays via `DownloadsSnapshot` items by `id`. */
export interface PlannedDownload {
  id: string;
  name: string;
  version: string;
  label: string;
  cached: boolean;
}
