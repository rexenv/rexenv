import { useEffect, useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Code,
  Copy,
  Database,
  ExternalLink,
  FolderOpen,
  Globe,
  LayoutGrid,
  Loader2,
  Lock,
  LockOpen,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { Placeholder } from "@/components/common/Placeholder";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { openSiteInEditor, usePreferredEditor } from "@/lib/useEditor";
import { usePreferredBrowser } from "@/lib/useBrowser";
import { AppIcon } from "@/components/ui/app-icon";
import { Menu } from "@/components/ui/menu";
import { SplitButton } from "@/components/ui/split-button";
import { BROWSER_MENU_WIDTH, useBrowserMenu, useEditorMenu } from "@/components/ui/open-in";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { SiteTerminal } from "@/components/terminal/SiteTerminal";
import { DatabaseTab } from "@/components/sites/DatabaseTab";
import { SiteLogs, logLineColor } from "@/components/sites/SiteLogs";
import { WordPressManager } from "@/components/wordpress/WordPressManager";
import { SiteAgentActivity } from "@/components/mcp/SiteAgentActivity";
import { SiteRepoTab } from "@/components/sites/SiteRepoTab";
import { siteTypeMeta } from "@/lib/siteType";
import { cn, TECH_INPUT } from "@/lib/utils";
import { eolNote, eolTag } from "@/lib/php";
import {
  changeSiteDomain,
  getSitesServing,
  frankenphpEmbeddedPhp,
  listPhpVersions,
  listSiteEnv,
  listSites,
  logTargets,
  moveSiteDocroot,
  openExternal,
  pickFolder,
  regenerateSiteCert,
  relinkSiteDocroot,
  renameSite,
  repoSiteInfo,
  setSiteEnv,
  revealPath,
  setSitePhpVersion,
  setSiteWebServer,
  setSiteXdebug,
  siteCertInfo,
  tailLog,
  tldPolicy,
  wpAdminLoginUrl,
  wpInfo,
} from "@/lib/ipc";
import { toast } from "@/lib/toast";
import type { DomainChange, EnvVar, Site, WebServer } from "@/types";

/** One-click "Open admin": open a magic auto-login link for the site's primary
 *  administrator (lands on /wp-admin/). If the link can't be issued — services
 *  stopped, no admin user, tools missing — say so and fall back to the plain
 *  WordPress login page. */
async function openWpAdmin(site: Pick<Site, "id" | "domain">) {
  await openExternal(await magicLoginUrl(site));
}

/** The url "Magic Login" should open, minted per click (the token is one-time,
 *  so it can't be computed ahead and parked in a menu). Shared with the chevron
 *  beside the button so BOTH paths get the same fallback — the failure the
 *  fallback covers (services stopped, no admin user, tools missing) doesn't
 *  care which browser the user picked. */
async function magicLoginUrl(site: Pick<Site, "id" | "domain">): Promise<string> {
  try {
    return await wpAdminLoginUrl(site.id);
  } catch (e) {
    toast.error(`Auto-login unavailable — opening the WordPress login page instead.\n${String(e)}`);
    return `https://${site.domain}/wp-admin/`;
  }
}

/** Web servers with real backends (OpenLiteSpeed is still deferred). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
  { value: "apache", label: "Apache (.htaccess)" },
];

const SELECT_CLS =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50";

type TabKey = "overview" | "wordpress" | "repository" | "database" | "logs" | "terminal" | "settings";

export function SiteDetail() {
  const { id, tab } = useParams<{ id: string; tab?: TabKey }>();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const { data: versions = [] } = useQuery({ queryKey: ["php-versions"], queryFn: listPhpVersions });
  const site = sites.find((s) => s.id === id);

  // WordPress detection drives whether the WordPress tab shows + the WP-admin link.
  // It runs three WP-CLI calls (each boots WordPress) — cache it and skip
  // window-focus refetches; the answer only changes on install/convert.
  const { data: wp, isFetched: wpResolved } = useQuery({
    queryKey: ["wp-info", id],
    queryFn: () => wpInfo(id!),
    enabled: !!id,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    retry: 1,
  });

  // Displayed status is this site's live *serving* state, not sites.status — serving
  // only when the edge is up AND its own upstream (php-fpm pool or FrankenPHP backend)
  // is up (task 2.1 / H1 + follow-up), so a partial stack shows honest per-site status.
  const { data: serving } = useQuery({
    queryKey: ["sites-serving"],
    queryFn: getSitesServing,
    refetchInterval: 2000,
  });

  const switchPhp = useMutation({
    mutationFn: (version: string) => setSitePhpVersion(id!, version),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });
  const switchServer = useMutation({
    mutationFn: (server: WebServer) => setSiteWebServer(id!, server),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => toastBackendError(e),
  });

  // Pure filesystem on the backend (no git spawn), so it is cheap enough to ask
  // for every site and honest enough to re-ask on focus: a `git init` or an
  // `rm -rf .git` in a terminal shows up the moment the user comes back.
  //
  // ABOVE the `!site` return, and that placement is the fix rather than a style
  // choice. It used to sit below, so a COLD render of this route ran fewer hooks
  // than the render after `sites` resolved, and React threw "Rendered more hooks
  // than during the previous render" — a blank screen instead of a site page.
  // Navigating from the Sites list hides it (the query is already cached and
  // `site` is found on the first render); a reload or a deep link straight to
  // /sites/:id does not. Found 21 Aug 2026 by the wk-check written for the
  // FrankenPHP picker, which loads this route cold — which is the whole reason
  // an L2 check earns its keep.
  const { data: repoInfo } = useQuery({
    queryKey: ["repo-site-info", site?.id],
    queryFn: () => repoSiteInfo(site!.id),
    enabled: !!site,
    staleTime: 0,
    refetchOnWindowFocus: true,
  });

  if (!site) {
    return (
      <>
        <TopBar title="Site" showSearch={false} />
        <Placeholder
          icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
          label="Site not found"
          hint="It may have been deleted."
        />
      </>
    );
  }

  // `wp-info` boots WP-CLI three times, so on a first visit its answer lands a
  // second or two in — long enough that the WordPress tab and Magic Login
  // POPPED IN after the page had settled. Until it resolves, fall back to the
  // site's OWN recorded type (set at create/import, already in hand from the
  // sites list) so the chrome is right immediately; the live answer corrects it
  // the moment it arrives — including the "recorded as WordPress but the
  // install never finished" case, where the tab goes away again.
  const isWordpress = wpResolved ? !!wp?.isWordpress : site.type === "wordpress";
  const isServing = !!serving?.find((s) => s.domain === site.domain)?.serving;
  const active: TabKey = tab ?? "overview";
  const tabs: { key: TabKey; label: string; show: boolean }[] = [
    { key: "overview", label: "Overview", show: true },
    { key: "wordpress", label: "WordPress", show: isWordpress },
    // Shown only when the site's OWN folder is a checkout — a cloned site
    // always, a linked one that happens to be a repository too. Gated on the
    // filesystem rather than on `gitUrl` so an adopted checkout is not
    // second-class, and never on a parent walk (see `repo_site_info`).
    { key: "repository", label: "Repository", show: !!repoInfo?.present },
    { key: "database", label: "Database", show: true },
    { key: "logs", label: "Logs", show: true },
    { key: "terminal", label: "Terminal", show: true },
    { key: "settings", label: "Settings", show: true },
  ];

  // Installed versions, plus the site's current one (so the select always shows
  // it). Carries each row's EOL date rather than just the minor: an option that
  // named a dead runtime with the same face as a live one is what this whole
  // tell exists to stop, and the current version may itself be the dead one.
  const options = versions
    .filter((v) => v.installed)
    .map((v) => ({ minor: v.minor, eolSince: v.eolSince }));
  if (!options.some((o) => o.minor === site.phpVersion)) {
    const row = versions.find((v) => v.minor === site.phpVersion);
    options.unshift({ minor: site.phpVersion, eolSince: row?.eolSince ?? null });
  }

  return (
    <>
      <SiteHeader
        site={site}
        isWordpress={isWordpress}
        status={isServing ? "running" : "stopped"}
        onBack={() => navigate("/sites")}
      />

      <div className="flex-none border-b border-rex-border-subtle px-[22px]">
        <div className="flex gap-0.5">
          {tabs
            .filter((t) => t.show)
            .map((t) => {
              const isActive = active === t.key;
              const wpInactive = t.key === "wordpress" && !isActive;
              return (
                <button
                  key={t.key}
                  onClick={() => navigate(`/sites/${site.id}/${t.key}`)}
                  className={cn(
                    "-mb-px border-b-2 px-3.5 py-2.5 text-[0.84375rem] font-medium transition-colors",
                    isActive
                      ? "border-brand text-rex-text"
                      : wpInactive
                        ? "border-transparent text-rex-accent-blue hover:text-rex-text"
                        : "border-transparent text-rex-text-muted hover:text-rex-text",
                  )}
                >
                  {t.label}
                </button>
              );
            })}
        </div>
      </div>

      {/* Database scrolls (its cards grow — imports, the rewrite consent);
          the frame flexes to the REMAINING height inside a min-h-full column,
          so with little above it it fills the region, and with a lot above it
          the region scrolls and the frame keeps its floor (UI-REVIEW §C2:
          overflow-hidden + a broken height chain clipped Adminer entirely).
          Terminal keeps the fixed full-height/no-scroll shape. */}
      <div
        className={`min-h-0 flex-1 px-[22px] pb-[22px] pt-[18px] ${
          active === "terminal" ? "overflow-hidden" : "overflow-auto"
        }`}
      >
        <div
          className={`flex flex-col gap-[14px] ${
            active === "terminal" ? "h-full" : active === "database" ? "min-h-full" : ""
          }`}
        >
          {active === "repository" && <SiteRepoTab site={site} />}
          {active === "overview" && (
            <Overview
              site={site}
              isWordpress={isWordpress}
              options={options}
              switchPhpPending={switchPhp.isPending}
              switchServerPending={switchServer.isPending}
              onPhp={async (v) => {
                // A select mis-click shouldn't silently restart the site —
                // confirm, naming exactly what changes (P2-6).
                if (v === site.phpVersion) return;
                const ok = await confirm({
                  title: `Switch ${site.domain} to PHP ${v}?`,
                  message: `Currently on PHP ${site.phpVersion}. The site restarts briefly while its config reloads.`,
                  confirmLabel: "Switch",
                });
                if (ok) switchPhp.mutate(v);
              }}
              onServer={async (srv) => {
                if (srv === site.webServer) return;
                const label = (x: string) =>
                  SERVERS.find((o) => o.value === x)?.label ?? x;
                const ok = await confirm({
                  title: `Switch ${site.domain} from ${label(site.webServer)} to ${label(srv)}?`,
                  message: "The site restarts briefly while it moves to the other server.",
                  confirmLabel: "Switch",
                });
                if (ok) switchServer.mutate(srv);
              }}
              // THIS site's database, like every other tile here — the engines
              // screen is a different question and lives in the sidebar.
              onDatabase={() => navigate(`/sites/${site.id}/database`)}
              onTerminal={() => navigate(`/sites/${site.id}/terminal`)}
              onViewLogs={() => navigate(`/sites/${site.id}/logs`)}
            />
          )}

          {active === "wordpress" && (
            <WordPressManager siteId={site.id} multisite={site.multisite} domain={site.domain} />
          )}
          {active === "database" && <DatabaseTab site={site} />}
          {active === "logs" && (
            <SiteLogs site={site} isWordpress={isWordpress} wpResolved={wpResolved} />
          )}
          {active === "terminal" && <SiteTerminal siteId={site.id} />}
          {active === "settings" && <SettingsTab site={site} />}
        </div>
      </div>
    </>
  );
}

function SiteHeader({
  site,
  isWordpress,
  status,
  onBack,
}: {
  site: Site;
  isWordpress: boolean;
  status: Site["status"];
  onBack: () => void;
}) {
  const t = siteTypeMeta(site.type);
  const url = `https://${site.domain}`;
  // The button wears the icon of the browser the click will ACTUALLY use —
  // preference, else the OS default (both resolved in `usePreferredBrowser`).
  const browser = usePreferredBrowser();
  const browserMenu = useBrowserMenu(url);
  const adminMenu = useBrowserMenu(() => magicLoginUrl(site));
  const [adminBusy, setAdminBusy] = useState(false);
  const onOpenAdmin = async () => {
    setAdminBusy(true);
    try {
      await openWpAdmin(site);
    } finally {
      setAdminBusy(false);
    }
  };
  return (
    <div
      onMouseDown={onTitleBarMouseDown}
      className="drag-region flex-none border-b border-rex-border-subtle px-[22px] pt-[18px]"
    >
      <button
        onClick={onBack}
        className="mb-[13px] inline-flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted transition-colors hover:text-rex-text-bright"
      >
        <ChevronLeft className="h-[13px] w-[13px]" strokeWidth={2} />
        All sites
      </button>
      <div className="flex items-start justify-between gap-[18px] pb-[18px]">
        <div className="flex min-w-0 items-center gap-[13px]">
          <div
            className="flex h-[38px] w-[38px] flex-none items-center justify-center rounded-[10px] border text-[0.9375rem] font-bold"
            style={{ background: t.bg, color: t.color, borderColor: t.border }}
          >
            {t.letter}
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-[11px]">
              <span className="text-[1.1875rem] font-semibold tracking-[-.01em] text-rex-text">
                {site.name}
              </span>
              <StatusPill status={status} />
            </div>
            <div className="mt-1 flex items-center gap-2 font-mono text-[0.78125rem] text-rex-text-muted">
              <span className="truncate">{site.domain}</span>
              <span className="flex-none text-[0.6875rem] text-rex-text-muted">· :443</span>
            </div>
          </div>
        </div>
        <div className="flex flex-none items-center gap-[9px]">
          <SplitButton
            onClick={() => void openExternal(url).catch(toastBackendError)}
            menu={browserMenu}
            menuWidth={BROWSER_MENU_WIDTH}
            chevronLabel="Open this site in another browser"
          >
            <AppIcon
              icon={browser?.icon}
              fallback={<ExternalLink className="h-[15px] w-[15px]" strokeWidth={1.8} />}
              className="h-[15px] w-[15px]"
            />
            Open in browser
          </SplitButton>
          {isWordpress && (
            <SplitButton
              variant="primary"
              disabled={adminBusy}
              onClick={onOpenAdmin}
              menu={adminMenu}
              menuWidth={BROWSER_MENU_WIDTH}
              chevronLabel="Sign in through another browser"
            >
              <WordPressIcon className="h-[15px] w-[15px]" />
              {adminBusy ? "Signing in…" : "Magic Login"}
            </SplitButton>
          )}
        </div>
      </div>
    </div>
  );
}

function Overview({
  site,
  isWordpress,
  options,
  switchPhpPending,
  switchServerPending,
  onPhp,
  onServer,
  onDatabase,
  onTerminal,
  onViewLogs,
}: {
  site: Site;
  isWordpress: boolean;
  options: { minor: string; eolSince: string | null }[];
  switchPhpPending: boolean;
  switchServerPending: boolean;
  onPhp: (v: string) => void;
  onServer: (s: WebServer) => void;
  onDatabase: () => void;
  onTerminal: () => void;
  onViewLogs: () => void;
}) {
  const url = `https://${site.domain}`;
  const wpConfig = `${site.path}/wp-config.php`;
  // FrankenPHP serves every site with its EMBEDDED PHP, never the site's pool
  // — so for a FrankenPHP site the picker is read-only and the number shown is
  // the version that actually serves, not the stored one (ruled 15 Aug 2026;
  // the stored `phpVersion` would be a promise the server cannot keep).
  const onFrankenphp = site.webServer === "frankenphp";
  const { data: embeddedPhp } = useQuery({
    queryKey: ["frankenphp-embedded-php"],
    queryFn: frankenphpEmbeddedPhp,
    enabled: onFrankenphp,
    staleTime: Infinity,
  });
  const editor = usePreferredEditor();
  const editorMenu = useEditorMenu(site.path);
  const browser = usePreferredBrowser();
  const browserMenu = useBrowserMenu(url);
  const adminMenu = useBrowserMenu(() => magicLoginUrl(site));
  const serverLabel = SERVERS.find((s) => s.value === site.webServer)?.label ?? site.webServer;

  // The site's OWN version's EOL date, when core says it has one. Read from the
  // same option rows the select renders, so the badge and the list can never
  // disagree about which runtimes are dead.
  const sitesEol = options.find((o) => o.minor === site.phpVersion)?.eolSince ?? null;
  return (
    <>
      <div className="rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
        <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
          Environment
        </div>
        {/* A site already running on a dead runtime is told here, not only at
            create — most sites on one got there by import or by outliving the
            version, never by picking it in a dialog. */}
        {sitesEol && (
          <div className="mb-[14px] rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] leading-[1.5] text-status-warning-bright">
            {eolNote(site.phpVersion, sitesEol, { wordpress: site.type === "wordpress" })}
          </div>
        )}
        <div className="grid grid-cols-3 gap-[14px]">
          <EnvMini label="PHP version">
            <span className="font-mono text-[1.125rem] font-semibold text-rex-text">
              {onFrankenphp ? (embeddedPhp ?? site.phpVersion) : site.phpVersion}
            </span>
            {onFrankenphp ? (
              <>
                <select
                  value={embeddedPhp ?? site.phpVersion}
                  disabled
                  title="FrankenPHP embeds its own PHP build; sites it serves never use the per-version pools."
                  className={SELECT_CLS}
                >
                  <option value={embeddedPhp ?? site.phpVersion}>
                    {embeddedPhp ?? site.phpVersion} — FrankenPHP's embedded PHP
                  </option>
                </select>
                <div className="mt-1 text-[0.6875rem] leading-[1.5] text-rex-text-muted">
                  Fixed by FrankenPHP. Switch the web server to Nginx or Apache
                  to choose a version.
                </div>
              </>
            ) : (
              <select
                value={site.phpVersion}
                disabled={switchPhpPending}
                onChange={(e) => onPhp(e.target.value)}
                className={SELECT_CLS}
              >
                {options.map((o) => (
                  <option key={o.minor} value={o.minor}>
                    {o.minor}
                    {eolTag(o.eolSince)}
                  </option>
                ))}
              </select>
            )}
          </EnvMini>
          <EnvMini label="Web server">
            <span className="text-[0.9375rem] font-semibold capitalize text-rex-text">
              {serverLabel}
            </span>
            <select
              value={site.webServer}
              disabled={switchServerPending}
              onChange={(e) => onServer(e.target.value as WebServer)}
              className={SELECT_CLS}
            >
              {!SERVERS.some((s) => s.value === site.webServer) && (
                <option value={site.webServer}>{site.webServer}</option>
              )}
              {SERVERS.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </EnvMini>
          <EnvMini label="SSL certificate">
            {site.ssl ? (
              <>
                <div className="flex items-center gap-2">
                  <Lock className="h-[17px] w-[17px] text-status-running" strokeWidth={1.8} />
                  <span className="text-[0.84375rem] font-semibold text-rex-text">Trusted</span>
                </div>
                <span className="font-mono text-[0.65625rem] text-rex-text-muted">rexenv&nbsp;CA</span>
              </>
            ) : (
              <div className="flex items-center gap-2">
                <LockOpen
                  className="h-[17px] w-[17px]"
                  style={{ color: "var(--rex-lock-insecure)" }}
                  strokeWidth={1.8}
                />
                <span className="text-[0.84375rem] font-semibold text-rex-text">Not secured</span>
              </div>
            )}
          </EnvMini>
        </div>
      </div>

      <div className="grid grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)] gap-[14px]">
        <div className="min-w-0 rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
          <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
            Paths
          </div>
          <div className="flex flex-col gap-3">
            <PathField label="Project path" value={site.path} openable />
            {isWordpress ? (
              <PathField label="Config path" value={wpConfig} />
            ) : (
              <PathField label="URL" value={url} />
            )}
          </div>
        </div>

        <div className="min-w-0 rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
          <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
            Quick links
          </div>
          <div className="grid grid-cols-2 gap-[9px]">
            <QuickTile
              icon={
                <AppIcon icon={browser?.icon} fallback={<Globe className="h-4 w-4" />} />
              }
              iconColor="text-rex-text-muted"
              label={browser ? `Open in ${browser.name}` : "Browser"}
              onClick={() => void openExternal(url).catch(toastBackendError)}
              menu={browserMenu}
              menuWidth={BROWSER_MENU_WIDTH}
              menuLabel="Open this site in another browser"
            />
            {isWordpress && (
              <QuickTile
                icon={<WordPressIcon className="h-4 w-4" />}
                iconColor="text-rex-accent-blue"
                label="Magic Login"
                onClick={() => void openWpAdmin(site)}
                menu={adminMenu}
                menuWidth={BROWSER_MENU_WIDTH}
                menuLabel="Sign in through another browser"
              />
            )}
            <QuickTile
              icon={<AppIcon icon={editor?.icon} fallback={<Code className="h-4 w-4" />} />}
              iconColor="text-rex-text-muted"
              label={editor ? `Open in ${editor.name}` : "Open in editor"}
              onClick={() => openSiteInEditor(editor, site.path)}
              menu={editorMenu}
              menuLabel="Open this project in another editor"
            />
            <QuickTile
              icon={<Database className="h-4 w-4" />}
              iconColor="text-rex-text-muted"
              label="Database"
              onClick={onDatabase}
            />
            <QuickTile
              icon={<TerminalSquare className="h-4 w-4" />}
              iconColor="text-brand-tint"
              label="Terminal"
              onClick={onTerminal}
            />
            <QuickTile
              icon={<FolderOpen className="h-4 w-4" />}
              iconColor="text-rex-text-muted"
              label="Open project folder"
              onClick={() => void openExternal(site.path).catch(toastBackendError)}
              // Keeps the 2-col grid full: WordPress adds Magic Login (odd tile
              // count), so the folder tile fills the trailing slot instead of
              // spanning and leaving a hole beside Terminal.
              span2={!isWordpress}
            />
          </div>
        </div>
      </div>

      <RecentLogs siteId={site.id} onViewAll={onViewLogs} />
      <SiteAgentActivity siteId={site.id} />
    </>
  );
}

const TYPE_LABELS: Record<string, string> = {
  wordpress: "WordPress",
  laravel: "Laravel",
  php: "Blank PHP",
};

const MULTISITE_LABELS: Record<string, string> = {
  none: "—",
  subdomain: "Subdomain network",
  subdirectory: "Subdirectory network",
};

/** RFC 3339 → local "Jul 10, 2026". */
function fmtCertDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function SettingsCard({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
      <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
        {label}
      </div>
      {children}
    </div>
  );
}

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <div className="flex-none text-[0.75rem] text-rex-text-muted">{label}</div>
      <div className="min-w-0 text-right text-[0.8125rem] text-rex-text-bright">{children}</div>
    </div>
  );
}

function SettingsTab({ site }: { site: Site }) {
  const qc = useQueryClient();
  const [name, setName] = useState(site.name);
  const [domainOpen, setDomainOpen] = useState(false);
  // Track renames that land from elsewhere (e.g. the Sites-list dialog).
  useEffect(() => setName(site.name), [site.name]);

  const rename = useMutation({
    mutationFn: (n: string) => renameSite(site.id, n),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites"] });
      toast.success("Site renamed");
    },
    onError: (e) => toastBackendError(e),
  });

  const moveSite = useMutation({
    mutationFn: (destParent: string) => moveSiteDocroot(site.id, destParent),
    onSuccess: (s) => {
      qc.invalidateQueries();
      toast.success(`Site folder moved to ${s?.path ?? "the new location"}`);
    },
    onError: (e) => {
      // A late failure (config reload) can land after the files moved — refresh
      // so the path shown is what's really on disk.
      qc.invalidateQueries();
      toastBackendError(e);
    },
  });
  const askMove = async () => {
    const parent = await pickFolder("Choose the new parent folder", site.path);
    if (!parent) return;
    const folder = site.path.replace(/\/+$/, "").split("/").pop() ?? site.domain;
    const ok = await confirm({
      title: "Move site folder?",
      message: (
        <>
          Moves <span className="font-mono">{site.path}</span> to{" "}
          <span className="font-mono">{`${parent}/${folder}`}</span> and updates the server config
          (the site may blip for a moment). Files are verified at the destination before anything
          old is removed. Two caveats: plugins that stored absolute paths in the database won't
          follow the move, and moving the folder outside your rexenv sites folder gives up
          rexenv's claim on it — from then on deleting the site leaves the folder in place.
        </>
      ),
      confirmLabel: "Move folder",
    });
    if (ok) moveSite.mutate(parent);
  };

  const relinkSite = useMutation({
    mutationFn: (path: string) => relinkSiteDocroot(site.id, path),
    onSuccess: (s) => {
      qc.invalidateQueries();
      toast.success(`Site now served from ${s?.path ?? "the new location"}`);
    },
    onError: (e) => toastBackendError(e),
  });
  const askRelink = async () => {
    const picked = await pickFolder("Choose the folder that now holds the site", site.path);
    if (!picked) return;
    const ok = await confirm({
      title: "Point the site at this folder?",
      message: (
        <>
          rexenv will serve <span className="font-mono">{site.domain}</span> from{" "}
          <span className="font-mono">{picked}</span> instead of{" "}
          <span className="font-mono">{site.path}</span>. No file is copied, moved or deleted —
          only the recorded location changes, so move the files yourself first. Pick the folder
          that holds the site's files (the one with index.php / wp-config.php), not its parent.
        </>
      ),
      confirmLabel: "Point here",
    });
    if (ok) relinkSite.mutate(picked);
  };

  const { data: cert, isLoading: certLoading } = useQuery({
    queryKey: ["site-cert", site.id],
    queryFn: () => siteCertInfo(site.id),
  });

  const regenCert = useMutation({
    mutationFn: () => regenerateSiteCert(site.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["site-cert", site.id] });
      toast.success("Certificate re-issued — the edge is serving it now.");
    },
    onError: (e) => {
      // A failed edge reload still leaves the new cert on disk — refresh the
      // card so the dates reflect what will be served after a retry/restart.
      void qc.invalidateQueries({ queryKey: ["site-cert", site.id] });
      toastBackendError(e);
    },
  });
  const askRegenCert = async () => {
    const ok = await confirm({
      title: cert ? "Regenerate HTTPS certificate?" : "Issue HTTPS certificate?",
      message: (
        <>
          Re-issues the certificate for <span className="font-mono">{site.domain}</span> and{" "}
          <span className="font-mono">*.{site.domain}</span> from the local CA (valid ~397 days),
          then briefly reloads the edge proxy — open connections to your sites may drop for a
          moment. No site data is affected.
        </>
      ),
      confirmLabel: cert ? "Regenerate" : "Issue certificate",
    });
    if (ok) regenCert.mutate();
  };

  const trimmed = name.trim();
  const dirty = trimmed.length > 0 && trimmed !== site.name;
  const hasDb = site.type !== "php";

  const days = cert?.daysLeft ?? 0;
  const expiryTone =
    days < 0
      ? "text-status-error-bright"
      : days < 30
        ? "text-status-warning-bright"
        : "text-rex-text-muted";
  const expiryNote = days < 0 ? `expired ${-days} days ago` : `in ${days} days`;

  return (
    <>
      {/* Two independent packed columns (items-start) — six stacked full-width
          cards each used half their width (QA P3-2). Identity & location left;
          read-only info + certificate right; env vars full-width below. */}
      <div className="grid grid-cols-2 items-start gap-[14px]">
        <div className="flex flex-col gap-[14px]">
          <SettingsCard label="Site name">
        <div className="flex items-center gap-2">
          <input
            {...TECH_INPUT}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && dirty && !rename.isPending) rename.mutate(trimmed);
            }}
            className="h-9 w-full max-w-[420px] rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[0.8125rem] text-rex-text outline-none transition-colors focus:border-brand"
          />
          <Button
            variant="primary"
            disabled={!dirty || rename.isPending}
            onClick={() => rename.mutate(trimmed)}
          >
            {rename.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
        <div className="mt-2 text-[0.75rem] text-rex-text-muted">
          Display name only — the domain, folder and database are unchanged.
        </div>
      </SettingsCard>
          <SettingsCard label="Domain">
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="font-mono text-[0.8125rem] text-rex-text-bright">{site.domain}</div>
            <div className="mt-1 text-[0.75rem] text-rex-text-muted">
              {site.multisite !== "none"
                ? "Domain change isn't supported on a multisite network yet — the network stores the domain in wp-config and per-site tables."
                : site.type === "wordpress"
                  ? "Changing the domain rewrites every URL in the database. A backup is exported to Downloads first."
                  : "Changing the domain re-issues the HTTPS certificate and updates the server config."}
            </div>
          </div>
          <Button
            variant="secondary"
            disabled={site.multisite !== "none"}
            title={site.multisite !== "none" ? "Not supported on a multisite network yet" : undefined}
            onClick={() => setDomainOpen(true)}
          >
            Change domain…
          </Button>
        </div>
      </SettingsCard>
          <SettingsCard label="Site folder">
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="truncate font-mono text-[0.78125rem] text-rex-text-bright">{site.path}</div>
            <div className="mt-1 text-[0.75rem] text-rex-text-muted">
              {site.docrootManaged === false
                ? "Your own folder — rexenv serves it in place and never moves or deletes it. Moved it yourself? Point rexenv at the new location."
                : "Move the site's files to another folder — domain, database and certificate stay the same."}
            </div>
          </div>
          {site.docrootManaged === false ? (
            <Button
              variant="secondary"
              disabled={relinkSite.isPending}
              onClick={() => void askRelink()}
            >
              {relinkSite.isPending ? "Pointing…" : "Point at new folder…"}
            </Button>
          ) : (
            <Button
              variant="secondary"
              disabled={moveSite.isPending}
              onClick={() => void askMove()}
            >
              {moveSite.isPending ? "Moving…" : "Move…"}
            </Button>
          )}
        </div>
      </SettingsCard>
        </div>
        <div className="flex flex-col gap-[14px]">
          <SettingsCard label="Site info">
        <div className="flex max-w-[560px] flex-col gap-3">
          <InfoRow label="Type">{TYPE_LABELS[site.type] ?? site.type}</InfoRow>
          <InfoRow label="Database name">
            {hasDb ? (
              <span className="inline-flex items-center gap-1.5">
                <span className="truncate font-mono text-[0.78125rem]">{site.dbName}</span>
                <CopyButton value={site.dbName} />
              </span>
            ) : (
              <span className="text-rex-text-muted">— (no database)</span>
            )}
          </InfoRow>
          {hasDb && (
            <InfoRow label="Database engine">
              <span className="font-mono text-[0.78125rem]">
                {site.dbEngine === "mariadb" ? "MariaDB" : "MySQL"}
                <span className="ml-2 text-rex-text-muted">
                  127.0.0.1:{site.dbEngine === "mariadb" ? 13307 : 13306}
                </span>
              </span>
            </InfoRow>
          )}
          <InfoRow label="Multisite">{MULTISITE_LABELS[site.multisite] ?? site.multisite}</InfoRow>
        </div>
      </SettingsCard>
          <SettingsCard label="HTTPS certificate">
        {certLoading ? (
          <div className="text-[0.78125rem] text-rex-text-muted">Reading certificate…</div>
        ) : !cert ? (
          <div className="flex items-center justify-between gap-4">
            <div className="text-[0.78125rem] text-rex-text-muted">
              No certificate on disk — issue one so HTTPS works (normally created when the site
              is provisioned).
            </div>
            <Button variant="secondary" disabled={regenCert.isPending} onClick={() => void askRegenCert()}>
              {regenCert.isPending ? "Issuing…" : "Issue certificate"}
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex max-w-[560px] flex-col gap-3">
              <InfoRow label="Issued">{fmtCertDate(cert.notBefore)}</InfoRow>
              <InfoRow label="Expires">
                <span>{fmtCertDate(cert.notAfter)}</span>
                <span className={cn("ml-2 font-mono text-[0.71875rem]", expiryTone)}>{expiryNote}</span>
              </InfoRow>
              <InfoRow label="Domains">
                <span className="font-mono text-[0.78125rem]">{cert.sans.join(", ")}</span>
              </InfoRow>
            </div>
            <PathField
              label="Certificate folder"
              value={cert.certDir}
              onOpen={() => void revealPath(cert.certDir)}
            />
            <div className="mt-1 flex items-center justify-between gap-4 border-t border-rex-border-subtle pt-3">
              <div className="text-[0.75rem] text-rex-text-muted">
                Re-issue from the local CA — for a cert nearing expiry, a corrupted file, or after
                the CA was re-created. Briefly reloads the edge.
              </div>
              <Button
                variant="secondary"
                disabled={regenCert.isPending}
                onClick={() => void askRegenCert()}
              >
                {regenCert.isPending ? "Regenerating…" : "Regenerate"}
              </Button>
            </div>
          </div>
        )}
      </SettingsCard>
          <XdebugCard site={site} />
        </div>
      </div>
      <EnvVarsCard siteId={site.id} />
      {domainOpen && <ChangeDomainDialog site={site} onClose={() => setDomainOpen(false)} />}
    </>
  );
}

/** Per-site Xdebug toggle (§8.2). On = this site's PHP runs in the version's
 *  DEBUG pool (Xdebug loaded, mode debug,develop); other sites on the same
 *  version are unaffected. Client-side disable mirrors the CORE rules — the
 *  backend is the enforcement.
 *
 *  Which minors support Xdebug comes from the registry row (`xdebugSupported`,
 *  derived in core from the pinned bottle table), NOT from a literal here. It
 *  was `minor === "8.0"`: a second copy of `binaries::xdebug_supported` free to
 *  disagree with it the moment the pinned set changed.
 *
 *  So does the REASON (`xdebugUnavailableReason`, 23 Aug 2026). The same lesson
 *  had been learned for the boolean and not for the sentence beside it: a single
 *  hardcoded "its build can't load extensions" covered both absences that
 *  existed and would have been wrong about the first unpinned minor to ship. */
function XdebugCard({ site }: { site: Site }) {
  const qc = useQueryClient();
  const { data: versions = [] } = useQuery({ queryKey: ["php-versions"], queryFn: listPhpVersions });
  const minor = site.phpVersion.split(".").slice(0, 2).join(".");
  const row = versions.find((v) => v.minor === minor);
  // Unknown row = say nothing yet rather than guess. The versions query is
  // shared cache with the page's own, so this is a first-paint blink at worst,
  // and the backend refuses regardless.
  const supported = row?.xdebugSupported ?? true;
  // The REASON comes from the row too, not from a literal here. It was a single
  // hardcoded sentence — "its build can't load extensions" — which is true of
  // 7.4 and 8.0 and a confident falsehood for a minor whose Xdebug bottle is
  // merely unpinned. Same defect as the `minor === "8.0"` literal this card
  // already had removed once: a second copy of a core rule, free to disagree.
  const blocked =
    site.webServer === "frankenphp"
      ? "Not available on FrankenPHP sites — FrankenPHP embeds its own PHP. Switch the site to Nginx or Apache first."
      : !supported
        ? (row?.xdebugUnavailableReason ?? `Not available for PHP ${minor}.`)
        : null;

  const toggle = useMutation({
    mutationFn: (on: boolean) => setSiteXdebug(site.id, on),
    onSuccess: (_s, on) => {
      void qc.invalidateQueries({ queryKey: ["sites"] });
      toast.success(
        on
          ? "Xdebug on — set your IDE to listen on port 9003, then start a session with the browser helper or ?XDEBUG_SESSION=1."
          : "Xdebug off — the site is back on the shared PHP pool.",
      );
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <SettingsCard label="Xdebug">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0 text-[0.75rem] text-rex-text-muted">
          {blocked ??
            (site.xdebug ? (
              <>
                Step debugging is on for this site (Xdebug 3, mode{" "}
                <span className="font-mono">debug,develop</span>). Your IDE listens on port{" "}
                <span className="font-mono">9003</span>; sessions start on request (browser
                helper or <span className="font-mono">?XDEBUG_SESSION=1</span>). Other sites on
                PHP {minor} are unaffected.
              </>
            ) : (
              <>
                Enable step debugging for this site only — its PHP moves to a separate debug
                pool with Xdebug loaded; every other site stays on the shared pool at full
                speed.
              </>
            ))}
        </div>
        <StartStopToggle
          running={site.xdebug}
          busy={toggle.isPending}
          disabled={!!blocked}
          title={blocked ?? undefined}
          variant="setting"
          onToggle={() => toggle.mutate(!site.xdebug)}
          label="Toggle Xdebug"
        />
      </div>
    </SettingsCard>
  );
}

/** Client-side mirror of core::site_env validation — INSTANT feedback only;
 *  the backend is the enforcement. Returns the problem, or null when valid. */
const ENV_RESERVED = new Set([
  "SCRIPT_FILENAME", "QUERY_STRING", "REQUEST_METHOD", "CONTENT_TYPE", "CONTENT_LENGTH",
  "SCRIPT_NAME", "REQUEST_URI", "DOCUMENT_URI", "DOCUMENT_ROOT", "SERVER_PROTOCOL",
  "GATEWAY_INTERFACE", "SERVER_SOFTWARE", "REMOTE_ADDR", "REMOTE_PORT", "SERVER_ADDR",
  "SERVER_PORT", "SERVER_NAME", "REQUEST_SCHEME", "HTTPS", "PATH_INFO", "PATH_TRANSLATED",
  "REDIRECT_STATUS", "AUTH_TYPE", "REMOTE_USER", "FCGI_ROLE", "PHP_VALUE", "PHP_ADMIN_VALUE",
  "PATH",
]);
function envVarProblem(v: EnvVar): string | null {
  const name = v.name.trim();
  if (!name) return "name is required";
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return "letters, digits and _ only (not starting with a digit)";
  if (ENV_RESERVED.has(name.toUpperCase())) return "reserved server parameter";
  if (name.toUpperCase().startsWith("HTTP_")) return "HTTP_ names would look like request headers";
  if (/[$]/.test(v.value)) return "value may not contain $";
  if (/[{}]/.test(v.value)) return "value may not contain { or }";
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1f\x7f]/.test(v.value)) return "value may not contain control characters";
  return null;
}

/** Per-site environment variables (§1.6): name/value rows, replace-all save.
 *  Injected per-request into the server config — the shared PHP pools are
 *  untouched. */
function EnvVarsCard({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [rows, setRows] = useState<EnvVar[] | null>(null); // null = not edited yet
  const { data: saved } = useQuery({
    queryKey: ["site-env", siteId],
    queryFn: () => listSiteEnv(siteId),
  });
  const shown = rows ?? saved ?? [];

  const save = useMutation({
    mutationFn: (vars: EnvVar[]) => setSiteEnv(siteId, vars),
    onSuccess: () => {
      setRows(null);
      void qc.invalidateQueries({ queryKey: ["site-env", siteId] });
      toast.success("Environment variables saved — server config reloaded.");
    },
    onError: (e) => toastBackendError(e),
  });

  const problems = shown.map(envVarProblem);
  const names = shown.map((r) => r.name.trim());
  const hasDuplicate = new Set(names).size !== names.length;
  const invalid = problems.some(Boolean) || hasDuplicate;
  const dirty = rows !== null;
  const edit = (i: number, patch: Partial<EnvVar>) =>
    setRows(shown.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  return (
    <SettingsCard label="Environment variables">
      <div className="flex flex-col gap-2">
        {shown.length === 0 && (
          <div className="text-[0.78125rem] text-rex-text-muted">
            No variables set. They're injected per request — the shared PHP pools are untouched.
          </div>
        )}
        {shown.map((r, i) => (
          <div key={i} className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <input
                {...TECH_INPUT}
                value={r.name}
                placeholder="NAME"
                disabled={save.isPending}
                onChange={(e) => edit(i, { name: e.target.value })}
                className="h-8 w-[220px] rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand"
              />
              <input
                {...TECH_INPUT}
                value={r.value}
                placeholder="value"
                disabled={save.isPending}
                onChange={(e) => edit(i, { value: e.target.value })}
                className="h-8 min-w-0 flex-1 rounded-md border border-rex-border bg-rex-well px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-brand"
              />
              <Button
                variant="ghost"
                size="icon"
                disabled={save.isPending}
                title="Remove variable"
                onClick={() => setRows(shown.filter((_, j) => j !== i))}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
            {problems[i] && (
              <div className="font-mono text-[0.6875rem] text-status-error-bright">{problems[i]}</div>
            )}
          </div>
        ))}
        {hasDuplicate && (
          <div className="font-mono text-[0.6875rem] text-status-error-bright">duplicate variable names</div>
        )}
        <div className="mt-1 flex items-center justify-between gap-4">
          <Button
            variant="secondary"
            disabled={save.isPending}
            onClick={() => setRows([...shown, { name: "", value: "" }])}
          >
            Add variable
          </Button>
          <Button
            variant="primary"
            disabled={!dirty || invalid || save.isPending}
            onClick={() => save.mutate(shown.map((r) => ({ name: r.name.trim(), value: r.value })))}
          >
            {save.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
        <div className="mt-1 text-[0.75rem] text-rex-text-muted">
          Available to PHP via getenv(), $_SERVER and $_ENV. Stored as plain text in the local
          server config; not for secrets.
        </div>
      </div>
    </SettingsCard>
  );
}

/** Change-domain dialog — mirrors ResetSiteDialog's overlay + destructive-confirm
 *  pattern (WordPressManager). The heavy lifting is one backend call that backs
 *  up the DB first and aborts unchanged if that fails; entering a valid new
 *  domain is the deliberate confirmation step. */
function ChangeDomainDialog({ site, onClose }: { site: Site; onClose: () => void }) {
  const qc = useQueryClient();
  const [input, setInput] = useState("");
  const [done, setDone] = useState<DomainChange | null>(null);
  const next = input.trim().toLowerCase();
  // Any development TLD (letters-only last label); the backend policy decides
  // whether it's allowed — this regex only gates obvious syntax errors.
  const wellFormed = /^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]+$/.test(next);
  const nextTld = wellFormed ? next.split(".").pop()! : "";
  // Backend policy for the entered TLD: blocked (refused with reason) or
  // warn-tier ("may shadow a real internet TLD"). Enforcement is server-side
  // either way — this is just early feedback.
  const { data: policy } = useQuery({
    queryKey: ["tld-policy", nextTld],
    queryFn: () => tldPolicy(nextTld),
    enabled: nextTld !== "",
  });
  const valid = wellFormed && next !== site.domain && policy?.allowed === true;
  const isWp = site.type === "wordpress";

  const change = useMutation({
    mutationFn: () => changeSiteDomain(site.id, next),
    onSuccess: (res) => {
      setDone(res);
      // Domain touches everything: sites list, cert card, wp-info, serving.
      qc.invalidateQueries();
    },
    onError: (e) => {
      // A late failure (e.g. edge reload) may land AFTER the row flipped — the
      // backup/cert may also already exist. Refresh so the UI shows reality.
      qc.invalidateQueries();
      toastBackendError(e);
    },
  });
  const busy = change.isPending;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={busy ? undefined : onClose}
    >
      <div
        className="w-[460px] rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {done ? (
          <>
            <div className="text-[0.9375rem] font-semibold text-rex-text">Domain changed</div>
            <div className="mt-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
              The site now lives at{" "}
              <span className="font-mono text-rex-text">https://{done.site.domain}</span>.
              {isWp && <> {done.replacements} URL references were rewritten in the database.</>}{" "}
              The site folder and database name keep their old names — that's cosmetic.
            </div>
            {done.backupPath && (
              <div className="mt-3 flex items-center justify-between gap-3 rounded-lg border border-rex-border-strong bg-rex-surface-2 px-3 py-2.5">
                <span className="truncate font-mono text-[0.71875rem] text-rex-text-bright">
                  {done.backupPath}
                </span>
                <Button
                  variant="secondary"
                  onClick={() => void revealPath(done.backupPath!).catch(toastBackendError)}
                >
                  Show in Finder
                </Button>
              </div>
            )}
            <div className="mt-4 flex justify-end gap-2">
              <Button
                variant="primary"
                onClick={() => void openExternal(`https://${done.site.domain}`).catch(toastBackendError)}
              >
                Open site
              </Button>
              <Button variant="secondary" onClick={onClose}>
                Close
              </Button>
            </div>
          </>
        ) : (
          <>
            <div className="text-[0.9375rem] font-semibold text-rex-text">Change domain?</div>
            <div className="mt-2 flex flex-col gap-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
              {isWp ? (
                <>
                  <p>
                    Every URL in the database of{" "}
                    <span className="font-mono text-rex-text">{site.domain}</span> is rewritten to
                    the new domain (including serialized data). This is{" "}
                    <span className="font-medium text-status-error-bright">not reversible</span> —
                    a database backup is exported to your Downloads folder first, and the change
                    aborts untouched if that export fails.
                  </p>
                  <p>
                    Email addresses ending in{" "}
                    <span className="font-mono text-rex-text">@{site.domain}</span> (e.g. the admin
                    email) are rewritten too. The site folder and database name keep their current
                    names. To reverse: change the domain back the same way, or import the backup.
                  </p>
                </>
              ) : (
                <p>
                  Re-issues the HTTPS certificate for the new domain and updates the server config
                  — this site stores no URLs in a database, so nothing else changes. The site
                  folder keeps its current name.
                </p>
              )}
            </div>
            <div className="mt-4 text-[0.78125rem] text-rex-text-muted">New domain:</div>
            <input
              {...TECH_INPUT}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && valid && !busy) change.mutate();
              }}
              placeholder="myshop.rex"
              disabled={busy}
              autoFocus
              className="mt-1.5 h-[32px] w-full rounded-md border border-rex-border bg-rex-surface-2 px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-status-error-border"
            />
            <div className="mt-1.5 text-[0.71875rem] text-rex-text-muted">
              Lowercase letters, digits and hyphens, ending in a development TLD (e.g.{" "}
              <span className="font-mono">.rex</span> or <span className="font-mono">.test</span>).
              First use of a new TLD asks for your password once to register it with macOS.
            </div>
            {policy && !policy.allowed && (
              <div className="mt-1.5 text-[0.71875rem] text-status-error-bright">{policy.reason}</div>
            )}
            {policy?.allowed && policy.warn && (
              <div className="mt-1.5 text-[0.71875rem] text-status-warning-bright">
                <span className="font-mono">.{nextTld}</span> may shadow a real internet TLD on
                this machine. <span className="font-mono">.test</span> is always safe.
              </div>
            )}
            <div className="mt-4 flex justify-end gap-2">
              <Button variant="secondary" disabled={busy} onClick={onClose}>
                Cancel
              </Button>
              <Button variant="danger" disabled={!valid || busy} onClick={() => change.mutate()}>
                {busy && <Loader2 className="h-3.5 w-3.5 animate-rex-spin" />}
                {busy ? (isWp ? "Backing up & rewriting…" : "Changing…") : "Change domain"}
              </Button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function RecentLogs({ siteId, onViewAll }: { siteId: string; onViewAll: () => void }) {
  const { data: targets = [] } = useQuery({
    queryKey: ["log-targets", siteId],
    queryFn: () => logTargets(siteId),
  });
  const key = targets[0]?.key ?? null;
  const { data: lines = [] } = useQuery({
    queryKey: ["tail-log-peek", key],
    queryFn: () => tailLog(key!, 6),
    enabled: !!key,
    refetchInterval: 5000,
  });
  const recent = lines.slice(-6);

  return (
    <div className="rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
      <div className="mb-[13px] flex items-center justify-between">
        <div className="font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
          Recent logs
        </div>
        <button
          onClick={onViewAll}
          className="flex items-center gap-1 text-[0.75rem] text-brand-tint transition-colors hover:underline"
        >
          View all logs
          <ChevronRight className="h-[13px] w-[13px]" strokeWidth={2} />
        </button>
      </div>
      <div className="rounded-[11px] border border-rex-well-border bg-rex-well-deep px-[14px] py-3 font-mono text-[0.71875rem] leading-[1.95]">
        {recent.length === 0 ? (
          <div className="text-rex-text-muted">
            No recent activity — start the site to see logs here.
          </div>
        ) : (
          recent.map((l, i) => (
            <div key={i} className={cn("truncate", logLineColor(l))} title={l}>
              {l}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

/** A mini-card inside the Environment grid: label + a value/control row. */
function EnvMini({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="rounded-[11px] border border-rex-border-subtle bg-rex-well px-[14px] py-[13px]">
      <div className="mb-[9px] text-[0.75rem] text-rex-text-muted">{label}</div>
      {/* flex-wrap: native selects can't shrink below their content, so at the
          980px min window the control reflows under the value instead of
          spilling out of the card. */}
      <div className="flex flex-wrap items-center justify-between gap-2">{children}</div>
    </div>
  );
}

/** A labelled path inside a recessed box: mono value + copy + open-folder. */
function PathField({
  label,
  value,
  openable,
  onOpen,
}: {
  label: string;
  value: string;
  openable?: boolean;
  /** Custom open action (e.g. reveal in Finder); defaults to opening the path. */
  onOpen?: () => void;
}) {
  return (
    <div>
      <div className="mb-1.5 text-[0.75rem] text-rex-text-muted">{label}</div>
      <div className="flex items-center gap-2 rounded-[10px] border border-rex-border-subtle bg-rex-well py-2 pl-[11px] pr-2">
        <span
          className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-text-bright"
          title={value}
        >
          {value}
        </span>
        <CopyButton value={value} />
        {(openable || onOpen) && (
          <IconBtn
            title={onOpen ? "Show in Finder" : "Open folder"}
            onClick={onOpen ?? (() => void openExternal(value).catch(toastBackendError))}
          >
            <FolderOpen className="h-3.5 w-3.5" />
          </IconBtn>
        )}
      </div>
    </div>
  );
}

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <IconBtn
      title={copied ? "Copied" : "Copy"}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
    >
      {copied ? <Check className="h-3.5 w-3.5 text-brand" /> : <Copy className="h-3.5 w-3.5" />}
    </IconBtn>
  );
}

/** A tile in the Quick-links grid: colored icon + label. */
/** A quick-link tile. With `menu`, the tile grows a chevron that opens it —
 *  as a SIBLING button, not a nested one: a `<button>` inside a `<button>` is
 *  invalid HTML and WebKit drops the inner click, so the frame moved to the
 *  wrapper and the label became its own button. */
export function QuickTile({
  icon,
  iconColor,
  label,
  onClick,
  span2,
  menu,
  menuLabel,
  menuWidth = 210,
}: {
  icon: React.ReactNode;
  iconColor: string;
  label: string;
  onClick?: () => void;
  span2?: boolean;
  menu?: React.ReactNode;
  menuLabel?: string;
  menuWidth?: number;
}) {
  return (
    <div
      className={cn(
        "flex min-w-0 items-stretch rounded-[10px] border border-rex-border-subtle bg-rex-well text-[0.78125rem] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2",
        span2 && "col-span-2",
      )}
    >
      <button
        onClick={onClick}
        className="flex min-w-0 flex-1 items-center gap-[9px] rounded-[10px] px-[11px] py-[10px] text-left"
      >
        <span className={cn("flex flex-none", iconColor)}>{icon}</span>
        <span className="truncate">{label}</span>
      </button>
      {menu && (
        <Menu align="right" width={menuWidth} trigger={
          <button
            aria-label={menuLabel}
            title={menuLabel}
            // The SEAM is the affordance: without a divider the chevron reads
            // as decoration on one wide button, and "what happens if I click
            // there" has no answer. Same seam the header split button gets.
            className="flex h-full flex-none items-center rounded-r-[10px] border-l border-rex-border-subtle px-[7px] text-rex-text-muted transition-colors hover:bg-rex-hover hover:text-rex-text"
          >
            <ChevronDown className="h-[13px] w-[13px]" strokeWidth={2} />
          </button>
        }>
          {menu}
        </Menu>
      )}
    </div>
  );
}

function IconBtn({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      className="rounded p-1 text-rex-text-muted transition-colors hover:bg-rex-surface-2 hover:text-rex-text"
    >
      {children}
    </button>
  );
}
