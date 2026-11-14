import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, FolderInput, Plus, Globe, FolderOpen, Database, LockOpen, Trash2, LayoutGrid, TerminalSquare, ScrollText, MoreHorizontal, MoreVertical, ArrowDownUp, Pencil, Copy, Code, Link, RefreshCw, Pin as PinIcon, PinOff, Bot, X, Play, Square, GitBranch } from "lucide-react";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { RexLogo } from "@/components/common/RexLogo";
import { toast, toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Menu, MenuItem, MenuSeparator } from "@/components/ui/menu";
import { SITE_ENABLED_KEY, useAllSitesEnabled, useSiteEnabled } from "@/lib/useSiteEnabled";
import { usePendingVariables } from "@/lib/usePendingVariables";
import { ConfirmDialog } from "@/components/ui/dialog";
import { isScratch, siteTypeMeta } from "@/lib/siteType";
import { StatusPill } from "@/components/common/StatusPill";
import { Placeholder } from "@/components/common/Placeholder";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import { useSiteActions } from "@/components/sites/useSiteActions";
import { Button } from "@/components/ui/button";
import { allSiteDomains, defaultTld, listSites, resolverDrift, openExternal, openPath, canOpenOnThisScreen, getSitesServing, sitesResources, siteProvisionActive, siteProvisionCancel, siteProvisionRetry, scanValetImport, dbImportRecords, keepSite, scratchPackages, onScratchReaped, agentActivity, worktreeRelations } from "@/lib/ipc";
import { openSiteInEditor, usePreferredEditor } from "@/lib/useEditor";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { usePreferredBrowser } from "@/lib/useBrowser";
import { AppIcon } from "@/components/ui/app-icon";
import { PreferredBrowserIcon } from "@/components/ui/open-in";
import { useSiteUrl } from "@/lib/siteUrl";
import { useModKey } from "@/lib/useModKey";
import { useStackUp, useServeFirst } from "@/lib/useServeFirst";
import { usePinSite, useSiteShortlist } from "@/lib/useSiteShortlist";
import { openWpAdmin } from "@/lib/wpAdmin";
import { useSitesListState, type SitesSort, type SitesStatusFilter, type SitesTypeFilter } from "@/lib/sitesListState";
import { SiteProvisionCard, useSiteProvision } from "@/components/sites/SiteProvisionCard";
import { useDownloads } from "@/lib/useDownloads";
import type { DbImportRecord, DriftedTakeover, ScratchPackage, Site, SiteResources, SiteType } from "@/types";

/** A scratch site's remaining life, or `null` when it has none.
 *
 * `expiresAt` null/absent means **never** — the shape a user's site and a KEPT
 * scratch site share, deliberately, so nothing can render "kept" as a third
 * state. Returning null here (rather than a "never" label) is what makes that
 * true at the render layer too. Stored as a SQLite UTC datetime, so it is
 * parsed as UTC rather than as the viewer's local time. */
export function expiryLabel(
  expiresAt: string | null | undefined,
): { text: string; expired: boolean } | null {
  if (!expiresAt) return null;
  const ms = Date.parse(`${expiresAt.replace(" ", "T")}Z`);
  if (Number.isNaN(ms)) return null;
  const left = ms - Date.now();
  if (left <= 0) return { text: "expired", expired: true };
  const mins = Math.round(left / 60000);
  if (mins < 60) return { text: `${mins}m left`, expired: false };
  const hours = Math.round(mins / 60);
  if (hours < 48) return { text: `${hours}h left`, expired: false };
  return { text: `${Math.round(hours / 24)}d left`, expired: false };
}

/** "5m ago" / "3h ago" / "2d ago" for a recorded sync time (SQLite UTC). */
export function agoLabel(at: string): string {
  const ms = Date.parse(`${at.replace(" ", "T")}Z`);
  if (Number.isNaN(ms)) return "unknown";
  const mins = Math.max(0, Math.round((Date.now() - ms) / 60000));
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  return hours < 48 ? `${hours}h ago` : `${Math.round(hours / 24)}d ago`;
}

/** Compact bytes for the per-site DB size. */
function fmtBytes(b: number): string {
  if (b >= 1024 * 1024 * 1024) return `${(b / (1024 * 1024 * 1024)).toFixed(1)}G`;
  if (b >= 1024 * 1024) return `${Math.round(b / (1024 * 1024))}M`;
  return `${Math.max(1, Math.round(b / 1024))}K`;
}

/** Honest per-site resources. A site is not a process: only FrankenPHP sites
 *  (dedicated backend) get real CPU/RAM; shared nginx+pool sites show ACTIVITY
 *  (last-60s requests) + DB size behind a "shared" badge — never a fabricated
 *  per-site CPU/RAM. */
function SiteMetrics({ res }: { res?: SiteResources }) {
  if (!res) return <span className="w-[168px] flex-none" />;
  const db = res.dbSizeBytes != null ? `${fmtBytes(res.dbSizeBytes)} DB` : null;
  if (res.dedicated) {
    const own =
      res.ramMb != null
        ? `${res.ramMb}M · ${(res.cpuPercent ?? 0).toFixed(1)}%`
        : "not running";
    return (
      <span
        className="w-[168px] flex-none truncate text-right font-mono text-[0.65625rem] text-rex-text-muted"
        title="Dedicated FrankenPHP process — real CPU/RAM for this site (plus its DB size)"
      >
        {own}
        {db ? ` · ${db}` : ""}
      </span>
    );
  }
  const req = res.requestsPerMin ?? 0;
  return (
    <span
      className="flex w-[168px] flex-none items-center justify-end gap-[6px] font-mono text-[0.65625rem] text-rex-text-muted"
      title="Shared nginx + PHP pool — a per-site CPU/RAM number doesn't exist here; showing real activity (requests in the last 60s) and DB size instead"
    >
      <span className="rounded-[5px] border border-rex-border bg-rex-surface-2 px-[5px] py-px text-[0.5625rem] uppercase tracking-[0.08em] text-rex-text-muted">
        shared
      </span>
      <span className="truncate">
        {req}/min{db ? ` · ${db}` : ""}
      </span>
    </span>
  );
}

/** The web-server column is a fixed 84px so rows align; a name longer than
 *  "frankenphp" spilled out of it ("openlitespeed", 16 Oct 2026), so it shows
 *  a short form and the full name rides the tooltip. */
const SERVER_BADGE: Partial<Record<string, string>> = { openlitespeed: "litespeed" };

type Filter = SitesStatusFilter;
/** The second axis: what KIND of site. `all` plus the three `SiteType`s, which
 *  is the same set the row avatars already colour — a filter that invented its
 *  own vocabulary for the letters on screen would be a third thing to learn. */
type TypeFilter = SitesTypeFilter;

const TYPE_LABEL: Record<TypeFilter, string> = {
  all: "All types",
  wordpress: "WordPress",
  laravel: "Laravel",
  php: "Blank PHP",
};
type Sort = SitesSort;

// "Newest" is by creation (it read "Recent" until 14 Nov 2026, when a second
// kind of recent arrived); "Opened" is by when the site's PAGE was last opened.
const SORT_LABEL: Record<Sort, string> = {
  name: "Name",
  status: "Status",
  recent: "Newest",
  opened: "Opened",
};
const SORT_CYCLE: Record<Sort, Sort> = {
  name: "status",
  status: "recent",
  recent: "opened",
  opened: "name",
};

/** A segmented control with live counts — status on one axis, site type on the
 *  other.
 *
 *  **Every count is computed against the OTHER control's current selection**
 *  (see `Sites`), so a tab's number is what clicking it would actually show. A
 *  count taken over all sites while the list is already filtered puts
 *  "Laravel 3" directly above an empty list, and the page then argues with
 *  itself about how many sites the user has. */
function FilterTabs<K extends string>({
  value,
  onChange,
  counts,
  tabs,
}: {
  value: K;
  onChange: (f: K) => void;
  /** A tab with no entry shows no number — the "All" tabs, whose count is the
   *  page's own subtitle and was on screen five times (14 Nov 2026). */
  counts: Partial<Record<K, number>>;
  tabs: { key: K; label: string }[];
}) {
  return (
    <div className="inline-flex items-center gap-1 rounded-[10px] border border-rex-well-border bg-rex-well p-[3px]">
      {tabs.map((t) => (
        <button
          key={t.key}
          data-filter-tab={t.key}
          onClick={() => onChange(t.key)}
          className={cn(
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[0.78125rem] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          {counts[t.key] !== undefined && (
            <span className="font-mono text-[0.65625rem]">{counts[t.key]}</span>
          )}
        </button>
      ))}
    </div>
  );
}

function SortButton({ value, onCycle }: { value: Sort; onCycle: () => void }) {
  return (
    <button
      onClick={onCycle}
      title="Change sort order"
      className="flex h-[34px] flex-none items-center gap-[7px] whitespace-nowrap rounded-[9px] border border-rex-border bg-rex-surface-1 px-3 text-[0.8125rem] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2"
    >
      <ArrowDownUp className="h-3.5 w-3.5 text-rex-text-muted" strokeWidth={1.8} />
      {SORT_LABEL[value]}
    </button>
  );
}

/** Exported for the dev harness (`?panel=provision` badge check). */
export function SiteRow({
  site,
  status,
  resources,
  dbState,
  onOpen,
  onDelete,
  onOpenTab,
  onRename,
  onDuplicate,
  onRetry,
  provisioning,
  onKeep,
  onToggleEnabled,
  pinned,
  onPin,
  packages,
  reapFailure,
  extraDomains,
  stoppedByUser,
  togglePending,
  worktreeBranch,
}: {
  site: Site;
  status: Site["status"];
  resources?: SiteResources;
  /** This site's database-import state (the ONE serialized DbImportRecord
   *  fact — see the badge comment). Undefined = no import. */
  dbState?: DbImportRecord["state"];
  /** The site's EXTRA domains (v42). Undefined/empty = it answers on its own
   *  domain only, which is most sites. */
  extraDomains?: string[];
  /** Set when this site is a git WORKTREE of another site: the branch it was
   *  made for (the recorded request — the row reads no git; the site's own
   *  page shows git's live answer). */
  worktreeBranch?: string;
  onOpen: () => void;
  onDelete: () => void;
  /** Open one of the site page's tabs (Database, Terminal, Logs, WordPress) —
   *  the row's menu is a way to them, the page is where they are. */
  onOpenTab: (tab: "database" | "terminal" | "logs" | "wordpress") => void;
  onRename: () => void;
  onDuplicate: () => void;
  onRetry?: () => void;
  /** Whether a provision job for THIS domain is running right now, asked of the
   *  backend's job registry (whoever started it: the New Site dialog, Retry,
   *  `rex site create`, an agent). `true`: the row is being made, not half-made
   *  — it reads "Setting up" and offers no Retry. `false`: no running job, so
   *  the warning badge below (the claim that the job DIED) and Retry render.
   *  `undefined`: not answered yet — neither, never a flash of "failed" first.
   *  For the whole of a first install the badge was on the row behind the
   *  dialog (30 Sep 2026). Meaningless for a provisioned site. */
  provisioning?: boolean;
  /** Adopt this scratch site (Keep). Present only for `origin === "agent"`. */
  onKeep?: () => void;
  /** The plugins/themes an agent cloned in — newest sync first. */
  packages?: ScratchPackage[];
  /** Why the reaper could not remove this expired site, from its own feed row. */
  reapFailure?: string;
  /** Is this site pinned, and the way to change it — absent for a scratch site
   *  (the reaper removes it when it expires, pinned or not). */
  pinned?: boolean;
  onPin?: (pinned: boolean) => void;
  /** Start or stop THIS site (v44). Absent for a site with nothing to serve. */
  onToggleEnabled?: (enabled: boolean) => void;
  /** The user stopped this site — as opposed to the stack being down. The two
   *  are the same word ("Stopped") in the pill and different sentences in the
   *  tooltip, because the fix is different: one is a button on this row, the
   *  other is Start all in the footer. */
  stoppedByUser?: boolean;
  togglePending?: boolean;
}) {
  const t = siteTypeMeta(site.type);
  const url = useSiteUrl()(site.domain);
  const scratch = isScratch(site);
  const ttl = scratch ? expiryLabel(site.expiresAt) : null;
  const pkg = scratch ? packages?.[0] : undefined;
  const [copied, setCopied] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const editor = usePreferredEditor();
  const words = usePlatformWords();
  const browser = usePreferredBrowser();
  const mod = useModKey();
  const rowRef = useRef<HTMLDivElement>(null);
  const screen = canOpenOnThisScreen();
  const isWordpress = site.type === "wordpress";
  const hasDb = !(site.type === "php" && !site.starterDb);
  // The URL answers something only while the stack serves it — or when the user
  // stopped THIS site, whose URL shows rexenv's own "stopped" page.
  const urlAnswers = status === "running" || !!stoppedByUser;
  // **Nothing serves this site and the user did not stop it: rexenv's services
  // are what is missing.** The browser and Magic Login actions then become
  // "Start & …" — they start the services and do what they said (`useServeFirst`)
  // instead of being dead buttons that say "Start all first".
  const serve = useServeFirst();
  const needsStack = site.provisioned && status !== "running" && !stoppedByUser;
  const startingThis = serve.startingId === site.id;
  // For a site the user stopped: is the stack itself off too?
  const stackUp = useStackUp();
  // **A normal site is quiet.** Provisioned, serving, not stopped by the user:
  // the row says so with a dot, and spends its words on the rows that are not.
  const quiet = site.provisioned && status === "running" && !stoppedByUser;
  const statusTitle = stoppedByUser
    ? "You stopped this site — it answers \"site stopped\" and nothing else is affected. Start it from the row menu."
    : status === "running"
      ? "Serving: the edge is up and so is this site's own upstream"
      : "Not serving — rexenv's services are off. The browser and WordPress icons on this row start them first.";
  const openSiteUrl = () => {
    if (needsStack) {
      void serve.ensure(site).then((ok) => { if (ok) void openExternal(url).catch(toastBackendError); });
      return;
    }
    void openExternal(url).catch(toastBackendError);
  };
  return (
    <div
      ref={rowRef}
      role="button"
      tabIndex={0}
      data-site-row={site.id}
      aria-label={`Open ${site.name}`}
      onClick={onOpen}
      onKeyDown={(e) => {
        // Only the row's own keys: a key pressed on one of its buttons is that
        // button's, and must not also open the site.
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" && mod.pressed(e)) {
          e.preventDefault();
          openSiteUrl();
        } else if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen();
        } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          const rows = [...document.querySelectorAll<HTMLElement>("[data-site-row]")];
          const next = rows[rows.indexOf(e.currentTarget) + (e.key === "ArrowDown" ? 1 : -1)];
          next?.focus();
        }
      }}
      // The focus ring is the brand colour: it was `ring-white/15`, which is
      // invisible on the light theme — found on the real app (Ubuntu VM, 14 Nov
      // 2026) the first time the rows were walked with ↓ ↑, where the ring is
      // the only thing that says which row Enter will open.
      // `select-none`: a right-click in WKWebView SELECTS the word under the
      // pointer before the menu opens — on the real app (macOS 15.8 VM, 14 Nov
      // 2026) the row's menu came up beside a highlighted "pg.rex". Playwright's
      // WebKit does not do it, so no check saw it. The domain is still one menu
      // item away (Copy domain).
      className="group flex h-11 cursor-pointer select-none items-center gap-[11px] rounded-[9px] pl-3 pr-2 transition-colors hover:bg-rex-surface-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand"
    >
      {/* The status, as a dot — for every row, in one column. The WORDS appear
          only where they say something (further right): 25 "Running" pills told
          the user nothing and hid the one site that was not (14 Nov 2026). */}
      <span
        data-site-dot={
          !site.provisioned ? "setup" : status === "running" && !stoppedByUser ? "running" : "stopped"
        }
        title={site.provisioned ? statusTitle : undefined}
        className="flex w-[9px] flex-none items-center justify-center"
      >
        {site.provisioned && (
          <span
            className={cn(
              "h-[8px] w-[8px] rounded-full",
              status === "running" && !stoppedByUser ? "bg-status-running" : "bg-status-stopped",
            )}
          />
        )}
      </span>
      <div
        className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border text-[0.6875rem] font-bold"
        style={{ background: t.bg, color: t.color, borderColor: t.border }}
      >
        {t.letter}
      </div>
      {/* The name column is the row's ONE flexible thing: everything to its
          right is nowrap/fixed, so when badges + pills + a long domain all
          land on one row, the name truncates instead of the badges
          ballooning into neighbouring rows (UI-REVIEW §C1.2). */}
      <div className="flex w-[188px] min-w-[110px] shrink flex-col gap-px">
        <div className="truncate text-[0.84375rem] font-semibold text-rex-text">
          {site.name}
        </div>
        <div className="flex items-baseline gap-1 truncate font-mono text-[0.6875rem] text-rex-text-muted">
          <span className="truncate">{site.domain}</span>
          {/* A site can answer on more than one hostname (v42). The row shows
              the PRIMARY — the name its files, database and certificate folder
              are keyed to — and marks the rest by COUNT rather than listing
              them: a row is 188px wide and three hostnames would push the name
              out, which is the defect the provision label already caused once
              (#248). The names themselves are one hover away, and on the site's
              own page. */}
          {extraDomains && extraDomains.length > 0 && (
            <span
              data-probe="extra-domains"
              title={`Also answers on ${extraDomains.join(", ")}`}
              className="flex-none text-rex-text-muted"
            >
              +{extraDomains.length}
            </span>
          )}
          {site.docrootManaged === false && (
            /* The folder is the user's own — served in place and never deleted with
               the site. True for a linked folder and for one moved out of the sites
               folder, which is why it reads "external" rather than "linked". A
               settled fact, so it is muted text beside the domain, not a chip: a
               chip is for a state that asks something of the user. */
            <span
              data-probe="external"
              className="flex-none"
              title={`Served from your own folder (${site.path}) — deleting the site leaves it in place`}
            >
              · external
            </span>
          )}
        </div>
      </div>
      <div className="flex-1" />
      {/* Metrics reserve a fixed 168px column for cross-row alignment; below
          xl that reservation is what pushed badge-heavy rows past the window
          edge, so the column yields entirely (supplementary data — SiteDetail
          has the full numbers). */}
      {/* A scratch row yields the metrics column outright, at every width.
          The same §C1.2 trade the other breakpoints already make, applied by
          ROW KIND rather than by width: this row carries three extra facts
          (client, last-synced, TTL), and of everything competing for the space,
          a disposable site's shared-pool request rate is the one nobody opened
          this page for — SiteDetail still has it. Measured, not guessed: the
          scratch row overflowed by 167px at 1440 and this column reserves 168. */}
      {!scratch && (
        <div className="hidden xl:contents">
          <SiteMetrics res={resources} />
        </div>
      )}
      {worktreeBranch && (
        <span
          className="flex max-w-[160px] flex-none items-center gap-1 truncate whitespace-nowrap rounded-full border border-rex-accent-blue-border bg-rex-accent-blue-bg px-2 py-1 font-mono text-[0.625rem] text-rex-accent-blue"
          title={`A git worktree site, made for the branch ${worktreeBranch}`}
        >
          <GitBranch className="h-3 w-3 flex-none" /> {worktreeBranch}
        </span>
      )}
      {dbState === "imported" && (
        /* Renders the SAME serialized fact as the SiteDetail summary
           (DbImportRecord, whose "connected" value only the rewrite job's
           verification can write) — the badge and the summary cannot
           disagree, because neither computes anything. The CONNECTED state has
           no chip here any more: it is settled, asks nothing, and is on the
           site's Database tab in full. This one asks for a decision. */
        <span
          data-probe="db-imported"
          className="flex-none whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="A copy of this site's database is on rexenv's engine, but the site isn't connected to it yet — its Database tab says what the site reads now and switches it over."
        >
          DB imported
        </span>
      )}
      {scratch && site.agentClient && (
        /* The MCP client's SELF-REPORTED name. Display only — nothing branches
           on it, because an agent chooses this string (v27). */
        <span
          className="hidden flex-none whitespace-nowrap rounded-full border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[0.625rem] text-rex-text-muted xl:block"
          title={`Created by "${site.agentClient}" — the name the AI client reports for itself`}
        >
          {site.agentClient}
        </span>
      )}
      {pkg && (
        /* The answer to "I changed my plugin and the site didn't see it",
           on screen BEFORE the user asks (§4.4). A moved source is its own
           state: rendering it as a stale timestamp would tell the user the
           site runs code from a directory that no longer exists. */
        <span
          className={cn(
            "hidden flex-none whitespace-nowrap rounded-full border px-2 py-1 font-mono text-[0.625rem] xl:block",
            pkg.sourceMissing
              ? "border-status-warning-border bg-status-warning-bg text-status-warning-bright"
              : "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted",
          )}
          title={
            pkg.sourceMissing
              ? `${pkg.slug} was copied from ${pkg.sourcePath}, which isn't there any more — the site still runs the copy taken ${agoLabel(pkg.syncedAt)}. Ask the agent to add it again from its new location.`
              : `${pkg.slug} (${pkg.kind}) was COPIED from ${pkg.sourcePath}. The site runs that snapshot — edits since are not in it until the agent syncs again.`
          }
        >
          {pkg.sourceMissing
            ? `${pkg.slug} · source moved`
            : `${pkg.slug} · synced ${agoLabel(pkg.syncedAt)}`}
          {packages && packages.length > 1 ? ` +${packages.length - 1}` : ""}
        </span>
      )}
      {ttl && (
        <span
          className={cn(
            "flex-none whitespace-nowrap rounded-full border px-2 py-1 font-mono text-[0.625rem]",
            ttl.expired
              ? "border-status-warning-border bg-status-warning-bg text-status-warning-bright"
              : "border-rex-border-strong bg-rex-surface-2 text-rex-text-muted",
          )}
          title={
            ttl.expired
              ? reapFailure
                ? `This scratch site expired and rexenv could not remove it: ${reapFailure}`
                : "This scratch site has expired — rexenv removes it on its next sweep. Keep it to make it yours."
              : "rexenv deletes this scratch site once nothing has used it for a while. Anything the agent does with it pushes this out; Keep makes it yours permanently."
          }
        >
          {ttl.expired && reapFailure ? "expired — couldn't remove" : ttl.text}
        </span>
      )}
      {site.provisioned ? (
        !quiet && (
          /* The status in WORDS, only when it is not "serving". The two
             stopped-nesses keep their two sentences (DESIGN.md): the fix is a
             button in this row's menu for one and Start all for the other. */
          <span title={statusTitle} data-probe="status-words" className="flex-none">
            {/* "Services off", not "Stopped": beside "Stopped by you" the bare
                word read as the same thing, and a "Stop site" item under a row
                that already said Stopped made no sense (owner, 14 Nov 2026). */}
            <StatusPill
              status={startingThis ? "starting" : status}
              className="min-w-[92px]"
              label={
                startingThis
                  ? "Starting…"
                  : stoppedByUser
                    ? "Stopped by you"
                    : status !== "running"
                      ? "Services off"
                      : undefined
              }
            />
          </span>
        )
      ) : provisioning ? (
        /* The job is still RUNNING: the backend persists the row at job start
           with provisioned=false, so without this branch a site being made
           looked exactly like one whose setup died. Nothing to retry yet. */
        <span
          data-probe="setup-running"
          className="flex-none"
          title="Setting up — the provision job is still running; its card is above the list"
        >
          <StatusPill status="starting" label="Setting up" className="min-w-[92px]" />
        </span>
      ) : provisioning === undefined ? (
        /* The registry has not answered yet: hold the slot, claim nothing. */
        <span className="min-w-[92px] flex-none" aria-hidden="true" />
      ) : (
        /* Honest half-site marker (v16): provisioning died or was cancelled —
           the site is NOT healthy-stopped. Retry re-runs the remaining
           idempotent steps; Delete (menu) removes it. Never rendered while
           the job runs or before that is known (the branches above; wk-check
           `badges`). */
        <span
          data-probe="setup-incomplete"
          className="flex min-w-[92px] flex-none items-center justify-center gap-1 whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="Provisioning did not finish — Retry re-runs the remaining steps; Delete removes the site."
        >
          setup incomplete
        </span>
      )}
      {!site.provisioned && provisioning === false && (
        <div className="flex-none" onClick={(e) => e.stopPropagation()}>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Retry setup"
            title="Retry setup — re-runs the remaining provisioning steps"
            onClick={onRetry}
          >
            <RefreshCw className="h-4 w-4" />
          </Button>
        </div>
      )}
      {/* The lock is drawn only when the site is NOT secured: on 25 secured
          sites it was 25 identical marks. */}
      {!site.ssl && (
        <span
          data-probe="insecure"
          title="No SSL — this site is served without a certificate"
          className="flex flex-none items-center"
        >
          {/* The colour sits ON the icon: it is a graphic (WCAG 1.4.11, 3:1), and
              on the wrapping span the contrast guard read it as text colour. */}
          <LockOpen className="h-3.5 w-3.5" strokeWidth={1.8} style={{ color: "var(--rex-lock-insecure)" }} />
        </span>
      )}
      {/* PHP and server as one quiet text in a fixed column (they were two
          bordered boxes per row). Below lg the column yields: the site page has
          both, and the three actions matter more than a version number. */}
      <span
        data-probe="stack"
        title={`PHP ${site.phpVersion} · ${site.webServer}`}
        className="hidden w-[104px] flex-none truncate font-mono text-[0.6875rem] text-rex-text-muted lg:block"
      >
        {site.phpVersion} · {SERVER_BADGE[site.webServer] ?? site.webServer}
      </span>
      {/* **The three things people open a site for, always on the row** — at
          every width, with no hover (ruling D7). They were `opacity-0` until
          hover and gone from layout below `lg`, so a narrow window offered a
          site nothing but its `⋮` menu, and Magic Login was not on the row at
          all. Everything else is in the menu (and on a right-click). */}
      <div
        data-site-row-actions
        className="flex flex-none items-center gap-px"
        onClick={(e) => e.stopPropagation()}
      >
        {/* A site the STACK is not serving has nothing at its URL — the browser
            would answer "can't connect" (the clean-VM run, 18 Sep 2026). The
            button says so instead of opening it. A site stopped BY THE USER is
            different: the edge is up and answers rexenv's own "this site is
            stopped" page, which is worth a look (DESIGN.md, two stopped-nesses). */}
        <span
          title={
            needsStack
              ? `Start rexenv's services and open ${url}`
              : browser
                ? `Open ${url} in ${browser.name}`
                : `Open ${url}`
          }
        >
          <Button
            variant="ghost"
            size="icon"
            aria-label="Open in browser"
            data-starts-services={needsStack ? "" : undefined}
            disabled={serve.busy || (!urlAnswers && !needsStack)}
            onClick={openSiteUrl}
          >
            {/* Icon of the browser the click really uses — no chevron here:
                choosing a different browser for one link lives on the site page. */}
            <PreferredBrowserIcon />
          </Button>
        </span>
        {/* The slot is held on a non-WordPress row so the columns line up. */}
        <span className="flex w-8 flex-none justify-center">
          {isWordpress && (
            <span
              title={
                stoppedByUser
                  ? "You stopped this site — start it first (row menu), then sign in"
                  : needsStack
                    ? "Start rexenv's services and sign in to this site's WordPress admin"
                    : "Magic Login — sign in to this site's WordPress admin"
              }
            >
              <Button
                variant="ghost"
                size="icon"
                data-starts-services={needsStack ? "" : undefined}
                // A site the USER stopped is not started behind their back: there
                // the sign-in stays off, and says why. With the services off it
                // starts them first — the tooltip says so before the click.
                disabled={!!stoppedByUser || signingIn || serve.busy || (status !== "running" && !needsStack)}
                onClick={() => {
                  setSigningIn(true);
                  void (needsStack ? serve.ensure(site) : Promise.resolve(true))
                    .then((ok) => (ok ? openWpAdmin(site, url) : undefined))
                    .catch(toastBackendError)
                    .finally(() => setSigningIn(false));
                }}
                aria-label="Magic Login"
              >
                <WordPressIcon className="h-4 w-4 text-rex-accent-blue" />
              </Button>
            </span>
          )}
        </span>
        {screen && (
          <Button
            variant="ghost"
            size="icon"
            aria-label={editor ? `Open in ${editor.name}` : "Open in editor"}
            title={editor ? `Open in ${editor.name}` : "Open in editor"}
            onClick={() => openSiteInEditor(editor, site.path, words.fileManager)}
          >
            <AppIcon
              icon={editor?.icon}
              fallback={<Code className="h-4 w-4" strokeWidth={1.7} />}
              className="h-4 w-4"
            />
          </Button>
        )}
      </div>
      <div className="flex-none" onClick={(e) => e.stopPropagation()}>
        <Menu
          contextTarget={rowRef}
          trigger={
            <button
              type="button"
              aria-label="More actions"
              className="flex h-7 w-7 items-center justify-center rounded-[7px] text-rex-text-muted transition-colors hover:bg-rex-hover-strong hover:text-rex-text"
            >
              <MoreVertical className="h-4 w-4" />
            </button>
          }
        >
          {onKeep && (
            /* Adoption, and the only door an agent does not have. Above Rename
               because for a scratch site it is the decision the user came for —
               and renaming would adopt it anyway (#214), which is a surprise
               rather than a choice. */
            <MenuItem
              icon={<PinIcon className="h-[15px] w-[15px]" strokeWidth={1.7} />}
              onSelect={onKeep}
            >
              Keep this site
            </MenuItem>
          )}
          {onToggleEnabled && (
            /* The row's own lifecycle, and the copy has to keep it apart from the
               footer's "Stop all": this stops ONE site's serving surface — the
               shared web server and PHP pool keep running for the others.

               ONE slot, four states (owner, 14 Nov 2026). With rexenv's services
               off the row already reads "Services off", and "Stop site" under it
               was an offer to stop something that is not running; "Start site"
               there recorded the switch and served nothing. So with the services
               off the slot starts them — and, for a site the user had stopped,
               starts the site too, in the words that say both. */
            <MenuItem
              icon={
                stoppedByUser || needsStack ? (
                  <Play className="h-[15px] w-[15px]" strokeWidth={1.7} />
                ) : (
                  <Square className="h-[15px] w-[15px]" strokeWidth={1.7} />
                )
              }
              disabled={serve.busy}
              onSelect={() => {
                if (needsStack) void serve.startStack();
                else if (stoppedByUser && stackUp === false)
                  void serve.startStack().then((ok) => { if (ok) onToggleEnabled(true); });
                else onToggleEnabled(!!stoppedByUser);
              }}
            >
              {togglePending
                ? stoppedByUser
                  ? "Starting…"
                  : "Stopping…"
                : needsStack
                  ? "Start services"
                  : stoppedByUser
                    ? stackUp === false
                      ? "Start site and services"
                      : "Start site"
                    : "Stop site"}
            </MenuItem>
          )}
          {onPin && (
            <MenuItem
              icon={
                pinned ? (
                  <PinOff className="h-[15px] w-[15px]" strokeWidth={1.7} />
                ) : (
                  <PinIcon className="h-[15px] w-[15px]" strokeWidth={1.7} />
                )
              }
              onSelect={() => onPin(!pinned)}
            >
              {pinned ? "Unpin" : "Pin to sidebar"}
            </MenuItem>
          )}
          {(onKeep || onToggleEnabled || onPin) && <MenuSeparator />}
          {/* The site page's tabs, one step from the list. */}
          {isWordpress && (
            <MenuItem
              icon={<LayoutGrid className="h-[15px] w-[15px]" strokeWidth={1.7} />}
              onSelect={() => onOpenTab("wordpress")}
            >
              WordPress
            </MenuItem>
          )}
          <MenuItem
            icon={<Database className="h-[15px] w-[15px]" strokeWidth={1.7} />}
            disabled={!hasDb}
            onSelect={() => onOpenTab("database")}
          >
            {hasDb ? "Database" : "Database (this site has none)"}
          </MenuItem>
          <MenuItem
            icon={<TerminalSquare className="h-[15px] w-[15px]" strokeWidth={1.7} />}
            onSelect={() => onOpenTab("terminal")}
          >
            Terminal
          </MenuItem>
          <MenuItem
            icon={<ScrollText className="h-[15px] w-[15px]" strokeWidth={1.7} />}
            onSelect={() => onOpenTab("logs")}
          >
            Logs
          </MenuItem>
          {screen && (
            <MenuItem
              icon={<FolderOpen className="h-[15px] w-[15px]" strokeWidth={1.7} />}
              onSelect={() => void openPath(site.path).catch(toastBackendError)}
            >
              Open folder
            </MenuItem>
          )}
          <MenuSeparator />
          <MenuItem icon={<Pencil className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onRename}>
            Rename
          </MenuItem>
          <MenuItem icon={<Copy className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onDuplicate}>
            Duplicate
          </MenuItem>
          <MenuItem
            icon={<Link className="h-[15px] w-[15px]" strokeWidth={1.7} />}
            onSelect={() => {
              void navigator.clipboard?.writeText(site.domain);
              setCopied(true);
              setTimeout(() => setCopied(false), 1200);
            }}
          >
            {copied ? "Copied!" : "Copy domain"}
          </MenuItem>
          <MenuSeparator />
          <MenuItem
            icon={<Trash2 className="h-[15px] w-[15px]" strokeWidth={1.7} />}
            danger
            onSelect={onDelete}
          >
            Delete
          </MenuItem>
        </Menu>
      </div>
    </div>
  );
}


export function Sites() {
  const qc = useQueryClient();
  // One read for the whole page (v42), the same shape as the serving map: a
  // list of twenty sites must not become twenty round trips to answer "does
  // this one answer on more than one name".
  const { data: extraDomains = {} } = useQuery({
    queryKey: ["site-domains", "all"],
    queryFn: allSiteDomains,
  });
  const navigate = useNavigate();
  const [showNew, setShowNew] = useState(false);
  // The view lives OUTSIDE the route (`useSitesListState`): it used to be
  // `useState` here and reset on every return from a site page.
  const view = useSitesListState();
  const { query, filter, typeFilter, sort } = view;
  const setQuery = (query: string) => view.set({ query });
  const setFilter = (filter: Filter) => view.set({ filter });
  const setTypeFilter = (typeFilter: TypeFilter) => view.set({ typeFilter });
  const { data: worktreeRows = [] } = useQuery({ queryKey: ["sites", "worktree-relations"], queryFn: worktreeRelations });
  const worktreeBranchOf = new Map(worktreeRows.map((w) => [w.siteId, w.askedBranch]));
  const { data: sites = [], isLoading } = useQuery({
    queryKey: ["sites"],
    queryFn: listSites,
  });
  // Empty-state copy names the USER'S configured TLD, not a hardcoded one.
  const { data: tld = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });
  // A site's displayed status is its live *serving* state, NOT the sites.status
  // column (task 2.1 / H1): serving only when the edge is up AND the site's own
  // upstream (php-fpm pool or FrankenPHP backend) is up, so a partial stack no
  // longer shows every site as running (H1 follow-up). Keyed by domain.
  const { data: serving } = useQuery({
    queryKey: ["sites-serving"],
    queryFn: getSitesServing,
    refetchInterval: 2000,
  });
  // Honest per-site resources (dedicated CPU/RAM vs activity+DB) — 5s poll:
  // each read parses the nginx access-log tail + one DB-sizes query.
  const { data: resources } = useQuery({
    queryKey: ["sites-resources"],
    queryFn: sitesResources,
    refetchInterval: 5000,
  });
  // Settled DB-import facts (v20) — drives the "DB imported · not connected"
  // badge. One query for the whole page.
  const { data: dbRecords = [] } = useQuery({
    queryKey: ["db-import-records"],
    queryFn: dbImportRecords,
  });
  const dbStates = useMemo(
    () => new Map(dbRecords.map((r) => [r.siteId, r.state])),
    [dbRecords],
  );
  const resourcesMap = useMemo(
    () => new Map((resources ?? []).map((r) => [r.id, r])),
    [resources],
  );
  const servingMap = useMemo(
    () => new Map((serving ?? []).map((s) => [s.domain, s.serving])),
    [serving],
  );
  const statusOf = (site: Site): Site["status"] =>
    servingMap.get(site.domain) ? "running" : "stopped";

  const shortlist = useSiteShortlist();
  const pin = usePinSite();
  const toggle = useSiteEnabled();
  const togglingSites = usePendingVariables<{ site: Site }>(SITE_ENABLED_KEY);
  const bulk = useAllSitesEnabled();
  // The two numbers the bulk menu needs, over PROVISIONED sites only — a
  // half-provisioned site is skipped by the backend, so counting it here would
  // offer an action for a site that will not move.
  const switchable = sites.filter((s) => s.provisioned);
  const stoppedCount = switchable.filter((s) => s.enabled === false).length;
  const servedCount = switchable.length - stoppedCount;

  // Streamed provision job (New Site / Retry) — re-adopted here so a create
  // started in the dialog survives closing it. The banner shows while
  // running and stays FROZEN after a failure/cancel (the row also carries
  // the "setup incomplete" badge); a job that settled ok needs no banner.
  const downloads = useDownloads();
  const prov = useSiteProvision();
  useEffect(() => {
    void prov.adopt().then((j) => {
      if (j && j.status === "ok") prov.clear();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const retry = useMutation({
    mutationFn: (site: Site) => siteProvisionRetry(site.id),
    onSuccess: (snap) => prov.start(snap),
    onError: (e) => toastBackendError(e),
  });
  // Which not-yet-provisioned rows have a job RUNNING right now, asked of the
  // backend's job registry — the one place that knows, whoever started the job
  // (the dialog, Retry, `rex site create`, an agent) — for as long as such a
  // row exists. A row is persisted at job start with provisioned=false, so the
  // flag alone read a site being made as one whose setup had died (30 Sep
  // 2026). Unknown until the first answer: the row then claims neither state.
  const unprovisioned = useMemo(() => sites.filter((s) => !s.provisioned).map((s) => s.domain), [sites]);
  const jobOf = useQueries({
    queries: unprovisioned.map((domain) => ({
      queryKey: ["site-provision-active", domain],
      queryFn: () => siteProvisionActive(domain),
      refetchInterval: 2000,
    })),
  });
  const provisioningOf = new Map<string, boolean>();
  unprovisioned.forEach((domain, i) => {
    const q = jobOf[i];
    if (q.data !== undefined) provisioningOf.set(domain, q.data?.status === "running");
  });

  // Rename / Duplicate / Delete — the dialogs and their mutations are shared with
  // the site page (`useSiteActions`), so the two screens cannot drift.
  const actions = useSiteActions({ onDuplicateStarted: prov.start });

  const running = sites.filter((s) => statusOf(s) === "running").length;

  // The page opens on All, and only a tab the user clicks changes it. It opened
  // on Running for a while (chosen once, after the first serving poll); the
  // owner asked for All back on 11 Sep 2026 — every site visible on arrival.

  // **Each control counts what the OTHER one has already selected.** The
  // alternative — both counting all sites — writes a number above a list that
  // does not contain that many rows, and the page ends up contradicting itself
  // about how many WordPress sites the user has. So the status tabs count
  // within the chosen type, and the type tabs count within the chosen status:
  // every number is exactly what clicking that tab would show.
  const inType = sites.filter((s) => typeFilter === "all" || s.type === typeFilter);
  const runningInType = inType.filter((s) => statusOf(s) === "running").length;
  const counts: Partial<Record<Filter, number>> = {
    running: runningInType,
    stopped: inType.length - runningInType,
  };
  const inStatus = sites.filter((s) => {
    const st = statusOf(s);
    if (filter === "running") return st === "running";
    if (filter === "stopped") return st !== "running";
    return true;
  });
  const typeCounts: Partial<Record<TypeFilter, number>> = {
    wordpress: inStatus.filter((s) => s.type === "wordpress").length,
    laravel: inStatus.filter((s) => s.type === "laravel").length,
    php: inStatus.filter((s) => s.type === "php").length,
  };

  // The type filter offers the types the user HAS, and nothing when they have
  // one: a person with 25 WordPress sites was shown "Laravel 0 · Blank PHP 0".
  // Over ALL sites, not the filtered ones — a type must not vanish from the
  // control because the status tab beside it emptied it. A type chosen earlier
  // and since deleted away stays listed, so the filter can always be seen.
  const typesPresent = (["wordpress", "laravel", "php"] as SiteType[]).filter(
    (k) => k === typeFilter || sites.some((s) => s.type === k),
  );

  // The scroll position survives a visit to a site, like the filter. Restored
  // once, after the rows exist to scroll to; saved as the list moves.
  const listRef = useRef<HTMLDivElement>(null);
  const restored = useRef(false);
  useLayoutEffect(() => {
    if (restored.current || isLoading || sites.length === 0) return;
    restored.current = true;
    if (listRef.current) listRef.current.scrollTop = useSitesListState.getState().scrollTop;
  }, [isLoading, sites.length]);

  // `/` puts the cursor in the filter — unless a key is already going somewhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
      const el = e.target as HTMLElement | null;
      if (el?.closest?.('input, textarea, select, [contenteditable="true"], .xterm')) return;
      if (document.querySelector('[role="dialog"]')) return;
      const box = document.querySelector<HTMLInputElement>("[data-topbar-search]");
      if (!box) return;
      e.preventDefault();
      box.focus();
      box.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Filter (segmented) → search (name/domain) → sort.
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = sites.filter((s) => {
      const st = statusOf(s);
      if (filter === "running" && st !== "running") return false;
      if (filter === "stopped" && st === "running") return false;
      if (typeFilter !== "all" && s.type !== typeFilter) return false;
      // Any name the site answers on: the row's `+N` tooltip shows the extra
      // domains, so typing one of them and getting "No sites match" is the
      // page contradicting itself.
      if (
        q &&
        !s.name.toLowerCase().includes(q) &&
        !s.domain.toLowerCase().includes(q) &&
        !(extraDomains[s.id] ?? []).some((d) => d.toLowerCase().includes(q))
      )
        return false;
      return true;
    });
    return [...list].sort((a, b) => {
      if (sort === "recent") return b.createdAt.localeCompare(a.createdAt);
      if (sort === "opened") {
        // Newest open first; a site never opened sorts after every opened one,
        // by name — "never" is a fact, not the oldest date.
        const oa = shortlist.openedAt[a.id];
        const ob = shortlist.openedAt[b.id];
        if (oa && ob && oa !== ob) return ob.localeCompare(oa);
        if (!!oa !== !!ob) return oa ? -1 : 1;
      }
      if (sort === "status") {
        // Serving sites first, then by name (per-site status is meaningful again).
        const rank = (s: Site) => (statusOf(s) === "running" ? 0 : 1);
        if (rank(a) !== rank(b)) return rank(a) - rank(b);
      }
      return a.name.localeCompare(b.name);
    });
    // `statusOf` is recreated every render and reads only `servingMap`, which is
    // already a dependency — depending on the FUNCTION would rebuild this list on
    // every render and defeat the memo. The data is the dependency that matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sites, filter, typeFilter, query, sort, servingMap, shortlist]);

  // The render split, on the RECORDED fact only (`isScratch`). Counts and the
  // filter stay over ALL sites: a scratch site is a real site, and grouping is
  // a way of showing them, not a second class of thing.
  const scratchVisible = useMemo(() => visible.filter(isScratch), [visible]);
  // Pinned sites lead the list in their own group, whatever the sort — a pin is
  // "keep this one at hand", and a sort that buried it would unsay that. A
  // scratch site is never pinned (core refuses it), so the groups cannot overlap.
  const pinnedSet = useMemo(() => new Set(shortlist.pinned), [shortlist]);
  const pinnedVisible = useMemo(
    () => visible.filter((s) => pinnedSet.has(s.id) && !isScratch(s)),
    [visible, pinnedSet],
  );
  const ownVisible = useMemo(
    () => visible.filter((s) => !isScratch(s) && !pinnedSet.has(s.id)),
    [visible, pinnedSet],
  );

  // Cloned plugins/themes — one read for the page, only when there is a scratch
  // site to describe, so a user who has never used an agent never pays for it.
  const anyScratch = sites.some(isScratch);
  const { data: packages = [] } = useQuery({
    queryKey: ["scratch-packages"],
    queryFn: scratchPackages,
    enabled: anyScratch,
  });
  const packagesBySite = useMemo(() => {
    const m = new Map<string, ScratchPackage[]>();
    for (const p of packages) {
      const list = m.get(p.siteId);
      if (list) list.push(p);
      else m.set(p.siteId, [p]);
    }
    return m;
  }, [packages]);

  // Why an expired scratch site is still here, from the reaper's OWN feed row
  // (#215) — never re-derived. Fetched only when something is actually expired,
  // which is the only time the answer is asked for.
  const anyExpired = scratchVisible.some((s) => expiryLabel(s.expiresAt)?.expired);
  const { data: feed = [] } = useQuery({
    queryKey: ["agent-activity", "reap-failures"],
    queryFn: () => agentActivity(null, 50),
    enabled: anyExpired,
  });
  const reapFailures = useMemo(() => {
    const m = new Map<string, string>();
    // Newest first, so the FIRST row for a site is its latest word — an older
    // failure must not outrank a newer success.
    for (const a of feed) {
      if (a.tool !== "scratch_reap" || !a.targetSite || m.has(a.targetSite)) continue;
      if (a.outcome === "error") m.set(a.targetSite, a.detail ?? "rexenv didn't record a reason");
      else m.set(a.targetSite, "");
    }
    return m;
  }, [feed]);

  const [keepTarget, setKeepTarget] = useState<Site | null>(null);
  const keep = useMutation({
    mutationFn: (site: Site) => keepSite(site.id),
    onSuccess: (changed, site) => {
      // `false` = it was already the user's (a race with Keep-by-mutation or a
      // second click). Not an error, and not worth a toast that implies one.
      if (changed) toast.success(`${site.domain} is yours now — rexenv won't clean it up.`);
      void qc.invalidateQueries({ queryKey: ["sites"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const noResults = sites.length > 0 && visible.length === 0;

  const newSiteButton = (
    <Button variant="primary" onClick={() => setShowNew(true)}>
      <Plus className="h-[15px] w-[15px]" strokeWidth={2.2} />
      New site
    </Button>
  );

  const headerActions = (
    <>
      <SortButton value={sort} onCycle={() => view.set({ sort: SORT_CYCLE[sort] })} />
      {newSiteButton}
      {/* The Sites page's OWN bulk switch, and the copy has to keep it
          apart from the footer's "Stop all": that one stops rexenv's
          services (machine-wide), this one stops the SITES and leaves the
          stack up. A menu rather than one guessing button — with some
          sites running and some stopped, either verb is a guess, and
          guessing wrong here changes every site. */}
      {/* `flex-none` goes on a WRAPPER, not the button: `Menu` renders its own
          `inline-flex` div, and THAT is the flex item this row shrinks — which
          wrapped the label onto two lines while the button itself was doing as
          it was told. */}
      <div className="flex-none">
      <Menu
        trigger={
          <button
            type="button"
            data-sites-bulk
            aria-label="Start or stop all sites"
            className="flex h-[34px] w-[34px] items-center justify-center rounded-[9px] border border-rex-border bg-rex-surface-1 text-rex-text-muted transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2 hover:text-rex-text"
            title="Start or stop every site — rexenv's services keep running"
          >
            {/* It read "All sites ▾" — beside the "All" filter tab, and the same
                two words as the site page's link back to this list, which did
                something else. The menu's own items name what they do. */}
            <MoreHorizontal className="h-4 w-4" />
          </button>
        }
      >
        <MenuItem
          icon={<Play className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          disabled={stoppedCount === 0 || bulk.isPending}
          onSelect={() => bulk.mutate(true)}
        >
          {stoppedCount === 0
            ? "Start all sites (none stopped)"
            : `Start all sites (${stoppedCount} stopped)`}
        </MenuItem>
        <MenuItem
          icon={<Square className="h-[15px] w-[15px]" strokeWidth={1.7} />}
          disabled={servedCount === 0 || bulk.isPending}
          onSelect={() => bulk.mutate(false)}
        >
          {servedCount === 0
            ? "Stop all sites (none started)"
            : /* "started", not "serving": this counts the SWITCH, and a site
                 can be started while the stack is down — "serving" would
                 promise something the number does not check. */
              `Stop all sites (${servedCount} started)`}
        </MenuItem>
      </Menu>
      </div>
    </>
  );

  return (
    <>
      <TopBar
        title="Sites"
        subtitle={
          isLoading ? "Loading…" : `${sites.length} sites · ${running} running`
        }
        searchPlaceholder="Filter sites…"
        searchValue={query}
        onSearchChange={setQuery}
        action={headerActions}
      />
      {!isLoading && sites.length > 0 && (
        /* Two filter controls, a bulk action and the column headings on one
           row: it WRAPS rather than overlapping — at 1100px the third group has
           nowhere to go, and a `justify-between` row silently drew them on top
           of each other. */
        <div className="flex flex-none flex-wrap items-center gap-y-2 px-[22px] pb-[9px] pt-[14px]">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <FilterTabs
              value={filter}
              onChange={setFilter}
              counts={counts}
              tabs={[
                { key: "all", label: "All" },
                { key: "running", label: "Running" },
                { key: "stopped", label: "Stopped" },
              ]}
            />
            {typesPresent.length > 1 && (
              <FilterTabs
                value={typeFilter}
                onChange={setTypeFilter}
                counts={typeCounts}
                tabs={(["all", ...typesPresent] as TypeFilter[]).map((k) => ({
                  key: k,
                  label: TYPE_LABEL[k],
                }))}
              />
            )}
          </div>
        </div>
      )}
      <div
        ref={listRef}
        data-sites-list
        onScroll={(e) => view.set({ scrollTop: e.currentTarget.scrollTop })}
        className="min-h-0 flex-1 overflow-auto px-[18px] pb-[18px]"
      >
        <ResolverDriftBanner />
        <ReapBanner />
        <ImportBanner />
        {prov.job && (prov.running || prov.job.status !== "ok") && (
          <div className="mb-2 mt-1">
            <SiteProvisionCard
              job={prov.job}
              lines={prov.lines}
              downloads={downloads}
              onCancel={() => void siteProvisionCancel(prov.job!.id).catch(toastBackendError)}
            />
            {!prov.running && (
              <button
                className="mt-1 text-[0.6875rem] text-rex-text-muted underline decoration-dotted hover:text-rex-text"
                onClick={() => prov.clear()}
              >
                Dismiss
              </button>
            )}
          </div>
        )}
        {isLoading ? (
          <Placeholder
            icon={<Globe className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Loading sites…"
            hint="Reading your local sites"
          />
        ) : sites.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-4 p-10 text-center">
            <div className="flex h-[60px] w-[60px] items-center justify-center rounded-2xl border border-rex-crown-border bg-gradient-to-br from-rex-crown-chip-from to-rex-crown-chip-to shadow-glow-crown">
              <RexLogo className="block h-[28px] w-auto" />
            </div>
            <div>
              <div className="text-[1.1875rem] font-semibold text-rex-text">No sites yet</div>
              <div className="mt-2 max-w-[400px] text-[0.84375rem] leading-[1.55] text-rex-text-muted">
                Point rexenv at a folder and it serves your site instantly — with its
                own <span className="font-mono">.{tld}</span> domain, PHP, and database.
              </div>
            </div>
            <Button variant="primary" size="lg" onClick={() => setShowNew(true)}>
              <Plus className="h-4 w-4" strokeWidth={2.3} />
              Create your first site
            </Button>
          </div>
        ) : noResults ? (
          <div className="flex flex-col items-center justify-center gap-1.5 px-5 py-[54px] text-center">
            <div className="text-[0.875rem] font-medium text-rex-text-bright">
              {query ? `No sites match “${query}”` : "No sites in this view"}
            </div>
            <div className="text-[0.78125rem] text-rex-text-muted">
              Try a different name, domain, or clear the filters.
            </div>
            {/* The way out is a BUTTON, not advice to go and find one: the
                filters that emptied this list are two controls up, and one of
                them (Running) can be chosen for the user on first load. */}
            <button
              className="mt-1 text-[0.78125rem] text-brand-tint underline decoration-dotted"
              onClick={() => view.reset()}
            >
              Show all sites
            </button>
          </div>
        ) : (
          (() => {
            const row = (site: Site) => (
              <SiteRow
                key={site.id}
                site={site}
                status={statusOf(site)}
                resources={resourcesMap.get(site.id)}
                dbState={dbStates.get(site.id)}
                extraDomains={extraDomains[site.id]}
                worktreeBranch={worktreeBranchOf.get(site.id)}
                packages={packagesBySite.get(site.id)}
                reapFailure={reapFailures.get(site.id) || undefined}
                provisioning={site.provisioned ? false : provisioningOf.get(site.domain)}
                onOpen={() => navigate(`/sites/${site.id}`)}
                onDelete={() => actions.remove(site)}
                onOpenTab={(tab) => navigate(`/sites/${site.id}/${tab}`)}
                onRename={() => actions.rename(site)}
                onDuplicate={() => actions.duplicate(site)}
                onRetry={() => retry.mutate(site)}
                onKeep={isScratch(site) ? () => setKeepTarget(site) : undefined}
                pinned={pinnedSet.has(site.id)}
                onPin={isScratch(site) ? undefined : (pinned) => pin.mutate({ site, pinned })}
                stoppedByUser={site.enabled === false}
                togglePending={togglingSites.some((t) => t.site.id === site.id)}
                onToggleEnabled={
                  /* A half-provisioned site has no serving surface to switch:
                     Retry is its verb, and the backend refuses this one anyway
                     — an action offered and then refused is worse than absent. */
                  site.provisioned
                    ? (enabled) => toggle.mutate({ site, enabled })
                    : undefined
                }
              />
            );
            return (
              <div className="flex flex-col">
                {pinnedVisible.length > 0 && (
                  <>
                    <GroupHeading icon={<PinIcon className="h-3.5 w-3.5 flex-none text-rex-text-dim" strokeWidth={1.8} />} label="Pinned" count={pinnedVisible.length} tight />
                    <div data-sites-group="pinned" className="flex flex-col">{pinnedVisible.map(row)}</div>
                    {ownVisible.length > 0 && (
                      <GroupHeading label="All sites" count={ownVisible.length} />
                    )}
                  </>
                )}
                {ownVisible.map(row)}
                {scratchVisible.length > 0 && (
                  <>
                    {/* The group exists only when there IS one, so a user who
                        has never used an agent never sees an empty section
                        telling them the feature exists. */}
                    <ScratchGroupHeading
                      count={scratchVisible.length}
                      tight={ownVisible.length === 0 && pinnedVisible.length === 0}
                    />
                    {scratchVisible.map(row)}
                  </>
                )}
              </div>
            );
          })()
        )}
      </div>
      {showNew && <NewSiteDialog onClose={() => setShowNew(false)} onStarted={prov.start} />}
      {actions.dialogs}
      {keepTarget && (
        <KeepSiteDialog
          site={keepTarget}
          onKeep={() => {
            keep.mutate(keepTarget);
            setKeepTarget(null);
          }}
          onCancel={() => setKeepTarget(null)}
        />
      )}
    </>
  );
}

/** A plain group heading in the Sites list — "Pinned", and "All sites" beneath
 *  it. Rendered only when there IS a pinned group: a user who has pinned nothing
 *  sees the list exactly as it was, with no heading announcing a feature. */
function GroupHeading({
  icon,
  label,
  count,
  tight,
}: {
  icon?: React.ReactNode;
  label: string;
  count: number;
  tight?: boolean;
}) {
  return (
    <div className={cn("flex items-center gap-2 px-3 pb-1 pt-5", tight && "pt-1")}>
      {icon}
      <span className="font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
        {label}
      </span>
      <span className="font-mono text-[0.625rem] text-rex-text-muted">{count}</span>
    </div>
  );
}

/**
 * The Agent-scratch section heading.
 *
 * Its own component so the L2 fixture reviews the SHIPPED heading rather than a
 * copy of it — a heading that only ever existed inline would be reviewed by
 * eye once and never again. It renders only when the group is non-empty (the
 * caller's job), so a user who has never used an agent never meets an empty
 * section advertising the feature.
 */
export function ScratchGroupHeading({ count, tight }: { count: number; tight?: boolean }) {
  return (
    <div className={cn("flex items-center gap-2 px-3 pb-1 pt-5", tight && "pt-1")}>
      <Bot className="h-3.5 w-3.5 flex-none text-rex-text-dim" strokeWidth={1.8} />
      {/* flex-none + nowrap: the description beside it is the part that gives
          way. Without them the label broke into "AGENT / SCRATCH" (Ubuntu VM). */}
      <span className="flex-none whitespace-nowrap font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
        Agent scratch
      </span>
      <span className="font-mono text-[0.625rem] text-rex-text-muted">{count}</span>
      <span className="min-w-0 truncate text-[0.71875rem] text-rex-text-muted">
        Disposable sites an AI agent created — rexenv deletes them once nothing has used them for a
        while. Keep one to make it yours.
      </span>
    </div>
  );
}

/**
 * The Keep confirm — **copy approved 2 Aug 2026 and landed verbatim** (#219).
 *
 * Every paragraph is doing a job, so none of them is filler to trim later:
 * the second says what does NOT change (the fear is that adopting rebuilds or
 * moves something), the third names the default that applies if they do
 * nothing, and the fourth states there is no un-keep — which is true, and
 * cheaper to say here than to discover afterwards. The only way back is
 * deleting the site like any other, and that is the sentence rather than a
 * disabled "un-keep" nobody would find.
 */
export function KeepSiteDialog({
  site,
  onKeep,
  onCancel,
}: {
  site: Site;
  onKeep: () => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      title={`Keep ${site.domain}?`}
      confirmLabel="Keep this site"
      onConfirm={onKeep}
      onCancel={onCancel}
      message={
        <div className="flex flex-col gap-2">
          <p>
            It becomes one of your own sites: rexenv stops treating it as disposable and will
            never clean it up. Nothing about the site itself changes — the files, the database
            and the URL stay exactly as they are, and it keeps working. It also frees a slot, so
            the agent can create another scratch site right away.
          </p>
          <p>Without this, rexenv deletes it once nothing has used it for a while.</p>
          <p>There&rsquo;s no un-keep — you&rsquo;d delete it like any other site.</p>
        </div>
      }
    />
  );
}

/**
 * The scratch reaper's sweep, surfaced once.
 *
 * A sweep at launch WITHOUT a summary is a silent bulk delete that a user
 * returning after a week cannot tell from data loss (#215) — so this exists to
 * be seen, not to be pretty. It carries rexenv's own text, which names the
 * domains rather than counting them, so someone recognises a site they cared
 * about and can act. Dismissible because the feed rows are the durable record:
 * nothing is lost by closing it. Nothing renders on a quiet launch — the
 * backend simply never emits, so a user with no scratch sites never learns the
 * reaper exists.
 */
function ReapBanner() {
  const [summary, setSummary] = useState<string | null>(null);
  useEffect(() => {
    let un: (() => void) | undefined;
    void onScratchReaped((text) => setSummary(text)).then((f) => {
      un = f;
    });
    return () => un?.();
  }, []);
  if (!summary) return null;
  return (
    <div className="mb-2 mt-1 flex items-start gap-3 rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3">
      <Bot className="mt-px h-4 w-4 flex-none text-rex-text-muted" strokeWidth={1.7} />
      <div className="min-w-0 flex-1">
        <div className="text-[0.8125rem] font-medium text-rex-text">Scratch sites cleaned up</div>
        <div className="whitespace-pre-line text-[0.71875rem] leading-[1.5] text-rex-text-muted">
          {summary}
        </div>
      </div>
      <Button variant="ghost" size="icon" aria-label="Dismiss" onClick={() => setSummary(null)}>
        <X className="h-4 w-4" />
      </Button>
    </div>
  );
}

/**
 * "Import from Valet, Herd or Local" nudge.
 *
 * Only appears when a scan would ACTUALLY find something, so a user with
 * none of those tools never sees it, and it is dismissible so it can't nag someone who
 * has already decided. Most useful on an empty site list — which is exactly
 * when a Valet user is wondering where their sites are.
 */
/** TLDs another tool took back — the user's sites on them are dark while
 *  every health check stays green, so the fact comes to them instead of
 *  waiting in a log or on the Import screen (ruled 15 Aug 2026; copy approved
 *  with one redline). Two load-bearing behaviours:
 *  - `[]` renders NOTHING. Nothing-taken-back is the ordinary state, and a
 *    notice for it would be the import bug's shape in a new place (#306).
 *  - Dismissal is PER-TAKEOVER: its key is `<tld>@<takenAt>`, and every
 *    re-take refreshes `takenAt`, so a dismissed .test returns on the NEXT
 *    loss and a newly lost .dev is never hidden by an old dismissal. The key
 *    used to be the bare TLD, cleared only when THIS banner saw the TLD read
 *    as ours again — a re-take made on Import or Settings never passed through
 *    here, and the next loss stayed hidden (the 15.8 VM, §F 7c, 9 Oct 2026,
 *    ledger #807). */
export function ResolverDriftBanner() {
  const navigate = useNavigate();
  const DISMISS_KEY = "rexenv.resolverDriftDismissedTlds";
  const [dismissed, setDismissed] = useState<string[]>(() => {
    try {
      const v = JSON.parse(localStorage.getItem(DISMISS_KEY) ?? "[]");
      return Array.isArray(v) ? v.filter((t) => typeof t === "string") : [];
    } catch {
      return [];
    }
  });
  const { data, isSuccess } = useQuery({
    queryKey: ["resolver-drift"],
    queryFn: resolverDrift,
    staleTime: 60_000,
  });
  // Coerced, not trusted: the dev harness's catch-all mock once answered `1`
  // here and the crash took the whole Sites route with it. A malformed answer
  // must degrade to the ordinary state, never to a white page.
  const records = useMemo(
    () =>
      Array.isArray(data)
        ? data.filter(
            (r): r is DriftedTakeover =>
              !!r && typeof r === "object" && typeof r.tld === "string" && typeof r.takenAt === "string",
          )
        : [],
    [data],
  );
  const drift = useMemo(() => records.map((r) => `${r.tld}@${r.takenAt}`), [records]);
  // Pruning keeps the stored set to CURRENT losses only (storage hygiene, and
  // pre-#807 bare-TLD entries fall out here); re-showing on the next loss does
  // not depend on it — a re-take is a new key.
  useEffect(() => {
    // ONLY on a resolved answer: while the query loads, `drift` is [] and []
    // means "unknown", not "ours again" — pruning on it wiped every dismissal
    // on every mount. The harness probe caught this on its first run (the
    // dismissed banner re-rendered after a reload).
    if (!isSuccess) return;
    const pruned = dismissed.filter((t) => drift.includes(t));
    if (pruned.length !== dismissed.length) {
      localStorage.setItem(DISMISS_KEY, JSON.stringify(pruned));
      setDismissed(pruned);
    }
  }, [isSuccess, drift, dismissed]);
  const visible = records.filter((r) => !dismissed.includes(`${r.tld}@${r.takenAt}`));
  if (visible.length === 0) return null;

  const names = visible.map((r) => `.${r.tld}`);
  const list =
    names.length === 1
      ? names[0]
      : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  const single = names.length === 1;
  return (
    <div
      data-probe="resolver-drift-banner"
      className="mb-2 mt-1 flex items-start gap-3 rounded-xl border border-status-warning-border bg-status-warning-bg px-4 py-3"
    >
      <AlertCircle className="mt-0.5 h-4 w-4 flex-none text-status-warning-bright" strokeWidth={1.8} />
      <div className="min-w-0 flex-1">
        <div className="text-[0.8125rem] font-medium text-rex-text">
          Your {list} sites stopped resolving
        </div>
        <div className="mt-0.5 text-[0.71875rem] leading-[1.5] text-rex-text-muted">
          {single
            ? `Valet or Herd took ${list}'s resolver file back, so those sites won't load until rexenv takes it over again. You can take it back from Settings or Import.`
            : `Valet or Herd took the resolver files for ${list} back, so those sites won't load until rexenv takes them over again. You can take them back from Settings or Import.`}
        </div>
      </div>
      <Button variant="secondary" onClick={() => navigate("/import")}>
        Go to Import
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          const next = [...new Set([...dismissed, ...visible.map((r) => `${r.tld}@${r.takenAt}`)])];
          localStorage.setItem(DISMISS_KEY, JSON.stringify(next));
          setDismissed(next);
        }}
      >
        Dismiss
      </Button>
    </div>
  );
}

function ImportBanner() {
  const navigate = useNavigate();
  const DISMISS_KEY = "rexenv.importBannerDismissed";
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(DISMISS_KEY) === "1",
  );
  const { data } = useQuery({
    queryKey: ["valet-scan"],
    queryFn: scanValetImport,
    enabled: !dismissed,
    staleTime: 60_000,
  });
  const ready = (data?.candidates ?? []).filter(
    (c) => c.status.status === "importable",
  ).length;
  if (dismissed || ready === 0) return null;
  return (
    <div className="mb-2 mt-1 flex items-center gap-3 rounded-xl border border-rex-border bg-rex-surface-1 px-4 py-3">
      <FolderInput className="h-4 w-4 flex-none text-brand" strokeWidth={1.7} />
      <div className="min-w-0 flex-1">
        <div className="text-[0.8125rem] text-rex-text">
          {ready} site{ready === 1 ? "" : "s"} found in Valet, Herd or Local
        </div>
        <div className="text-[0.71875rem] text-rex-text-muted">
          Import them where they already live — nothing is copied, and your old setup is left
          untouched.
        </div>
      </div>
      <Button variant="secondary" onClick={() => navigate("/import")}>
        Review import
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          localStorage.setItem(DISMISS_KEY, "1");
          setDismissed(true);
        }}
      >
        Dismiss
      </Button>
    </div>
  );
}

