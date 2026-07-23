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
  domain: string; // e.g. "mysite.rex"
  type: SiteType;
  status: ServiceStatus;
  phpVersion: string; // e.g. "8.3"
  webServer: WebServer;
  ssl: boolean;
  path: string;
  createdAt: string; // SQLite datetime, mirrors the Rust Site struct
  multisite: MultisiteMode; // WordPress multisite mode (§10.1)
  dbName: string; // database name — stored at creation, stable across domain changes
  dbEngine: SiteDbEngine; // SQL engine hosting that database (chosen at create)
  xdebug: boolean; // per-site Xdebug toggle (§8.2) — routes .php to the minor's debug pool
}

/** SQL engine backing a site's database (mirrors the Rust SiteDbEngine). */
export type SiteDbEngine = "mysql" | "mariadb";

/** WordPress multisite mode (mirrors the Rust MultisiteMode). */
export type MultisiteMode = "none" | "subdomain" | "subdirectory";

/** One per-site environment variable (mirrors the Rust EnvVarInput). PHP sees
 *  it via getenv(), $_SERVER and $_ENV on both nginx and FrankenPHP sites. */
export interface EnvVar {
  name: string;
  value: string;
}

/** Result of a domain change (mirrors the Rust DomainChange): the updated site,
 *  where the pre-change DB backup landed (WordPress only), and how many
 *  search-replace substitutions ran. */
export interface DomainChange {
  site: Site;
  backupPath: string | null;
  replacements: number;
}

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
  dbEngine?: SiteDbEngine; // omitted → mysql (serde default)
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

/** One editable per-version PHP ini setting (mirrors the Rust PhpSettingView).
 *  `value` null = unset — PHP's compiled `default` applies (no php.ini is loaded). */
export interface PhpSetting {
  key: string;
  value: string | null;
  default: string;
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
  /** Served site for per-site FrankenPHP override rows — rendered as the
   *  row's sub-line, never inside the version badge. */
  domain?: string;
  /** True for user-toggled engines Start-all never starts (Postgres) — the
   *  sidebar footer counts them only while running. */
  optional?: boolean;
  /** Set only for independently-toggleable services (db engine key or
   *  "mailpit") — drives the per-row Start/Stop toggle. Serving-core rows
   *  (edge/nginx/pools/FrankenPHP) omit it: group-managed by design. */
  serviceKey?: string;
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
  /** Human title from the plugin header; may be empty (drop-ins) — fall back to the slug. */
  title: string;
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

/** One noise file the cleanup refused/failed to delete, with the reason. */
export interface WpSkippedNoiseFile {
  path: string;
  reason: string;
}

/** Result of the checksum-panel cleanup (mirrors the Rust ChecksumCleanup):
 *  what was removed/skipped + a fresh post-cleanup verify report. */
export interface WpChecksumCleanup {
  removed: number;
  skipped: WpSkippedNoiseFile[];
  report: WpChecksumReport;
}

/** One WordPress release (mirrors the Rust WpCoreVersion / stable-check API). */
export interface WpCoreVersion {
  version: string;
  status: string; // latest | outdated | insecure
}

/** Result of a core version switch (mirrors the Rust WpCoreSwitch).
 *  dbUpdateRequired = wp-admin will show "Database Update Required". */
export interface WpCoreSwitch {
  version: string;
  dbUpdateRequired: boolean;
}

/** One whitelisted, typed site option (mirrors the Rust WpOptionRow). */
export interface WpOptionRow {
  name: string;
  label: string;
  kind: string; // text | email | int | bool | weekday | timezone | role
  min: number | null;
  max: number | null;
  value: string;
  /** false = shown but refused (non-scalar value / unreadable) — see note. */
  editable: boolean;
  note: string | null;
}

/** A role (`wp role list` row). */
export interface WpRole {
  name: string;
  role: string;
}

/** The site-options form (mirrors the Rust WpOptionsForm). */
export interface WpOptionsForm {
  fields: WpOptionRow[];
  timezones: string[];
  roles: WpRole[];
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
/** `rex` CLI install state (mirrors the Rust `CliStatus`). */
export interface CliStatus {
  available: boolean; // the bundled sidecar exists — install is possible
  installed: boolean; // something is symlinked at linkPath
  /** The link resolves to THIS app's bundled rex (false = stale/foreign). */
  current: boolean;
  linkPath: string;
  bundledPath: string | null;
}

export interface DnsStatus {
  running: boolean; // a resolver with our semantics answers on the loopback port
  /** Who serves DNS: LaunchAgent (survives app quits), legacy in-process
   *  fallback (dies with the app), or nothing. */
  mode: "agent" | "in-process" | "down";
  port: number;
  resolverInstalled: boolean; // /etc/resolver/test present
  resolverPath: string;
  caTrusted: boolean; // local CA trusted for THIS user (per-user, unlike the resolver)
}

/** One WordPress.org plugin-directory search hit (mirrors the Rust WpOrgPlugin). */
export interface WpOrgPlugin {
  slug: string;
  name: string;
  author: string; // plain text
  rating: number; // 0-100 (divide by 20 for stars)
  numRatings: number;
  activeInstalls: number;
  icon: string | null;
  shortDescription: string;
}

/** One WordPress.org theme-directory search hit (mirrors the Rust WpOrgTheme). */
export interface WpOrgTheme {
  slug: string;
  name: string;
  author: string;
  rating: number; // 0-100
  numRatings: number;
  activeInstalls: number;
  screenshot: string | null;
}

/** A detected code editor (mirrors the Rust EditorApp DTO). */
export interface EditorApp {
  id: string; // stable key stored as the preferred_editor setting
  name: string; // display name, e.g. "Visual Studio Code"
}

/** Firefox trust state (mirrors the Rust FirefoxTrustStatus DTO). Firefox keeps
 *  its OWN trust store: our keychain CA is only honored when its OS-roots
 *  import pref is on (default since Firefox 120; rexenv forces it per profile). */
export interface FirefoxTrustStatus {
  installed: boolean; // a profiles.ini exists for this user
  profiles: number; // profiles found
  forced: number; // profiles whose user.js already forces the import pref
  caPath: string; // CA file for the manual Authorities → Import fallback
}

/** TLD policy classification (mirrors the Rust core::tld::TldPolicy DTO).
 *  Display metadata only — the backend refuses blocked TLDs either way. */
export interface TldPolicy {
  allowed: boolean; // false = hard-blocked (.local, .dev, 2-letter, popular gTLDs)
  warn: boolean; // allowed but outside {test, localhost, example, invalid}
  reason: string; // why a blocked TLD is refused ("" when allowed)
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

/** Logs-tab category grouping (mirrors the Rust LogCategory). */
export type LogCategory = "server" | "database" | "git";

/** A selectable log source for the Logs viewer (mirrors the Rust LogTarget DTO). */
export interface LogTarget {
  key: string; // file name within the log dir
  label: string;
  category: LogCategory;
  path: string; // absolute file path — path row / "Open file"
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

// ── Add plugin/theme from Git ─────────────────────────────────────────────────

/** `repo_probe` result: normalized source + what the remote offers. Probing
 *  validates URL AND auth before any clone starts. */
export interface RepoProbeResult {
  url: string;
  host: string;
  dirName: string;
  /** Branch candidate parsed from a pasted /tree/ web URL — only trusted if
   *  it matches a real ref below. */
  refCandidate: string | null;
  defaultBranch: string | null;
  branches: string[];
  tags: string[];
}

/** What a cloned repo needs (read-only detection — runs no repo code). */
export interface RepoInspection {
  composer: boolean;
  node: { manager: string; pinnedBy: string; hasBuild: boolean } | null;
  wp: { kind: "plugin" | "theme" | "none"; name: string | null };
  nodeWant: string | null;
}

export interface RepoStepState {
  key: "clone" | "detect" | "composer" | "install" | "build";
  label: string;
  status: "pending" | "running" | "ok" | "failed" | "cancelled";
  error: string | null;
}

/** One add-from-Git job's full snapshot — the `repo-job://state/<id>` event
 *  payload and the `repo_job_state` poll share this shape. */
export interface RepoJobState {
  id: string;
  siteId: string;
  kind: "plugin" | "theme";
  dirName: string;
  url: string;
  gitRef: string | null;
  /** "add" (clone+detect flow) or a git op ("fetch" | "pull" | "checkout"
   *  | "push"). The add panel adopts only add-jobs; RepoPanel owns ops. */
  op: string;
  /** Flat log-file key (`repo-<domain>-<dir>.log`) — seeds the log pane via
   *  `tailLog` when the panel reconnects to a live job after a remount. */
  logKey: string;
  steps: RepoStepState[];
  inspection: RepoInspection | null;
  nodeWarning: string | null;
  finishedOk: boolean;
}

/** A git-sourced wp-content dir's provenance (the list "git" badge). */
export interface GitAsset {
  kind: "plugin" | "theme";
  dirName: string;
  url: string;
  gitRef: string | null;
  /** "cloned" | "adopted" | "linked" — linked assets delete by UNLINK. */
  source: string;
}

/** Live checkout state (RepoPanel + the delete-safety confirm). */
export interface RepoAssetStatus {
  branch: string | null;
  detached: boolean;
  /** Where a detached HEAD sits (exact tag name, else short commit id).
   *  Null unless detached. */
  detachedAt: string | null;
  unborn: boolean;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  changed: number;
  untracked: number;
  remote: string | null;
  /** What deleting this checkout destroys, ready to show verbatim — null =
   *  clean and provably pushed. */
  lossWarning: string | null;
  /** Last add-job log key, when the file exists. */
  logKey: string | null;
  /** For symlinked dirs: where the link points (the user's real checkout). */
  linkTarget: string | null;
}

/** A wp-content dir that looks like a git checkout but isn't managed yet. */
export interface UnmanagedRepo {
  dirName: string;
  linked: boolean;
}

/** git/node availability for the Git add panel (composer is always the
 *  bundled phar). */
export interface RepoToolStatus {
  name: string;
  ok: boolean;
  version: string | null;
  path: string | null;
  error: string | null;
}

/** Local + remote-tracking branches for the checkout dropdown. */
export interface RepoBranches {
  current: string | null;
  local: string[];
  remote: string[];
  /** Local tags, newest first. Checkout target is `refs/tags/<name>`. */
  tags: string[];
}

/** One PR/MR head ref the remote advertises (refs-only — number + sha is all
 *  a ref carries; titles/authors would need the host API). Checkout of `ref`
 *  lands detached. */
export interface RepoPullRef {
  number: number;
  sha: string;
  ref: string;
}

/** One offerable package.json script (RepoPanel scripts row). */
export interface RepoScript {
  name: string;
  /** The script's command line — shown so the user sees WHAT runs. */
  command: string;
  /** Long-running by name (dev/watch/start/serve/hot) → offered as Watch. */
  watchy: boolean;
}

export interface RepoScriptsInfo {
  manager: string | null;
  scripts: RepoScript[];
}

/** A live (or crashed) watcher — npm run dev/watch/…; dies with the app,
 *  never auto-restarts. */
export interface RepoWatchState {
  id: string;
  siteId: string;
  kind: "plugin" | "theme";
  dirName: string;
  script: string;
  status: "running" | "exited";
  exit: number | null;
}

/** repo_link result: what landed + what detection saw. */
export interface RepoLinkResult {
  dirName: string;
  isGit: boolean;
  wp: { kind: "plugin" | "theme" | "none"; name: string | null };
}
