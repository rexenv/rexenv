import { useState } from "react";
import { toast } from "@/lib/toast";
import { confirm } from "@/components/ui/dialog";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpCircle, Check, Download, ExternalLink, Globe, LogIn, Network, Palette, Plus, RefreshCw, Replace, RotateCcw, Search, Shield, Trash2, UserPlus } from "lucide-react";
import { cn } from "@/lib/utils";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import {
  openExternal,
  wpCoreReinstall,
  wpCoreUpdate,
  wpDebugGet,
  wpDebugSet,
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

const WP_ROLES = ["subscriber", "contributor", "author", "editor", "administrator"];

// Per-role accent (Administrator violet, Editor blue, others teal/neutral).
const ROLE_META: Record<string, { color: string; bg: string; border: string }> = {
  administrator: { color: "#C9BCFF", bg: "rgba(124,92,255,0.14)", border: "rgba(124,92,255,0.28)" },
  editor: { color: "#7DB8D8", bg: "rgba(74,134,170,0.15)", border: "rgba(74,134,170,0.30)" },
  author: { color: "#5FBFA8", bg: "rgba(45,156,143,0.13)", border: "rgba(45,156,143,0.28)" },
};
const DEFAULT_ROLE = { color: "#8A90A0", bg: "rgba(110,118,129,0.13)", border: "rgba(110,118,129,0.22)" };
const roleMeta = (role: string) => ROLE_META[role] ?? DEFAULT_ROLE;

type SubTab = "plugins" | "themes" | "users" | "network" | "tools";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[12px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

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
  const { data: plugins = [] } = useQuery({ queryKey: ["wp-plugins", siteId], queryFn: () => wpPlugins(siteId) });
  const { data: themes = [] } = useQuery({ queryKey: ["wp-themes", siteId], queryFn: () => wpThemes(siteId) });
  const { data: users = [] } = useQuery({ queryKey: ["wp-users", siteId], queryFn: () => wpUsers(siteId) });
  const { data: netSites = [] } = useQuery({
    queryKey: ["wp-network-sites", siteId],
    queryFn: () => wpNetworkSites(siteId),
    enabled: isNetwork,
  });

  const subs: { key: SubTab; label: string; count?: number }[] = [
    { key: "plugins", label: "Plugins", count: plugins.length },
    { key: "themes", label: "Themes", count: themes.length },
    { key: "users", label: "Users", count: users.length },
    { key: "tools", label: "Tools" },
    ...(isNetwork ? [{ key: "network" as const, label: "Network", count: netSites.length }] : []),
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
      {sub === "network" && isNetwork && (
        <NetworkPanel siteId={siteId} mode={multisite} domain={domain} />
      )}
      {sub === "tools" && <ToolsPanel siteId={siteId} />}
    </>
  );
}

function NetworkPanel({ siteId, mode, domain }: { siteId: string; mode: MultisiteMode; domain: string }) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [admin, setAdmin] = useState("");

  const { data: sites = [], isLoading } = useQuery({
    queryKey: ["wp-network-sites", siteId],
    queryFn: () => wpNetworkSites(siteId),
  });
  const { data: plugins = [] } = useQuery({
    queryKey: ["wp-plugins", siteId],
    queryFn: () => wpPlugins(siteId),
  });
  const { data: supers = [] } = useQuery({
    queryKey: ["wp-super-admins", siteId],
    queryFn: () => wpSuperAdmins(siteId),
  });

  const sitesRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-network-sites", siteId] }),
    onError: (e) => toast.error(String(e)),
  });
  const pluginRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] }),
    onError: (e) => toast.error(String(e)),
  });
  const superRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => {
      setAdmin("");
      qc.invalidateQueries({ queryKey: ["wp-super-admins", siteId] });
    },
    onError: (e) => toast.error(String(e)),
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
          <input
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
          <div className="py-4 text-center text-[12.5px] text-rex-text-muted">Loading sub-sites…</div>
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
                <IconBtn title="Admin" onClick={() => openExternal(`${s.url.replace(/\/$/, "")}/wp-admin`)}>
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
          <input
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

function ToolsPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [dryRun, setDryRun] = useState(true);
  const [srResult, setSrResult] = useState<string | null>(null);
  const [coreOut, setCoreOut] = useState<string | null>(null);

  const { data: wpDebug } = useQuery({
    queryKey: ["wp-debug", siteId],
    queryFn: () => wpDebugGet(siteId),
  });

  const toggleDebug = useMutation({
    mutationFn: (on: boolean) => wpDebugSet(siteId, on),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-debug", siteId] }),
    onError: (e) => toast.error(String(e)),
  });

  const searchReplace = useMutation({
    mutationFn: () => wpSearchReplace(siteId, from.trim(), to.trim(), dryRun),
    onSuccess: (n) =>
      setSrResult(dryRun ? `${n} row(s) would change (dry run — nothing modified)` : `${n} row(s) changed`),
    onError: (e) => toast.error(String(e)),
  });

  const flush = useMutation({
    mutationFn: () => wpRewriteFlush(siteId),
    onSuccess: () => toast.success("Permalinks regenerated."),
    onError: (e) => toast.error(String(e)),
  });

  const coreUpdate = useMutation({
    mutationFn: () => wpCoreUpdate(siteId),
    onSuccess: (out) => setCoreOut(out),
    onError: (e) => toast.error(String(e)),
  });
  const coreReinstall = useMutation({
    mutationFn: () => wpCoreReinstall(siteId),
    onSuccess: (out) => setCoreOut(out),
    onError: (e) => toast.error(String(e)),
  });
  const adminLogin = useMutation({
    mutationFn: () => wpUserLoginUrl(siteId, 1),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toast.error(String(e)),
  });
  const working = coreUpdate.isPending || coreReinstall.isPending;
  // No backend yet for DB export / full reset — UI shells.
  const todo = (what: string) => toast.info(`${what} isn't wired yet (UI only).`);
  const maintBtn = BTN + " flex w-full items-center justify-center gap-1.5";

  return (
    <div className="grid grid-cols-2 gap-3">
      {/* Search & replace — spans the row */}
      <div className="col-span-2">
        <Card title="Search & replace">
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <input
                value={from}
                onChange={(e) => setFrom(e.target.value)}
                placeholder="old (e.g. old.test)"
                className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
              />
              <span className="text-rex-text-muted">→</span>
              <input
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
          <button className={maintBtn} onClick={() => todo("Database export")}>
            <Download className="h-3.5 w-3.5" />
            Export database
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
            onClick={async () => {
              if (await confirm({ title: "Reset site?", message: "Reset this site to a clean WordPress install? This erases its content.", danger: true, confirmLabel: "Reset" }))
                todo("Reset site");
            }}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Reset site to a clean install
          </button>
          {working && <span className="text-center text-[12px] text-rex-text-muted">Working…</span>}
        </div>
        {coreOut && <pre className="mt-2 whitespace-pre-wrap font-mono text-[11.5px] text-rex-text-muted">{coreOut}</pre>}
      </Card>
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

  const { data: users = [], isLoading } = useQuery({
    queryKey: ["wp-users", siteId],
    queryFn: () => wpUsers(siteId),
  });

  const create = useMutation({
    mutationFn: () => wpUserCreate(siteId, login.trim(), email.trim(), role),
    onSuccess: () => {
      setLogin("");
      setEmail("");
      qc.invalidateQueries({ queryKey: ["wp-users", siteId] });
    },
    onError: (e) => toast.error(String(e)),
  });

  const loginAs = useMutation({
    mutationFn: (userId: number) => wpUserLoginUrl(siteId, userId),
    onSuccess: (url) => openExternal(url),
    onError: (e) => toast.error(String(e)),
  });

  return (
    <div className="flex flex-col gap-3">
      {/* Add user */}
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input
          value={login}
          onChange={(e) => setLogin(e.target.value)}
          placeholder="username"
          className="h-[30px] w-32 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <input
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
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">Loading users…</div>
        ) : users.length === 0 ? (
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">No users.</div>
        ) : (
          <>
            <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[10px] uppercase tracking-[0.1em] text-rex-text-label">
              <span className="flex-1">User</span>
              <span className="w-[120px]">Role</span>
              <span className="w-[100px]">Last login</span>
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
      {/* Last-login isn't in the WpUser DTO yet (TODO). */}
      <span className="w-[100px] font-mono text-[11px] text-rex-text-dim">—</span>
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

  const { data: themes = [], isLoading } = useQuery({
    queryKey: ["wp-themes", siteId],
    queryFn: () => wpThemes(siteId),
  });

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-themes", siteId] }),
    onError: (e) => toast.error(String(e)),
  });
  const busy = run.isPending;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input
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
        <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center text-[12.5px] text-rex-text-muted">
          Loading themes…
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
      <div className="relative flex aspect-[4/3] items-center justify-center rounded-t-xl bg-gradient-to-br from-rex-surface-3 to-rex-surface-1 text-rex-text-dim">
        <Palette className="h-7 w-7" strokeWidth={1.4} />
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

  const { data: plugins = [], isLoading } = useQuery({
    queryKey: ["wp-plugins", siteId],
    queryFn: () => wpPlugins(siteId),
  });

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
    onError: (e) => toast.error(String(e)),
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
        <input
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
          <input
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
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">Loading plugins…</div>
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
