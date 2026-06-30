import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUp, ArrowUpRight, ChevronRight, FileText, Github, Info, RefreshCw, Server, Settings as SettingsIcon, Shield, ShieldCheck, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { Button } from "@/components/ui/button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import {
  autostartStatus,
  deleteBlueprint,
  dnsStatus,
  getSetting,
  listBlueprints,
  listPhpVersions,
  openExternal,
  regenerateCerts,
  saveBlueprint,
  setAutostart,
  setDefaultPhpVersion,
  setPhpVersionInstalled,
  setSetting,
  sitesFolder,
  trustLocalCa,
  uninstallSystem,
} from "@/lib/ipc";
import { getStoredTheme, setTheme, type Theme } from "@/lib/theme";
import type { Blueprint, MultisiteMode, PhpVersion } from "@/types";

const SITES_DIR_KEY = "sites_dir";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="mb-3 text-[13px] font-semibold text-rex-text">{title}</div>
      {children}
    </div>
  );
}

function ThemeSetting() {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme());
  const OPTIONS: { value: Theme; label: string }[] = [
    { value: "dark", label: "Dark" },
    { value: "light", label: "Light" },
    { value: "system", label: "System" },
  ];
  const choose = (t: Theme) => {
    setThemeState(t);
    setTheme(t); // persists + applies immediately
  };
  return (
    <div className="flex items-center justify-between">
      <span className="text-[12.5px] text-rex-text-muted">Theme</span>
      <div className="flex gap-1 rounded-lg border border-rex-border bg-rex-surface-2 p-0.5">
        {OPTIONS.map((o) => (
          <button
            key={o.value}
            onClick={() => choose(o.value)}
            className={`rounded-md px-2.5 py-1 text-[12px] transition-colors ${
              theme === o.value
                ? "bg-brand text-white"
                : "text-rex-text-muted hover:text-rex-text"
            }`}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function SitesFolderSetting() {
  const qc = useQueryClient();
  // Resolved folder (for the placeholder) + the raw override setting.
  const { data: resolved } = useQuery({ queryKey: ["sites-folder"], queryFn: sitesFolder });
  const { data: override } = useQuery({
    queryKey: ["setting", SITES_DIR_KEY],
    queryFn: () => getSetting(SITES_DIR_KEY),
  });
  const [value, setValue] = useState("");
  useEffect(() => {
    if (override != null) setValue(override);
  }, [override]);

  const save = useMutation({
    mutationFn: (v: string) => setSetting(SITES_DIR_KEY, v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites-folder"] });
      qc.invalidateQueries({ queryKey: ["setting", SITES_DIR_KEY] });
    },
    onError: (e) => window.alert(String(e)),
  });

  return (
    <div>
      <label className="mb-1.5 block text-[12.5px] text-rex-text-muted">
        Sites folder
      </label>
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={value}
          placeholder={resolved ?? "default"}
          onChange={(e) => setValue(e.target.value)}
          className="h-[34px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none transition-colors focus:border-brand"
        />
        <Button
          variant="primary"
          disabled={save.isPending}
          onClick={() => save.mutate(value.trim())}
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
      <div className="mt-2 font-mono text-[11px] text-rex-text-dim">
        New sites are created under: {resolved ?? "…"}
        {!override && " (default)"}
      </div>
    </div>
  );
}

function PhpVersionRow({
  v,
  busy,
  onToggle,
  onMakeDefault,
}: {
  v: PhpVersion;
  busy: boolean;
  onToggle: (installed: boolean) => void;
  onMakeDefault: () => void;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle py-2.5 last:border-b-0">
      <div className="min-w-0 flex-1">
        <span className="font-mono text-[13px] text-rex-text">PHP {v.minor}</span>
        <span className="ml-2 font-mono text-[11px] text-rex-text-dim">{v.patch}</span>
        {v.isDefault && (
          <span className="ml-2 rounded border border-brand/40 bg-brand/10 px-1.5 py-0.5 text-[10px] font-medium text-brand">
            Default
          </span>
        )}
      </div>
      {v.installed ? (
        <>
          {/* New sites use the default version; let the user move it (§4.4). */}
          {!v.isDefault && (
            <Button variant="ghost" disabled={busy} onClick={onMakeDefault}>
              {busy ? "…" : "Make default"}
            </Button>
          )}
          <span className="text-[11.5px] text-status-running">Installed</span>
          {!v.isDefault && (
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => onToggle(false)}
              className="hover:text-status-error"
            >
              {busy ? "…" : "Remove"}
            </Button>
          )}
        </>
      ) : (
        <Button variant="primary" disabled={busy} onClick={() => onToggle(true)}>
          {busy ? "…" : "Install"}
        </Button>
      )}
    </div>
  );
}

function PhpVersionsSetting() {
  const qc = useQueryClient();
  const { data: versions = [], isLoading } = useQuery({
    queryKey: ["php-versions"],
    queryFn: listPhpVersions,
  });
  const toggle = useMutation({
    mutationFn: ({ minor, installed }: { minor: string; installed: boolean }) =>
      setPhpVersionInstalled(minor, installed),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["php-versions"] }),
    onError: (e) => window.alert(String(e)),
  });
  const makeDefault = useMutation({
    mutationFn: (minor: string) => setDefaultPhpVersion(minor),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["php-versions"] }),
    onError: (e) => window.alert(String(e)),
  });
  const busyFor = (minor: string) =>
    (toggle.isPending && toggle.variables?.minor === minor) ||
    (makeDefault.isPending && makeDefault.variables === minor);

  if (isLoading) {
    return <div className="text-[12.5px] text-rex-text-muted">Loading…</div>;
  }
  return (
    <div>
      {versions.map((v) => (
        <PhpVersionRow
          key={v.minor}
          v={v}
          busy={busyFor(v.minor)}
          onToggle={(installed) => toggle.mutate({ minor: v.minor, installed })}
          onMakeDefault={() => makeDefault.mutate(v.minor)}
        />
      ))}
      <div className="mt-2.5 text-[11px] text-rex-text-dim">
        Installed versions each run a php-fpm pool; new sites use the default. A site can pick its own
        version in its detail view.
      </div>
    </div>
  );
}

function DnsSslSetting() {
  const { data: dns } = useQuery({ queryKey: ["dns-status"], queryFn: dnsStatus });
  const [msg, setMsg] = useState<string | null>(null);

  const trust = useMutation({
    mutationFn: trustLocalCa,
    onSuccess: () => setMsg("Local CA re-trusted in your login keychain."),
    onError: (e) => window.alert(String(e)),
  });
  const regen = useMutation({
    mutationFn: regenerateCerts,
    onSuccess: (n) => setMsg(`Regenerated ${n} site certificate${n === 1 ? "" : "s"}.`),
    onError: (e) => window.alert(String(e)),
  });

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between border-b border-rex-border-subtle pb-3">
        <div>
          <div className="text-[12.5px] text-rex-text">Embedded DNS resolver</div>
          <div className="font-mono text-[11px] text-rex-text-dim">
            {dns ? `${dns.resolverPath} · :${dns.port}` : "…"}
          </div>
        </div>
        <div className="flex items-center gap-3">
          <StatusDot ok={!!dns?.running} label={dns?.running ? "Running" : "Stopped"} />
          <StatusDot ok={!!dns?.resolverInstalled} label={dns?.resolverInstalled ? "Resolver" : "No resolver"} />
        </div>
      </div>

      <div className="flex items-center justify-between">
        <span className="text-[12.5px] text-rex-text-muted">
          Local CA trust + per-site HTTPS certificates.
        </span>
        <div className="flex items-center gap-2">
          <Button variant="ghost" disabled={trust.isPending} onClick={() => { setMsg(null); trust.mutate(); }}>
            {trust.isPending ? "…" : "Re-trust CA"}
          </Button>
          <Button variant="primary" disabled={regen.isPending} onClick={() => { setMsg(null); regen.mutate(); }}>
            {regen.isPending ? "Regenerating…" : "Regenerate certs"}
          </Button>
        </div>
      </div>
      {msg && <div className="font-mono text-[11.5px] text-status-running">{msg}</div>}
    </div>
  );
}

function StatusDot({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className="flex items-center gap-1.5 text-[11.5px] text-rex-text-muted">
      <span className={`h-2 w-2 rounded-full ${ok ? "bg-status-running" : "bg-status-error"}`} />
      {label}
    </span>
  );
}

function AutostartSetting() {
  const qc = useQueryClient();
  const { data: enabled } = useQuery({ queryKey: ["autostart"], queryFn: autostartStatus });
  const toggle = useMutation({
    mutationFn: (on: boolean) => setAutostart(on),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["autostart"] }),
    onError: (e) => window.alert(String(e)),
  });

  return (
    <div className="flex items-center justify-between">
      <div>
        <div className="text-[12.5px] text-rex-text">Start rexenv on login</div>
        <div className="text-[11px] text-rex-text-dim">
          Launches rexenv automatically when you log in (macOS launchd agent).
        </div>
      </div>
      <button
        role="switch"
        aria-checked={!!enabled}
        disabled={toggle.isPending}
        onClick={() => toggle.mutate(!enabled)}
        className={`relative h-[22px] w-[40px] rounded-full transition-colors ${
          enabled ? "bg-brand" : "bg-rex-surface-3"
        }`}
      >
        <span
          className={`absolute top-[2px] h-[18px] w-[18px] rounded-full bg-white transition-all ${
            enabled ? "left-[20px]" : "left-[2px]"
          }`}
        />
      </button>
    </div>
  );
}

const CSV = (s: string) => s.split(",").map((x) => x.trim()).filter(Boolean);

function BlueprintsSetting() {
  const qc = useQueryClient();
  const { data: blueprints = [] } = useQuery({ queryKey: ["blueprints"], queryFn: listBlueprints });

  const [name, setName] = useState("");
  const [multisite, setMultisite] = useState<MultisiteMode>("none");
  const [plugins, setPlugins] = useState("");
  const [themes, setThemes] = useState("");
  const [wpDebug, setWpDebug] = useState(false);

  const invalidate = () => qc.invalidateQueries({ queryKey: ["blueprints"] });
  const save = useMutation({
    mutationFn: (bp: Blueprint) => saveBlueprint(bp),
    onSuccess: () => {
      setName(""); setPlugins(""); setThemes(""); setWpDebug(false); setMultisite("none");
      invalidate();
    },
    onError: (e) => window.alert(String(e)),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteBlueprint(id),
    onSuccess: invalidate,
    onError: (e) => window.alert(String(e)),
  });

  const add = () => {
    const n = name.trim();
    if (!n) return;
    save.mutate({
      id: crypto.randomUUID(),
      name: n,
      spec: {
        siteType: "wordpress", phpVersion: "8.3", webServer: "nginx", multisite,
        plugins: CSV(plugins).map((slug) => ({ slug, activate: true })),
        themes: CSV(themes).map((slug) => ({ slug, activate: false })),
        wpDebug, language: "",
      },
    });
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[12px] text-rex-text-muted">
        Reusable WordPress setups — pick one in <span className="font-mono">New site</span> to auto-install its
        plugins/themes and apply multisite.
      </p>

      {blueprints.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-rex-border">
          {blueprints.map((b) => (
            <div key={b.id} className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0">
              <div className="min-w-0 flex-1">
                <div className="truncate text-[12.5px] text-rex-text">{b.name}</div>
                <div className="truncate font-mono text-[10.5px] text-rex-text-dim">
                  {b.spec.multisite !== "none" ? `multisite:${b.spec.multisite} · ` : ""}
                  {b.spec.plugins.length} plugin(s){b.spec.wpDebug ? " · WP_DEBUG" : ""}
                </div>
              </div>
              <Button variant="ghost" disabled={remove.isPending} onClick={() => remove.mutate(b.id)} className="hover:text-status-error">
                Delete
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Add */}
      <div className="flex flex-col gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <div className="flex items-center gap-2">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Blueprint name"
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 text-[12px] text-rex-text outline-none focus:border-brand"
          />
          <select value={multisite} onChange={(e) => setMultisite(e.target.value as MultisiteMode)} className={SELECT}>
            <option value="none">Single site</option>
            <option value="subdomain">Subdomain MS</option>
            <option value="subdirectory">Subdirectory MS</option>
          </select>
        </div>
        <input
          value={plugins}
          onChange={(e) => setPlugins(e.target.value)}
          placeholder="Plugin slugs (comma-separated, e.g. woocommerce, jetpack)"
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <input
          value={themes}
          onChange={(e) => setThemes(e.target.value)}
          placeholder="Theme slugs (comma-separated)"
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <div className="flex items-center justify-between">
          <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
            <input type="checkbox" checked={wpDebug} onChange={(e) => setWpDebug(e.target.checked)} />
            Enable WP_DEBUG
          </label>
          <Button variant="primary" disabled={save.isPending || !name.trim()} onClick={add}>
            {save.isPending ? "Saving…" : "Add blueprint"}
          </Button>
        </div>
      </div>
    </div>
  );
}

const SELECT =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 text-[12px] text-rex-text outline-none focus:border-brand";

function UninstallSetting() {
  const [msg, setMsg] = useState<string | null>(null);
  const run = useMutation({
    mutationFn: uninstallSystem,
    onSuccess: () =>
      setMsg("System changes removed: services stopped, .test resolver deleted, local CA untrusted. You can now quit and delete rexenv."),
    onError: (e) => window.alert(String(e)),
  });

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[12px] text-rex-text-muted">
        Reverse the system-level changes rexenv made — stop all services, remove the{" "}
        <span className="font-mono">.test</span> DNS resolver, and untrust the local HTTPS certificate
        authority. Your site files and databases are <span className="font-medium">not</span> deleted.
      </p>
      <div className="flex items-center justify-between rounded-lg border border-status-error/40 bg-status-error/5 p-3">
        <span className="text-[12.5px] text-rex-text">Remove rexenv's system changes</span>
        <Button
          variant="ghost"
          disabled={run.isPending}
          onClick={() => {
            setMsg(null);
            if (
              window.confirm(
                "Remove rexenv's system changes?\n\nThis stops all services, deletes /etc/resolver/test, and untrusts the local CA (you'll be asked for your password). Your sites and databases are kept.",
              )
            )
              run.mutate();
          }}
          className="border border-status-error/50 text-status-error hover:bg-status-error/10"
        >
          {run.isPending ? "Removing…" : "Remove"}
        </Button>
      </div>
      {msg && <div className="font-mono text-[11.5px] text-status-running">{msg}</div>}
    </div>
  );
}

const APP_VERSION = "0.1.0";
const NEXT_VERSION = "0.2.0";

/** The violet crown medallion (reused by Updates + About). */
function CrownBadge({ size }: { size: number }) {
  return (
    <div
      className="flex flex-none items-center justify-center rounded-xl border border-[var(--rex-crown-border)] bg-gradient-to-br from-[#20232C] to-[#13151B] shadow-glow-crown"
      style={{ width: size, height: size }}
    >
      <svg width={size * 0.5} height={size * 0.5} viewBox="0 0 24 24" className="block">
        <path
          d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
          fill="#7C5CFF"
          stroke="#7C5CFF"
          strokeWidth="1.1"
          strokeLinejoin="round"
        />
        <circle cx="3" cy="8.4" r="1.4" fill="#B9A6FF" />
        <circle cx="12" cy="5" r="1.6" fill="#C9BCFF" />
        <circle cx="21" cy="8.4" r="1.4" fill="#B9A6FF" />
      </svg>
    </div>
  );
}

/**
 * The Updates section — static shell. The real updater (Tauri updater) is
 * deferred (TASKS-RELEASE §6.1); the actions here are UI-only for now.
 */
function UpdatesSetting() {
  const [checking, setChecking] = useState(false);
  const [autoUpdate, setAutoUpdate] = useState(false);
  const deferred = () =>
    window.alert("Auto-update isn't wired yet — the Tauri updater is deferred (TASKS-RELEASE §6.1).");

  return (
    <>
      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
        <div className="flex items-center gap-[14px]">
          <CrownBadge size={44} />
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-semibold text-rex-text">
              rexenv <span className="font-mono font-medium">{APP_VERSION}</span>
            </div>
            <div className="mt-0.5 text-[12.5px] text-rex-text-muted">
              {checking
                ? "Checking for updates…"
                : UPDATE_READY
                  ? "An update is ready to install."
                  : "You're on the latest version."}
            </div>
          </div>
          <button
            onClick={() => {
              setChecking(true);
              setTimeout(() => setChecking(false), 1200);
            }}
            disabled={checking}
            className="flex h-9 items-center gap-2 rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[15px] text-[13px] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover disabled:opacity-60"
          >
            {checking && (
              <span className="h-3 w-3 rounded-full border-2 border-rex-text-muted/40 border-t-rex-text-bright animate-rex-spin motion-reduce:animate-none" />
            )}
            {checking ? "Checking…" : "Check again"}
          </button>
        </div>
        {UPDATE_READY && (
          <div className="mt-4 flex items-center gap-[11px] rounded-[11px] border border-status-warning-border bg-status-warning-bg px-[14px] py-[13px]">
            <ArrowUp className="h-[17px] w-[17px] flex-none text-status-warning-bright" strokeWidth={2} />
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium text-rex-text">
                Version {NEXT_VERSION} is available
              </div>
              <div className="mt-px text-[11.5px] text-rex-text-muted">
                Faster service start, PostgreSQL 16, and bug fixes.
              </div>
            </div>
            <button
              onClick={deferred}
              className="h-8 flex-none rounded-lg bg-primary px-[14px] text-[12.5px] font-medium text-white shadow-glow-primary transition-[filter] hover:brightness-110"
            >
              Install &amp; restart
            </button>
          </div>
        )}
      </div>

      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
        <div className="flex items-center gap-[14px] py-[15px]">
          <div className="flex-1">
            <div className="text-[13.5px] font-medium text-rex-text">
              Install updates automatically
            </div>
            <div className="mt-0.5 text-[12px] text-rex-text-muted">
              Download and apply new versions in the background.
            </div>
          </div>
          <StartStopToggle
            running={autoUpdate}
            variant="setting"
            onToggle={() => {
              setAutoUpdate((v) => !v);
              deferred();
            }}
            label="Install updates automatically"
          />
        </div>
      </div>
    </>
  );
}

/** The About section — identity, version, links, credits. */
function AboutSetting() {
  const linkRow = (
    icon: React.ReactNode,
    color: string,
    label: string,
    url: string | null,
  ) => (
    <button
      onClick={() => url && openExternal(url)}
      className="flex w-full items-center gap-3 border-b border-rex-border-subtle py-[14px] text-left transition-opacity last:border-b-0 hover:opacity-80"
    >
      <span className="flex flex-none" style={{ color }}>
        {icon}
      </span>
      <span className="flex-1 text-[13.5px] text-rex-text">{label}</span>
      {url && (
        <span className="font-mono text-[11px] text-rex-text-dim">
          {url.replace(/^https?:\/\//, "")}
        </span>
      )}
      {url ? (
        <ArrowUpRight className="h-[15px] w-[15px] flex-none text-rex-text-dim" strokeWidth={1.7} />
      ) : (
        <ChevronRight className="h-[15px] w-[15px] flex-none text-rex-text-dim" strokeWidth={1.7} />
      )}
    </button>
  );

  return (
    <>
      <div className="flex flex-col items-center gap-3 rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5 py-6 text-center">
        <CrownBadge size={56} />
        <div>
          <div className="font-display text-[22px] font-semibold tracking-[-0.02em] text-rex-text">
            rexenv
          </div>
          <div className="mt-1 font-mono text-[11.5px] text-rex-text-muted">
            {APP_VERSION} (build 104) · macOS · Apple silicon
          </div>
        </div>
        <div className="max-w-[380px] text-[12.5px] leading-[1.55] text-rex-text-muted">
          A calm, fast command room for your local kingdom — every server, site, and database in one place.
        </div>
      </div>

      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
        {linkRow(
          <FileText className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "#7DB8D8",
          "Documentation",
          "https://docs.rexenv.app",
        )}
        {linkRow(
          <Github className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "#C7CBD4",
          "GitHub",
          "https://github.com/rexenv",
        )}
        {linkRow(
          <ShieldCheck className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "#5FBFA8",
          "Licenses & credits",
          null,
        )}
      </div>

      <div className="text-center text-[11.5px] leading-[1.6] text-rex-text-faint">
        Built on open source — nginx, PHP, MariaDB, PostgreSQL, Redis, Mailpit, Adminer & cloudflared.
        <br />
        Made for developers who run their kingdom locally.
      </div>
    </>
  );
}

type Section = "general" | "dns" | "services" | "updates" | "about";

const SECTIONS: { key: Section; label: string; icon: LucideIcon }[] = [
  { key: "general", label: "General", icon: SettingsIcon },
  { key: "dns", label: "DNS & SSL", icon: Shield },
  { key: "services", label: "Services", icon: Server },
  { key: "updates", label: "Updates", icon: RefreshCw },
  { key: "about", label: "About", icon: Info },
];

// Mock: a newer version is available (drives the sidebar dot + Updates section).
const UPDATE_READY = true;

export function Settings() {
  const [section, setSection] = useState<Section>("general");
  const current = SECTIONS.find((s) => s.key === section)!;

  return (
    <>
      <TopBar title="Settings" subtitle={current.label} showSearch={false} />
      <div className="flex min-h-0 flex-1">
        <nav className="flex w-[188px] flex-none flex-col gap-0.5 border-r border-rex-border-subtle p-2.5">
          {SECTIONS.map((s) => {
            const Icon = s.icon;
            const active = s.key === section;
            return (
              <button
                key={s.key}
                onClick={() => setSection(s.key)}
                className={cn(
                  "flex items-center gap-2.5 rounded-[9px] px-[11px] py-[9px] text-left text-[13px] transition-colors",
                  active
                    ? "bg-brand-active text-brand-tint"
                    : "text-rex-text-muted hover:bg-white/[0.04] hover:text-rex-text",
                )}
              >
                <Icon className="h-4 w-4 flex-none" strokeWidth={1.7} />
                <span className="flex-1">{s.label}</span>
                {s.key === "updates" && UPDATE_READY && (
                  <span className="h-[7px] w-[7px] rounded-full bg-status-warning" />
                )}
              </button>
            );
          })}
        </nav>

        <div className="min-h-0 flex-1 overflow-auto px-[26px] py-[22px]">
          <div className="flex max-w-[640px] flex-col gap-4">
            {section === "general" && (
              <>
                <Card title="Theme">
                  <ThemeSetting />
                </Card>
                <Card title="Sites folder">
                  <SitesFolderSetting />
                </Card>
                <Card title="Blueprints">
                  <BlueprintsSetting />
                </Card>
              </>
            )}
            {section === "dns" && (
              <Card title="DNS & SSL">
                <DnsSslSetting />
              </Card>
            )}
            {section === "services" && (
              <>
                <Card title="Startup">
                  <AutostartSetting />
                </Card>
                <Card title="PHP versions">
                  <PhpVersionsSetting />
                </Card>
                <Card title="Uninstall">
                  <UninstallSetting />
                </Card>
              </>
            )}
            {section === "updates" && <UpdatesSetting />}
            {section === "about" && <AboutSetting />}
          </div>
        </div>
      </div>
    </>
  );
}
