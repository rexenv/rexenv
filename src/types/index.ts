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
  /** Provisioning completeness (v16): false = the streamed create job died or
   *  was cancelled mid-provision — the list shows the honest "setup
   *  incomplete" badge with Retry/Delete. Flipped to true only when a
   *  provision job settles ok. */
  provisioned: boolean;
  /** Does rexenv own this site's docroot — may deleting the site remove the
   *  folder? (v17, mirrors the Rust Site.) `true` = we created it under the
   *  sites folder; `false` = a folder you linked, or one moved outside the
   *  sites folder — either way rexenv never deletes it. `null` = a pre-v17 row
   *  the startup backfill hasn't recorded yet. Drives the "external folder"
   *  badge and the delete-confirm copy. */
  docrootManaged: boolean | null;
  /** Did rexenv CREATE this site's database — may deleting the site drop it?
   *  (v19, mirrors the Rust Site.) `true` = a database import created it;
   *  `false` = the name already existed on our engine and we restored into it,
   *  so it is never dropped; `null` = created by rexenv's own provisioning (or
   *  no database at all). Optional so payloads written before v19 still parse. */
  dbCreated?: boolean | null;
  /** Who this site belongs to (v27, mirrors the Rust SiteOrigin). `"agent"` =
   *  a disposable scratch site an AI agent created through the MCP server;
   *  anything else — including absent — is the user's own site. Optional and
   *  defaulting to the user's for the same reason the Rust side parses
   *  leniently: only `"agent"` may ever mean disposable. */
  origin?: SiteOrigin;
  /** The MCP client's SELF-REPORTED name (v27) — the scratch card's badge.
   *  Display ONLY: it is agent-controlled, so nothing may branch on it. */
  agentClient?: string | null;
  /** When a scratch site expires (v27). **`null`/absent means never** — the
   *  shape a user site and a Kept scratch site share, so nothing can treat
   *  "no expiry" as two different states. */
  expiresAt?: string | null;
  /** Folder inside `path` that the web server roots at (v32). `""`/absent = the
   *  path itself; a Laravel site rexenv created stores `"public"`, keeping its
   *  `.env` above anything served. Display only — the backend decides it. */
  docrootSubdir?: string;
  /** The repository this site's code was CLONED from (v33), normalized by the
   *  backend. `null`/absent = the code did not come from a repo — true of every
   *  site made before the feature existed. Drives the "git" badge. */
  gitUrl?: string | null;
  /** The branch or tag PICKED when the site was created (v33), or `null` for
   *  the remote's default. A record of that choice — NOT what is checked out
   *  now, which only git can answer. */
  gitRef?: string | null;
  /** Did the user ask for `artisan migrate` at create (v34)? `null`/absent
   *  means yes — exact, since every Laravel site made before the column
   *  migrated unconditionally. */
  gitMigrate?: boolean | null;
  /** Did the user ask for the repo's front-end assets to be built (v35)?
   *  `null`/absent means no — exact, since nothing ran a package manager during
   *  provisioning before the column existed. */
  gitBuildAssets?: boolean | null;
}

/** A plugin or theme an agent cloned into a scratch site (v29), as the Sites
 *  page reads it. Mirrors the Rust `ScratchPackageView`. */
export interface ScratchPackage {
  siteId: string;
  slug: string;
  /** `"plugin"` or `"theme"` — DERIVED from the source's own header at add
   *  time, never asserted by the agent. */
  kind: string;
  /** The USER'S own project directory the clone was taken from — recorded at
   *  add time and never re-derived. Safe and useful to show: they chose it. */
  sourcePath: string;
  /** When the clone was last taken. The site runs a SNAPSHOT as of this
   *  moment, not what is in the editor now. */
  syncedAt: string;
  /** The recorded source is not a directory right now (stat-ed at read).
   *  A state of its own — never collapse it into "nothing changed". */
  sourceMissing: boolean;
}

/** Who a site belongs to (mirrors the Rust SiteOrigin). */
export type SiteOrigin = "user" | "agent";

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

/** What "Remove system changes" actually did to the OS resolver files (mirrors
 *  the Rust TeardownReport). Files rexenv BORROWED from Valet/Herd are handed
 *  back rather than deleted, so a generic "removed everything" would be a lie. */
export interface TeardownReport {
  /** Ours outright — deleted. */
  removed: string[];
  /** Borrowed — their file put back from our backup. */
  restored: string[];
  /** They had already reclaimed these; rexenv touched nothing. */
  leftAlone: string[];
  /** Borrowed, but our backup was gone: ours removed, theirs unrecoverable. */
  backupMissing: string[];
}

/** Which tool a discovered site came from. */
export type ImportSource = "valet" | "herd";

/** Why a discovered row can or can't be imported (mirrors the Rust SiteStatus,
 *  an internally-tagged enum). */
export type ImportStatus =
  | { status: "importable" }
  | { status: "needsAttention"; reason: string }
  | { status: "unsupported"; reason: string }
  | { status: "alreadyImported" };

/** One reviewable row of the Valet/Herd migration list. */
export interface ImportCandidate {
  source: ImportSource;
  name: string;
  domain: string;
  /** Their project folder — null when the target is missing. */
  path: string | null;
  /** The folder we'd actually SERVE (often a framework subfolder). */
  servePath: string | null;
  docrootRel: string | null;
  siteType: SiteType | null;
  label: string | null;
  /** The PHP minor they pinned, as their config writes it. */
  phpMinor: string | null;
  /** The minor we'd use — null when theirs isn't one we ship and the user must
   *  choose. We never substitute silently. */
  phpTarget: string | null;
  secured: boolean;
  proxyTo: string | null;
  /** The same domain also exists in the other tool. */
  alsoIn: ImportSource | null;
  hasCustomValetDriver: boolean;
  status: ImportStatus;
}

/** Whether a TLD's OS resolver file is ours, theirs, or missing. */
export interface ResolverTldStatus {
  tld: string;
  /** `absent` · `ours` · `borrowed` (ours, taken from them) · `foreign`
   *  (theirs) · `drifted` (we borrowed it, they took it back). */
  owner: "absent" | "ours" | "borrowed" | "foreign" | "drifted";
  path: string;
  /** Their file verbatim, to show beside ours before asking for consent. */
  theirContent: string | null;
  ourContent: string;
  /** rexenv sites already on this TLD — the hand-back warning needs it. */
  rexenvSites: number;
}

/** One discovered Valet/Herd environment. */
export interface ImportSourceInfo {
  kind: ImportSource;
  home: string;
  tld: string;
  loopback: string;
  parked: string[];
  /** Anything odd worth showing rather than leaving the user to wonder. */
  notes: string[];
}

/** Everything the migration screen needs, from one read-only call. */
export interface ImportScan {
  sources: ImportSourceInfo[];
  candidates: ImportCandidate[];
  tlds: ResolverTldStatus[];
  availablePhp: string[];
}

/** What handing a resolver file back actually did (mirrors ResolverPlan). */
export interface ResolverPlan {
  remove: string[];
  restore: [string, string][];
  dropRecords: string[];
  backupMissing: string[];
  reclaimed: string[];
}

/** What the user asked the importer to bring over. */
export interface ImportRequest {
  /** Domains to import, in the order shown. */
  domains: string[];
  /** Per-domain PHP minor, for rows where they had to choose. */
  php: Record<string, string>;
  /** After each site imports, also run its database import. The screen ticks
   *  this by default; the old database is only ever read. */
  importDatabases?: boolean;
}

/** What happened to one row — terminal; every requested domain gets exactly one. */
export interface ImportOutcome {
  domain: string;
  /** `imported` · `failed` · `skipped` (not importable, or cancelled before we
   *  reached it). */
  status: "imported" | "failed" | "skipped";
  reason: string | null;
  siteId: string | null;
  /** The job log, so a failure is diagnosable rather than just red. */
  logKey: string | null;
  /** Database outcome when importDatabases was on: `imported` · `failed: …` ·
   *  `skipped: …`. Null when databases weren't requested. */
  db: string | null;
}

/** Where a running import is right now — the screen's only in-flight signal.
 *
 *  Honest by construction: `done` counts terminal rows, `sitePct` is the
 *  running job's OWN backend-computed pct, `detail` is that job's own step
 *  label verbatim. `pct` is monotonic, capped at 99 until the batch settles,
 *  and a failed row still counts as done — the bar advances on real
 *  completions only, never on a clock. */
export interface ImportProgress {
  /** Rows the user asked for — fixed for the whole run. */
  total: number;
  /** Rows with a terminal outcome (imported, failed or skipped). */
  done: number;
  /** 1-based position of the site being worked on; 0 during the shared
   *  preparation steps that belong to no single site. */
  index: number;
  domain: string | null;
  stage: "scanning" | "resolvers" | "php" | "site" | "database" | "checking" | "done";
  /** The running job's own step label, verbatim. */
  detail: string | null;
  /** The current site's own fraction, 0..100 (provision, plus its database
   *  job when databases were requested). */
  sitePct: number;
  /** The whole batch, 0..100. */
  pct: number;
}

/** End-of-run summary: which succeeded, which didn't, and why. */
export interface ImportResult {
  outcomes: ImportOutcome[];
  imported: number;
  failed: number;
  skipped: number;
  /** Databases that came over / didn't, when importDatabases was on. */
  dbImported: number;
  dbFailed: number;
  /** Checked once at the end: why the imported sites won't load yet, or null
   *  when they will. `kind: "stopped"` means nothing is listening on :443 —
   *  which HERE means rexenv's own stack isn't running, because importing never
   *  starts it. `kind: "foreign"` means something else holds the port. The two
   *  used to be one boolean that said "quit it" for both. */
  serving: {
    kind: "foreign" | "stopped";
    holder: string | null;
    app: string | null;
    fix: string | null;
  } | null;
}

/** How a `connected` state was proven (Stage 3 §6). `signin` = the rewritten
 *  settings sign in to the rexenv copy — the gate for `connected`;
 *  `signin+http` adds the supplementary HTTP probe, which can only ever
 *  upgrade `signin`, never gate and never un-set. */
export type DbConnectedVerified = "signin" | "signin+http";

interface DbImportRecordCommon {
  siteId: string;
  dbName: string;
  tableCount: number;
  sizeBytes: number;
  /** e.g. "MySQL 8.0.27 at 127.0.0.1:3306". */
  sourceLabel: string;
  /** Null = their config connects as root: the interim change is three keys. */
  mirroredUser: string | null;
  /** Tables the SOURCE server could not read back, left out of the copy on
   *  purpose. Empty on a complete copy. Rendered wherever the copy is
   *  described: `tableCount` alone makes an incomplete copy look complete. */
  skippedTables: string[];
  importedAt: string;
}

/** The settled outcome of a site's database import — the ONE fact the badge,
 *  the summary sentence and the detail panel all render from, so they can
 *  never disagree. `state` is a closed set: `imported` (the copy exists on
 *  rexenv's engine and the site still reads the OLD database) or `connected`
 *  (the Stage 3 rewrite job VERIFIED the rewritten settings — only its
 *  verification path can write this value, so it always carries HOW it was
 *  proven). */
export type DbImportRecord = DbImportRecordCommon &
  (
    | { state: "imported" }
    | { state: "connected"; verified: DbConnectedVerified }
  );

/** One line of a rewrite diff, derived from the bytes that will be written —
 *  never from intent — so what it shows is provably what changes. */
export interface RewriteDiffLine {
  /** "-" (a line of the original) or "+" (a line of the new content). */
  sign: string;
  /** 1-based line number in its own file version. */
  line: number;
  text: string;
}

/** The rewrite preview (Stage 3). `ready`'s diff IS the write: apply writes
 *  exactly the bytes the diff was derived from, or refuses. */
export type RewritePreview =
  | {
      status: "ready";
      file: string;
      diff: RewriteDiffLine[];
      /** sha256 of the WHOLE file at preview time — apply refuses on drift. */
      fingerprint: string;
      /** Non-null = the root case: this dedicated account (holding the
       *  config's existing password) will be created on apply. */
      createsUser: string | null;
      /** An earlier rewrite's backup exists and is kept — first backup wins. */
      backupExists: boolean;
      /** .env shape with bootstrap/cache/config.php present: the cached-config
       *  warning must lead the panel. */
      laravelCacheWarning: boolean;
      /** Where the site will connect, e.g. "127.0.0.1:13306". */
      target: string;
    }
  | { status: "refused"; reason: string; file: string | null };

/** The apply outcome. Only `applied` flips the record to connected — and it
 *  carries the updated record rather than asking the UI to infer. */
export type RewriteApplied =
  | { status: "applied"; record: DbImportRecord; message: string }
  | { status: "fileChanged"; message: string }
  | { status: "engineStopped"; message: string }
  | { status: "verifyFailed"; reason: string; message: string }
  | { status: "refused"; reason: string; file: string | null };

/** Why a revert refused without force. */
export type RewriteFileEditedReason =
  | "editedSinceRewrite"
  | "unknownDigest"
  | "fileMissing";

/** The revert outcome — every ugly case is a named state, not a generic
 *  failure. `backupMissing` leaves the site connected (still true) and their
 *  file untouched; only OUR copy of the original is gone. */
export type RewriteRevertOutcome =
  | { status: "reverted"; file: string; message: string }
  | {
      status: "refusedEdited";
      file: string;
      reason: RewriteFileEditedReason;
      message: string;
    }
  | { status: "backupMissing"; file: string; message: string }
  | { status: "noRewrite"; message: string };

/** One phase of a running database-import job. */
export interface DbImportPhase {
  key: string;
  label: string;
  status: "pending" | "running" | "ok" | "failed" | "cancelled" | "skipped";
}

/** A database-import job's whole truth (honest-progress contract: monotonic,
 *  ≤99 until settle, frozen on failure/cancel). */
export interface DbImportJobState {
  id: string;
  siteId: string;
  domain: string;
  phases: DbImportPhase[];
  phaseCursor: number;
  pct: number;
  status: "running" | "ok" | "failed" | "cancelled";
  error: string | null;
  logKey: string;
  /** On failure: the kept dump file — it contains the database's data, so it
   *  is named rather than left for someone to find. */
  keptArtifact: string | null;
  result: DbImportRecord | null;
}

/** A leftover dump kept by a failed import — their data, so it is visible and
 *  removable, never a file someone finds later. */
export interface LeftoverDump {
  file: string;
  path: string;
  sizeBytes: number;
}

/** What linking a folder would do — from `inspectLinkedFolder`, shown before
 *  anything is created. Detection is pure filesystem; nothing in the folder is
 *  executed. */
export interface LinkedFolderInfo {
  /** The project folder, canonicalized. */
  root: string;
  /** The folder we'd actually serve — often a subfolder (Laravel `public/`,
   *  Bedrock `web/`), since a docroot is rarely the project root. */
  servePath: string;
  /** That subfolder relative to the root ("" = the root itself). */
  docrootRel: string;
  siteType: SiteType;
  /** Framework name for display ("WordPress", "Laravel", …). */
  label: string;
  /** The folder already holds an app — we adopt it and install nothing. */
  existingInstall: boolean;
  /** A LocalValetDriver.php picks this project's docroot by running PHP, so our
   *  detection may disagree with what Valet served. */
  hasCustomValetDriver: boolean;
}

/** Input for creating a site (mirrors the Rust NewSite). */
export interface NewSiteInput {
  name: string;
  domain: string;
  type: SiteType;
  phpVersion: string;
  webServer: WebServer;
  /** Empty → rexenv creates the docroot under the sites folder and owns it.
   *  Non-empty → LINK that existing folder: it is served in place, never
   *  created or written into, and never deleted with the site. */
  path: string;
  dbEngine?: SiteDbEngine; // omitted → mysql (serde default)
  /** Non-empty → CLONE this repository into a docroot rexenv creates (v33).
   *  Mutually exclusive with `path`: linking adopts a folder rexenv must never
   *  write into, cloning fills one it just made. Sending both is refused. */
  gitUrl?: string;
  /** Branch or tag to check out; omitted → the remote's default. */
  gitRef?: string | null;
  /** Run `php artisan migrate` once the app is wired (v34). Omitted → true:
   *  the database is created by the same job and is empty, so there is nothing
   *  a migration can lose. Recorded on the row, so a Retry honours it. */
  gitMigrate?: boolean;
  /** Install and build the repo's front-end assets (v35). Omitted → false, so a
   *  caller that never heard of this field cannot make rexenv run a package
   *  manager's install scripts. */
  gitBuildAssets?: boolean;
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

/** A PHP version in the registry (mirrors the Rust PhpVersionView).
 *
 *  `xdebugSupported` / `xdebugVersion` are DERIVED in core from the pinned build
 *  set and sent down — never re-decided here. The UI used to disable the Xdebug
 *  toggle on a literal `minor === "8.0"`, i.e. a second copy of
 *  `binaries::xdebug_supported` that could silently disagree with it. */
export interface PhpVersion {
  minor: string; // "8.3" — the key + what Site.phpVersion references
  /** The patch this BUILD pins for the minor — derived in core, never stored. */
  patch: string; // "8.3.31"
  /** The patch the live pool is ACTUALLY executing, when it differs from `patch`.
   *  `null` covers "no pool", "pool is on the pin" and "pool unidentifiable"
   *  alike — none of those is a disagreement, so the row says nothing extra.
   *  Present so a failed patch bump cannot render as the pin while the pool
   *  serves older bytes. */
  serving: string | null;
  fpmPort: number;
  installed: boolean;
  isDefault: boolean;
  /** Can the per-site Xdebug toggle be offered for this minor? */
  xdebugSupported: boolean;
  /** The Xdebug release this minor's debug pool loads — not app-wide; a minor
   *  past Xdebug's support window is frozen at its last release. */
  xdebugVersion: string | null;
  /** `YYYY-MM-DD` when upstream security support ENDED, or null while it is
   *  still supported. Computed in core against today, so it becomes true on the
   *  day it becomes true — never a stored flag someone has to remember to flip. */
  eolSince: string | null;
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
  /** Git commit this binary was built from (`-dirty` if the tree had
   *  uncommitted changes). Answers "am I running the code I just fixed?". */
  commit: string;
  /** UTC build timestamp. */
  builtAt: string;
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

/** What the backend can honestly say about a live tunnel's public URL —
 *  one fact, probed every 30 s (mirrors Rust `TunnelHealth`). */
export type TunnelHealth = "unverified" | "reachable" | "broken";

/** WHY the URL is unreachable from this machine, when it is — the
 *  failure-gated diagnosis (mirrors Rust `TunnelDiagnosis`). Drives the line
 *  under the badge; the badge itself keeps its meaning. */
export type TunnelDiagnosis = "local-dns-behind" | "dns-propagating" | "edge-gone" | "offline";

/** A per-site public tunnel (mirrors the Rust TunnelInfo DTO). */
export interface TunnelInfo {
  domain: string;
  url: string; // public https://<id>.trycloudflare.com
  running: boolean;
  health: TunnelHealth;
  diagnosis?: TunnelDiagnosis | null;
}

/** A WordPress plugin row (mirrors the Rust WpPlugin DTO / `wp plugin list`). */
export interface WpPlugin {
  name: string;
  status: string; // active | inactive | active-network | must-use | dropin
  version: string;
  update: string; // none | available | …
  /** The version the update installs; empty until the update-check pass lands. */
  updateVersion: string;
  /** Human title from the plugin header; may be empty (drop-ins) — fall back to the slug. */
  title: string;
}

/** One progress snapshot of a running plugin update (mirrors the Rust
 *  UpdateSnapshot). `fraction` = items finished + the current item's STEP
 *  position — WP-CLI reports no bytes, so nothing here is a byte percentage. */
export interface WpUpdateProgress {
  total: number;
  done: number;
  current: string; // slug being updated
  phase: string; // "Downloading" | "Unpacking" | … (WP-CLI's own step)
  fraction: number; // 0..1
  line: string; // the raw wp-cli line
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
  /** The version the update installs; empty until the update-check pass lands. */
  updateVersion: string;
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
  icon: string | null; // the app's own icon as a data: URI, null when unreadable
}

/** A detected web browser (mirrors the Rust BrowserApp DTO). */
export interface BrowserApp {
  id: string; // stable key stored as the preferred_browser setting
  name: string; // display name, e.g. "Google Chrome"
  /** The app's OWN icon as a `data:image/png;base64,…` URI. `null` is honest and
   *  expected for apps that ship their icon only in a compiled asset catalog —
   *  render the monochrome glyph, never an invented brand mark. */
  icon: string | null;
  /** This is the OS's current default handler for https. Display only: it picks
   *  which icon the button wears when the user has chosen nothing. */
  systemDefault: boolean;
  /** This browser can be opened straight into a private/incognito window. False
   *  is honest and common (Safari has no such command line) — that row then
   *  shows NO private target, because an affordance that quietly opened a
   *  recorded window would be worse than none. */
  supportsPrivate: boolean;
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
  /** True when the answer can't be trusted: a non-stock layout (Bedrock/
   *  Radicle) keeps its defines outside wp-config.php — "off" would really
   *  mean "looked in the wrong place". */
  indeterminate: boolean;
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
  /** Dependency steps (composer/install/build) plus each job kind's own op
   *  step (git ops use the op name, scripts "script", the dep check "check"). */
  key:
    | "clone"
    | "detect"
    | "composer"
    | "install"
    | "build"
    | "fetch"
    | "pull"
    | "checkout"
    | "push"
    | "stash"
    | "stash-pop"
    | "reset"
    | "status"
    | "script"
    | "check";
  label: string;
  /** "skipped" = never ran (an earlier Run-all step failed or the run was
   *  cancelled) — distinct from pending (may still run) and cancelled
   *  (killed mid-run); still individually re-runnable. */
  status: "pending" | "running" | "ok" | "failed" | "cancelled" | "skipped";
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
  /** "add" (clone+detect flow) or a git op ({@link RepoGitOp}). The add panel
   *  adopts only add-jobs; RepoPanel owns ops. */
  op: string;
  /** Flat log-file key (`repo-<domain>-<dir>.log`) — seeds the log pane via
   *  `tailLog` when the panel reconnects to a live job after a remount. */
  logKey: string;
  steps: RepoStepState[];
  inspection: RepoInspection | null;
  nodeWarning: string | null;
  finishedOk: boolean;
  /** Set by ops that PRODUCE a file (today: "dist-archive"); null otherwise. */
  archive: ArchiveResult | null;
}

/** What a dist-archive job left the user with. `path` is the file that really
 *  exists after collision numbering — never a predicted name. */
export interface ArchiveResult {
  path: string;
  fileName: string;
  /** No version in the plugin header, style.css or composer.json, so the name
   *  carries none. Not an error — worth saying quietly, not blocking on. */
  versionMissing: boolean;
}

/** One streamed install job — the `wp-install://state/<id>` payload.
 *  Honesty contract: `itemCursor` is an ATTEMPT cursor ("installing item k
 *  of N", never "k done" — an already-installed slug prints no header yet
 *  counts as a summary success), and it does NOT advance at all when
 *  `source === "zip"` (wp-cli prints the per-item header only on the wp.org
 *  path), so the card hides it there rather than showing a frozen "1 of N";
 *  `summary` is the verbatim terminal Success:/Error: line; "ok" does NOT
 *  imply activation (chained --activate failures don't touch the exit code —
 *  the list refresh is that truth). */
export interface WpInstallState {
  id: string;
  siteId: string;
  kind: "plugin" | "theme";
  /** Where the items came from — decides what `slugs` holds and how it reads. */
  source: "wporg" | "zip";
  /** The install arguments verbatim: wp.org slugs, or absolute .zip paths. */
  slugs: string[];
  itemsTotal: number;
  itemCursor: number;
  /** Phase-based determinate progress (0–100). OBSERVED discrete progress —
   *  every tick corresponds to a line wp-cli actually printed — NOT the
   *  byte-level estimate the B25 rule bans (do not "fix" back to
   *  indeterminate). Monotonic; capped at 99 until the terminal summary;
   *  frozen in place on failure/cancel/timeout. */
  pct: number;
  status: "running" | "ok" | "partial" | "failed" | "cancelled" | "timed_out";
  summary: string | null;
  error: string | null;
  logKey: string;
}

/** One phase of a streamed site-provision job. Phases are the BACKEND'S own
 *  step boundaries (deterministic Rust code) — never parsed from subprocess
 *  output; wp-cli's verbatim lines are display-only sub-detail. */
export interface ProvisionPhase {
  key: string; // "prepare" | "fetch" | "db" | "core_download" | "configure" | "core_install" | "blueprint" | "serve"
  label: string;
  status: "pending" | "running" | "ok" | "skipped" | "failed" | "cancelled";
}

/** One streamed site-provision job — the `site-provision://state/<id>`
 *  payload. Honesty contract (mirrors the install card): `pct` is
 *  phase-weighted OBSERVED progress (real phase completions + the download
 *  Hub's real byte fraction during fetch — never a time estimate), monotonic,
 *  ≤99 until the job settles ok, FROZEN in place on failure/cancel/timeout.
 *  `downloadIds` are the Hub items the fetch phase waits on — filter the
 *  app-wide downloads snapshot to these for the byte sub-row. */
export interface SiteProvisionState {
  id: string;
  domain: string;
  siteId: string | null;
  phases: ProvisionPhase[];
  phaseCursor: number;
  pct: number;
  status: "running" | "ok" | "failed" | "cancelled" | "timed_out";
  summary: string | null;
  error: string | null;
  logKey: string;
  downloadIds: string[];
  /** The site was created, but something else is answering :443 in front of our
   *  edge (a running Herd shadow-binds 127.0.0.1:443), so it won't load yet.
   *  A FIELD rather than a `status` value: status is read as ok-or-failure, so a
   *  new variant would show the failure glyph on a job that actually succeeded. */
  servingBlocked?: boolean;
  /** Who is answering :443, attributed where possible — never a guess. */
  servingHolder?: string | null;
  /** The app to quit, when identifiable. */
  servingApp?: string | null;
  /** The front-end asset build didn't complete, and why (v35). Same "succeeded,
   *  but" shape as `servingBlocked`: the build runs the DEVELOPER'S toolchain
   *  against the repo's own scripts, so its failure is not evidence that
   *  provisioning failed — the site is created, wired and serving. `null`/absent
   *  = it wasn't asked for, or it worked. */
  assetsWarning?: string | null;
}

/** A git-sourced wp-content dir's provenance (the list "git" badge). */
/** What a repo panel/IPC call targets. `"site"` is the SITE's own checkout —
 *  a cloned site's project root — which carries no folder name of its own: the
 *  `dirName` sent alongside it is display text and never reaches a path. */
export type RepoKind = "plugin" | "theme" | "site";

/** What a site's OWN folder is, repository-wise (Stage 3). */
export interface SiteRepoInfo {
  /** `<path>/.git` exists — the Repository tab has something to show. */
  present: boolean;
  /** The folder that was looked at, so "no repository here" names the place. */
  projectRoot: string;
  /** The repo rexenv cloned it from (v33). Display only — what is checked out
   *  NOW comes from git, live. */
  clonedFrom: string | null;
}

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
  /** Is there a `.distignore` — i.e. can this asset be archived at all? The
   *  same predicate the command enforces, so the button and the command can
   *  never disagree. */
  hasDistignore: boolean;
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

/** The ops `repo_git_op` accepts (mirrors the Rust whitelist). The last four
 *  are working-tree ops: `stash`/`stash-pop` are the recoverable way out of a
 *  dirty tree, `reset` the unrecoverable one, `status` reads and writes
 *  nothing. */
export type RepoGitOp =
  | "fetch"
  | "pull"
  | "checkout"
  | "push"
  | "stash"
  | "stash-pop"
  | "reset"
  | "status";

/** One `git stash list` entry (mirrors the Rust StashEntry DTO). */
export interface RepoStashEntry {
  /** The ref exactly as git names it — `stash@{0}`. What restore is given. */
  reference: string;
  /** Git's own subject ("On dev: rexenv: 3 changed, 2 untracked"). */
  message: string;
  /** Relative age, e.g. "2 hours ago". */
  age: string;
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

// ── MCP: the "AI agents" Settings card + per-site activity ───────────────────

/** How one agent tool call turned out (mirrors `mcp_server::feed::Outcome`). */
export type AgentOutcome = "ok" | "error" | "denied" | "unknown-tool" | "bad-request";

/** One recorded agent action (mirrors `mcp_server::feed::AgentAction`). Only
 *  `targetSite` is stored from a call's arguments — there is no free-form arg
 *  field, by construction. */
/** Who performed a feed row's action (mirrors the Rust `FeedActor`). `"rexenv"`
 *  = rexenv's own housekeeping (the scratch reaper), listed like any other row
 *  but labelled as ours — and excluded from the header's agent status line. */
export type FeedActor = "agent" | "rexenv";

export interface AgentAction {
  id: number;
  at: string;
  /** Who did this. A row that isn't the agent's must never render as one. */
  actor: FeedActor;
  /** For an agent row, the client's self-reported name. */
  client: string;
  tool: string;
  /** The stable site id the call named (a UUID) — not human-readable. */
  targetSite: string | null;
  /** The named site's current domain, resolved at read time; null when there's
   *  no target or the site was deleted (then fall back to `targetSite`). */
  targetLabel: string | null;
  outcome: AgentOutcome;
  detail: string | null;
  /** What the call was ABOUT, when the tool's name and target don't say (v30).
   *  Null for most tools, and that is an ANSWER rather than a gap: their name
   *  and target already describe them. At most two `[a-z][a-z0-9-]` tokens,
   *  clamped in Rust at the write — so it can never impersonate the client-name
   *  slot, rexenv's own rows, or the separators between them. It is a VERB, not
   *  the command: `plugin activate` does not say which plugin, and `eval` says
   *  nothing about the code. */
  argsSummary: string | null;
  concerning: boolean;
}

/** The header status line's state — derived from recent call OUTCOMES in a
 *  15-minute window, never the socket handshake alone (mirrors the Rust
 *  `ActivityStatus` tagged enum). Self-recovering: an error state ages out. */
export type ActivityStatus =
  | { kind: "off" }
  | { kind: "idle" }
  | { kind: "working"; lastTool: string; minutesAgo: number }
  | { kind: "erroring"; errored: number; minutesAgo: number };

/** The MCP card's whole state in one read, so the header and the feed it shows
 *  come from the same snapshot (mirrors `commands::mcp::McpStatus`). */
export interface McpStatus {
  enabled: boolean;
  connectCommand: string;
  activity: ActivityStatus;
  recent: AgentAction[];
  /** The MAIL sub-toggle (M2b) — off by default and INDEPENDENT of `enabled`:
   *  turning the endpoint on does not turn mail on. While it is true, every
   *  scratch site carries rexenv's `From` stamp; while false, none does. */
  mailEnabled: boolean;
}
