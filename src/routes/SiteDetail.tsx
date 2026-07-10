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
import { Button } from "@/components/ui/button";
import { confirm } from "@/components/ui/dialog";
import { SiteTerminal } from "@/components/terminal/SiteTerminal";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { WordPressManager } from "@/components/wordpress/WordPressManager";
import { adminerUrl, siteDbName } from "@/lib/adminer";
import { siteTypeMeta } from "@/lib/siteType";
import { cn, TECH_INPUT } from "@/lib/utils";
import {
  getSitesServing,
  listPhpVersions,
  listSites,
  logTargets,
  openExternal,
  regenerateSiteCert,
  renameSite,
  revealPath,
  setSitePhpVersion,
  setSiteWebServer,
  siteCertInfo,
  tailLog,
  wpAdminLoginUrl,
  wpDebugLogClear,
  wpDebugLogDownload,
  wpDebugLogStatus,
  wpDebugLogTail,
  wpInfo,
} from "@/lib/ipc";
import { toast } from "@/lib/toast";
import type { Site, WebServer } from "@/types";

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

/** Web servers selectable in Phase 2 (Apache/OpenLiteSpeed are deferred). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
];

const SELECT_CLS =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50";

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
                    "-mb-px border-b-2 px-3.5 py-2.5 text-[13.5px] font-medium transition-colors",
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
              onPhp={(v) => switchPhp.mutate(v)}
              onServer={(s) => switchServer.mutate(s)}
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
              <AdminerFrame src={adminerUrl({ engine: "mysql", db: siteDbName(site.domain) })} />
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
        className="mb-[13px] inline-flex items-center gap-1.5 text-[12px] text-rex-text-dim transition-colors hover:text-rex-text-bright"
      >
        <ChevronLeft className="h-[13px] w-[13px]" strokeWidth={2} />
        All sites
      </button>
      <div className="flex items-start justify-between gap-[18px] pb-[18px]">
        <div className="flex min-w-0 items-center gap-[13px]">
          <div
            className="flex h-[38px] w-[38px] flex-none items-center justify-center rounded-[10px] border text-[15px] font-bold"
            style={{ background: t.bg, color: t.color, borderColor: t.border }}
          >
            {t.letter}
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-[11px]">
              <span className="text-[19px] font-semibold tracking-[-.01em] text-rex-text">
                {site.name}
              </span>
              <StatusPill status={status} />
            </div>
            <div className="mt-1 flex items-center gap-2 font-mono text-[12.5px] text-rex-text-muted">
              <span className="truncate">{site.domain}</span>
              <span className="flex-none text-[11px] text-rex-text-dim">· :443</span>
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
        <div className="mb-[14px] font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
          Environment
        </div>
        <div className="grid grid-cols-3 gap-[14px]">
          <EnvMini label="PHP version">
            <span className="font-mono text-[18px] font-semibold text-rex-text">
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
            <span className="text-[15px] font-semibold capitalize text-rex-text">
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
                  <span className="text-[13.5px] font-semibold text-rex-text">Trusted</span>
                </div>
                <span className="font-mono text-[10.5px] text-rex-text-dim">rexenv&nbsp;CA</span>
              </>
            ) : (
              <div className="flex items-center gap-2">
                <LockOpen
                  className="h-[17px] w-[17px]"
                  style={{ color: "var(--rex-lock-insecure)" }}
                  strokeWidth={1.8}
                />
                <span className="text-[13.5px] font-semibold text-rex-text">Not secured</span>
              </div>
            )}
          </EnvMini>
        </div>
      </div>

      <div className="grid grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)] gap-[14px]">
        <div className="min-w-0 rounded-xl border border-rex-border-subtle bg-rex-surface-1 p-[18px]">
          <div className="mb-[14px] font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
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
          <div className="mb-[14px] font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
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
      <div className="mb-[14px] font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
        {label}
      </div>
      {children}
    </div>
  );
}

function InfoRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <div className="flex-none text-[12px] text-rex-text-muted">{label}</div>
      <div className="min-w-0 text-right text-[13px] text-rex-text-bright">{children}</div>
    </div>
  );
}

function SettingsTab({ site }: { site: Site }) {
  const qc = useQueryClient();
  const [name, setName] = useState(site.name);
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
      <SettingsCard label="Site name">
        <div className="flex items-center gap-2">
          <input
            {...TECH_INPUT}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && dirty && !rename.isPending) rename.mutate(trimmed);
            }}
            className="h-9 w-full max-w-[420px] rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[13px] text-rex-text outline-none transition-colors focus:border-brand"
          />
          <Button
            variant="primary"
            disabled={!dirty || rename.isPending}
            onClick={() => rename.mutate(trimmed)}
          >
            {rename.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
        <div className="mt-2 text-[12px] text-rex-text-dim">
          Display name only — the domain, folder and database are unchanged.
        </div>
      </SettingsCard>

      <SettingsCard label="Site info">
        <div className="flex max-w-[560px] flex-col gap-3">
          <InfoRow label="Type">{TYPE_LABELS[site.type] ?? site.type}</InfoRow>
          <InfoRow label="Database name">
            {hasDb ? (
              <span className="inline-flex items-center gap-1.5">
                <span className="truncate font-mono text-[12.5px]">{siteDbName(site.domain)}</span>
                <CopyButton value={siteDbName(site.domain)} />
              </span>
            ) : (
              <span className="text-rex-text-dim">— (no database)</span>
            )}
          </InfoRow>
          <InfoRow label="Multisite">{MULTISITE_LABELS[site.multisite] ?? site.multisite}</InfoRow>
        </div>
      </SettingsCard>

      <SettingsCard label="HTTPS certificate">
        {certLoading ? (
          <div className="text-[12.5px] text-rex-text-dim">Reading certificate…</div>
        ) : !cert ? (
          <div className="flex items-center justify-between gap-4">
            <div className="text-[12.5px] text-rex-text-dim">
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
                <span className={cn("ml-2 font-mono text-[11.5px]", expiryTone)}>{expiryNote}</span>
              </InfoRow>
              <InfoRow label="Domains">
                <span className="font-mono text-[12.5px]">{cert.sans.join(", ")}</span>
              </InfoRow>
            </div>
            <PathField
              label="Certificate folder"
              value={cert.certDir}
              onOpen={() => void revealPath(cert.certDir)}
            />
            <div className="mt-1 flex items-center justify-between gap-4 border-t border-rex-border-subtle pt-3">
              <div className="text-[12px] text-rex-text-dim">
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
    </>
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
        <div className="font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
          Recent logs
        </div>
        <button
          onClick={onViewAll}
          className="flex items-center gap-1 text-[12px] text-brand-tint transition-colors hover:underline"
        >
          View all logs
          <ChevronRight className="h-[13px] w-[13px]" strokeWidth={2} />
        </button>
      </div>
      <div className="rounded-[11px] border border-rex-well-border bg-rex-well-deep px-[14px] py-3 font-mono text-[11.5px] leading-[1.95]">
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
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand"
          >
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
            {paused ? "Resume" : "Pause"}
          </button>
        </div>
        <div
          ref={scrollRef}
          onScroll={onScroll}
          className={`${isWordpress ? "h-[42vh]" : "h-[60vh]"} overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[11.5px] leading-relaxed text-rex-text`}
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
    "flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand disabled:opacity-50";

  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-rex-border p-2.5">
        <div className="flex min-w-0 items-center gap-2.5">
          <FileText className="h-4 w-4 flex-none text-rex-text-muted" strokeWidth={1.7} />
          <span className="text-[13px] font-medium text-rex-text">WordPress debug log</span>
          {status && (
            <span
              className={cn(
                "rounded-full border px-2 py-0.5 font-mono text-[10.5px]",
                loggingOn
                  ? "border-rex-border text-status-running"
                  : "border-rex-border text-rex-text-dim",
              )}
            >
              {loggingOn ? "logging on" : "logging off"}
            </span>
          )}
          {status?.exists && (
            <span className="font-mono text-[10.5px] text-rex-text-dim">
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
          <span className="truncate font-mono text-[11px] text-rex-text-dim" title={status.path}>
            {status.path}
          </span>
        </div>
      )}

      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="h-[32vh] overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[11.5px] leading-relaxed text-rex-text"
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
            <pre className="w-fit rounded-lg border border-rex-border-subtle bg-rex-well px-3 py-2 text-[11px] text-rex-text-bright">
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
      <div className="mb-[9px] text-[12px] text-rex-text-muted">{label}</div>
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
      <div className="mb-1.5 text-[12px] text-rex-text-muted">{label}</div>
      <div className="flex items-center gap-2 rounded-[10px] border border-rex-border-subtle bg-rex-well py-2 pl-[11px] pr-2">
        <span
          className="min-w-0 flex-1 truncate font-mono text-[12px] text-rex-text-bright"
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
        "flex items-center gap-[9px] rounded-[10px] border border-rex-border-subtle bg-rex-well px-[11px] py-[10px] text-[12.5px] text-rex-text-bright transition-colors hover:border-rex-border-strong hover:bg-rex-surface-2",
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
