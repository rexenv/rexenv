import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  Copy,
  Database,
  ExternalLink,
  FolderOpen,
  Globe,
  LayoutGrid,
  ScrollText,
  TerminalSquare,
} from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import {
  listPhpVersions,
  listSites,
  openExternal,
  setSitePhpVersion,
  setSiteWebServer,
  wpInfo,
} from "@/lib/ipc";
import type { Site, WebServer, WpInfo } from "@/types";

/** Web servers selectable in Phase 2 (Apache/OpenLiteSpeed are deferred). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
];

const SELECT_CLS =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50";

type TabKey = "overview" | "wordpress" | "database" | "logs" | "settings";

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
    { key: "settings", label: "Settings", show: true },
  ];

  // Installed versions, plus the site's current one (so the select always shows it).
  const options = versions.filter((v) => v.installed).map((v) => v.minor);
  if (!options.includes(site.phpVersion)) options.unshift(site.phpVersion);

  return (
    <>
      <TopBar title={site.name} subtitle={site.domain} showSearch={false} />

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

      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          {active === "overview" && (
            <Overview
              site={site}
              wp={wp}
              isWordpress={isWordpress}
              options={options}
              switchPhpPending={switchPhp.isPending}
              switchServerPending={switchServer.isPending}
              onPhp={(v) => switchPhp.mutate(v)}
              onServer={(s) => switchServer.mutate(s)}
              onDatabase={() => navigate("/databases")}
            />
          )}

          {active === "wordpress" && (
            <Placeholder
              icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
              label="WordPress Manager"
              hint="Plugins, themes, users & tools land in §6/§7."
            />
          )}
          {active === "database" && (
            <Placeholder
              icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
              label="Database browser"
              hint="Adminer embeds here in §5."
            />
          )}
          {active === "logs" && (
            <Placeholder
              icon={<ScrollText className="h-[22px] w-[22px]" strokeWidth={1.6} />}
              label="Logs"
              hint="Live log tailing lands in §3."
            />
          )}
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

function Overview({
  site,
  wp,
  isWordpress,
  options,
  switchPhpPending,
  switchServerPending,
  onPhp,
  onServer,
  onDatabase,
}: {
  site: Site;
  wp?: WpInfo;
  isWordpress: boolean;
  options: string[];
  switchPhpPending: boolean;
  switchServerPending: boolean;
  onPhp: (v: string) => void;
  onServer: (s: WebServer) => void;
  onDatabase: () => void;
}) {
  const url = `https://${site.domain}`;
  const wpConfig = `${site.path}/wp-config.php`;

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
            disabled
            title="Built-in terminal lands in §4"
          />
          <QuickLink
            icon={<FolderOpen className="h-4 w-4" />}
            label="Folder"
            onClick={() => openExternal(site.path)}
          />
        </div>
      </Card>

      <Card title="Environment">
        <Row label="PHP version">
          <select
            value={site.phpVersion}
            disabled={switchPhpPending}
            onChange={(e) => onPhp(e.target.value)}
            className={SELECT_CLS}
          >
            {options.map((m) => (
              <option key={m} value={m}>
                PHP {m}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Web server">
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
        </Row>
        <Field label="Type" value={site.type} />
        {isWordpress && <Field label="WordPress" value={wp?.version ? `v${wp.version}` : "yes"} />}
      </Card>

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

function Field({ label, value }: { label: string; value: string }) {
  return (
    <Row label={label}>
      <span className="font-mono text-[12px] text-rex-text">{value}</span>
    </Row>
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
