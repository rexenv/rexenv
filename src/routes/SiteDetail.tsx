import { useEffect, useRef, useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Database,
  Download,
  ExternalLink,
  FileText,
  FolderOpen,
  Globe,
  LayoutGrid,
  Loader2,
  Lock,
  LockOpen,
  Pause,
  Play,
  RefreshCw,
  Settings,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { SiteTerminal } from "@/components/terminal/SiteTerminal";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { WordPressManager } from "@/components/wordpress/WordPressManager";
import { adminerFrameSrc } from "@/lib/adminer";
import { siteTypeMeta } from "@/lib/siteType";
import { cn, TECH_INPUT } from "@/lib/utils";
import {
  changeSiteDomain,
  getSitesServing,
  listPhpVersions,
  listSiteEnv,
  listSites,
  logTargets,
  moveSiteDocroot,
  openExternal,
  pickFolder,
  regenerateSiteCert,
  renameSite,
  setSiteEnv,
  revealPath,
  setSitePhpVersion,
  setSiteWebServer,
  setSiteXdebug,
  siteCertInfo,
  tailLog,
  tldPolicy,
  wpAdminLoginUrl,
  wpDebugLogClear,
  wpDebugLogDownload,
  wpDebugLogStatus,
  wpDebugLogTail,
  wpInfo,
} from "@/lib/ipc";
import { toast } from "@/lib/toast";
import type { DomainChange, EnvVar, Site, WebServer } from "@/types";

/** One-click "Open admin": open a magic auto-login link for the site's primary
 *  administrator (lands on /wp-admin/). If the link can't be issued — services
 *  stopped, no admin user, tools missing — say so and fall back to the plain
 *  WordPress login page. */
async function openWpAdmin(site: Pick<Site, "id" | "domain">) {
  try {
    await openExternal(await wpAdminLoginUrl(site.id));
  } catch (e) {
    toast.error(`Auto-login unavailable — opening the WordPress login page instead.\n${String(e)}`);
    await openExternal(`https://${site.domain}/wp-admin/`);
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

type TabKey = "overview" | "wordpress" | "database" | "logs" | "terminal" | "settings";

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
  const { data: wp } = useQuery({
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

  const isWordpress = !!wp?.isWordpress;
  const isServing = !!serving?.find((s) => s.domain === site.domain)?.serving;
  const active: TabKey = tab ?? "overview";
  const tabs: { key: TabKey; label: string; show: boolean }[] = [
    { key: "overview", label: "Overview", show: true },
    { key: "wordpress", label: "WordPress", show: isWordpress },
    { key: "database", label: "Database", show: true },
    { key: "logs", label: "Logs", show: true },
    { key: "terminal", label: "Terminal", show: true },
    { key: "settings", label: "Settings", show: true },
  ];

  // Installed versions, plus the site's current one (so the select always shows it).
  const options = versions.filter((v) => v.installed).map((v) => v.minor);
  if (!options.includes(site.phpVersion)) options.unshift(site.phpVersion);

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

      <div
        className={`min-h-0 flex-1 px-[22px] pb-[22px] pt-[18px] ${
          active === "terminal" || active === "database" ? "overflow-hidden" : "overflow-auto"
        }`}
      >
        <div
          className={`flex flex-col gap-[14px] ${
            active === "terminal" || active === "database" ? "h-full" : ""
          }`}
        >
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
              onDatabase={() => navigate("/databases")}
              onTerminal={() => navigate(`/sites/${site.id}/terminal`)}
              onViewLogs={() => navigate(`/sites/${site.id}/logs`)}
            />
          )}

          {active === "wordpress" && (
            <WordPressManager siteId={site.id} multisite={site.multisite} domain={site.domain} />
          )}
          {active === "database" &&
            (site.type === "php" ? (
              <Placeholder
                icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
                label="No database"
                hint="Blank PHP sites have no database. WordPress / Laravel sites embed Adminer here."
              />
            ) : (
              <AdminerFrame src={adminerFrameSrc({ engine: site.dbEngine, db: site.dbName })} />
            ))}
          {active === "logs" && <LogsTab siteId={site.id} isWordpress={isWordpress} />}
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
        className="mb-[13px] inline-flex items-center gap-1.5 text-[0.75rem] text-rex-text-dim transition-colors hover:text-rex-text-bright"
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
              <span className="flex-none text-[0.6875rem] text-rex-text-dim">· :443</span>
            </div>
          </div>
        </div>
        <div className="flex flex-none items-center gap-[9px]">
          <Button variant="secondary" onClick={() => openExternal(url)}>
            <ExternalLink className="h-[15px] w-[15px]" strokeWidth={1.8} />
            Open in browser
          </Button>
          {isWordpress && (
            <Button variant="primary" disabled={adminBusy} onClick={onOpenAdmin}>
              <Settings className="h-[15px] w-[15px]" strokeWidth={1.7} />
              {adminBusy ? "Signing in…" : "Open admin"}
            </Button>
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
  options: string[];
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
  const serverLabel = SERVERS.find((s) => s.value === site.webServer)?.label ?? site.webServer;

  return (
    <>
      <div className="rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
        <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
          Environment
        </div>
        <div className="grid grid-cols-3 gap-[14px]">
          <EnvMini label="PHP version">
            <span className="font-mono text-[1.125rem] font-semibold text-rex-text">
              {site.phpVersion}
            </span>
            <select
              value={site.phpVersion}
              disabled={switchPhpPending}
              onChange={(e) => onPhp(e.target.value)}
              className={SELECT_CLS}
            >
              {options.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
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
                <span className="font-mono text-[0.65625rem] text-rex-text-dim">rexenv&nbsp;CA</span>
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
          <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
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
          <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
            Quick links
          </div>
          <div className="grid grid-cols-2 gap-[9px]">
            <QuickTile
              icon={<Globe className="h-4 w-4" />}
              iconColor="text-rex-text-muted"
              label="Browser"
              onClick={() => openExternal(url)}
            />
            {isWordpress && (
              <QuickTile
                icon={<ExternalLink className="h-4 w-4" />}
                iconColor="text-rex-accent-blue"
                label="WP admin"
                onClick={() => void openWpAdmin(site)}
              />
            )}
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
              onClick={() => openExternal(site.path)}
              span2
            />
          </div>
        </div>
      </div>

      <RecentLogs siteId={site.id} onViewAll={onViewLogs} />
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
      <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
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
          follow the move, and a folder outside the rexenv sites folder is kept — not deleted — if
          you ever delete the site.
        </>
      ),
      confirmLabel: "Move folder",
    });
    if (ok) moveSite.mutate(parent);
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
        : "text-rex-text-dim";
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
        <div className="mt-2 text-[0.75rem] text-rex-text-dim">
          Display name only — the domain, folder and database are unchanged.
        </div>
      </SettingsCard>
          <SettingsCard label="Domain">
        <div className="flex items-center justify-between gap-4">
          <div className="min-w-0">
            <div className="font-mono text-[0.8125rem] text-rex-text-bright">{site.domain}</div>
            <div className="mt-1 text-[0.75rem] text-rex-text-dim">
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
            <div className="mt-1 text-[0.75rem] text-rex-text-dim">
              Move the site's files to another folder — domain, database and certificate stay the
              same.
            </div>
          </div>
          <Button
            variant="secondary"
            disabled={moveSite.isPending}
            onClick={() => void askMove()}
          >
            {moveSite.isPending ? "Moving…" : "Move…"}
          </Button>
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
              <span className="text-rex-text-dim">— (no database)</span>
            )}
          </InfoRow>
          {hasDb && (
            <InfoRow label="Database engine">
              <span className="font-mono text-[0.78125rem]">
                {site.dbEngine === "mariadb" ? "MariaDB" : "MySQL"}
                <span className="ml-2 text-rex-text-dim">
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
          <div className="text-[0.78125rem] text-rex-text-dim">Reading certificate…</div>
        ) : !cert ? (
          <div className="flex items-center justify-between gap-4">
            <div className="text-[0.78125rem] text-rex-text-dim">
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
              <div className="text-[0.75rem] text-rex-text-dim">
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
 *  version are unaffected. Client-side disable mirrors the CORE rules
 *  (FrankenPHP / PHP 8.0) — the backend is the enforcement. */
function XdebugCard({ site }: { site: Site }) {
  const qc = useQueryClient();
  const minor = site.phpVersion.split(".").slice(0, 2).join(".");
  const blocked =
    site.webServer === "frankenphp"
      ? "Not available on FrankenPHP sites — FrankenPHP embeds its own PHP. Switch the site to Nginx or Apache first."
      : minor === "8.0"
        ? "Not available for PHP 8.0 — its build can't load extensions. Switch the site to PHP 8.1 or newer first."
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
        <div className="min-w-0 text-[0.75rem] text-rex-text-dim">
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
          <div className="text-[0.78125rem] text-rex-text-dim">
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
        <div className="mt-1 text-[0.75rem] text-rex-text-dim">
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
                onClick={() => void openExternal(`https://${done.site.domain}`)}
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
            <div className="mt-1.5 text-[0.71875rem] text-rex-text-dim">
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

/** Heuristic per-line tint for unstructured log text. */
function logLineColor(line: string): string {
  if (/\berror\b/i.test(line)) return "text-status-error-bright";
  if (/\bwarn(ing)?\b/i.test(line)) return "text-status-warning-bright";
  return "text-rex-text-value";
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
        <div className="font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-label">
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
          <div className="text-rex-text-dim">
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

const LOG_LINES = 500;

function LogsTab({ siteId, isWordpress }: { siteId: string; isWordpress: boolean }) {
  const [selected, setSelected] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  const { data: targets = [] } = useQuery({
    queryKey: ["log-targets", siteId],
    queryFn: () => logTargets(siteId),
  });

  // Default to the first source once targets load.
  const active = selected ?? targets[0]?.key ?? null;

  const { data: lines = [] } = useQuery({
    queryKey: ["tail-log", active],
    queryFn: () => tailLog(active!, LOG_LINES),
    enabled: !!active,
    refetchInterval: paused ? false : 1000,
  });

  // Auto-scroll to the newest line unless the user scrolled up (or paused).
  useEffect(() => {
    const el = scrollRef.current;
    if (el && atBottomRef.current && !paused) el.scrollTop = el.scrollHeight;
  }, [lines, paused]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  return (
    <>
      <div className="rounded-xl border border-rex-border bg-rex-surface-1">
        <div className="flex items-center justify-between gap-2 border-b border-rex-border p-2.5">
          <select
            value={active ?? ""}
            onChange={(e) => setSelected(e.target.value)}
            className={SELECT_CLS}
          >
            {targets.map((t) => (
              <option key={t.key} value={t.key}>
                {t.label}
              </option>
            ))}
          </select>
          <button
            onClick={() => setPaused((p) => !p)}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand"
          >
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {paused ? "Resume" : "Pause"}
          </button>
        </div>
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className={`${isWordpress ? "h-[42vh]" : "h-[60vh]"} overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[0.71875rem] leading-relaxed text-rex-text`}
        >
          {lines.length === 0 ? (
            <div className="text-rex-text-muted">
              No log output yet — start the site's services and traffic will appear here.
            </div>
          ) : (
            lines.map((l, i) => (
              <div key={i} className="whitespace-pre-wrap break-all">
                {l}
              </div>
            ))
          )}
        </div>
      </div>

      {isWordpress && <WpDebugLogCard siteId={siteId} />}
    </>
  );
}

/** Human-readable byte count for the debug-log size chip. */
function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** WordPress debug.log viewer: status-aware (WP_DEBUG / WP_DEBUG_LOG), live
 *  tail with pause, plus Clear / Download / Open-file actions. */
function WpDebugLogCard({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [paused, setPaused] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);

  const { data: status } = useQuery({
    queryKey: ["wp-debug-log-status", siteId],
    queryFn: () => wpDebugLogStatus(siteId),
    refetchInterval: paused ? false : 5000,
  });

  const loggingOn = !!status && status.debug && status.logEnabled;
  const { data: lines = [], refetch } = useQuery({
    queryKey: ["wp-debug-log-tail", siteId],
    queryFn: () => wpDebugLogTail(siteId, LOG_LINES),
    // Only poll while the log is live; a stale file stays readable on demand.
    enabled: !!status?.exists,
    refetchInterval: paused || !loggingOn ? false : 2000,
  });

  const clear = useMutation({
    mutationFn: () => wpDebugLogClear(siteId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["wp-debug-log-tail", siteId] });
      qc.invalidateQueries({ queryKey: ["wp-debug-log-status", siteId] });
      toast.success("Debug log cleared.");
    },
    onError: (e) => toastBackendError(e),
  });
  const download = useMutation({
    mutationFn: () => wpDebugLogDownload(siteId),
    onSuccess: (dest) => toast.success(`Saved to ${dest}`),
    onError: (e) => toastBackendError(e),
  });

  useEffect(() => {
    const el = scrollRef.current;
    if (el && atBottomRef.current && !paused) el.scrollTop = el.scrollHeight;
  }, [lines, paused]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  const actionCls =
    "flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:opacity-50";

  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-rex-border p-2.5">
        <div className="flex min-w-0 items-center gap-2.5">
          <FileText className="h-4 w-4 flex-none text-rex-text-muted" strokeWidth={1.7} />
          <span className="text-[0.8125rem] font-medium text-rex-text">WordPress debug log</span>
          {status && (
            <span
              className={cn(
                "rounded-full border px-2 py-0.5 font-mono text-[0.65625rem]",
                loggingOn
                  ? "border-rex-border text-status-running"
                  : "border-rex-border text-rex-text-dim",
              )}
            >
              {loggingOn ? "logging on" : "logging off"}
            </span>
          )}
          {status?.exists && (
            <span className="font-mono text-[0.65625rem] text-rex-text-dim">
              {fmtBytes(status.sizeBytes)}
            </span>
          )}
        </div>
        <div className="flex flex-none items-center gap-1.5">
          <button
            onClick={() => (paused ? setPaused(false) : void refetch())}
            title={paused ? "Resume live refresh" : "Refresh now"}
            className={actionCls}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </button>
          <button
            onClick={() => setPaused((p) => !p)}
            title={paused ? "Resume live refresh" : "Pause live refresh"}
            className={actionCls}
          >
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {paused ? "Resume" : "Pause"}
          </button>
          <button
            onClick={() => clear.mutate()}
            disabled={!status?.exists || clear.isPending}
            title="Empty the debug.log file"
            className={actionCls}
          >
            <Trash2 className="h-3.5 w-3.5" />
            Clear
          </button>
          <button
            onClick={() => download.mutate()}
            disabled={!status?.exists || download.isPending}
            title="Save a copy to Downloads"
            className={actionCls}
          >
            <Download className="h-3.5 w-3.5" />
            Download
          </button>
          <button
            onClick={() => status && void openExternal(status.path)}
            disabled={!status?.exists}
            title="Open the log file in the default app"
            className={actionCls}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            Open file
          </button>
        </div>
      </div>

      {status && (
        <div className="border-b border-rex-border-subtle px-3 py-1.5">
          <span className="truncate font-mono text-[0.6875rem] text-rex-text-dim" title={status.path}>
            {status.path}
          </span>
        </div>
      )}

      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="h-[32vh] overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[0.71875rem] leading-relaxed text-rex-text"
      >
        {!status ? null : !loggingOn && !status.exists ? (
          <div className="flex flex-col gap-2 text-rex-text-muted">
            <div>
              WordPress debug logging is off — nothing is being written to{" "}
              <span className="font-mono">debug.log</span>.
            </div>
            <div>
              Turn on <span className="font-mono">WP_DEBUG</span> from the{" "}
              <span className="text-rex-text">WordPress → Tools</span> tab, and add this to{" "}
              <span className="font-mono">wp-config.php</span> to log to a file:
            </div>
            <pre className="w-fit rounded-lg border border-rex-border-subtle bg-rex-well px-3 py-2 text-[0.6875rem] text-rex-text-bright">
              {"define( 'WP_DEBUG', true );\ndefine( 'WP_DEBUG_LOG', true );"}
            </pre>
          </div>
        ) : !status.exists ? (
          <div className="text-rex-text-muted">
            No <span className="font-mono">debug.log</span> yet — WordPress creates it when the
            first notice, warning, or error is logged.
          </div>
        ) : lines.length === 0 ? (
          <div className="text-rex-text-muted">The debug log is empty.</div>
        ) : (
          <>
            {!loggingOn && (
              <div className="mb-2 text-rex-text-dim">
                Note: logging is currently off (
                {!status.debug ? "WP_DEBUG is false" : "WP_DEBUG_LOG is false"}) — these are
                older entries.
              </div>
            )}
            {lines.map((l, i) => (
              <div key={i} className={cn("whitespace-pre-wrap break-all", logLineColor(l))}>
                {l}
              </div>
            ))}
          </>
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
      <div className="flex items-center justify-between gap-2">{children}</div>
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
            onClick={onOpen ?? (() => openExternal(value))}
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
function QuickTile({
  icon,
  iconColor,
  label,
  onClick,
  span2,
}: {
  icon: React.ReactNode;
  iconColor: string;
  label: string;
  onClick?: () => void;
  span2?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-[9px] rounded-[10px] border border-rex-border-subtle bg-rex-well px-[11px] py-[10px] text-[0.78125rem] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2",
        span2 && "col-span-2",
      )}
    >
      <span className={cn("flex", iconColor)}>{icon}</span>
      {label}
    </button>
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
