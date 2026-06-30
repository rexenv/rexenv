import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronLeft,
  Copy,
  Database,
  ExternalLink,
  FolderOpen,
  Globe,
  LayoutGrid,
  Lock,
  LockOpen,
  Pause,
  Play,
  Settings,
  TerminalSquare,
} from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { StatusPill } from "@/components/common/StatusPill";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { Button } from "@/components/ui/button";
import { SiteTerminal } from "@/components/terminal/SiteTerminal";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { WordPressManager } from "@/components/wordpress/WordPressManager";
import { adminerUrl, siteDbName } from "@/lib/adminer";
import { siteTypeMeta } from "@/lib/siteType";
import {
  listPhpVersions,
  listSites,
  logTargets,
  openExternal,
  setSitePhpVersion,
  setSiteWebServer,
  startSite,
  stopSite,
  tailLog,
  wpInfo,
} from "@/lib/ipc";
import type { Site, WebServer } from "@/types";

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
  const { data: wp } = useQuery({
    queryKey: ["wp-info", id],
    queryFn: () => wpInfo(id!),
    enabled: !!id,
  });

  const switchPhp = useMutation({
    mutationFn: (version: string) => setSitePhpVersion(id!, version),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
  });
  const switchServer = useMutation({
    mutationFn: (server: WebServer) => setSiteWebServer(id!, server),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
  });
  const toggle = useMutation({
    mutationFn: () => (site?.status === "running" ? stopSite(id!) : startSite(id!)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
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
        busy={toggle.isPending}
        onToggle={() => toggle.mutate()}
        onBack={() => navigate("/sites")}
      />

      <div className="border-b border-rex-border px-[18px]">
        <div className="mx-auto flex max-w-2xl gap-1">
          {tabs
            .filter((t) => t.show)
            .map((t) => (
              <button
                key={t.key}
                onClick={() => navigate(`/sites/${site.id}/${t.key}`)}
                className={`-mb-px border-b-2 px-3 py-2.5 text-[13px] transition-colors ${
                  active === t.key
                    ? "border-brand font-medium text-rex-text"
                    : "border-transparent text-rex-text-muted hover:text-rex-text"
                }`}
              >
                {t.label}
              </button>
            ))}
        </div>
      </div>

      <div
        className={`min-h-0 flex-1 p-[18px] ${
          active === "terminal" || active === "database" ? "overflow-hidden" : "overflow-auto"
        }`}
      >
        <div
          className={`mx-auto flex flex-col gap-4 ${
            active === "terminal" || active === "database" ? "h-full max-w-none" : "max-w-2xl"
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
          {active === "logs" && <LogsTab siteId={site.id} />}
          {active === "terminal" && <SiteTerminal siteId={site.id} />}
          {active === "settings" && (
            <Placeholder
              icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
              label="Site settings"
              hint="Per-site settings land in a later task."
            />
          )}
        </div>
      </div>
    </>
  );
}

function SiteHeader({
  site,
  isWordpress,
  busy,
  onToggle,
  onBack,
}: {
  site: Site;
  isWordpress: boolean;
  busy: boolean;
  onToggle: () => void;
  onBack: () => void;
}) {
  const t = siteTypeMeta(site.type);
  const url = `https://${site.domain}`;
  const running = site.status === "running";
  return (
    <div className="flex-none border-b border-rex-border-subtle px-[22px] pt-[18px]">
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
              <StatusPill status={site.status} />
            </div>
            <div className="mt-1 flex items-center gap-2 font-mono text-[12.5px] text-rex-text-muted">
              <span className="truncate">{site.domain}</span>
              <span className="flex-none text-[11px] text-rex-text-dim">· :443</span>
            </div>
          </div>
        </div>
        <div className="flex flex-none items-center gap-[9px]">
          <StartStopToggle
            running={running}
            busy={busy}
            onToggle={onToggle}
            label={`${running ? "Stop" : "Start"} ${site.name}`}
          />
          <Button variant="secondary" onClick={() => openExternal(url)}>
            <ExternalLink className="h-[15px] w-[15px]" strokeWidth={1.8} />
            Open in browser
          </Button>
          {isWordpress && (
            <Button variant="primary" onClick={() => openExternal(`${url}/wp-admin`)}>
              <Settings className="h-[15px] w-[15px]" strokeWidth={1.7} />
              Open admin
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
}) {
  const url = `https://${site.domain}`;
  const wpConfig = `${site.path}/wp-config.php`;
  const serverLabel = SERVERS.find((s) => s.value === site.webServer)?.label ?? site.webServer;

  return (
    <>
      <Card title="Quick links">
        <div className="flex flex-wrap gap-2">
          <QuickLink icon={<Globe className="h-4 w-4" />} label="Browser" onClick={() => openExternal(url)} />
          {isWordpress && (
            <QuickLink
              icon={<ExternalLink className="h-4 w-4" />}
              label="WP admin"
              onClick={() => openExternal(`${url}/wp-admin`)}
            />
          )}
          <QuickLink icon={<Database className="h-4 w-4" />} label="Database" onClick={onDatabase} />
          <QuickLink
            icon={<TerminalSquare className="h-4 w-4" />}
            label="Terminal"
            onClick={onTerminal}
          />
          <QuickLink
            icon={<FolderOpen className="h-4 w-4" />}
            label="Folder"
            onClick={() => openExternal(site.path)}
          />
        </div>
      </Card>

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

      <Card title="Paths">
        <PathRow label="Document root" value={site.path} openable />
        {isWordpress && <PathRow label="wp-config.php" value={wpConfig} />}
        <Row label="URL">
          <span className="font-mono text-[12px] text-rex-text">{url}</span>
        </Row>
      </Card>

      <Card title="Recent logs">
        <div className="text-[12.5px] text-rex-text-muted">
          Live log tailing arrives in §3 — this peek will show the latest lines.
        </div>
      </Card>
    </>
  );
}

const LOG_LINES = 500;

function LogsTab({ siteId }: { siteId: string }) {
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
        className="h-[60vh] overflow-auto bg-rex-surface-2/40 p-3 font-mono text-[11.5px] leading-relaxed text-rex-text"
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
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="mb-3 text-[13px] font-semibold text-rex-text">{title}</div>
      {children}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between border-b border-rex-border-subtle py-2 last:border-b-0">
      <span className="text-[12.5px] text-rex-text-muted">{label}</span>
      {children}
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

function PathRow({ label, value, openable }: { label: string; value: string; openable?: boolean }) {
  return (
    <Row label={label}>
      <span className="flex items-center gap-1.5">
        <span className="max-w-[280px] truncate font-mono text-[12px] text-rex-text" title={value}>
          {value}
        </span>
        <CopyButton value={value} />
        {openable && (
          <IconBtn title="Open folder" onClick={() => openExternal(value)}>
            <FolderOpen className="h-3.5 w-3.5" />
          </IconBtn>
        )}
      </span>
    </Row>
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

function QuickLink({
  icon,
  label,
  onClick,
  disabled,
  title,
}: {
  icon: React.ReactNode;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-3 py-1.5 text-[12.5px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border"
    >
      {icon}
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
