import { useMemo, useState } from "react";
import { toast, toastBackendError } from "@/lib/toast";
import { confirm } from "@/components/ui/dialog";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ArrowUpCircle, Check, Download, ExternalLink, Globe, Loader2, LogIn, Network, Palette, Plus, RefreshCw, Replace, RotateCcw, Search, Shield, Trash2, UserPlus } from "lucide-react";
import { cn, TECH_INPUT } from "@/lib/utils";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import {
  openExternal,
  wpCoreReinstall,
  wpDbExport,
  wpSiteReset,
  wpCoreUpdate,
  wpDebugGet,
  wpDebugSet,
  wpMultisiteConvert,
  wpNetworkSiteCreate,
  wpNetworkSiteDelete,
  wpNetworkSites,
  wpPluginActivateNetwork,
  wpPluginDeactivateNetwork,
  wpRewriteFlush,
  wpSearchReplace,
  wpPluginActivate,
  wpPluginDeactivate,
  wpPluginDelete,
  wpPluginInstall,
  wpPluginUpdate,
  wpPlugins,
  wpSuperAdminAdd,
  wpSuperAdmins,
  wpThemeActivate,
  wpThemeDelete,
  wpThemeInstall,
  wpThemeUpdate,
  wpThemes,
  wpUserCreate,
  wpUserLoginUrl,
  wpUsers,
} from "@/lib/ipc";
import type { MultisiteMode, WpPlugin, WpTheme, WpUser } from "@/types";
import { MultiCard } from "@/components/sites/NewSiteDialog";

const WP_ROLES = ["subscriber", "contributor", "author", "editor", "administrator"];

// Per-role accent (Administrator violet, Editor blue, others teal/neutral).
const ROLE_META: Record<string, { color: string; bg: string; border: string }> = {
  administrator: { color: "var(--rex-brand-tint)", bg: "var(--rex-brand-tint-bg)", border: "var(--rex-brand-tint-border)" },
  editor: { color: "var(--rex-accent-blue)", bg: "var(--rex-accent-blue-bg)", border: "var(--rex-accent-blue-border)" },
  author: { color: "var(--rex-accent-teal)", bg: "var(--rex-accent-teal-bg)", border: "var(--rex-accent-teal-border)" },
};
const DEFAULT_ROLE = { color: "var(--rex-text-muted)", bg: "var(--rex-stopped-bg)", border: "var(--rex-stopped-border)" };
const roleMeta = (role: string) => ROLE_META[role] ?? DEFAULT_ROLE;

type SubTab = "plugins" | "themes" | "users" | "network" | "tools";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[12px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

// Every WP-CLI list call boots WordPress (~0.5s+) — cache results briefly, skip
// window-focus refetches, and fail after ONE retry so a broken site surfaces an
// error instead of spinning through react-query's default 3 retries.
const WP_QUERY = { staleTime: 30_000, refetchOnWindowFocus: false, retry: 1 } as const;

/** Plugins in two passes: an instant list (no update check), then a background
 *  pass with the wordpress.org update check (slow; a hang when offline) that
 *  only refreshes the update badges when it lands. Mutations invalidate
 *  `["wp-plugins", siteId]`, which prefix-matches both keys. */
function useWpPlugins(siteId: string) {
  const fast = useQuery({
    queryKey: ["wp-plugins", siteId],
    queryFn: () => wpPlugins(siteId),
    ...WP_QUERY,
  });
  const updates = useQuery({
    queryKey: ["wp-plugins", siteId, "updates"],
    queryFn: () => wpPlugins(siteId, true),
    enabled: fast.isSuccess,
    ...WP_QUERY,
    staleTime: 5 * 60_000,
  });
  const plugins = useMemo(() => {
    const base = fast.data ?? [];
    if (!updates.data) return base;
    const upd = new Map(updates.data.map((p) => [p.name, p.update]));
    return base.map((p) => ({ ...p, update: upd.get(p.name) ?? p.update }));
  }, [fast.data, updates.data]);
  return { plugins, isLoading: fast.isLoading, isError: fast.isError, error: fast.error, refetch: fast.refetch };
}

/** Themes, same two-pass shape as `useWpPlugins`. */
function useWpThemes(siteId: string) {
  const fast = useQuery({
    queryKey: ["wp-themes", siteId],
    queryFn: () => wpThemes(siteId),
    ...WP_QUERY,
  });
  const updates = useQuery({
    queryKey: ["wp-themes", siteId, "updates"],
    queryFn: () => wpThemes(siteId, true),
    enabled: fast.isSuccess,
    ...WP_QUERY,
    staleTime: 5 * 60_000,
  });
  const themes = useMemo(() => {
    const base = fast.data ?? [];
    if (!updates.data) return base;
    const upd = new Map(updates.data.map((t) => [t.name, t.update]));
    return base.map((t) => ({ ...t, update: upd.get(t.name) ?? t.update }));
  }, [fast.data, updates.data]);
  return { themes, isLoading: fast.isLoading, isError: fast.isError, error: fast.error, refetch: fast.refetch };
}

function useWpUsers(siteId: string) {
  return useQuery({ queryKey: ["wp-users", siteId], queryFn: () => wpUsers(siteId), ...WP_QUERY });
}

function PanelLoading({ what }: { what: string }) {
  return (
    <div className="flex items-center justify-center gap-2 p-6 text-[12.5px] text-rex-text-muted">
      <Loader2 className="h-4 w-4 animate-spin" />
      Loading {what}…
    </div>
  );
}

function PanelError({ what, error, onRetry }: { what: string; error: unknown; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-center gap-2 p-6 text-center">
      <AlertTriangle className="h-5 w-5 text-status-error-bright" />
      <div className="text-[12.5px] font-medium text-rex-text">Couldn't load {what}</div>
      <div className="max-w-[440px] break-words font-mono text-[11px] text-rex-text-muted">{String(error)}</div>
      <button className={BTN + " mt-1 flex items-center gap-1.5"} onClick={onRetry}>
        <RefreshCw className="h-3.5 w-3.5" />
        Retry
      </button>
    </div>
  );
}

export function WordPressManager({
  siteId,
  multisite = "none",
  domain,
}: {
  siteId: string;
  multisite?: MultisiteMode;
  domain: string;
}) {
  const isNetwork = multisite !== "none";
  const [sub, setSub] = useState<SubTab>("plugins");

  // Counts for the tab badges (react-query reuses the panels' cached results).
  const { plugins } = useWpPlugins(siteId);
  const { themes } = useWpThemes(siteId);
  const { data: users = [] } = useWpUsers(siteId);
  const { data: netSites = [] } = useQuery({
    queryKey: ["wp-network-sites", siteId],
    queryFn: () => wpNetworkSites(siteId),
    enabled: isNetwork,
    ...WP_QUERY,
  });

  const subs: { key: SubTab; label: string; count?: number }[] = [
    { key: "plugins", label: "Plugins", count: plugins.length },
    { key: "themes", label: "Themes", count: themes.length },
    { key: "users", label: "Users", count: users.length },
    { key: "tools", label: "Tools" },
    // Always shown: multisite sites get the network manager, single sites the
    // convert panel (the §10.1 convert had no post-create UI until this).
    { key: "network", label: "Network", count: isNetwork ? netSites.length : undefined },
  ];

  return (
    <>
      <div className="flex self-start gap-0.5 rounded-[10px] border border-rex-border-subtle bg-rex-well p-[3px]">
        {subs.map((s) => (
          <button
            key={s.key}
            onClick={() => setSub(s.key)}
            className={cn(
              "flex h-8 items-center gap-1.5 rounded-[7px] px-3 text-[12.5px] font-medium transition-colors",
              sub === s.key
                ? "bg-brand-tint-bg text-brand-tint"
                : "text-rex-text-muted hover:text-rex-text-bright",
            )}
          >
            {s.label}
            {s.count !== undefined && (
              <span className="font-mono text-[10.5px] opacity-70">{s.count}</span>
            )}
          </button>
        ))}
      </div>

      {sub === "plugins" && <PluginsPanel siteId={siteId} />}
      {sub === "themes" && <ThemesPanel siteId={siteId} />}
      {sub === "users" && <UsersPanel siteId={siteId} />}
      {sub === "network" &&
        (isNetwork ? (
          <NetworkPanel siteId={siteId} mode={multisite} domain={domain} />
        ) : (
          <ConvertPanel siteId={siteId} domain={domain} />
        ))}
      {sub === "tools" && <ToolsPanel siteId={siteId} domain={domain} />}
    </>
  );
}

/** Network sub-tab for a single (non-multisite) site: convert it to a network.
 *  Backend (`wp_multisite_convert`, §10.1) persists the mode and reloads the
 *  edge — subdomain mode picks up the wildcard cert/route automatically (the
 *  per-site cert always carries the `*.domain` SAN). On success the parent's
 *  `multisite` prop refreshes via ["sites"] and this tab flips to NetworkPanel. */
function ConvertPanel({ siteId, domain }: { siteId: string; domain: string }) {
  const qc = useQueryClient();
  const [mode, setMode] = useState<Exclude<MultisiteMode, "none">>("subdirectory");

  const convert = useMutation({
    mutationFn: () => wpMultisiteConvert(siteId, mode),
    onSuccess: () => {
      toast.success(`Converted to ${mode} multisite`);
      qc.invalidateQueries({ queryKey: ["sites"] });
      qc.invalidateQueries({ queryKey: ["wp-info", siteId] });
      qc.invalidateQueries({ queryKey: ["wp-network-sites", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-3">
        <Network className="h-4 w-4 text-brand" />
        <span className="text-[13px] font-medium text-rex-text">Multisite network</span>
        <span className="rounded-full bg-rex-well px-2 py-0.5 text-[11px] font-medium text-rex-text-muted">
          Not enabled
        </span>
      </div>

      <Card title="Convert to multisite">
        <div className="text-[12.5px] leading-[1.5] text-rex-text-muted">
          Run many sites from this one WordPress install. Choose how sub-sites are addressed:
        </div>
        <div className="mt-3 grid grid-cols-2 gap-[10px]">
          <MultiCard
            label="Subdomain"
            example={`site1.${domain}`}
            selected={mode === "subdomain"}
            onClick={() => setMode("subdomain")}
          />
          <MultiCard
            label="Subdirectory"
            example={`${domain}/site1`}
            selected={mode === "subdirectory"}
            onClick={() => setMode("subdirectory")}
          />
        </div>
        {mode === "subdomain" && (
          <div className="mt-2.5 text-[12px] text-rex-text-muted">
            <span className="font-mono">*.{domain}</span> DNS, HTTPS certificate and routing are
            handled automatically.
          </div>
        )}
        <div className="mt-2.5 flex items-start gap-2 rounded-[9px] border border-status-warning-border bg-status-warning-bg px-3 py-2 text-[12px] leading-[1.5] text-status-warning-bright">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 flex-none" />
          <span>
            Conversion edits <span className="font-mono">wp-config.php</span> and changes the
            site's URL structure. Switching between subdomain and subdirectory later isn't
            straightforward — pick the mode you'll keep.
          </span>
        </div>
        <button
          className={BTN + " mt-3 flex items-center gap-1.5"}
          disabled={convert.isPending}
          onClick={async () => {
            if (
              await confirm({
                title: "Convert to multisite?",
                message: `Convert ${domain} to a ${mode} network? This edits wp-config.php and changes the URL structure. (Reset site returns it to a clean single-site install.)`,
                confirmLabel: "Convert",
              })
            )
              convert.mutate();
          }}
        >
          {convert.isPending ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Network className="h-3.5 w-3.5" />
          )}
          {convert.isPending ? "Converting…" : "Convert to multisite"}
        </button>
      </Card>
    </div>
  );
}

function NetworkPanel({ siteId, mode, domain }: { siteId: string; mode: MultisiteMode; domain: string }) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [admin, setAdmin] = useState("");

  const {
    data: sites = [],
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["wp-network-sites", siteId],
    queryFn: () => wpNetworkSites(siteId),
    ...WP_QUERY,
  });
  const { plugins } = useWpPlugins(siteId);
  const { data: supers = [] } = useQuery({
    queryKey: ["wp-super-admins", siteId],
    queryFn: () => wpSuperAdmins(siteId),
    ...WP_QUERY,
  });

  const sitesRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-network-sites", siteId] }),
    onError: (e) => toastBackendError(e),
  });
  const pluginRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] }),
    onError: (e) => toastBackendError(e),
  });
  const superRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => {
      setAdmin("");
      qc.invalidateQueries({ queryKey: ["wp-super-admins", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const modeLabel = mode === "subdomain" ? "Subdomain" : "Subdirectory";

  return (
    <div className="flex flex-col gap-3">
      {/* Mode badge */}
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-3">
        <Network className="h-4 w-4 text-brand" />
        <span className="text-[13px] font-medium text-rex-text">Multisite network</span>
        <span className="rounded-full bg-brand/15 px-2 py-0.5 text-[11px] font-medium text-brand">
          {modeLabel}
        </span>
      </div>

      {/* Sub-sites */}
      <Card title="Sub-sites">
        <div className="mb-3 flex items-center gap-2">
          <input {...TECH_INPUT}
            value={slug}
            onChange={(e) => setSlug(e.target.value)}
            placeholder={mode === "subdomain" ? `slug (→ slug.${domain})` : `slug (→ ${domain}/slug)`}
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
          />
          <button
            className={BTN + " flex items-center gap-1.5"}
            disabled={sitesRun.isPending || !slug.trim()}
            onClick={() => {
              const s = slug.trim();
              sitesRun.mutate(() => wpNetworkSiteCreate(siteId, s).then(() => setSlug("")));
            }}
          >
            <Plus className="h-3.5 w-3.5" />
            Create
          </button>
        </div>
        {isLoading ? (
          <PanelLoading what="sub-sites" />
        ) : isError ? (
          <PanelError what="sub-sites" error={error} onRetry={refetch} />
        ) : sites.length === 0 ? (
          <div className="py-4 text-center text-[12.5px] text-rex-text-muted">No sub-sites yet.</div>
        ) : (
          <div className="overflow-hidden rounded-lg border border-rex-border">
            {sites.map((s) => (
              <div
                key={s.id}
                className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
              >
                <span className="rounded bg-rex-surface-3 px-1.5 py-0.5 font-mono text-[10.5px] text-rex-text-muted">
                  #{s.id}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-rex-text" title={s.url}>
                  {s.url}
                </span>
                {s.deleted && (
                  <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] text-amber-400">
                    archived
                  </span>
                )}
                <IconBtn title="Visit" onClick={() => openExternal(s.url)}>
                  <Globe className="h-3.5 w-3.5" />
                </IconBtn>
                <IconBtn title="Admin" onClick={() => openExternal(`${s.url.replace(/\/$/, "")}/wp-admin/`)}>
                  <ExternalLink className="h-3.5 w-3.5" />
                </IconBtn>
                <button
                  className={BTN + " hover:border-red-500/60 hover:text-red-400 disabled:hover:border-rex-border disabled:hover:text-rex-text"}
                  disabled={sitesRun.isPending || s.id === "1"}
                  title={s.id === "1" ? "Can't delete the main site" : "Delete sub-site"}
                  onClick={async () => {
                    if (await confirm({ title: "Delete sub-site?", message: s.url, danger: true, confirmLabel: "Delete" }))
                      sitesRun.mutate(() => wpNetworkSiteDelete(siteId, s.id));
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </div>
            ))}
          </div>
        )}
      </Card>

      {/* Network-active plugins */}
      <Card title="Plugins (network)">
        <div className="overflow-hidden rounded-lg border border-rex-border">
          {plugins.length === 0 ? (
            <div className="py-4 text-center text-[12.5px] text-rex-text-muted">No plugins installed.</div>
          ) : (
            plugins.map((p) => {
              const net = p.status === "active-network";
              return (
                <div
                  key={p.name}
                  className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
                >
                  <span className="min-w-0 flex-1 truncate text-[12.5px] text-rex-text">{p.name}</span>
                  {net && (
                    <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-[10.5px] font-medium text-emerald-400">
                      Network active
                    </span>
                  )}
                  <button
                    className={BTN}
                    disabled={pluginRun.isPending}
                    onClick={() =>
                      pluginRun.mutate(() =>
                        net
                          ? wpPluginDeactivateNetwork(siteId, [p.name])
                          : wpPluginActivateNetwork(siteId, [p.name]),
                      )
                    }
                  >
                    {net ? "Network deactivate" : "Network activate"}
                  </button>
                </div>
              );
            })
          )}
        </div>
      </Card>

      {/* Super admins */}
      <Card title="Super admins">
        <div className="mb-3 flex items-center gap-2">
          <Shield className="h-4 w-4 text-rex-text-muted" />
          <input {...TECH_INPUT}
            value={admin}
            onChange={(e) => setAdmin(e.target.value)}
            placeholder="username or email"
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
          />
          <button
            className={BTN + " flex items-center gap-1.5"}
            disabled={superRun.isPending || !admin.trim()}
            onClick={() => {
              const u = admin.trim();
              superRun.mutate(() => wpSuperAdminAdd(siteId, u));
            }}
          >
            <UserPlus className="h-3.5 w-3.5" />
            Add
          </button>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {supers.length === 0 ? (
            <span className="text-[12.5px] text-rex-text-muted">No super admins.</span>
          ) : (
            supers.map((u) => (
              <span
                key={u}
                className="flex items-center gap-1 rounded-full bg-rex-surface-3 px-2 py-0.5 font-mono text-[11.5px] text-rex-text"
              >
                <Shield className="h-3 w-3 text-brand" />
                {u}
              </span>
            ))
          )}
        </div>
      </Card>
    </div>
  );
}

function ToolsPanel({ siteId, domain }: { siteId: string; domain: string }) {
  const qc = useQueryClient();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [dryRun, setDryRun] = useState(true);
  const [srResult, setSrResult] = useState<string | null>(null);
  const [coreOut, setCoreOut] = useState<string | null>(null);

  const { data: wpDebug } = useQuery({
    queryKey: ["wp-debug", siteId],
    queryFn: () => wpDebugGet(siteId),
    ...WP_QUERY,
  });

  const toggleDebug = useMutation({
    mutationFn: (on: boolean) => wpDebugSet(siteId, on),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-debug", siteId] }),
    onError: (e) => toastBackendError(e),
  });

  const searchReplace = useMutation({
    mutationFn: () => wpSearchReplace(siteId, from.trim(), to.trim(), dryRun),
    onSuccess: (n) =>
      setSrResult(dryRun ? `${n} row(s) would change (dry run — nothing modified)` : `${n} row(s) changed`),
    onError: (e) => toastBackendError(e),
  });

  const flush = useMutation({
    mutationFn: () => wpRewriteFlush(siteId),
    onSuccess: () => toast.success("Permalinks regenerated."),
    onError: (e) => toastBackendError(e),
  });

  const coreUpdate = useMutation({
    mutationFn: () => wpCoreUpdate(siteId),
    onSuccess: (out) => setCoreOut(out),
    onError: (e) => toastBackendError(e),
  });
  const coreReinstall = useMutation({
    mutationFn: () => wpCoreReinstall(siteId),
    onSuccess: (out) => setCoreOut(out),
    onError: (e) => toastBackendError(e),
  });
  const adminLogin = useMutation({
    mutationFn: () => wpUserLoginUrl(siteId, 1),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toastBackendError(e),
  });
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) => toast.success(`Database exported to ${path}`),
    onError: (e) => toastBackendError(e),
  });
  const working = coreUpdate.isPending || coreReinstall.isPending;
  const [resetOpen, setResetOpen] = useState(false);
  const maintBtn = BTN + " flex w-full items-center justify-center gap-1.5";

  return (
    <div className="grid grid-cols-2 gap-3">
      {/* Search & replace — spans the row */}
      <div className="col-span-2">
        <Card title="Search & replace">
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <input {...TECH_INPUT}
                value={from}
                onChange={(e) => setFrom(e.target.value)}
                placeholder="old (e.g. old.test)"
                className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
              />
              <span className="text-rex-text-muted">→</span>
              <input {...TECH_INPUT}
                value={to}
                onChange={(e) => setTo(e.target.value)}
                placeholder="new (e.g. new.test)"
                className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
              />
            </div>
            <div className="flex items-center justify-between">
              <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
                <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} />
                Dry run (report only, don't change data)
              </label>
              <button
                className={BTN + " flex items-center gap-1.5"}
                disabled={searchReplace.isPending || !from.trim() || !to.trim()}
                onClick={() => {
                  setSrResult(null);
                  searchReplace.mutate();
                }}
              >
                <Replace className="h-3.5 w-3.5" />
                {dryRun ? "Preview" : "Run"}
              </button>
            </div>
            {srResult && (
              <div className="flex items-center gap-2.5 rounded-lg border border-rex-border-strong border-l-[3px] border-l-brand bg-rex-surface-2 px-3 py-2.5">
                <Replace className="h-4 w-4 flex-none text-brand-tint" />
                <span className="font-mono text-[11.5px] text-rex-text-bright">{srResult}</span>
              </div>
            )}
          </div>
        </Card>
      </div>

      {/* Debugging */}
      <Card title="Debugging">
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <span className="text-[12.5px] text-rex-text-muted">
              Log PHP notices/errors to <span className="font-mono">wp-content/debug.log</span>.
            </span>
            <StartStopToggle
              running={!!wpDebug}
              variant="setting"
              onToggle={() => toggleDebug.mutate(!wpDebug)}
              label="Toggle WP_DEBUG"
            />
          </div>
          <button className={maintBtn} disabled={adminLogin.isPending} onClick={() => adminLogin.mutate()}>
            <LogIn className="h-3.5 w-3.5" />
            One-click admin login
          </button>
        </div>
      </Card>

      {/* Maintenance */}
      <Card title="Maintenance">
        <div className="flex flex-col gap-2">
          <button className={maintBtn} disabled={flush.isPending} onClick={() => flush.mutate()}>
            Regenerate permalinks
          </button>
          <button className={maintBtn} disabled={dbExport.isPending} onClick={() => dbExport.mutate()}>
            <Download className="h-3.5 w-3.5" />
            {dbExport.isPending ? "Exporting…" : "Export database"}
          </button>
          <button
            className={maintBtn}
            disabled={coreUpdate.isPending}
            onClick={() => {
              setCoreOut(null);
              coreUpdate.mutate();
            }}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Update core
          </button>
          <button
            className={maintBtn}
            disabled={coreReinstall.isPending}
            onClick={async () => {
              if (await confirm({ title: "Re-install core?", message: "Re-download WordPress core files (current version)?", confirmLabel: "Re-install" })) {
                setCoreOut(null);
                coreReinstall.mutate();
              }
            }}
          >
            Re-install core
          </button>
          <button
            className={BTN + " flex w-full items-center justify-center gap-1.5 border-status-error-border text-status-error-bright hover:bg-status-error-bg"}
            onClick={() => setResetOpen(true)}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Erase database &amp; reset site
          </button>
          {working && <span className="text-center text-[12px] text-rex-text-muted">Working…</span>}
        </div>
        {coreOut && <pre className="mt-2 whitespace-pre-wrap font-mono text-[11.5px] text-rex-text-muted">{coreOut}</pre>}
      </Card>
      {resetOpen && <ResetSiteDialog siteId={siteId} domain={domain} onClose={() => setResetOpen(false)} />}
    </div>
  );
}

/** Type-to-confirm erase + reset. The fresh install uses the default
 *  local-dev credentials (admin / admin) — deterministic, nothing stored. */
function ResetSiteDialog({ siteId, domain, onClose }: { siteId: string; domain: string; onClose: () => void }) {
  const qc = useQueryClient();
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState(false);
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) => toast.success(`Database exported to ${path}`),
    onError: (e) => toastBackendError(e),
  });
  const reset = useMutation({
    mutationFn: () => wpSiteReset(siteId),
    onSuccess: () => {
      setDone(true);
      // Everything about this site changed (content, users, multisite flag).
      qc.invalidateQueries();
    },
    onError: (e) => toastBackendError(e),
  });
  const match = typed === domain;
  const busy = reset.isPending;
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={busy ? undefined : onClose}
    >
      <div
        className="w-[440px] rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {done ? (
          <>
            <div className="text-[15px] font-semibold text-rex-text">Site reset complete</div>
            <div className="mt-2 text-[13px] leading-[1.55] text-rex-text-muted">
              <span className="font-mono">{domain}</span> is a clean WordPress install again. Admin account:
            </div>
            <div className="mt-3 rounded-lg border border-rex-border-strong bg-rex-surface-2 px-3 py-2.5 font-mono text-[12.5px] text-rex-text-bright">
              admin / admin
            </div>
            <div className="mt-2 text-[12px] text-rex-text-muted">One-click admin login keeps working too.</div>
            <div className="mt-4 flex justify-end">
              <button className={BTN} onClick={onClose}>
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="text-[15px] font-semibold text-rex-text">Erase database &amp; reset site?</div>
            <div className="mt-2 flex flex-col gap-2 text-[13px] leading-[1.55] text-rex-text-muted">
              <p>
                This <span className="font-medium text-status-error-bright">permanently erases the database</span> of{" "}
                <span className="font-mono text-rex-text">{domain}</span> — all posts, pages, comments, users, and
                settings. It cannot be undone.
              </p>
              <p>
                Files stay on disk: plugins (they end up deactivated), themes, and uploads (no longer in the Media
                Library). A fresh admin account is created with the default local credentials{" "}
                <span className="font-mono text-rex-text">admin / admin</span>.
              </p>
            </div>
            <button
              className={BTN + " mt-3 flex items-center gap-1.5"}
              disabled={dbExport.isPending || busy}
              onClick={() => dbExport.mutate()}
            >
              <Download className="h-3.5 w-3.5" />
              {dbExport.isPending ? "Exporting…" : "Export database first"}
            </button>
            <div className="mt-4 text-[12.5px] text-rex-text-muted">
              Type <span className="font-mono text-rex-text">{domain}</span> to confirm:
            </div>
            <input {...TECH_INPUT}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={domain}
              disabled={busy}
              autoFocus
              className="mt-1.5 h-[32px] w-full rounded-md border border-rex-border bg-rex-surface-2 px-2.5 font-mono text-[12.5px] text-rex-text outline-none focus:border-status-error-border"
            />
            <div className="mt-4 flex justify-end gap-2">
              <button className={BTN} disabled={busy} onClick={onClose}>
                Cancel
              </button>
              <button
                className={BTN + " flex items-center gap-1.5 border-status-error-border text-status-error-bright hover:bg-status-error-bg"}
                disabled={!match || busy}
                onClick={() => reset.mutate()}
              >
                {busy && <Loader2 className="h-3.5 w-3.5 animate-rex-spin" />}
                {busy ? "Erasing & reinstalling…" : "Erase database & reset"}
              </button>
            </div>
          </>
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

function IconBtn({ title, onClick, children }: { title: string; onClick: () => void; children: React.ReactNode }) {
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

function UsersPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [login, setLogin] = useState("");
  const [email, setEmail] = useState("");
  const [role, setRole] = useState("subscriber");

  const { data: users = [], isLoading, isError, error, refetch } = useWpUsers(siteId);

  const create = useMutation({
    mutationFn: () => wpUserCreate(siteId, login.trim(), email.trim(), role),
    onSuccess: () => {
      setLogin("");
      setEmail("");
      qc.invalidateQueries({ queryKey: ["wp-users", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const loginAs = useMutation({
    mutationFn: (userId: number) => wpUserLoginUrl(siteId, userId),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex flex-col gap-3">
      {/* Add user */}
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={login}
          onChange={(e) => setLogin(e.target.value)}
          placeholder="username"
          className="h-[30px] w-32 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <input {...TECH_INPUT}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="email@site.test"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <select
          value={role}
          onChange={(e) => setRole(e.target.value)}
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 text-[12px] text-rex-text outline-none focus:border-brand"
        >
          {WP_ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={create.isPending || !login.trim() || !email.trim()}
          onClick={() => create.mutate()}
        >
          <UserPlus className="h-3.5 w-3.5" />
          Add user
        </button>
      </div>

      <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
        {isLoading ? (
          <PanelLoading what="users" />
        ) : isError ? (
          <PanelError what="users" error={error} onRetry={refetch} />
        ) : users.length === 0 ? (
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">No users.</div>
        ) : (
          <>
            <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[10px] uppercase tracking-[0.1em] text-rex-text-label">
              <span className="flex-1">User</span>
              <span className="w-[120px]">Role</span>
              <span className="w-[84px]" />
            </div>
            {users.map((u) => (
              <UserRow
                key={u.id}
                u={u}
                busy={loginAs.isPending}
                onLoginAs={() => loginAs.mutate(u.id)}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function UserRow({ u, busy, onLoginAs }: { u: WpUser; busy: boolean; onLoginAs: () => void }) {
  const role = u.roles.split(",")[0]?.trim() || "";
  const rm = roleMeta(role);
  const initial = (u.name || u.login).trim().charAt(0).toUpperCase() || "?";
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2.5 last:border-b-0">
      <span
        className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border text-[12px] font-semibold"
        style={{ background: rm.bg, color: rm.color, borderColor: rm.border }}
      >
        {initial}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium text-rex-text">{u.login}</div>
        <div className="truncate font-mono text-[11px] text-rex-text-dim">{u.email}</div>
      </div>
      <span className="w-[120px]">
        {role && (
          <span
            className="rounded-full border px-2 py-0.5 text-[10.5px] font-medium capitalize"
            style={{ background: rm.bg, color: rm.color, borderColor: rm.border }}
          >
            {role}
          </span>
        )}
      </span>
      <button className={BTN + " flex w-[84px] items-center justify-center gap-1.5"} disabled={busy} onClick={onLoginAs}>
        <LogIn className="h-3.5 w-3.5" />
        Log in
      </button>
    </div>
  );
}

function ThemesPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(false);

  const { themes, isLoading, isError, error, refetch } = useWpThemes(siteId);

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-themes", siteId] }),
    onError: (e) => toastBackendError(e),
  });
  const busy = run.isPending;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          placeholder="Theme slug (e.g. twentytwentyfour)"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
          <input type="checkbox" checked={activateOnAdd} onChange={(e) => setActivateOnAdd(e.target.checked)} />
          Activate
        </label>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={busy || !slug.trim()}
          onClick={() => {
            const s = slug.trim();
            run.mutate(() => wpThemeInstall(siteId, s, activateOnAdd).then(() => setSlug("")));
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          Add
        </button>
      </div>

      {!isLoading && themes.length > 0 && (
        <div className="px-0.5 font-mono text-[11px] text-rex-text-dim">
          {themes.length} {themes.length === 1 ? "theme" : "themes"} ·{" "}
          {themes.filter((t) => t.status === "active").length} active
        </div>
      )}
      {isLoading ? (
        <div className="rounded-xl border border-rex-border bg-rex-surface-1">
          <PanelLoading what="themes" />
        </div>
      ) : isError ? (
        <div className="rounded-xl border border-rex-border bg-rex-surface-1">
          <PanelError what="themes" error={error} onRetry={refetch} />
        </div>
      ) : themes.length === 0 ? (
        <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center text-[12.5px] text-rex-text-muted">
          No themes installed.
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          {themes.map((t) => (
            <ThemeCard
              key={t.name}
              t={t}
              busy={busy}
              onActivate={() => run.mutate(() => wpThemeActivate(siteId, t.name))}
              onUpdate={() => run.mutate(() => wpThemeUpdate(siteId, [t.name]))}
              onDelete={async () => {
                if (await confirm({ title: `Delete theme "${t.name}"?`, danger: true, confirmLabel: "Delete" }))
                  run.mutate(() => wpThemeDelete(siteId, [t.name]));
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ThemeCard({
  t,
  busy,
  onActivate,
  onUpdate,
  onDelete,
}: {
  t: WpTheme;
  busy: boolean;
  onActivate: () => void;
  onUpdate: () => void;
  onDelete: () => void;
}) {
  const active = t.status === "active";
  const updatable = t.update === "available";
  return (
    <div
      className={`flex flex-col rounded-xl border bg-rex-surface-1 ${
        active ? "border-brand/60" : "border-rex-border"
      }`}
    >
      <div className="relative aspect-[4/3] overflow-hidden rounded-t-xl bg-gradient-to-br from-rex-surface-3 to-rex-surface-1">
        {t.screenshot ? (
          <img
            src={t.screenshot}
            alt={`${t.name} preview`}
            loading="lazy"
            className="h-full w-full object-cover"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-rex-text-dim">
            <Palette className="h-7 w-7" strokeWidth={1.4} />
          </div>
        )}
        {active && (
          <span className="absolute right-2 top-2 flex items-center gap-1 rounded-full bg-status-running-bg px-2 py-0.5 text-[10px] font-medium text-status-running-bright">
            <span className="h-1.5 w-1.5 rounded-full bg-status-running" />
            Live
          </span>
        )}
      </div>
      <div className="flex flex-1 flex-col gap-2 p-3">
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-rex-text">{t.name}</span>
          {active && (
            <span className="flex items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-medium text-emerald-400">
              <Check className="h-3 w-3" />
              Active
            </span>
          )}
          {updatable && !active && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">v{t.version}</div>
        <div className="mt-auto flex items-center gap-1.5">
          {!active && (
            <button className={BTN + " flex-1"} disabled={busy} onClick={onActivate}>
              Activate
            </button>
          )}
          {updatable && (
            <button className={BTN + " flex items-center gap-1"} disabled={busy} onClick={onUpdate} title="Update">
              <ArrowUpCircle className="h-3.5 w-3.5" />
            </button>
          )}
          <button
            className={BTN + " hover:border-red-500/60 hover:text-red-400 disabled:hover:border-rex-border disabled:hover:text-rex-text"}
            disabled={busy || active}
            onClick={onDelete}
            title={active ? "Can't delete the active theme" : "Delete"}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </div>
  );
}

type PluginFilter = "all" | "active" | "updates";

function PluginFilterTabs({
  value,
  onChange,
  counts,
}: {
  value: PluginFilter;
  onChange: (f: PluginFilter) => void;
  counts: Record<PluginFilter, number>;
}) {
  const tabs: { key: PluginFilter; label: string; amber?: boolean }[] = [
    { key: "all", label: "All" },
    { key: "active", label: "Active" },
    { key: "updates", label: "Updates", amber: true },
  ];
  return (
    <div className="inline-flex items-center gap-1 rounded-[10px] border border-rex-border-subtle bg-rex-well p-[3px]">
      {tabs.map((t) => (
        <button
          key={t.key}
          onClick={() => onChange(t.key)}
          className={cn(
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[12.5px] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          {counts[t.key] > 0 && (
            <span
              className={cn(
                "rounded-[5px] px-1 font-mono text-[10px]",
                t.amber ? "bg-status-warning-bg text-status-warning-bright" : "opacity-70",
              )}
            >
              {counts[t.key]}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

function PluginsPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<PluginFilter>("all");

  const { plugins, isLoading, isError, error, refetch } = useWpPlugins(siteId);

  const isActive = (p: WpPlugin) => p.status === "active" || p.status === "active-network";
  const counts: Record<PluginFilter, number> = {
    all: plugins.length,
    active: plugins.filter(isActive).length,
    updates: plugins.filter((p) => p.update === "available").length,
  };
  const q = query.trim().toLowerCase();
  const visible = plugins.filter((p) => {
    if (filter === "active" && !isActive(p)) return false;
    if (filter === "updates" && p.update !== "available") return false;
    if (q && !p.name.toLowerCase().includes(q)) return false;
    return true;
  });

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => {
      setSelected(new Set());
      qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });
  const busy = run.isPending;

  const toggleSel = (name: string) =>
    setSelected((s) => {
      const next = new Set(s);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });
  const selNames = [...selected];

  return (
    <div className="flex flex-col gap-3">
      {/* Add by slug */}
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          placeholder="Plugin slug (e.g. hello-dolly)"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
          <input type="checkbox" checked={activateOnAdd} onChange={(e) => setActivateOnAdd(e.target.checked)} />
          Activate
        </label>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={busy || !slug.trim()}
          onClick={() => {
            const s = slug.trim();
            run.mutate(() => wpPluginInstall(siteId, s, activateOnAdd).then(() => setSlug("")));
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          Add
        </button>
      </div>

      {/* Search + filter toolbar */}
      <div className="flex items-center gap-2">
        <div className="relative w-[230px]">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-rex-text-muted" />
          <input {...TECH_INPUT}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search plugins…"
            className="h-[30px] w-full rounded-lg border border-rex-border bg-rex-surface-2 pl-8 pr-2.5 text-[12px] text-rex-text outline-none transition-colors focus:border-brand"
          />
        </div>
        <PluginFilterTabs value={filter} onChange={setFilter} counts={counts} />
      </div>

      {/* Bulk bar */}
      {selNames.length > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-brand/40 bg-rex-surface-1 p-2.5 text-[12px]">
          <span className="text-rex-text-muted">
            {selNames.length} plugin{selNames.length === 1 ? "" : "s"} selected
          </span>
          <button className={BTN} onClick={() => setSelected(new Set())}>
            Clear
          </button>
          <div className="flex-1" />
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginActivate(siteId, selNames))}>
            Activate
          </button>
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginDeactivate(siteId, selNames))}>
            Deactivate
          </button>
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginUpdate(siteId, selNames))}>
            Update
          </button>
          <button
            className={BTN + " hover:border-red-500/60 hover:text-red-400"}
            disabled={busy}
            onClick={async () => {
              if (await confirm({ title: `Delete ${selNames.length} plugin(s)?`, danger: true, confirmLabel: "Delete" }))
                run.mutate(() => wpPluginDelete(siteId, selNames));
            }}
          >
            Delete
          </button>
        </div>
      )}

      {/* List */}
      <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
        {isLoading ? (
          <PanelLoading what="plugins" />
        ) : isError ? (
          <PanelError what="plugins" error={error} onRetry={refetch} />
        ) : visible.length === 0 ? (
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">
            {plugins.length === 0 ? "No plugins installed." : "No plugins match."}
          </div>
        ) : (
          <>
          <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[10px] uppercase tracking-[0.1em] text-rex-text-label">
            <span className="w-[14px]" />
            <span className="flex-1">Plugin</span>
            <span className="w-[150px]">Status</span>
          </div>
          {visible.map((p) => (
            <PluginRow
              key={p.name}
              p={p}
              selected={selected.has(p.name)}
              busy={busy}
              onSelect={() => toggleSel(p.name)}
              onActivate={() => run.mutate(() => wpPluginActivate(siteId, [p.name]))}
              onDeactivate={() => run.mutate(() => wpPluginDeactivate(siteId, [p.name]))}
              onUpdate={() => run.mutate(() => wpPluginUpdate(siteId, [p.name]))}
              onDelete={async () => {
                if (await confirm({ title: `Delete plugin "${p.name}"?`, danger: true, confirmLabel: "Delete" }))
                  run.mutate(() => wpPluginDelete(siteId, [p.name]));
              }}
            />
          ))}
          </>
        )}
      </div>
    </div>
  );
}

function PluginRow({
  p,
  selected,
  busy,
  onSelect,
  onActivate,
  onDeactivate,
  onUpdate,
  onDelete,
}: {
  p: WpPlugin;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onActivate: () => void;
  onDeactivate: () => void;
  onUpdate: () => void;
  onDelete: () => void;
}) {
  const active = p.status === "active" || p.status === "active-network";
  const updatable = p.update === "available";
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2.5 last:border-b-0">
      <input type="checkbox" checked={selected} onChange={onSelect} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-rex-text">{p.name}</span>
          {updatable && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">v{p.version}</div>
      </div>
      {updatable && (
        <button className={BTN + " flex items-center gap-1"} disabled={busy} onClick={onUpdate} title="Update">
          <ArrowUpCircle className="h-3.5 w-3.5" />
        </button>
      )}
      <span
        className={cn(
          "w-[58px] text-right text-[11.5px] font-medium",
          active ? "text-status-running-bright" : "text-rex-text-muted",
        )}
      >
        {active ? "Active" : "Inactive"}
      </span>
      <StartStopToggle
        running={active}
        onToggle={active ? onDeactivate : onActivate}
        label={`${active ? "Deactivate" : "Activate"} ${p.name}`}
      />
      <button
        className={BTN + " hover:border-red-500/60 hover:text-red-400"}
        disabled={busy}
        onClick={onDelete}
        title="Delete"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
