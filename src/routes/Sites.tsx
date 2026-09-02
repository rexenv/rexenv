import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, FolderInput, Plus, Globe, FolderOpen, Database, Lock, LockOpen, Trash2, MoreVertical, ArrowDownUp, Pencil, Copy, Code, Link, RefreshCw, Pin as PinIcon, Bot, X } from "lucide-react";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { RexLogo } from "@/components/common/RexLogo";
import { toast, toastBackendError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Menu, MenuItem, MenuSeparator } from "@/components/ui/menu";
import { ConfirmDialog, PromptDialog } from "@/components/ui/dialog";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { siteTypeMeta } from "@/lib/siteType";
import { StatusPill } from "@/components/common/StatusPill";
import { Placeholder } from "@/components/common/Placeholder";
import { NewSiteDialog } from "@/components/sites/NewSiteDialog";
import { Button } from "@/components/ui/button";
import { allSiteDomains, defaultTld, listSites, resolverDrift, deleteSite, renameSite, openExternal, getSitesServing, sitesResources, siteProvisionCancel, siteProvisionRetry , scanValetImport, dbImportRecords, rewriteRevert, keepSite, scratchPackages, onScratchReaped, agentActivity } from "@/lib/ipc";
import { openSiteInEditor, usePreferredEditor } from "@/lib/useEditor";
import { usePreferredBrowser } from "@/lib/useBrowser";
import { AppIcon } from "@/components/ui/app-icon";
import { SiteProvisionCard, useSiteProvision } from "@/components/sites/SiteProvisionCard";
import { useDownloads } from "@/lib/useDownloads";
import type { DbImportRecord, ScratchPackage, Site, SiteResources } from "@/types";

/** **THE scratch predicate — the recorded fact, and nothing else.**
 *
 * `origin === "agent"` is written at creation and is the only thing any policy
 * reads (#204). The `.scratch.<tld>` suffix is UX so a human can scan the list;
 * a site the USER hand-created at `foo.scratch.rex` is an ordinary site of
 * theirs and must appear in their own group. Matching on the name here would be
 * the obvious shortcut and would quietly put someone's real site in the
 * agent's disposable section. Pinned on both sides: `Site::is_scratch` in Rust
 * (`a_stored_origin_that_isnt_exactly_agent_reads_as_the_users_site`) and the L2
 * `scratch-rows` probe here, whose fixture includes a KEPT site and a
 * hand-named `mine.scratch.rex` — both `origin: "user"`, both ending in
 * `.scratch.rex`, so the suffix shortcut fails the probe by name.
 */
const isScratch = (s: Site) => s.origin === "agent";

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

function Badge({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "flex-none rounded-[6px] border border-rex-border-strong bg-rex-surface-1 px-[7px] py-[3px] font-mono text-[0.65625rem] text-rex-text-bright",
        className,
      )}
    >
      {children}
    </span>
  );
}

type Filter = "all" | "running" | "stopped";
type Sort = "name" | "status" | "recent";

const SORT_LABEL: Record<Sort, string> = {
  name: "Name",
  status: "Status",
  recent: "Recent",
};
const SORT_CYCLE: Record<Sort, Sort> = {
  name: "status",
  status: "recent",
  recent: "name",
};

/** All / Running / Stopped segmented control with live counts. */
function FilterTabs({
  value,
  onChange,
  counts,
}: {
  value: Filter;
  onChange: (f: Filter) => void;
  counts: Record<Filter, number>;
}) {
  const tabs: { key: Filter; label: string }[] = [
    { key: "all", label: "All" },
    { key: "running", label: "Running" },
    { key: "stopped", label: "Stopped" },
  ];
  return (
    <div className="inline-flex items-center gap-1 rounded-[10px] border border-rex-well-border bg-rex-well p-[3px]">
      {tabs.map((t) => (
        <button
          key={t.key}
          onClick={() => onChange(t.key)}
          className={cn(
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[0.78125rem] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          <span className="font-mono text-[0.65625rem] opacity-70">{counts[t.key]}</span>
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
      className="flex h-[34px] items-center gap-[7px] rounded-[9px] border border-rex-border bg-rex-surface-1 px-3 text-[0.8125rem] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2"
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
  onOpenDatabase,
  onOpenWordpress,
  onRename,
  onDuplicate,
  onRetry,
  onKeep,
  packages,
  reapFailure,
  extraDomains,
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
  onOpen: () => void;
  onDelete: () => void;
  onOpenDatabase: () => void;
  onOpenWordpress: () => void;
  onRename: () => void;
  onDuplicate: () => void;
  onRetry?: () => void;
  /** Adopt this scratch site (Keep). Present only for `origin === "agent"`. */
  onKeep?: () => void;
  /** The plugins/themes an agent cloned in — newest sync first. */
  packages?: ScratchPackage[];
  /** Why the reaper could not remove this expired site, from its own feed row. */
  reapFailure?: string;
}) {
  const t = siteTypeMeta(site.type);
  const scratch = isScratch(site);
  const ttl = scratch ? expiryLabel(site.expiresAt) : null;
  const pkg = scratch ? packages?.[0] : undefined;
  const [copied, setCopied] = useState(false);
  const editor = usePreferredEditor();
  const browser = usePreferredBrowser();
  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`Open ${site.name}`}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen();
        }
      }}
      className="group flex h-11 cursor-pointer items-center gap-[11px] rounded-[9px] pl-3 pr-2 transition-colors hover:bg-rex-surface-1 focus:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-white/15"
    >
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
              className="flex-none text-rex-text-muted opacity-70"
            >
              +{extraDomains.length}
            </span>
          )}
        </div>
      </div>
      {/* Quick actions reserve ~140px even while invisible (opacity). Below
          lg they disappear from LAYOUT too — every action still exists in
          the row menu — so badge-heavy rows fit narrow windows (§C1.2). */}
      <div
        className="ml-1 hidden items-center gap-px opacity-0 transition-opacity group-hover:opacity-100 lg:flex"
        onClick={(e) => e.stopPropagation()}
      >
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open in browser"
          title={browser ? `Open https://${site.domain} in ${browser.name}` : `Open https://${site.domain}`}
          onClick={() => void openExternal(`https://${site.domain}`).catch(toastBackendError)}
        >
          {/* Icon of the browser the click really uses — no chevron here: the
              row already reserves a fixed width for its quick actions, and a
              per-row menu next to a per-row menu is noise (§C1.2). Choosing a
              different browser for one link lives on the site page. */}
          <AppIcon icon={browser?.icon} fallback={<Globe className="h-4 w-4" />} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open folder"
          title="Reveal site folder"
          onClick={() => void openExternal(site.path).catch(toastBackendError)}
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Open database"
          title={
            site.type === "php" && !site.starterDb
              ? "This Blank PHP site has no database"
              : "Open database"
          }
          disabled={site.type === "php" && !site.starterDb}
          onClick={onOpenDatabase}
        >
          <Database className="h-4 w-4" />
        </Button>
        {site.type === "wordpress" && (
          <Button
            variant="ghost"
            size="icon"
            aria-label="Manage WordPress"
            title="Manage WordPress"
            onClick={onOpenWordpress}
          >
            <WordPressIcon className="h-4 w-4" />
          </Button>
        )}
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
      <span
        title={site.ssl ? "SSL · trusted" : "No SSL"}
        className="flex flex-none items-center"
        style={{ color: site.ssl ? "var(--rex-lock-secure)" : "var(--rex-lock-insecure)" }}
      >
        {site.ssl ? (
          <Lock className="h-3.5 w-3.5" strokeWidth={1.8} />
        ) : (
          <LockOpen className="h-3.5 w-3.5" strokeWidth={1.8} />
        )}
      </span>
      <Badge>{site.phpVersion}</Badge>
      <Badge className="w-[84px] text-center">{site.webServer}</Badge>
      {site.docrootManaged === false && (
        /* The folder is the user's own — served in place and never deleted with
           the site. True for a linked folder and for one moved out of the sites
           folder, which is why it reads "external" rather than "linked". */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[0.625rem] text-rex-text-muted"
          title={`Served from your own folder (${site.path}) — deleting the site leaves it in place`}
        >
          external
        </span>
      )}
      {dbState === "imported" && (
        /* Renders the SAME serialized fact as the SiteDetail summary
           (DbImportRecord, whose "connected" value only the rewrite job's
           verification can write) — the badge and the summary cannot
           disagree, because neither computes anything. */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="A copy of this site's database is on rexenv's engine, but the site still reads and writes the old one — they drift apart until you switch it over (see the site's Database tab)."
        >
          DB imported
        </span>
      )}
      {dbState === "connected" && (
        /* Wording matches what was PROVEN: the rewritten settings sign in to
           the rexenv copy — not "the site is now using this database". */
        <span
          className="flex-none whitespace-nowrap rounded-full border border-status-running-border bg-status-running-bg px-2 py-1 font-mono text-[0.625rem] text-status-running-bright"
          title="This site's connection settings were rewritten and verified: they sign in to the rexenv copy (see the site's Database tab)."
        >
          DB connected
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
        <StatusPill status={status} className="min-w-[92px]" />
      ) : (
        /* Honest half-site marker (v16): provisioning died or was cancelled —
           the site is NOT healthy-stopped. Retry re-runs the remaining
           idempotent steps; Delete (menu) removes it. */
        <span
          className="flex min-w-[92px] flex-none items-center justify-center gap-1 whitespace-nowrap rounded-full border border-status-warning-border bg-status-warning-bg px-2 py-1 font-mono text-[0.625rem] text-status-warning-bright"
          title="Provisioning did not finish — Retry re-runs the remaining steps; Delete removes the site."
        >
          setup incomplete
        </span>
      )}
      {!site.provisioned && (
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
      <div className="flex-none" onClick={(e) => e.stopPropagation()}>
        <Menu
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
        <MenuItem icon={<Pencil className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onRename}>
          Rename
        </MenuItem>
        <MenuItem icon={<Copy className="h-[15px] w-[15px]" strokeWidth={1.7} />} onSelect={onDuplicate}>
          Duplicate
        </MenuItem>
        <MenuItem
          icon={
            <AppIcon
              icon={editor?.icon}
              fallback={<Code className="h-[15px] w-[15px]" strokeWidth={1.7} />}
              className="h-[15px] w-[15px]"
            />
          }
          onSelect={() => openSiteInEditor(editor, site.path)}
        >
          {editor ? `Open in ${editor.name}` : "Open in editor"}
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
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("name");
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

  const remove = useMutation({
    mutationFn: (site: Site) => deleteSite(site.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });

  // D2's default leg for a CONNECTED site: revert the connection change,
  // then delete. If the revert can't run cleanly (file edited since, backup
  // missing), stop and say so — never delete on the back of a revert that
  // didn't happen.
  const revertThenRemove = useMutation({
    mutationFn: async (site: Site) => {
      const out = await rewriteRevert(site.id, false);
      if (out.status === "reverted" || out.status === "noRewrite") {
        await deleteSite(site.id);
        return null;
      }
      return out.message;
    },
    onSuccess: (blocked, site) => {
      if (blocked) {
        toast.info(
          `${site.domain} was not deleted — ${blocked} Resolve it on the site's Database tab, or delete without reverting.`,
        );
      }
      void qc.invalidateQueries({ queryKey: ["sites"] });
      void qc.invalidateQueries({ queryKey: ["db-import-records"] });
    },
    onError: (e) => toastBackendError(e),
  });

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

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renameSite(id, name),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });

  // In-app modals (WKWebView doesn't support window.confirm/prompt reliably).
  const [renameTarget, setRenameTarget] = useState<Site | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Site | null>(null);

  // "Duplicate" opens New Site prefilled with this site's setup (type/PHP/server);
  // the user picks a fresh domain. A byte-for-byte clone would need a backend copy.
  const [dupSource, setDupSource] = useState<Site | null>(null);

  const running = sites.filter((s) => statusOf(s) === "running").length;
  const counts: Record<Filter, number> = {
    all: sites.length,
    running,
    stopped: sites.length - running,
  };

  // Filter (segmented) → search (name/domain) → sort.
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = sites.filter((s) => {
      const st = statusOf(s);
      if (filter === "running" && st !== "running") return false;
      if (filter === "stopped" && st === "running") return false;
      if (q && !s.name.toLowerCase().includes(q) && !s.domain.toLowerCase().includes(q))
        return false;
      return true;
    });
    return [...list].sort((a, b) => {
      if (sort === "recent") return b.createdAt.localeCompare(a.createdAt);
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
  }, [sites, filter, query, sort, servingMap]);

  // The render split, on the RECORDED fact only (`isScratch`). Counts and the
  // filter stay over ALL sites: a scratch site is a real site, and grouping is
  // a way of showing them, not a second class of thing.
  const scratchVisible = useMemo(() => visible.filter(isScratch), [visible]);
  const ownVisible = useMemo(() => visible.filter((s) => !isScratch(s)), [visible]);

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
      <SortButton value={sort} onCycle={() => setSort((s) => SORT_CYCLE[s])} />
      {newSiteButton}
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
        <div className="flex flex-none items-center justify-between px-[22px] pb-[9px] pt-[14px]">
          <FilterTabs value={filter} onChange={setFilter} counts={counts} />
          <div className="flex items-center gap-[18px] pr-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
            <span className="w-[118px]">Stack</span>
            <span className="w-[88px]">Status</span>
          </div>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto px-[18px] pb-[18px]">
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
              Try a different name, domain, or clear the filter.
            </div>
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
                packages={packagesBySite.get(site.id)}
                reapFailure={reapFailures.get(site.id) || undefined}
                onOpen={() => navigate(`/sites/${site.id}`)}
                onDelete={() => setDeleteTarget(site)}
                onOpenDatabase={() => navigate(`/sites/${site.id}/database`)}
                onOpenWordpress={() => navigate(`/sites/${site.id}/wordpress`)}
                onRename={() => setRenameTarget(site)}
                onDuplicate={() => setDupSource(site)}
                onRetry={() => retry.mutate(site)}
                onKeep={isScratch(site) ? () => setKeepTarget(site) : undefined}
              />
            );
            return (
              <div className="flex flex-col">
                {ownVisible.map(row)}
                {scratchVisible.length > 0 && (
                  <>
                    {/* The group exists only when there IS one, so a user who
                        has never used an agent never sees an empty section
                        telling them the feature exists. */}
                    <ScratchGroupHeading
                      count={scratchVisible.length}
                      tight={ownVisible.length === 0}
                    />
                    {scratchVisible.map(row)}
                  </>
                )}
              </div>
            );
          })()
        )}
      </div>
      {(showNew || dupSource) && (
        <NewSiteDialog
          initial={
            dupSource
              ? {
                  name: `${dupSource.name} copy`,
                  siteType: dupSource.type,
                  phpVersion: dupSource.phpVersion,
                  webServer: dupSource.webServer,
                }
              : undefined
          }
          onClose={() => {
            setShowNew(false);
            setDupSource(null);
          }}
        />
      )}
      {renameTarget && (
        <PromptDialog
          title="Rename site"
          label="Display name"
          initialValue={renameTarget.name}
          submitLabel="Rename"
          onSubmit={(name) => {
            if (name !== renameTarget.name) rename.mutate({ id: renameTarget.id, name });
            setRenameTarget(null);
          }}
          onCancel={() => setRenameTarget(null)}
        />
      )}
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
      {deleteTarget && (
        <DeleteSiteDialog
          site={deleteTarget}
          dbState={dbStates.get(deleteTarget.id)}
          onPlainDelete={() => {
            remove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onRevertThenDelete={() => {
            revertThenRemove.mutate(deleteTarget);
            setDeleteTarget(null);
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </>
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
      <span className="font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
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
 * "Import from Valet or Herd" nudge.
 *
 * Only appears when a scan would ACTUALLY find something, so a user with
 * neither tool never sees it, and it is dismissible so it can't nag someone who
 * has already decided. Most useful on an empty site list — which is exactly
 * when a Valet user is wondering where their sites are.
 */
/** TLDs another tool took back — the user's sites on them are dark while
 *  every health check stays green, so the fact comes to them instead of
 *  waiting in a log or on the Import screen (ruled 15 Aug 2026; copy approved
 *  with one redline). Two load-bearing behaviours:
 *  - `[]` renders NOTHING. Nothing-taken-back is the ordinary state, and a
 *    notice for it would be the import bug's shape in a new place (#306).
 *  - Dismissal is PER-TLD and CLEARS when that TLD reads as ours again: the
 *    stored set self-heals against the live answer, so a dismissed .test
 *    returns on the NEXT loss and a newly lost .dev is never hidden by an
 *    old dismissal — nobody tracks that by hand. */
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
  const drift = useMemo(() => (Array.isArray(data) ? data.filter((t) => typeof t === "string") : []), [data]);
  // Self-heal: a dismissal only means anything about a CURRENTLY drifted TLD.
  // Pruning here is what makes "dismissed .test re-shows on the next loss"
  // true without any second record of when a takeover was redone.
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
  const visible = drift.filter((t) => !dismissed.includes(t));
  if (visible.length === 0) return null;

  const names = visible.map((t) => `.${t}`);
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
            ? `Valet or Herd took ${list}'s resolver file back, so those sites won't load until rexenv takes it over again. You can take it back from Import.`
            : `Valet or Herd took the resolver files for ${list} back, so those sites won't load until rexenv takes them over again. You can take them back from Import.`}
        </div>
      </div>
      <Button variant="secondary" onClick={() => navigate("/import")}>
        Go to Import
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          const next = [...new Set([...dismissed, ...visible])];
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
          {ready} site{ready === 1 ? "" : "s"} found in Valet or Herd
        </div>
        <div className="text-[0.71875rem] text-rex-text-muted">
          Import them where they already live — nothing is copied, and your Valet setup is left
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

