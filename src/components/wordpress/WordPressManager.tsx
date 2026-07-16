import { useEffect, useMemo, useState } from "react";
import { toast, toastBackendError } from "@/lib/toast";
import { confirm, PromptDialog } from "@/components/ui/dialog";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ArrowUpCircle, Check, Download, ExternalLink, FileUp, Globe, Loader2, Lock, LogIn, Network, Palette, Plus, RefreshCw, Replace, RotateCcw, Eye, EyeOff, KeyRound, Search, Shield, Star, Trash2, UserPlus } from "lucide-react";
import { cn, TECH_INPUT } from "@/lib/utils";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import {
  openExternal,
  pickSqlFile,
  revealPath,
  wpCoreReinstall,
  wpCoreSwitchVersion,
  wpCoreVersions,
  wpDbExport,
  wpInfo,
  wpDbImport,
  wpSiteReset,
  wpCacheFlush,
  wpChecksumCleanup,
  wpContentExport,
  wpCoreUpdate,
  wpCoreVerifyChecksums,
  wpCronEvents,
  wpCronRunDue,
  wpCronRunHook,
  wpDebugFlagGet,
  wpDebugFlagSet,
  wpDebugGet,
  wpDebugSet,
  wpLanguages,
  wpMaintenanceGet,
  wpMaintenanceSet,
  wpMultisiteConvert,
  wpNetworkSiteCreate,
  wpOptions,
  wpOptionUpdate,
  wpPermalinkGet,
  wpPermalinkSet,
  wpNetworkSiteDelete,
  wpNetworkSites,
  wpPluginActivateNetwork,
  wpPluginDeactivateNetwork,
  wpRewriteFlush,
  wpSearchReplace,
  wpSwitchLanguage,
  wpPluginActivate,
  wpPluginDeactivate,
  wpPluginDelete,
  wpOrgSearchPlugins,
  wpOrgSearchThemes,
  wpPluginInstall,
  wpPluginUpdate,
  wpPlugins,
  wpPrimaryAdmin,
  wpSuperAdminAdd,
  wpSuperAdmins,
  wpThemeActivate,
  wpThemeDelete,
  wpThemeInstall,
  wpThemeUpdate,
  wpThemes,
  wpTransientDeleteAll,
  wpUserCreate,
  wpUserSetPassword,
  wpUserLoginUrl,
  wpUserSetRole,
  wpUsers,
} from "@/lib/ipc";
import type { WpDebugFlag } from "@/lib/ipc";
import type { MultisiteMode, WpChecksumReport, WpCoreSwitch, WpOptionRow, WpOrgPlugin, WpOrgTheme, WpPlugin, WpSkippedNoiseFile, WpTheme, WpUser } from "@/types";
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
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

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
    <div className="flex items-center justify-center gap-2 p-6 text-[0.78125rem] text-rex-text-muted">
      <Loader2 className="h-4 w-4 animate-spin" />
      Loading {what}…
    </div>
  );
}

function PanelError({ what, error, onRetry }: { what: string; error: unknown; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-center gap-2 p-6 text-center">
      <AlertTriangle className="h-5 w-5 text-status-error-bright" />
      <div className="text-[0.78125rem] font-medium text-rex-text">Couldn't load {what}</div>
      <div className="max-w-[440px] break-words font-mono text-[0.6875rem] text-rex-text-muted">{String(error)}</div>
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
              "flex h-8 items-center gap-1.5 rounded-[7px] px-3 text-[0.78125rem] font-medium transition-colors",
              sub === s.key
                ? "bg-brand-tint-bg text-brand-tint"
                : "text-rex-text-muted hover:text-rex-text-bright",
            )}
          >
            {s.label}
            {s.count !== undefined && (
              <span className="font-mono text-[0.65625rem] opacity-70">{s.count}</span>
            )}
          </button>
        ))}
      </div>

      {sub === "plugins" && <PluginsPanel siteId={siteId} />}
      {sub === "themes" && <ThemesPanel siteId={siteId} />}
      {sub === "users" && <UsersPanel siteId={siteId} domain={domain} />}
      {sub === "network" &&
        (isNetwork ? (
          <NetworkPanel siteId={siteId} mode={multisite} domain={domain} />
        ) : (
          <ConvertPanel siteId={siteId} domain={domain} />
        ))}
      {sub === "tools" && <ToolsPanel siteId={siteId} domain={domain} isNetwork={isNetwork} />}
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
        <span className="text-[0.8125rem] font-medium text-rex-text">Multisite network</span>
        <span className="rounded-full bg-rex-well px-2 py-0.5 text-[0.6875rem] font-medium text-rex-text-muted">
          Not enabled
        </span>
      </div>

      <Card title="Convert to multisite">
        <div className="text-[0.78125rem] leading-[1.5] text-rex-text-muted">
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
          <div className="mt-2.5 text-[0.75rem] text-rex-text-muted">
            <span className="font-mono">*.{domain}</span> DNS, HTTPS certificate and routing are
            handled automatically.
          </div>
        )}
        <div className="mt-2.5 flex items-start gap-2 rounded-[9px] border border-status-warning-border bg-status-warning-bg px-3 py-2 text-[0.75rem] leading-[1.5] text-status-warning-bright">
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
        <span className="text-[0.8125rem] font-medium text-rex-text">Multisite network</span>
        <span className="rounded-full bg-brand/15 px-2 py-0.5 text-[0.6875rem] font-medium text-brand">
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
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
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
          <div className="py-4 text-center text-[0.78125rem] text-rex-text-muted">No sub-sites yet.</div>
        ) : (
          <div className="overflow-hidden rounded-lg border border-rex-border">
            {sites.map((s) => (
              <div
                key={s.id}
                className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
              >
                <span className="rounded bg-rex-surface-3 px-1.5 py-0.5 font-mono text-[0.65625rem] text-rex-text-muted">
                  #{s.id}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-text" title={s.url}>
                  {s.url}
                </span>
                {s.deleted && (
                  <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[0.625rem] text-amber-400">
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
            <div className="py-4 text-center text-[0.78125rem] text-rex-text-muted">No plugins installed.</div>
          ) : (
            plugins.map((p) => {
              const net = p.status === "active-network";
              // Must-use/drop-in files load everywhere automatically — network
              // (de)activation doesn't exist for them either.
              const immutable = p.status === "must-use" || p.status === "dropin";
              return (
                <div
                  key={p.name}
                  className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
                >
                  <span className="min-w-0 flex-1 truncate text-[0.78125rem] text-rex-text">{p.name}</span>
                  {net && (
                    <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-[0.65625rem] font-medium text-emerald-400">
                      Network active
                    </span>
                  )}
                  {immutable ? (
                    <span
                      className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-[0.65625rem] font-medium text-emerald-400"
                      title="Loads automatically on every site (must-use / drop-in) — nothing to toggle."
                    >
                      {p.status === "must-use" ? "Must-use" : "Drop-in"}
                    </span>
                  ) : (
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
                  )}
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
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
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
            <span className="text-[0.78125rem] text-rex-text-muted">No super admins.</span>
          ) : (
            supers.map((u) => (
              <span
                key={u}
                className="flex items-center gap-1 rounded-full bg-rex-surface-3 px-2 py-0.5 font-mono text-[0.71875rem] text-rex-text"
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

/** wp-admin's stock permalink choices (mirrors the Rust PERMALINK_STRUCTURES
 *  whitelist — the backend rejects anything else). */
const PERMALINK_PRESETS: { value: string; label: string; sample: string }[] = [
  { value: "", label: "Plain", sample: "/?p=123" },
  { value: "/%year%/%monthnum%/%day%/%postname%/", label: "Day and name", sample: "/2026/07/11/sample-post/" },
  { value: "/%year%/%monthnum%/%postname%/", label: "Month and name", sample: "/2026/07/sample-post/" },
  { value: "/archives/%post_id%", label: "Numeric", sample: "/archives/123" },
  { value: "/%postname%/", label: "Post name", sample: "/sample-post/" },
];

/** The individually-toggleable debug constants (Tools → Debugging). */
const DEBUG_FLAG_ROWS: { name: WpDebugFlag; hint: string }[] = [
  { name: "WP_DEBUG_LOG", hint: "write errors to wp-content/debug.log" },
  { name: "WP_DEBUG_DISPLAY", hint: "print errors on pages" },
  { name: "SCRIPT_DEBUG", hint: "use unminified core JS/CSS" },
];

function ToolsPanel({
  siteId,
  domain,
  isNetwork,
}: {
  siteId: string;
  domain: string;
  isNetwork: boolean;
}) {
  const qc = useQueryClient();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [dryRun, setDryRun] = useState(true);
  const [srResult, setSrResult] = useState<string | null>(null);
  const [coreOut, setCoreOut] = useState<string | null>(null);
  const [verifyOut, setVerifyOut] = useState<WpChecksumReport | null>(null);

  const { data: wpDebug } = useQuery({
    queryKey: ["wp-debug", siteId],
    queryFn: () => wpDebugGet(siteId),
    ...WP_QUERY,
  });

  const toggleDebug = useMutation({
    mutationFn: (on: boolean) => wpDebugSet(siteId, on),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["wp-debug", siteId] });
      // The main toggle writes WP_DEBUG_LOG/WP_DEBUG_DISPLAY too.
      qc.invalidateQueries({ queryKey: ["wp-debug-flags", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const { data: debugFlags } = useQuery({
    queryKey: ["wp-debug-flags", siteId],
    queryFn: async () => {
      const [log, display, script] = await Promise.all([
        wpDebugFlagGet(siteId, "WP_DEBUG_LOG"),
        wpDebugFlagGet(siteId, "WP_DEBUG_DISPLAY"),
        wpDebugFlagGet(siteId, "SCRIPT_DEBUG"),
      ]);
      return { WP_DEBUG_LOG: log, WP_DEBUG_DISPLAY: display, SCRIPT_DEBUG: script };
    },
    ...WP_QUERY,
  });

  const setDebugFlag = useMutation({
    mutationFn: ({ name, on }: { name: WpDebugFlag; on: boolean }) =>
      wpDebugFlagSet(siteId, name, on),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-debug-flags", siteId] }),
    onError: (e) => toastBackendError(e),
  });

  const { data: maint } = useQuery({
    queryKey: ["wp-maintenance", siteId],
    queryFn: () => wpMaintenanceGet(siteId),
    ...WP_QUERY,
  });

  const toggleMaint = useMutation({
    mutationFn: (on: boolean) => wpMaintenanceSet(siteId, on),
    onSuccess: (_data, on) => {
      qc.invalidateQueries({ queryKey: ["wp-maintenance", siteId] });
      toast.success(
        on
          ? "Maintenance mode on — visitors see the “briefly unavailable” page."
          : "Maintenance mode off — the site is public again.",
      );
    },
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

  const { data: permalink } = useQuery({
    queryKey: ["wp-permalink", siteId],
    queryFn: () => wpPermalinkGet(siteId),
    ...WP_QUERY,
  });

  const setPermalink = useMutation({
    mutationFn: (structure: string) => wpPermalinkSet(siteId, structure),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["wp-permalink", siteId] });
      toast.success("Permalink structure updated (rewrites flushed).");
    },
    onError: (e) => toastBackendError(e),
  });

  const { data: langs } = useQuery({
    queryKey: ["wp-languages", siteId],
    queryFn: () => wpLanguages(siteId),
    ...WP_QUERY,
    // The available-languages list is fetched from api.wordpress.org — keep it
    // for the session instead of re-paying ~3s on every Tools visit.
    staleTime: 5 * 60_000,
  });
  const activeLocale = langs?.find((l) => l.status === "active")?.language ?? "en_US";
  const switchLang = useMutation({
    mutationFn: (locale: string) => wpSwitchLanguage(siteId, locale),
    onSuccess: (_d, locale) => {
      qc.invalidateQueries({ queryKey: ["wp-languages", siteId] });
      const row = langs?.find((l) => l.language === locale);
      toast.success(`Site language switched to ${row?.englishName ?? locale}.`);
    },
    // Failed download → language unchanged (backend gates on is-installed);
    // the friendly "couldn't download" error comes from the backend.
    onError: (e) => toastBackendError(e),
  });
  // Only an uninstalled pick needs the ~5s download; switching is ~1s.
  const langBusyLabel =
    switchLang.isPending &&
    langs?.find((l) => l.language === switchLang.variables)?.status === "uninstalled"
      ? "Installing language…"
      : "Switching…";

  const cacheFlush = useMutation({
    mutationFn: () => wpCacheFlush(siteId),
    onSuccess: (msg) =>
      toast.success(msg.replace(/^Success:\s*/, "").trim() || "Object cache flushed."),
    onError: (e) => toastBackendError(e),
  });

  const contentExport = useMutation({
    mutationFn: () => wpContentExport(siteId),
    onSuccess: (paths) =>
      toast.success(
        paths.length === 1
          ? `Content exported to ${paths[0]}`
          : `Content exported to ${paths.length} files in Downloads`,
        {
          label: "Show in Finder",
          onClick: () => void revealPath(paths[0]).catch(toastBackendError),
        },
      ),
    onError: (e) => toastBackendError(e),
  });

  const transients = useMutation({
    mutationFn: () => wpTransientDeleteAll(siteId),
    onSuccess: (msg) =>
      toast.success(msg.replace(/^Success:\s*/, "").trim() || "Transients deleted."),
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
  const verify = useMutation({
    mutationFn: () => wpCoreVerifyChecksums(siteId),
    onSuccess: (r) => setVerifyOut(r),
    onError: (e) => toastBackendError(e),
  });
  const adminLogin = useMutation({
    mutationFn: () => wpUserLoginUrl(siteId, 1),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toastBackendError(e),
  });
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: "Show in Finder",
        onClick: () => void revealPath(path).catch(toastBackendError),
      }),
    onError: (e) => toastBackendError(e),
  });
  const working = coreUpdate.isPending || coreReinstall.isPending;
  const [resetOpen, setResetOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const maintBtn = BTN + " flex w-full items-center justify-center gap-1.5";

  return (
    <div className="flex flex-col gap-3">
      {/* Wide tools first: Search & replace needs the full row for its inputs. */}
        <Card title="Search & replace">
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <input {...TECH_INPUT}
                value={from}
                onChange={(e) => setFrom(e.target.value)}
                placeholder="old (e.g. old.rex)"
                className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
              />
              <span className="text-rex-text-muted">→</span>
              <input {...TECH_INPUT}
                value={to}
                onChange={(e) => setTo(e.target.value)}
                placeholder="new (e.g. new.rex)"
                className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
              />
            </div>
            <div className="flex items-center justify-between">
              <label className="flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
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
                <span className="font-mono text-[0.71875rem] text-rex-text-bright">{srResult}</span>
              </div>
            )}
          </div>
        </Card>

            {/* Six half-width cards in TWO INDEPENDENT COLUMNS (items-start, each
          column packs its own heights) — no grid rows to leave half-empty
          slots when a tall card meets a short one. */}
      <div className="grid grid-cols-2 items-start gap-3">
        <div className="flex flex-col gap-3">
      <Card title="Debugging">
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <span className="text-[0.78125rem] text-rex-text-muted">
              <span className="font-mono text-[0.71875rem] text-rex-text-bright">WP_DEBUG</span>
              <span className="ml-1.5">— sets the recommended trio (log on, display off).</span>
            </span>
            <StartStopToggle
              running={!!wpDebug}
              variant="setting"
              onToggle={() => toggleDebug.mutate(!wpDebug)}
              label="Toggle WP_DEBUG"
            />
          </div>
          <div className="flex flex-col gap-2 border-t border-rex-border-subtle pt-2.5">
            {DEBUG_FLAG_ROWS.map((f) => (
              <div key={f.name} className="flex items-center justify-between gap-2">
                <span className="min-w-0 text-[0.78125rem] text-rex-text-muted">
                  <span className="font-mono text-[0.71875rem] text-rex-text-bright">{f.name}</span>
                  <span className="ml-1.5">— {f.hint}</span>
                </span>
                <StartStopToggle
                  running={!!debugFlags?.[f.name]}
                  variant="setting"
                  onToggle={() => setDebugFlag.mutate({ name: f.name, on: !debugFlags?.[f.name] })}
                  label={`Toggle ${f.name}`}
                />
              </div>
            ))}
          </div>
          <button className={maintBtn} disabled={adminLogin.isPending} onClick={() => adminLogin.mutate()}>
            <LogIn className="h-3.5 w-3.5" />
            One-click admin login
          </button>
        </div>
      </Card>

            <Card title="Maintenance">
        <div className="flex flex-col gap-2">
          <div className="mb-1 flex items-center justify-between gap-2">
            <span className="text-[0.78125rem] text-rex-text-muted">
              Maintenance mode — visitors see &ldquo;briefly unavailable&rdquo;.
            </span>
            <StartStopToggle
              running={!!maint}
              variant="setting"
              onToggle={() => toggleMaint.mutate(!maint)}
              label="Toggle maintenance mode"
            />
          </div>
          <button className={maintBtn} disabled={cacheFlush.isPending} onClick={() => cacheFlush.mutate()}>
            {cacheFlush.isPending ? "Flushing…" : "Flush object cache"}
          </button>
          <button className={maintBtn} disabled={transients.isPending} onClick={() => transients.mutate()}>
            {transients.isPending ? "Deleting…" : "Delete all transients"}
          </button>
          <button
            className={BTN + " flex w-full items-center justify-center gap-1.5 border-status-error-border text-status-error-bright hover:bg-status-error-bg"}
            onClick={() => setResetOpen(true)}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Erase database &amp; reset site
          </button>
        </div>
      </Card>
            <Card title="Backup & restore">
        <div className="flex flex-col gap-2">
          <button className={maintBtn} disabled={dbExport.isPending} onClick={() => dbExport.mutate()}>
            <Download className="h-3.5 w-3.5" />
            {dbExport.isPending ? "Exporting…" : "Export database"}
          </button>
          <button className={maintBtn} onClick={() => setImportOpen(true)}>
            <ArrowUpCircle className="h-3.5 w-3.5" />
            Import database…
          </button>
          <button
            className={maintBtn}
            disabled={contentExport.isPending}
            onClick={() => contentExport.mutate()}
          >
            <Download className="h-3.5 w-3.5" />
            {contentExport.isPending ? "Exporting…" : "Export content (WXR)"}
          </button>
        </div>
      </Card>

              </div>
        <div className="flex flex-col gap-3">
      <Card title="Permalinks">
        <div className="flex flex-col gap-2">
          <select
            value={permalink ?? ""}
            disabled={permalink === undefined || setPermalink.isPending}
            onChange={(e) => setPermalink.mutate(e.target.value)}
            className="h-[30px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
          >
            {permalink !== undefined &&
              !PERMALINK_PRESETS.some((p) => p.value === permalink) && (
                <option value={permalink}>Custom ({permalink})</option>
              )}
            {PERMALINK_PRESETS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label} — {p.sample}
              </option>
            ))}
          </select>
          <div className="font-mono text-[0.71875rem] text-rex-text-dim">
            {permalink === undefined ? "…" : permalink === "" ? "?p=123 (plain)" : permalink}
          </div>
          <button className={maintBtn} disabled={flush.isPending} onClick={() => flush.mutate()}>
            Regenerate permalinks
          </button>
        </div>
      </Card>

            <Card title="Language">
        <div className="flex flex-col gap-2">
          <select
            value={activeLocale}
            disabled={!langs || switchLang.isPending}
            onChange={(e) => switchLang.mutate(e.target.value)}
            className="h-[30px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
          >
            {!langs ? (
              <option value={activeLocale}>Loading languages…</option>
            ) : (
              <>
                <optgroup label="Installed">
                  {langs
                    .filter((l) => l.status !== "uninstalled")
                    .map((l) => (
                      <option key={l.language} value={l.language}>
                        {l.nativeName} · {l.language}
                      </option>
                    ))}
                </optgroup>
                <optgroup label="Available (downloads on select)">
                  {langs
                    .filter((l) => l.status === "uninstalled")
                    .map((l) => (
                      <option key={l.language} value={l.language}>
                        {l.nativeName} · {l.language}
                      </option>
                    ))}
                </optgroup>
              </>
            )}
          </select>
          {switchLang.isPending && (
            <span className="flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {langBusyLabel}
            </span>
          )}
          <div className="text-[0.71875rem] leading-[1.5] text-rex-text-dim">
            Core translations only — plugins and themes fetch their own packs.
            {isNetwork && " On a multisite network this switches the main site; subsites set theirs in their own admin."}
          </div>
        </div>
      </Card>

            <Card title="Core">
        <div className="flex flex-col gap-2">
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
            className={maintBtn}
            disabled={verify.isPending}
            onClick={() => {
              setVerifyOut(null);
              verify.mutate();
            }}
          >
            <Shield className="h-3.5 w-3.5" />
            {verify.isPending ? "Verifying…" : "Verify core checksums"}
          </button>
          {verifyOut && <ChecksumResult r={verifyOut} siteId={siteId} onReport={setVerifyOut} />}
          {working && <span className="text-center text-[0.75rem] text-rex-text-muted">Working…</span>}
        </div>
        {coreOut && <pre className="mt-2 whitespace-pre-wrap font-mono text-[0.71875rem] text-rex-text-muted">{coreOut}</pre>}
        <CoreVersionSwitch siteId={siteId} />
      </Card>

              </div>
      </div>
      {/* Wide tables last: Site options (two-column rows) and Cron. */}
        <OptionsCard siteId={siteId} />

              <CronCard siteId={siteId} />
      {resetOpen && <ResetSiteDialog siteId={siteId} domain={domain} onClose={() => setResetOpen(false)} />}
      {importOpen && <ImportDbDialog siteId={siteId} domain={domain} onClose={() => setImportOpen(false)} />}
    </div>
  );
}

/** Type-to-confirm database import: DESTRUCTIVE (the dump's tables overwrite
 *  existing ones), so it mirrors ResetSiteDialog — backup-first offer + typed
 *  domain confirm before the Import button arms. */
function ImportDbDialog({ siteId, domain, onClose }: { siteId: string; domain: string; onClose: () => void }) {
  const qc = useQueryClient();
  const [typed, setTyped] = useState("");
  const [file, setFile] = useState<string | null>(null);
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: "Show in Finder",
        onClick: () => void revealPath(path).catch(toastBackendError),
      }),
    onError: (e) => toastBackendError(e),
  });
  const doImport = useMutation({
    mutationFn: () => wpDbImport(siteId, file!),
    onSuccess: () => {
      // Content, users, options — everything may have changed.
      qc.invalidateQueries();
      toast.success("Database imported.");
      onClose();
    },
    onError: (e) => toastBackendError(e),
  });
  const busy = doImport.isPending;
  const match = typed === domain;
  const fileName = file?.split("/").pop() ?? null;
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={busy ? undefined : onClose}
    >
      <div
        className="w-[440px] rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-[0.9375rem] font-semibold text-rex-text">Import a database dump?</div>
        <div className="mt-2 flex flex-col gap-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
          <p>
            This runs a <span className="font-mono">.sql</span> dump against{" "}
            <span className="font-mono text-rex-text">{domain}</span>&rsquo;s database —{" "}
            <span className="font-medium text-status-error-bright">tables in the dump overwrite existing ones</span>.
            It cannot be undone.
          </p>
          <p>
            A dump from a different site will carry its old URLs — run a search-replace (Tools) afterwards if the site
            misbehaves.
          </p>
        </div>
        <button
          className={BTN + " mt-3 flex items-center gap-1.5"}
          disabled={dbExport.isPending || busy}
          onClick={() => dbExport.mutate()}
        >
          <Download className="h-3.5 w-3.5" />
          {dbExport.isPending ? "Exporting…" : "Export current database first"}
        </button>
        <button
          className={BTN + " mt-2 flex w-full items-center justify-center gap-1.5"}
          disabled={busy}
          onClick={async () => {
            const picked = await pickSqlFile("Choose a .sql dump to import");
            if (picked) setFile(picked);
          }}
        >
          <FileUp className="h-3.5 w-3.5" />
          {fileName ? `File: ${fileName}` : "Choose .sql file…"}
        </button>
        <div className="mt-4 text-[0.78125rem] text-rex-text-muted">
          Type <span className="font-mono text-rex-text">{domain}</span> to confirm:
        </div>
        <input {...TECH_INPUT}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          placeholder={domain}
          disabled={busy}
          autoFocus
          className="mt-1.5 h-[32px] w-full rounded-md border border-rex-border bg-rex-surface-2 px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-status-error-border"
        />
        <div className="mt-4 flex justify-end gap-2">
          <button className={BTN} disabled={busy} onClick={onClose}>
            Cancel
          </button>
          <button
            className={BTN + " border-status-error-border text-status-error-bright hover:bg-status-error-bg disabled:opacity-40"}
            disabled={!match || !file || busy}
            onClick={() => doImport.mutate()}
          >
            {busy ? "Importing…" : "Import & overwrite"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Checksum verdict, four states, checked in this order so real findings can
 *  never be swallowed: real issues (amber, benign collapsed to one dim line) →
 *  wp-cli itself failed with no parsed findings (e.g. checksum download
 *  failed: amber + raw output) → pass with benign OS clutter (soft green) →
 *  clean pass. NOTE: wp-cli's own exit code is 0 even with "should not exist"
 *  extras — it only fails on modified/missing core files — so `r.ok` alone
 *  must never drive the pass branch. */
function ChecksumResult({
  r,
  siteId,
  onReport,
}: {
  r: WpChecksumReport;
  siteId: string;
  onReport: (r: WpChecksumReport) => void;
}) {
  const hasReal = r.real.length > 0;
  const toolFailed = !hasReal && !r.ok;
  const pass = !hasReal && !toolFailed;
  const [lastSkipped, setLastSkipped] = useState<WpSkippedNoiseFile[]>([]);
  const cleanup = useMutation({
    // The backend re-validates every path (noise basename, inside-docroot,
    // no symlinks) — this list is a suggestion, not an instruction.
    mutationFn: () => wpChecksumCleanup(siteId, r.benign),
    onSuccess: (res) => {
      setLastSkipped(res.skipped);
      onReport(res.report); // fresh verify — clutter gone → clean pass
      toast.success(
        res.skipped.length === 0
          ? `Removed ${res.removed} system file${res.removed === 1 ? "" : "s"}.`
          : `Removed ${res.removed}, skipped ${res.skipped.length} — see the panel for reasons.`,
      );
    },
    onError: (e) => toastBackendError(e),
  });
  const askCleanup = async () => {
    const n = r.benign.length;
    if (
      await confirm({
        title: `Delete ${n} macOS system file${n === 1 ? "" : "s"}?`,
        message:
          "Harmless Finder clutter (.DS_Store etc.) inside this site's folder — macOS recreates it as needed. Files are deleted permanently (not moved to Trash), then the checksums are re-verified.",
        confirmLabel: "Delete & re-verify",
      })
    )
      cleanup.mutate();
  };
  return (
    <div
      className={cn(
        "flex flex-col gap-1.5 rounded-lg border border-rex-border-strong border-l-[3px] bg-rex-surface-2 px-3 py-2.5",
        pass ? "border-l-status-running" : "border-l-status-warning-bright",
      )}
    >
      <div className="flex items-center gap-2">
        {pass ? (
          <Check className="h-4 w-4 flex-none text-status-running" />
        ) : (
          <AlertTriangle className="h-4 w-4 flex-none text-status-warning-bright" />
        )}
        <span className="text-[0.75rem] text-rex-text-bright">
          {hasReal
            ? "Verification found modified, missing or foreign core files:"
            : toolFailed
              ? "Verification could not complete:"
              : r.benign.length > 0
                ? `Core files verify. ${r.benign.length} macOS system file(s) found — harmless, safe to ignore.`
                : "Core files verify against wordpress.org checksums."}
        </span>
      </div>
      {hasReal && (
        <pre className="max-h-[160px] overflow-y-auto whitespace-pre-wrap font-mono text-[0.6875rem] text-rex-text-muted">
          {r.real.join("\n")}
        </pre>
      )}
      {toolFailed && (
        <pre className="max-h-[160px] overflow-y-auto whitespace-pre-wrap font-mono text-[0.6875rem] text-rex-text-muted">
          {r.output}
        </pre>
      )}
      {r.benign.length > 0 && (
        <div className="font-mono text-[0.65625rem] text-rex-text-dim" title={r.benign.join("\n")}>
          {pass
            ? r.benign.join("  ·  ")
            : `+ ${r.benign.length} OS system file(s) (.DS_Store etc.) — harmless.`}
        </div>
      )}
      {r.benign.length > 0 && (
        <button
          className={cn(BTN, "flex items-center justify-center gap-1.5 self-start")}
          disabled={cleanup.isPending}
          onClick={() => void askCleanup()}
        >
          {cleanup.isPending ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Trash2 className="h-3.5 w-3.5" />
          )}
          {cleanup.isPending
            ? "Cleaning up…"
            : `Clean up ${r.benign.length} system file${r.benign.length === 1 ? "" : "s"}`}
        </button>
      )}
      {lastSkipped.length > 0 && (
        <div className="font-mono text-[0.65625rem] text-status-warning-bright">
          {lastSkipped.map((s) => (
            <div key={s.path}>
              skipped {s.path} — {s.reason}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Read-only cron schedule + a "run due now" trigger (Tools → Cron). */
function CronCard({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const { data: events, isLoading, isError, error, refetch } = useQuery({
    queryKey: ["wp-cron", siteId],
    queryFn: () => wpCronEvents(siteId),
    ...WP_QUERY,
  });

  const runDue = useMutation({
    mutationFn: () => wpCronRunDue(siteId),
    onSuccess: (msg) => {
      toast.success(msg.replace(/^Success:\s*/, "").trim() || "Due cron events executed.");
      qc.invalidateQueries({ queryKey: ["wp-cron", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const runHook = useMutation({
    mutationFn: (hook: string) => wpCronRunHook(siteId, hook),
    onSuccess: (msg) => {
      toast.success(msg.replace(/^Success:\s*/, "").trim() || "Event executed.");
      qc.invalidateQueries({ queryKey: ["wp-cron", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const busy = runDue.isPending || runHook.isPending;

  return (
    <Card title="Cron">
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-[0.75rem] text-rex-text-muted">
            {events ? `${events.length} scheduled event(s)` : "…"} — local dev has no visitors,
            so overdue events are normal; run them on demand.
          </span>
          <button
            className={BTN + " flex items-center gap-1.5"}
            disabled={busy}
            onClick={() => runDue.mutate()}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            {runDue.isPending ? "Running…" : "Run due now"}
          </button>
        </div>
        <div className="overflow-hidden rounded-lg border border-rex-border-subtle">
          {isLoading ? (
            <PanelLoading what="cron events" />
          ) : isError ? (
            <PanelError what="cron events" error={error} onRetry={refetch} />
          ) : !events || events.length === 0 ? (
            <div className="p-4 text-center text-[0.78125rem] text-rex-text-muted">
              No scheduled cron events.
            </div>
          ) : (
            <>
              <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-label">
                <span className="flex-1">Hook</span>
                <span className="w-[170px]">Next run</span>
                <span className="w-[110px]">Recurrence</span>
                <span className="w-[64px]" />
              </div>
              <div className="max-h-[300px] overflow-y-auto">
                {events.map((e, i) => (
                  <div
                    key={`${e.hook}-${i}`}
                    className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-text-bright" title={e.hook}>
                      {e.hook}
                    </span>
                    <span className="w-[170px] text-[0.75rem] text-rex-text-muted" title={`${e.nextRun} GMT`}>
                      {e.nextRunRelative || "now"}
                    </span>
                    <span className="w-[110px] text-[0.75rem] text-rex-text-muted">{e.recurrence}</span>
                    <button
                      className={BTN + " w-[64px] justify-center text-center"}
                      title={`Run ${e.hook} now (due or not)`}
                      disabled={busy}
                      onClick={() => runHook.mutate(e.hook)}
                    >
                      {runHook.isPending && runHook.variables === e.hook ? "…" : "Run"}
                    </button>
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
      </div>
    </Card>
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
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: "Show in Finder",
        onClick: () => void revealPath(path).catch(toastBackendError),
      }),
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
            <div className="text-[0.9375rem] font-semibold text-rex-text">Site reset complete</div>
            <div className="mt-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
              <span className="font-mono">{domain}</span> is a clean WordPress install again. Admin account:
            </div>
            <div className="mt-3 rounded-lg border border-rex-border-strong bg-rex-surface-2 px-3 py-2.5 font-mono text-[0.78125rem] text-rex-text-bright">
              admin / admin
            </div>
            <div className="mt-2 text-[0.75rem] text-rex-text-muted">One-click admin login keeps working too.</div>
            <div className="mt-4 flex justify-end">
              <button className={BTN} onClick={onClose}>
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="text-[0.9375rem] font-semibold text-rex-text">Erase database &amp; reset site?</div>
            <div className="mt-2 flex flex-col gap-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
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
            <div className="mt-4 text-[0.78125rem] text-rex-text-muted">
              Type <span className="font-mono text-rex-text">{domain}</span> to confirm:
            </div>
            <input {...TECH_INPUT}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={domain}
              disabled={busy}
              autoFocus
              className="mt-1.5 h-[32px] w-full rounded-md border border-rex-border bg-rex-surface-2 px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-status-error-border"
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

/** Core version switch (Core card). Picker fed by the stable-check API —
 *  never free text; the backend re-validates shape + membership on a fresh
 *  fetch and gates success on `wp core version`. The confirm carries the
 *  honest DB warning; "Export database first" sits in the same flow. */
function CoreVersionSwitch({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const { data: info } = useQuery({
    queryKey: ["wp-info", siteId],
    queryFn: () => wpInfo(siteId),
    ...WP_QUERY,
  });
  const { data: versions, isError: versionsFailed } = useQuery({
    queryKey: ["wp-core-versions"],
    queryFn: wpCoreVersions,
    ...WP_QUERY,
    staleTime: 60 * 60_000, // release list barely moves; don't refetch per visit
  });
  const [picked, setPicked] = useState("");
  const [result, setResult] = useState<WpCoreSwitch | null>(null);
  const current = info?.version ?? null;

  const exportDb = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: "Show in Finder",
        onClick: () => void revealPath(path).catch(toastBackendError),
      }),
    onError: (e) => toastBackendError(e),
  });
  const switchVersion = useMutation({
    mutationFn: (version: string) => wpCoreSwitchVersion(siteId, version),
    onSuccess: (res) => {
      setResult(res);
      setPicked("");
      qc.invalidateQueries({ queryKey: ["wp-info", siteId] });
      toast.success(`Core switched to ${res.version}.`);
    },
    onError: (e) => toastBackendError(e),
  });

  const askSwitch = async () => {
    const row = versions?.find((v) => v.version === picked);
    if (!row) return;
    if (
      await confirm({
        title: `Switch WordPress core from ${current ?? "?"} to ${row.version}?`,
        message: (
          <>
            Downgrading does <b>not</b> downgrade the database. WordPress may show a one-click
            “Database Update Required” screen — but data written by newer core, or plugins that
            require {current ?? "the current version"}, can break, and that isn't detectable in
            advance. Not guaranteed reversible without a backup — use “Export DB first”.
            {row.status === "insecure" && (
              <>
                {" "}
                <b>{row.version} has known security issues</b> — fine for local testing, don't
                expose it via a tunnel.
              </>
            )}
          </>
        ),
        confirmLabel: "Switch version",
      })
    )
      switchVersion.mutate(row.version);
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-t border-rex-border-subtle pt-2">
      <div className="text-[0.75rem] text-rex-text-muted">
        Core version{current ? <span className="font-mono"> · {current}</span> : null}
      </div>
      {versionsFailed ? (
        <div className="text-[0.71875rem] text-status-warning-bright">
          Couldn't fetch the release list — check your connection.
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <select
            value={picked}
            disabled={!versions || switchVersion.isPending}
            onChange={(e) => setPicked(e.target.value)}
            className={cn(OPTION_SELECT, "min-w-0 flex-1")}
          >
            <option value="">{versions ? "Pick a version…" : "Loading releases…"}</option>
            {versions?.map((v) => (
              <option key={v.version} value={v.version} disabled={v.version === current}>
                {v.version}
                {v.version === current
                  ? " — current"
                  : v.status === "latest"
                    ? " — latest"
                    : v.status === "insecure"
                      ? " — insecure"
                      : ""}
              </option>
            ))}
          </select>
          <button
            className={BTN}
            disabled={exportDb.isPending || switchVersion.isPending}
            onClick={() => exportDb.mutate()}
            title="Escape hatch: export the database before switching"
          >
            {exportDb.isPending ? "Exporting…" : "Export DB first"}
          </button>
          <button
            className={BTN}
            disabled={!picked || switchVersion.isPending}
            onClick={() => void askSwitch()}
          >
            {switchVersion.isPending ? (
              <span className="flex items-center gap-1.5">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                Switching…
              </span>
            ) : (
              "Switch version"
            )}
          </button>
        </div>
      )}
      {result && (
        <div className="text-[0.71875rem] text-rex-text-dim">
          Now on <span className="font-mono">{result.version}</span>.{" "}
          {result.dbUpdateRequired
            ? "WordPress will ask to update the database on the next wp-admin visit (normally one click)."
            : "No database update needed."}
        </div>
      )}
    </div>
  );
}

const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

const OPTION_SELECT =
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50";

/** Client-side pre-validation (Save disabled + hint) — the backend re-validates
 *  every write regardless. */
function optionDraftProblem(row: WpOptionRow, val: string): string | null {
  if (row.kind === "int" || row.kind === "weekday") {
    const n = Number(val);
    if (!/^-?\d+$/.test(val.trim()) || (row.min != null && n < row.min) || (row.max != null && n > row.max))
      return `whole number ${row.min}–${row.max}`;
  }
  if (row.kind === "email" && !/^\S+@\S+\.\S+$/.test(val)) return "not a valid email";
  return null;
}

/** Curated site-options editor (Tools). Only the backend's scalar whitelist is
 *  reachable — dangerous options (siteurl, home, active_plugins…) don't exist
 *  in this API. Saves confirm old → new, then the form re-reads from the site
 *  (never trusts its own draft). */
function OptionsCard({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["wp-options", siteId],
    queryFn: () => wpOptions(siteId),
    ...WP_QUERY,
  });
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const save = useMutation({
    mutationFn: (v: { name: string; value: string; label: string }) =>
      wpOptionUpdate(siteId, v.name, v.value),
    onSuccess: (_d, v) => {
      setDrafts((d) => {
        const next = { ...d };
        delete next[v.name];
        return next;
      });
      qc.invalidateQueries({ queryKey: ["wp-options", siteId] });
      toast.success(`${v.label} updated.`);
    },
    onError: (e) => toastBackendError(e),
  });
  const askSave = async (row: WpOptionRow, value: string) => {
    if (
      await confirm({
        title: `Change ${row.label}?`,
        message: (
          <span className="font-mono text-[0.75rem]">
            “{row.value}” → “{value}”
          </span>
        ),
        confirmLabel: "Save",
      })
    )
      save.mutate({ name: row.name, value, label: row.label });
  };

  return (
    <Card title="Site options">
      {isLoading ? (
        <div className="text-[0.78125rem] text-rex-text-dim">Reading options…</div>
      ) : isError ? (
        <div className="text-[0.78125rem] text-status-warning-bright">
          Could not read options: {String(error)}
        </div>
      ) : !data ? null : (
        // Two columns of label+control rows — a single ~400px-wide column in a
        // full-width card left the whole right half empty (QA P3-1).
        <div className="grid grid-cols-2 gap-x-8 gap-y-2">
          {data.fields.map((row) => {
            const draft = drafts[row.name] ?? row.value;
            const dirty = draft !== row.value;
            const problem = dirty ? optionDraftProblem(row, draft) : null;
            const setDraft = (v: string) => setDrafts((d) => ({ ...d, [row.name]: v }));
            const disabled = !row.editable || save.isPending;
            return (
              <div key={row.name} className="flex min-w-0 items-center gap-2">
                <div className="w-[150px] flex-none text-[0.75rem] text-rex-text-muted">
                  {row.label}
                </div>
                {row.kind === "bool" ? (
                  <select value={draft} disabled={disabled} onChange={(e) => setDraft(e.target.value)} className={cn(OPTION_SELECT, "w-[220px]")}>
                    <option value="1">Yes</option>
                    <option value="0">No</option>
                  </select>
                ) : row.kind === "weekday" ? (
                  <select value={draft} disabled={disabled} onChange={(e) => setDraft(e.target.value)} className={cn(OPTION_SELECT, "w-[220px]")}>
                    {WEEKDAYS.map((d, i) => (
                      <option key={d} value={String(i)}>{d}</option>
                    ))}
                  </select>
                ) : row.kind === "timezone" ? (
                  <select value={draft} disabled={disabled} onChange={(e) => setDraft(e.target.value)} className={cn(OPTION_SELECT, "w-[220px]")}>
                    <option value="">None (UTC offset)</option>
                    {data.timezones.map((t) => (
                      <option key={t} value={t}>{t}</option>
                    ))}
                  </select>
                ) : row.kind === "role" ? (
                  <select value={draft} disabled={disabled} onChange={(e) => setDraft(e.target.value)} className={cn(OPTION_SELECT, "w-[220px]")}>
                    {data.roles.map((r) => (
                      <option key={r.role} value={r.role}>{r.name}</option>
                    ))}
                  </select>
                ) : (
                  <input {...TECH_INPUT}
                    value={draft}
                    disabled={disabled}
                    type={row.kind === "int" ? "number" : "text"}
                    min={row.min ?? undefined}
                    max={row.max ?? undefined}
                    onChange={(e) => setDraft(e.target.value)}
                    className={cn(OPTION_SELECT, "w-[220px] font-mono")}
                  />
                )}
                {!row.editable && row.note && (
                  <span className="min-w-0 truncate text-[0.71875rem] text-rex-text-dim" title={row.note}>
                    {row.note}
                  </span>
                )}
                {dirty && problem && (
                  <span className="min-w-0 truncate text-[0.71875rem] text-status-warning-bright" title={problem}>
                    {problem}
                  </span>
                )}
                {dirty && !problem && (
                  <button
                    className={BTN}
                    disabled={save.isPending}
                    onClick={() => void askSave(row, draft)}
                  >
                    {save.isPending ? "Saving…" : "Save"}
                  </button>
                )}
                {dirty && (
                  <button
                    className="text-[0.71875rem] text-rex-text-dim hover:underline"
                    onClick={() => setDrafts((d) => {
                      const next = { ...d };
                      delete next[row.name];
                      return next;
                    })}
                  >
                    Reset
                  </button>
                )}
              </div>
            );
          })}
          <div className="mt-1 text-[0.71875rem] text-rex-text-dim">
            Only this curated, known-safe set is editable — site URLs, plugin/theme state and
            serialized options can't be changed here. Saves apply immediately (no undo).
          </div>
        </div>
      )}
    </Card>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="mb-3 text-[0.8125rem] font-semibold text-rex-text">{title}</div>
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

function UsersPanel({ siteId, domain }: { siteId: string; domain: string }) {
  const qc = useQueryClient();
  const [login, setLogin] = useState("");
  const [email, setEmail] = useState("");
  // Email auto-follows the username (<login>@<domain>) until the user edits it.
  const [emailTouched, setEmailTouched] = useState(false);
  // Local-dev default password, VISIBLE by default — it's a throwaway on a
  // local site, and seeing it beats a mystery masked value.
  const [password, setPassword] = useState("123456");
  const [showPw, setShowPw] = useState(true);
  const [role, setRole] = useState("subscriber");
  const [pwFor, setPwFor] = useState<WpUser | null>(null);

  const { data: users = [], isLoading, isError, error, refetch } = useWpUsers(siteId);

  const create = useMutation({
    mutationFn: () => wpUserCreate(siteId, login.trim(), email.trim(), role, password),
    onSuccess: () => {
      setLogin("");
      setEmail("");
      setEmailTouched(false);
      setPassword("123456");
      qc.invalidateQueries({ queryKey: ["wp-users", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  const setUserPassword = useMutation({
    mutationFn: ({ userId, pw }: { userId: number; pw: string }) =>
      wpUserSetPassword(siteId, userId, pw),
    onSuccess: () => toast.success("Password changed."),
    onError: (e) => toastBackendError(e),
  });

  const loginAs = useMutation({
    mutationFn: (userId: number) => wpUserLoginUrl(siteId, userId),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toastBackendError(e),
  });

  const { data: primaryAdmin } = useQuery({
    queryKey: ["wp-primary-admin", siteId],
    queryFn: () => wpPrimaryAdmin(siteId),
    ...WP_QUERY,
  });

  const changeRole = useMutation({
    mutationFn: ({ userId, role: r }: { userId: number; role: string }) =>
      wpUserSetRole(siteId, userId, r),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["wp-users", siteId] });
      toast.success("Role updated.");
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex flex-col gap-3">
      {/* Add user */}
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={login}
          onChange={(e) => {
            const v = e.target.value;
            setLogin(v);
            // Follow the username until the email is hand-edited.
            if (!emailTouched) setEmail(v.trim() ? `${v.trim()}@${domain}` : "");
          }}
          placeholder="username"
          className="h-[30px] w-32 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        <input {...TECH_INPUT}
          value={email}
          onChange={(e) => {
            setEmail(e.target.value);
            setEmailTouched(true);
          }}
          placeholder={`email@${domain}`}
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        <span className="relative">
          <input {...TECH_INPUT}
            type={showPw ? "text" : "password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="password"
            title="Password for the new user (local default: 123456)"
            className="h-[30px] w-[110px] rounded border border-rex-border bg-rex-surface-2 py-0 pl-2 pr-7 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
          />
          <button
            type="button"
            onClick={() => setShowPw((v) => !v)}
            title={showPw ? "Hide password" : "Show password"}
            className="absolute right-1.5 top-1/2 -translate-y-1/2 text-rex-text-muted transition-colors hover:text-rex-text-bright"
          >
            {showPw ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
          </button>
        </span>
        <select
          value={role}
          onChange={(e) => setRole(e.target.value)}
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.75rem] text-rex-text outline-none focus:border-brand"
        >
          {WP_ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={create.isPending || !login.trim() || !email.trim() || !password}
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
          <div className="p-6 text-center text-[0.78125rem] text-rex-text-muted">No users.</div>
        ) : (
          <>
            <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-label">
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
                primary={u.id === primaryAdmin}
                roleBusy={changeRole.isPending}
                onSetRole={(r) => changeRole.mutate({ userId: u.id, role: r })}
                onChangePassword={() => setPwFor(u)}
              />
            ))}
          </>
        )}
      </div>
      {pwFor && (
        <PromptDialog
          title={`Change password — ${pwFor.login}`}
          label="New password"
          initialValue="123456"
          mono
          submitLabel="Change password"
          onSubmit={(v) => {
            setUserPassword.mutate({ userId: pwFor.id, pw: v });
            setPwFor(null);
          }}
          onCancel={() => setPwFor(null)}
        />
      )}
    </div>
  );
}

function UserRow({
  u,
  busy,
  onLoginAs,
  primary,
  roleBusy,
  onSetRole,
  onChangePassword,
}: {
  u: WpUser;
  busy: boolean;
  onLoginAs: () => void;
  /** The primary administrator — its role is locked (backend refuses too). */
  primary: boolean;
  roleBusy: boolean;
  onSetRole: (role: string) => void;
  onChangePassword: () => void;
}) {
  const role = u.roles.split(",")[0]?.trim() || "";
  const rm = roleMeta(role);
  const initial = (u.name || u.login).trim().charAt(0).toUpperCase() || "?";
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2.5 last:border-b-0">
      <span
        className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border text-[0.75rem] font-semibold"
        style={{ background: rm.bg, color: rm.color, borderColor: rm.border }}
      >
        {initial}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[0.8125rem] font-medium text-rex-text">{u.login}</div>
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-dim">{u.email}</div>
      </div>
      <span className="w-[120px]">
        {primary ? (
          <span
            title="Primary administrator — role locked (one-click login and site tools depend on it)"
            className="inline-flex cursor-help items-center gap-1 rounded-full border px-2 py-0.5 text-[0.65625rem] font-medium capitalize"
            style={{ background: rm.bg, color: rm.color, borderColor: rm.border }}
          >
            {role}
            <Lock className="h-2.5 w-2.5" strokeWidth={2.2} />
          </span>
        ) : (
          <select
            value={role}
            disabled={roleBusy}
            onChange={(e) => onSetRole(e.target.value)}
            className="h-[26px] w-full rounded border border-rex-border bg-rex-surface-2 px-1.5 text-[0.71875rem] capitalize text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
          >
            {!WP_ROLES.includes(role) && role && <option value={role}>{role}</option>}
            {WP_ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
        )}
      </span>
      <button
        className={BTN}
        title={`Change ${u.login}'s password`}
        onClick={onChangePassword}
      >
        <KeyRound className="h-3.5 w-3.5" />
      </button>
      <button className={BTN + " flex w-[84px] items-center justify-center gap-1.5"} disabled={busy} onClick={onLoginAs}>
        <LogIn className="h-3.5 w-3.5" />
        Log in
      </button>
    </div>
  );
}

/** One wp.org theme search hit (Add-theme dropdown row). */
function WpOrgThemeHit({ t, onPick }: { t: WpOrgTheme; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={onPick}
      className="flex w-full items-center gap-2.5 border-b border-rex-border-subtle px-2.5 py-2 text-left transition-colors last:border-b-0 hover:bg-rex-surface-2"
    >
      {t.screenshot ? (
        <img src={t.screenshot} alt="" loading="lazy" className="h-9 w-12 flex-none rounded-[5px] border border-rex-border-subtle object-cover" />
      ) : (
        <span className="flex h-9 w-12 flex-none items-center justify-center rounded-[5px] border border-rex-border bg-rex-surface-2 text-rex-text-dim">
          <Palette className="h-4 w-4" strokeWidth={1.5} />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[0.78125rem] font-medium text-rex-text">{t.name}</span>
        <span className="block truncate text-[0.6875rem] text-rex-text-dim">
          {t.author && `by ${t.author} · `}
          <span className="font-mono">{t.slug}</span>
        </span>
      </span>
      <span className="flex flex-none items-center gap-2 font-mono text-[0.65625rem] text-rex-text-dim">
        {t.rating > 0 && (
          <span className="flex items-center gap-0.5">
            <Star className="h-3 w-3 fill-current text-amber-400" />
            {(t.rating / 20).toFixed(1)}
          </span>
        )}
        <span>{fmtInstalls(t.activeInstalls)}</span>
      </span>
    </button>
  );
}

function ThemesPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(false);
  // wp.org live search — same pattern as PluginsPanel (debounce, fail fast,
  // manual slug always works).
  const [picked, setPicked] = useState(false);
  const debouncedSlug = useDebounced(slug.trim(), 350);
  const search = useQuery({
    queryKey: ["wporg-themes", debouncedSlug],
    queryFn: () => wpOrgSearchThemes(debouncedSlug),
    enabled: !picked && debouncedSlug.length >= 2,
    staleTime: 60_000,
    retry: false,
  });
  const showSearch = !picked && slug.trim().length >= 2;

  const { themes, isLoading, isError, error, refetch } = useWpThemes(siteId);

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-themes", siteId] }),
    onError: (e) => toastBackendError(e),
  });
  const busy = run.isPending;

  return (
    <div className="flex flex-col gap-3">
      <div className="relative flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={slug}
          onChange={(e) => {
            setSlug(e.target.value);
            setPicked(false);
          }}
          placeholder="Search WordPress.org or enter a slug…"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        {showSearch && (
          <div className="absolute left-2.5 right-2.5 top-[46px] z-20 overflow-hidden rounded-lg border border-rex-border-strong bg-rex-surface-1 shadow-xl">
            {search.isLoading ? (
              <div className="flex items-center gap-2 px-3 py-2.5 text-[0.75rem] text-rex-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Searching WordPress.org…
              </div>
            ) : search.isError ? (
              <div className="px-3 py-2.5 text-[0.75rem] text-status-error-bright">
                {String(search.error)} — you can still enter the theme slug manually.
              </div>
            ) : (search.data ?? []).length === 0 ? (
              <div className="px-3 py-2.5 text-[0.75rem] text-rex-text-muted">
                No themes match “{slug.trim()}” — if you know the exact slug, just Add it.
              </div>
            ) : (
              <div className="max-h-[300px] overflow-y-auto">
                {(search.data ?? []).map((t) => (
                  <WpOrgThemeHit
                    key={t.slug}
                    t={t}
                    onPick={() => {
                      setSlug(t.slug);
                      setPicked(true);
                    }}
                  />
                ))}
              </div>
            )}
          </div>
        )}
        <label className="flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
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
        <div className="px-0.5 font-mono text-[0.6875rem] text-rex-text-dim">
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
        <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center text-[0.78125rem] text-rex-text-muted">
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
          <span className="absolute right-2 top-2 flex items-center gap-1 rounded-full bg-status-running-bg px-2 py-0.5 text-[0.625rem] font-medium text-status-running-bright">
            <span className="h-1.5 w-1.5 rounded-full bg-status-running" />
            Live
          </span>
        )}
      </div>
      <div className="flex flex-1 flex-col gap-2 p-3">
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-[0.8125rem] font-medium text-rex-text">{t.name}</span>
          {active && (
            <span className="flex items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[0.625rem] font-medium text-emerald-400">
              <Check className="h-3 w-3" />
              Active
            </span>
          )}
          {updatable && !active && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[0.625rem] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[0.6875rem] text-rex-text-dim">v{t.version}</div>
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
            "flex h-7 items-center gap-1.5 rounded-[7px] px-[11px] text-[0.78125rem] font-medium transition-colors",
            value === t.key
              ? "bg-brand-tint-bg text-brand-tint"
              : "text-rex-text-muted hover:text-rex-text-bright",
          )}
        >
          {t.label}
          {counts[t.key] > 0 && (
            <span
              className={cn(
                "rounded-[5px] px-1 font-mono text-[0.625rem]",
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

/** Debounce a fast-changing value (live directory search). */
function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** "5M+" / "300K+" active-install shorthand (wp.org rounds these anyway). */
function fmtInstalls(n: number): string {
  if (n >= 1_000_000) return `${Math.round(n / 1_000_000)}M+`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K+`;
  return `${n}`;
}

/** One wp.org search hit (Add-plugin dropdown row). */
function WpOrgHit({ p, onPick }: { p: WpOrgPlugin; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={onPick}
      className="flex w-full items-center gap-2.5 border-b border-rex-border-subtle px-2.5 py-2 text-left transition-colors last:border-b-0 hover:bg-rex-surface-2"
    >
      {p.icon ? (
        <img src={p.icon} alt="" className="h-7 w-7 flex-none rounded-[6px] object-cover" />
      ) : (
        <span className="flex h-7 w-7 flex-none items-center justify-center rounded-[6px] border border-rex-border bg-rex-surface-2 font-mono text-[0.6875rem] font-bold text-rex-text-muted">
          {p.name.slice(0, 1).toUpperCase() || "?"}
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[0.78125rem] font-medium text-rex-text">{p.name}</span>
        <span className="block truncate text-[0.6875rem] text-rex-text-dim">
          {p.author && `by ${p.author} · `}
          <span className="font-mono">{p.slug}</span>
        </span>
      </span>
      <span className="flex flex-none items-center gap-2 font-mono text-[0.65625rem] text-rex-text-dim">
        {p.rating > 0 && (
          <span className="flex items-center gap-0.5">
            <Star className="h-3 w-3 fill-current text-amber-400" />
            {(p.rating / 20).toFixed(1)}
          </span>
        )}
        <span>{fmtInstalls(p.activeInstalls)}</span>
      </span>
    </button>
  );
}

function PluginsPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<PluginFilter>("all");
  // wp.org live search: the slug input doubles as the search box (wp-admin
  // style). Picking a hit fills the slug; typing manually still works as-is.
  const [picked, setPicked] = useState(false);
  const debouncedSlug = useDebounced(slug.trim(), 350);
  const search = useQuery({
    queryKey: ["wporg-plugins", debouncedSlug],
    queryFn: () => wpOrgSearchPlugins(debouncedSlug),
    enabled: !picked && debouncedSlug.length >= 2,
    staleTime: 60_000,
    retry: false, // offline → fail fast + honest message, no retry spinner
  });
  const showSearch = !picked && slug.trim().length >= 2;

  const { plugins, isLoading, isError, error, refetch } = useWpPlugins(siteId);

  // Must-use plugins / drop-ins are always loaded — they belong under "Active".
  const isActive = (p: WpPlugin) =>
    p.status === "active" ||
    p.status === "active-network" ||
    p.status === "must-use" ||
    p.status === "dropin";
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
      {/* Add: live wp.org search that fills the slug (manual slug still works) */}
      <div className="relative flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input {...TECH_INPUT}
          value={slug}
          onChange={(e) => {
            setSlug(e.target.value);
            setPicked(false);
          }}
          placeholder="Search WordPress.org or enter a slug…"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        {showSearch && (
          <div className="absolute left-2.5 right-2.5 top-[46px] z-20 overflow-hidden rounded-lg border border-rex-border-strong bg-rex-surface-1 shadow-xl">
            {search.isLoading ? (
              <div className="flex items-center gap-2 px-3 py-2.5 text-[0.75rem] text-rex-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-rex-spin" /> Searching WordPress.org…
              </div>
            ) : search.isError ? (
              <div className="px-3 py-2.5 text-[0.75rem] text-status-error-bright">
                {String(search.error)} — you can still enter the plugin slug manually.
              </div>
            ) : (search.data ?? []).length === 0 ? (
              <div className="px-3 py-2.5 text-[0.75rem] text-rex-text-muted">
                No plugins match “{slug.trim()}” — if you know the exact slug, just Add it.
              </div>
            ) : (
              <div className="max-h-[300px] overflow-y-auto">
                {(search.data ?? []).map((p) => (
                  <WpOrgHit
                    key={p.slug}
                    p={p}
                    onPick={() => {
                      setSlug(p.slug);
                      setPicked(true);
                    }}
                  />
                ))}
              </div>
            )}
          </div>
        )}
        <label className="flex items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
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
            className="h-[30px] w-full rounded-lg border border-rex-border bg-rex-surface-2 pl-8 pr-2.5 text-[0.75rem] text-rex-text outline-none transition-colors focus:border-brand"
          />
        </div>
        <PluginFilterTabs value={filter} onChange={setFilter} counts={counts} />
      </div>

      {/* Bulk bar */}
      {selNames.length > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-brand/40 bg-rex-surface-1 p-2.5 text-[0.75rem]">
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
          <div className="p-6 text-center text-[0.78125rem] text-rex-text-muted">
            {plugins.length === 0 ? "No plugins installed." : "No plugins match."}
          </div>
        ) : (
          <>
          <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-label">
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
  // Must-use plugins and drop-ins load automatically by their location on disk
  // — WordPress has no activate/deactivate (or wp-cli delete) for them, so
  // offering those controls would be a lie. Lock them with an explanation.
  const immutable = p.status === "must-use" || p.status === "dropin";
  const immutableWhy =
    p.status === "must-use"
      ? "Must-use plugin — loads automatically from wp-content/mu-plugins and is always active. Remove its file to disable it."
      : "Drop-in — loads automatically from wp-content and can't be toggled here. Remove its file to disable it.";
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2.5 last:border-b-0">
      <input
        type="checkbox"
        checked={selected}
        onChange={onSelect}
        disabled={immutable}
        title={immutable ? immutableWhy : undefined}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[0.8125rem] font-medium text-rex-text">{p.name}</span>
          {updatable && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[0.625rem] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[0.6875rem] text-rex-text-dim">v{p.version}</div>
      </div>
      {updatable && (
        <button className={BTN + " flex items-center gap-1"} disabled={busy} onClick={onUpdate} title="Update">
          <ArrowUpCircle className="h-3.5 w-3.5" />
        </button>
      )}
      <span
        className={cn(
          "w-[58px] text-right text-[0.71875rem] font-medium",
          active || immutable ? "text-status-running-bright" : "text-rex-text-muted",
        )}
        title={immutable ? immutableWhy : undefined}
      >
        {immutable ? (p.status === "must-use" ? "Must-use" : "Drop-in") : active ? "Active" : "Inactive"}
      </span>
      <StartStopToggle
        running={active || immutable}
        disabled={immutable}
        title={immutable ? immutableWhy : undefined}
        onToggle={active ? onDeactivate : onActivate}
        label={
          immutable
            ? `${p.name} is always active`
            : `${active ? "Deactivate" : "Activate"} ${p.name}`
        }
      />
      <button
        className={cn(
          BTN,
          immutable
            ? "cursor-not-allowed opacity-45"
            : "hover:border-red-500/60 hover:text-red-400",
        )}
        disabled={busy || immutable}
        onClick={onDelete}
        title={immutable ? immutableWhy : "Delete"}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
