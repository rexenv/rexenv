import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast, toastBackendError } from "@/lib/toast";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { confirm, PromptDialog } from "@/components/ui/dialog";
import { Menu } from "@/components/ui/menu";
import { BROWSER_MENU_WIDTH, PreferredBrowserIcon, useBrowserMenu, useTerminalMenu } from "@/components/ui/open-in";
import { WordPressIcon } from "@/components/common/WordPressIcon";
import { SplitButton } from "@/components/ui/split-button";
import { confirmPhraseMatches, TypeToConfirm } from "@/components/ui/type-to-confirm";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ArrowUpCircle, Check, ChevronDown, Download, FileUp, Loader2, Lock, Network, Palette, Plus, RefreshCw, Replace, RotateCcw, Eye, EyeOff, KeyRound, Search, Shield, Star, TerminalSquare, Trash2, UserPlus, X } from "lucide-react";
import { CHECK_INPUT, cn, TECH_INPUT } from "@/lib/utils";
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
  wpAdminLoginUrl,
  wpPluginActivateNetwork,
  wpPluginDeactivateNetwork,
  wpRewriteFlush,
  wpSearchReplace,
  wpSwitchLanguage,
  wpPluginActivate,
  wpPluginDeactivate,
  wpPluginDelete,
  onWpInstallOutput,
  onWpInstallState,
  tailLog,
  wpInstallActive,
  wpInstallCancel,
  wpInstallJob,
  wpOrgPluginIcons,
  wpOrgSearchPlugins,
  wpOrgSearchThemes,
  wpPluginUpdate,
  onWpUpdate,
  wpPlugins,
  wpPrimaryAdmin,
  wpSuperAdminAdd,
  wpSuperAdmins,
  wpThemeActivate,
  wpThemeDisableNetwork,
  wpThemeEnableNetwork,
  wpThemesNetworkEnabled,
  wpThemeDelete,
  repoAdopt,
  repoAssetStatus,
  repoAssets,
  repoSiteJobs,
  repoUnmanaged,
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
import { GitAddPanel } from "./GitAddPanel";
import { RepoPanel } from "./RepoPanel";
import { LinkFolderPanel } from "./LinkFolderPanel";
import { ZipAddPanel } from "./ZipAddPanel";
import type { MultisiteMode, WpChecksumReport, WpCoreSwitch, WpInstallState, WpOptionRow, WpOrgPlugin, WpOrgTheme, WpPlugin, WpSkippedNoiseFile, WpTheme, WpUpdateProgress, WpUser } from "@/types";
import { WpInstallCard, installLabels } from "./WpInstallCard";
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

/** The terminal control on a plugin/theme row: the icon opens rexenv's built-in
 *  Terminal tab in that folder, the chevron beside it opens the SAME folder in
 *  one of the user's own terminal apps. The chevron disappears when the machine
 *  has no terminal we can drive (never on macOS — Terminal.app is not
 *  removable), leaving the plain button the row always had. */
function AssetTerminalButton({
  siteId,
  kind,
  name,
  onTerminal,
}: {
  siteId: string;
  kind: "plugin" | "theme";
  /** The asset's FOLDER name — the same slug the backend resolves the directory
   *  from, not the display title. */
  name: string;
  onTerminal: () => void;
}) {
  const menu = useTerminalMenu(siteId, { kind, name });
  const label = `Open a terminal in ${name}'s folder`;
  if (!menu) {
    return (
      <button className={BTN} onClick={onTerminal} title={label} aria-label={label}>
        <TerminalSquare className="h-3.5 w-3.5" />
      </button>
    );
  }
  return (
    <div className="flex items-stretch rounded-md border border-rex-border bg-rex-surface-2 text-rex-text transition-colors hover:border-brand">
      <button onClick={onTerminal} title={label} aria-label={label} className="px-2.5 py-1">
        <TerminalSquare className="h-3.5 w-3.5" />
      </button>
      <Menu
        align="right"
        trigger={
          <button
            title={`Open ${name}'s folder in your own terminal`}
            aria-label={`Open ${name}'s folder in your own terminal`}
            // Same seam as the quick-tile chevron: without the divider it reads
            // as decoration on one wide button.
            className="flex h-full items-center rounded-r-md border-l border-rex-border px-1 text-rex-text-muted transition-colors hover:bg-rex-hover hover:text-rex-text"
          >
            <ChevronDown className="h-3 w-3" strokeWidth={2} />
          </button>
        }
      >
        {menu}
      </Menu>
    </div>
  );
}

/** Row-selection checkbox — big enough to hit, pointer cursor. */
/** One queued install target in the tag-style Add bar (plugins & themes).
 *  `icon` is wp.org art (plugin icon / theme screenshot); null → letter tile. */
type PendingInstall = { slug: string; icon: string | null };

/** Streamed-install card state shared by the plugin/theme panels: start a
 *  job, subscribe to its `wp-install://` events, re-adopt after a remount,
 *  refresh the list + clear the queue on settle. */
/** Announce a settled install ONCE. wp-cli's own summary is preferred when it
 *  has one — "partial" is a real outcome here (three slugs, one bad) and
 *  flattening it to success or failure would be a lie in one direction or the
 *  other. */
function announceInstall(s: WpInstallState): void {
  if (announcedInstalls.has(s.id)) return;
  announcedInstalls.add(s.id);
  const what = subject(installLabels(s), s.kind);
  const detail = s.summary?.split("\n").find((l) => l.trim() !== "")?.trim();
  // Nothing was installed, and nothing was harmed either: wp-cli refused to
  // unpack over a directory that is already there. Reporting that as
  // "Install … failed: Error: No plugins installed." is technically wp-cli's
  // sentence and practically a lie about what happened — the user sees a
  // failure where the card is offering them a one-click way forward.
  if (s.blockedBy && s.status !== "ok") {
    return toast.info(
      `${s.blockedBy} is already installed — nothing was unpacked. Use Replace to overwrite it.`,
    );
  }
  switch (s.status) {
    case "ok":
      return toast.success(`Installed ${what}`);
    case "partial":
      return toast.info(detail ? `Installed some of ${what}: ${detail}` : `Installed some of ${what}`);
    case "cancelled":
      return toast.info(`Install of ${what} cancelled`);
    case "timed_out":
      return toast.error(`Install of ${what} timed out`);
    default:
      return toast.error(detail ? `Install of ${what} failed: ${detail}` : `Install of ${what} failed`);
  }
}

/** How long a SUCCESSFUL install card stays on screen before it clears itself.
 *  Only success: the toast already said "Installed 1 of 1", the list below it
 *  now shows the plugin, and a card repeating that is a panel a user has to
 *  tidy up after every install. Every OTHER outcome stays until dismissed —
 *  a failure is the one case where the log is the point, and a card that
 *  vanished after three seconds would take the only copy of the reason with
 *  it. */
const INSTALL_CARD_LINGER_MS = 3_000;

function useWpInstall(
  siteId: string,
  kind: "plugin" | "theme",
  onOk: () => void,
) {
  const qc = useQueryClient();
  const listKey = kind === "plugin" ? ["wp-plugins", siteId] : ["wp-themes", siteId];
  const [job, setJob] = useState<WpInstallState | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  /** The card asks for a HOLD while its log pane is open. Without it, opening
   *  the log on a successful install starts a countdown against the reader —
   *  three seconds is exactly long enough to click "Show log" and lose it. */
  const [held, setHeld] = useState(false);
  // Re-adopt a live/settled job after a tab switch (job survives unmount).
  useEffect(() => {
    void wpInstallActive(siteId, kind)
      .then((j) => {
        if (j) {
          setJob(j);
          void tailLog(j.logKey, 300).then(setLines).catch(() => {});
          // A job that settled while this panel was UNMOUNTED (sub-tab switch,
          // or the user off in the browser) emitted its state event to nobody.
          // Adoption is the second way we learn an outcome, so it announces
          // too; `announcedInstalls` keeps that from doubling the event path.
          if (j.status !== "running") announceInstall(j);
        }
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteId]);
  useEffect(() => {
    if (!job?.id) return;
    let dead = false;
    const un: Array<() => void> = [];
    void onWpInstallState(job.id, (s) => {
      if (dead) return;
      setJob(s);
      if (s.status !== "running") {
        qc.invalidateQueries({ queryKey: listKey });
        if (s.status === "ok") onOk();
        announceInstall(s);
      }
    }).then((u) => un.push(u));
    void onWpInstallOutput(job.id, (l) => {
      if (!dead) setLines((x) => [...x.slice(-499), l]);
    }).then((u) => un.push(u));
    return () => {
      dead = true;
      un.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [job?.id]);
  const dismiss = useCallback(() => {
    setJob(null);
    setLines([]);
    setHeld(false);
  }, []);
  // Success clears itself; nothing else does. Keyed on the JOB ID as well as
  // the status, so the timer of a settled job is torn down the moment another
  // install starts — a stale timer firing on a NEW card is the one way an
  // auto-hide can eat something a user is reading.
  useEffect(() => {
    if (job?.status !== "ok" || held) return;
    const t = setTimeout(dismiss, INSTALL_CARD_LINGER_MS);
    return () => clearTimeout(t);
  }, [job?.id, job?.status, held, dismiss]);
  return {
    job,
    lines,
    start: (snap: WpInstallState) => {
      setJob(snap);
      setLines([]);
      setHeld(false);
    },
    dismiss,
    hold: setHeld,
    running: job?.status === "running",
  };
}

/** Queue an item, deduplicating by slug. */
function addPending(list: PendingInstall[], item: PendingInstall): PendingInstall[] {
  return list.some((t) => t.slug === item.slug) ? list : [...list, item];
}

/** A queued-install chip: icon + slug + remove (×). */
function SlugTag({ slug, icon, onRemove }: { slug: string; icon: string | null; onRemove: () => void }) {
  return (
    <span className="flex items-center gap-1.5 rounded-md border border-rex-border-strong bg-rex-surface-2 py-0.5 pl-1 pr-0.5">
      {icon ? (
        <img src={icon} alt="" className="h-4 w-4 flex-none rounded-[3px] object-cover" />
      ) : (
        <span className="flex h-4 w-4 flex-none items-center justify-center rounded-[3px] bg-rex-surface-1 font-mono text-[0.5625rem] font-bold text-rex-text-muted">
          {slug.slice(0, 1).toUpperCase() || "?"}
        </span>
      )}
      <span className="font-mono text-[0.71875rem] text-rex-text">{slug}</span>
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${slug}`}
        className="flex h-4 w-4 items-center justify-center rounded text-rex-text-muted transition-colors hover:text-status-error-bright"
      >
        <X className="h-3 w-3" />
      </button>
    </span>
  );
}

/** wp.org ↔ Git source switch for the add bar (plugins & themes). `gitBusy`
 *  marks a live add-from-Git job so it stays visible from the wp.org tab —
 *  the job survives the panel (backend registry), the UI must say so. */
/** Where a new plugin/theme comes from. "wporg" and "zip" are the same
 *  streamed install job (different backend gate); "git"/"link" are their own
 *  flows. */
type AddSource = "wporg" | "zip" | "git" | "link";

function SourceTabs({
  source,
  onChange,
  gitBusy,
}: {
  source: AddSource;
  onChange: (s: AddSource) => void;
  gitBusy?: boolean;
}) {
  return (
    <div className="mb-2 flex items-center gap-1">
      {(
        [
          ["wporg", "WordPress.org"],
          ["zip", "Upload zip"],
          ["git", "From Git"],
          ["link", "Link folder"],
        ] as const
      ).map(([key, label]) => (
        <button
          key={key}
          type="button"
          onClick={() => onChange(key)}
          className={cn(
            "flex items-center gap-1.5 rounded-md px-2 py-0.5 text-[0.6875rem] font-medium transition-colors",
            source === key ? "bg-rex-surface-3 text-rex-text" : "text-rex-text-muted hover:text-rex-text",
          )}
        >
          {label}
          {key === "git" && gitBusy && (
            <Loader2 className="h-3 w-3 animate-rex-spin text-brand" />
          )}
        </button>
      ))}
    </div>
  );
}

// Every WP-CLI list call boots WordPress (~0.5s+) — cache results briefly, skip
// window-focus refetches, and fail after ONE retry so a broken site surfaces an
// error instead of spinning through react-query's default 3 retries.
const WP_QUERY = { staleTime: 30_000, refetchOnWindowFocus: false, retry: 1 } as const;

// ...except for the lists that describe state WordPress itself can change while
// rexenv is showing it. A plugin activated in wp-admin (or by wp-cli in the
// Terminal tab) left this list saying the opposite, and the only way back was
// to leave the tab and return — the app looked broken because it was showing a
// 30-second-old fact with no way to say so. These refetch whenever the NATIVE
// window regains focus (see `lib/window-focus.ts`), which is exactly the moment
// the user comes back from changing something elsewhere. One wp-cli list call
// per return, and only for the panel that is actually open (react-query
// refetches ACTIVE queries only).
const WP_LIVE = { staleTime: 0, refetchOnWindowFocus: true, retry: 1 } as const;

/** One list action plus the sentence it earns when it succeeds. Pairing them at
 *  the CALL SITE is the point: a shared mutation that only knew "something
 *  finished" could say nothing useful, which is how these buttons ended up
 *  silent. */
interface Action {
  fn: () => Promise<void>;
  /** Omitted when the call only STARTS streamed work: the install job announces
   *  its own outcome when it settles, and a "done" at spawn time would be a
   *  claim about work that has not run yet. */
  done?: string;
}

/** What a toast should CALL the target of an action: the item itself when there
 *  is one, a count when it is a bulk run. The name is what the user selected —
 *  never a slug we guessed. */
function subject(names: string[], noun: "plugin" | "theme"): string {
  return names.length === 1 ? names[0] : `${names.length} ${noun}s`;
}

/** Install jobs already announced, keyed by job id — module scope, because the
 *  panel unmounts on every sub-tab switch and a re-adopted settled job would
 *  otherwise announce itself again on each return (the same trap the repo
 *  panel's zip toast fell into). */
const announcedInstalls = new Set<string>();

/** Plugins in two passes: an instant list (no update check), then a background
 *  pass with the wordpress.org update check (slow; a hang when offline) that
 *  only refreshes the update badges when it lands. Mutations invalidate
 *  `["wp-plugins", siteId]`, which prefix-matches both keys. */
function useWpPlugins(siteId: string) {
  const fast = useQuery({
    queryKey: ["wp-plugins", siteId],
    queryFn: () => wpPlugins(siteId),
    ...WP_LIVE,
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
    // The badge AND the target version normally come from the checked pass,
    // but `--skip-update-check` does NOT mean "no update info": it means "read
    // the update transient without refreshing it", so the fast row carries a
    // claim too — an older one. Either way it is a claim, and `verdict` is
    // what decides whether it may be shown.
    const upd = new Map((updates.data ?? []).map((p) => [p.name, p]));
    return base.map((p) => verdict(p, upd.get(p.name) ?? p));
  }, [fast.data, updates.data]);
  return {
    plugins,
    isLoading: fast.isLoading,
    isError: fast.isError,
    error: fast.error,
    // Refresh means BOTH passes: the update badges are the half a user is most
    // likely to be re-checking, and they live in the slow query.
    refetch: () => {
      void fast.refetch();
      void updates.refetch();
    },
    isFetching: fast.isFetching || updates.isFetching,
  };
}

/** Themes, same two-pass shape as `useWpPlugins`. */
function useWpThemes(siteId: string) {
  const fast = useQuery({
    queryKey: ["wp-themes", siteId],
    queryFn: () => wpThemes(siteId),
    ...WP_LIVE,
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
    const upd = new Map((updates.data ?? []).map((t) => [t.name, t]));
    return base.map((t) => verdict(t, upd.get(t.name) ?? t));
  }, [fast.data, updates.data]);
  return {
    themes,
    isLoading: fast.isLoading,
    isError: fast.isError,
    error: fast.error,
    refetch: () => {
      void fast.refetch();
      void updates.refetch();
    },
    isFetching: fast.isFetching || updates.isFetching,
  };
}

function useWpUsers(siteId: string) {
  // Users change in wp-admin too, but far less often and this list is mounted
  // for the tab badge even when nobody is looking at it — so it refetches on
  // focus only once it is actually stale, rather than on every alt-tab.
  return useQuery({
    queryKey: ["wp-users", siteId],
    queryFn: () => wpUsers(siteId),
    ...WP_QUERY,
    refetchOnWindowFocus: true,
  });
}

/** Re-read a panel's data on demand. The auto-refresh on window focus covers
 *  the common case (change something elsewhere, come back); this covers the
 *  one it cannot — a change made while rexenv already has focus, e.g. wp-cli in
 *  the Terminal tab, or a plugin that deactivates itself. */
function RefreshButton({
  onClick,
  busy,
  what,
}: {
  onClick: () => void;
  busy: boolean;
  what: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      title={`Refresh ${what}`}
      aria-label={`Refresh ${what}`}
      className={cn(
        "flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg border border-rex-border",
        "bg-rex-surface-2 text-rex-text-muted transition-colors",
        "hover:border-brand/60 hover:text-rex-text-bright disabled:opacity-60 disabled:hover:border-rex-border",
      )}
    >
      <RefreshCw className={cn("h-3.5 w-3.5", busy && "animate-spin")} />
    </button>
  );
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
              <span className="font-mono text-[0.65625rem]">{s.count}</span>
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
  // Subdomain, matching New Site's multisite toggle — the two screens create
  // the same kind of network, and the choice is permanent enough that a
  // different default in each place is a trap.
  const [mode, setMode] = useState<Exclude<MultisiteMode, "none">>("subdomain");
  // The FILE may already run a network rexenv has recorded as a single site —
  // a Valet/Herd network imported before 13 Sep 2026. Then there is nothing to
  // convert: the backend records the mode its wp-config declares instead.
  const { data: info } = useQuery({
    queryKey: ["wp-info", siteId],
    queryFn: () => wpInfo(siteId),
    ...WP_QUERY,
  });
  const alreadyNetwork = info?.multisite === true;

  const convert = useMutation({
    mutationFn: () => wpMultisiteConvert(siteId, mode),
    onSuccess: () => {
      toast.success(alreadyNetwork ? "Recorded as a multisite network" : `Converted to ${mode} multisite`);
      qc.invalidateQueries({ queryKey: ["sites"] });
      qc.invalidateQueries({ queryKey: ["wp-info", siteId] });
      qc.invalidateQueries({ queryKey: ["wp-network-sites", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  if (alreadyNetwork) {
    return (
      <div className="flex flex-col gap-3">
        <Card title="Already a multisite network">
          <div className="text-[0.78125rem] leading-[1.5] text-rex-text-muted">
            This site's <span className="font-mono">wp-config.php</span> already runs a network,
            but rexenv has it recorded as a single site — so its sub-sites aren't served.
            Recording it reads the mode from that file and changes nothing in WordPress.
          </div>
          <button
            className={BTN + " mt-3 flex items-center gap-1.5"}
            disabled={convert.isPending}
            onClick={() => convert.mutate()}
          >
            {convert.isPending ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Network className="h-3.5 w-3.5" />
            )}
            {convert.isPending ? "Recording…" : "Record as a network"}
          </button>
        </Card>
      </div>
    );
  }

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
  const { themes } = useWpThemes(siteId);
  // `wp theme list` cannot answer which themes the NETWORK has enabled — theme
  // status is only active/parent/inactive, with no `active-network` the way
  // plugins have — so the enabled set is a separate read of the `allowedthemes`
  // network option. That gap is why `wpThemeEnableNetwork` shipped with typed
  // wrappers and nothing calling them: without the state there is no honest
  // toggle, only a pair of buttons that cannot say what they would undo.
  const { data: netThemes = [] } = useQuery({
    queryKey: ["wp-network-themes", siteId],
    queryFn: () => wpThemesNetworkEnabled(siteId),
    ...WP_QUERY,
  });
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
  const themeRun = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-network-themes", siteId] }),
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
              <SubSiteRow
                key={s.id}
                siteId={siteId}
                site={s}
                deleteDisabled={sitesRun.isPending || s.id === "1"}
                onDelete={async () => {
                  if (await confirm({ title: "Delete sub-site?", message: s.url, danger: true, confirmLabel: "Delete" }))
                    sitesRun.mutate(() => wpNetworkSiteDelete(siteId, s.id));
                }}
              />
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
                    <span className="rounded-full bg-status-running-bg px-2 py-0.5 text-[0.65625rem] font-medium text-status-running-bright">
                      Network active
                    </span>
                  )}
                  {immutable ? (
                    <span
                      className="rounded-full bg-status-running-bg px-2 py-0.5 text-[0.65625rem] font-medium text-status-running-bright"
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

      {/* Network-enabled themes.
        *
        * Sibling of the plugins card above, and placed here rather than on the
        * Themes tab for the reason that tab cannot carry it: network enabling
        * only exists for a multisite, and NetworkPanel is the one place that is
        * structurally true. A toggle on the themes list would need a runtime
        * `isNetwork` check to hide itself, which is the shape that renders for
        * a moment on a single site. */}
      <Card title="Themes (network)">
        <p className="mb-2 text-[0.71875rem] leading-relaxed text-rex-text-muted">
          Network-enabled themes can be activated by any site in the network. The
          active theme of the main site is separate — enabling here only makes a
          theme available.
        </p>
        <div className="overflow-hidden rounded-lg border border-rex-border">
          {themes.length === 0 ? (
            <div className="py-4 text-center text-[0.78125rem] text-rex-text-muted">No themes installed.</div>
          ) : (
            themes.map((t) => {
              const net = netThemes.includes(t.name);
              return (
                <div
                  key={t.name}
                  className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
                >
                  <span className="min-w-0 flex-1 truncate text-[0.78125rem] text-rex-text">
                    {t.title || t.name}
                  </span>
                  <span className="font-mono text-[0.65625rem] text-rex-text-muted">{t.name}</span>
                  {net && (
                    <span className="rounded-full bg-status-running-bg px-2 py-0.5 text-[0.65625rem] font-medium text-status-running-bright">
                      Network enabled
                    </span>
                  )}
                  <button
                    className={BTN}
                    disabled={themeRun.isPending}
                    onClick={() =>
                      themeRun.mutate(() =>
                        net
                          ? wpThemeDisableNetwork(siteId, t.name)
                          : wpThemeEnableNetwork(siteId, t.name),
                      )
                    }
                  >
                    {net ? "Network disable" : "Network enable"}
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
  const words = usePlatformWords();
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
          label: words.reveal,
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

  // Core is ONE item, and it names itself: the backend tracker calls it
  // "WordPress" (there is no slug to report), so the panel starts the run
  // under that same name and the two agree about what the bar is measuring.
  const coreUpdate = useUpdateStream("core", siteId, () => wpCoreUpdate(siteId), (out) => {
    setCoreOut(out);
    // The core VERSION shown elsewhere (Overview, the version switcher's
    // "current") comes from `wp-info` — without this it kept naming the
    // release we just left, the same stale-after-update fault the lists had.
    qc.invalidateQueries({ queryKey: ["wp-info", siteId] });
  });
  const coreUpdating = coreUpdate.rowUpdate(CORE_ITEM);
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
    onSuccess: (url) => void openExternal(url).catch(toastBackendError),
    onError: (e) => toastBackendError(e),
  });
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: words.reveal,
        onClick: () => void revealPath(path).catch(toastBackendError),
      }),
    onError: (e) => toastBackendError(e),
  });
  const working = coreUpdate.pending || coreReinstall.isPending;
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
              <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
                <input
                  type="checkbox"
                  checked={dryRun}
                  onChange={(e) => setDryRun(e.target.checked)}
                  className={CHECK_INPUT}
                />
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
            <WordPressIcon className="h-3.5 w-3.5" />
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
          <div className="font-mono text-[0.71875rem] text-rex-text-muted">
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
          <div className="text-[0.71875rem] leading-[1.5] text-rex-text-muted">
            Core translations only — plugins and themes fetch their own packs.
            {isNetwork && " On a multisite network this switches the main site; subsites set theirs in their own admin."}
          </div>
        </div>
      </Card>

            <Card title="Core">
        <div className="flex flex-col gap-2">
          <button
            className={maintBtn}
            disabled={coreUpdate.pending}
            onClick={() => {
              setCoreOut(null);
              coreUpdate.start([CORE_ITEM]);
            }}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Update core
          </button>
          {/* A core update downloads a full release — the same dead air the
              plugin list had, so it reports wp-cli's own steps too. */}
          {coreUpdating && (
            <div className="flex flex-col gap-1">
              <ProgressBar fraction={coreUpdating.fraction} />
              <span className="text-center font-mono text-[0.625rem] text-rex-text-muted">
                {coreUpdating.phase}
              </span>
            </div>
          )}
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
  const words = usePlatformWords();
  const [typed, setTyped] = useState("");
  const [file, setFile] = useState<string | null>(null);
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: words.reveal,
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
  const match = confirmPhraseMatches(typed, domain);
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
        <TypeToConfirm phrase={domain} value={typed} onChange={setTyped} disabled={busy} autoFocus />
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
        title: `Delete ${n} file-manager leftover${n === 1 ? "" : "s"}?`,
        message:
          "Harmless files a file manager leaves in folders (.DS_Store, ._ files, Thumbs.db, desktop.ini) inside this site's folder — it recreates them as needed. Files are deleted permanently (not moved to the trash), then the checksums are re-verified.",
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
        <div className="font-mono text-[0.65625rem] text-rex-text-muted" title={r.benign.join("\n")}>
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
export function CronCard({ siteId }: { siteId: string }) {
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

  // A real site schedules dozens of hooks; finding one by eye means scrolling a
  // 300px window. Filter on the hook name (and recurrence — "45 minutes" is how
  // people remember an odd one out).
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shown = !events ? [] : !q
    ? events
    : events.filter(
        (e) =>
          e.hook.toLowerCase().includes(q) ||
          e.recurrence.toLowerCase().includes(q) ||
          e.args.toLowerCase().includes(q),
      );

  // Hooks scheduled more than once. Computed over ALL events, not the filtered
  // set: a search that hides one instance must not make the Run button stop
  // warning that it runs both.
  const dupHooks = useMemo(() => {
    const seen = new Map<string, number>();
    for (const e of events ?? []) seen.set(e.hook, (seen.get(e.hook) ?? 0) + 1);
    return new Set([...seen].filter(([, n]) => n > 1).map(([h]) => h));
  }, [events]);

  return (
    <Card title="Cron">
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-[0.75rem] text-rex-text-muted">
            {!events
              ? "…"
              : q
                ? `${shown.length} of ${events.length} scheduled event(s)`
                : `${events.length} scheduled event(s)`}{" "}
            — local dev has no visitors, so overdue events are normal; run them on demand.
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
        {events && events.length > 0 && (
          <div className="relative w-[230px]">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-rex-text-muted" />
            <input {...TECH_INPUT}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search hooks…"
              className="h-[30px] w-full rounded-lg border border-rex-border bg-rex-surface-2 pl-8 pr-7 text-[0.75rem] text-rex-text outline-none transition-colors focus:border-brand"
            />
            {q && (
              <button
                className="absolute right-2 top-1/2 -translate-y-1/2 text-rex-text-muted transition-colors hover:text-rex-text"
                title="Clear search"
                onClick={() => setQuery("")}
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
        )}
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
              <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
                <span className="flex-1">Hook</span>
                <span className="w-[130px]">Arguments</span>
                <span className="w-[170px]">Next run</span>
                <span className="w-[110px]">Recurrence</span>
                <span className="w-[64px]" />
              </div>
              <div className="max-h-[300px] overflow-y-auto">
                {shown.length === 0 && (
                  <div className="p-4 text-center text-[0.78125rem] text-rex-text-muted">
                    No hook matches “{query.trim()}”.
                  </div>
                )}
                {shown.map((e, i) => (
                  <div
                    key={`${e.hook}-${i}`}
                    className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 last:border-b-0"
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-text-bright" title={e.hook}>
                      {e.hook}
                    </span>
                    {/* WP-CLI addresses events by HOOK, not by id, so args are
                      * the only thing telling two events of one hook apart —
                      * Action Scheduler schedules `action_scheduler_run_queue`
                      * more than once with different runners. Without this
                      * column those rows are identical and the Run button on
                      * each does the same thing. */}
                    <span
                      className="w-[130px] truncate font-mono text-[0.71875rem] text-rex-text-muted"
                      title={e.args || "no arguments"}
                    >
                      {e.args}
                    </span>
                    <span className="w-[170px] text-[0.75rem] text-rex-text-muted" title={`${e.nextRun} GMT`}>
                      {e.nextRunRelative || "now"}
                    </span>
                    <span className="w-[110px] text-[0.75rem] text-rex-text-muted">{e.recurrence}</span>
                    <button
                      className={BTN + " w-[64px] justify-center text-center"}
                      title={
                        dupHooks.has(e.hook)
                          ? `Run ${e.hook} now (due or not). This hook is scheduled more than once and WP-CLI runs events by hook name, so EVERY instance runs — not just this row.`
                          : `Run ${e.hook} now (due or not)`
                      }
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
  const words = usePlatformWords();
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState(false);
  const dbExport = useMutation({
    mutationFn: () => wpDbExport(siteId),
    onSuccess: (path) =>
      toast.success(`Database exported to ${path}`, {
        label: words.reveal,
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
  const match = confirmPhraseMatches(typed, domain);
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
            <TypeToConfirm phrase={domain} value={typed} onChange={setTyped} disabled={busy} autoFocus />
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
  const words = usePlatformWords();
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
        label: words.reveal,
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
        <div className="text-[0.71875rem] text-rex-text-muted">
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
        <div className="text-[0.78125rem] text-rex-text-muted">Reading options…</div>
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
                  <span className="min-w-0 truncate text-[0.71875rem] text-rex-text-muted" title={row.note}>
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
                    className="text-[0.71875rem] text-rex-text-muted hover:underline"
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
          <div className="mt-1 text-[0.71875rem] text-rex-text-muted">
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

/** One network sub-site row: Visit and Magic Login, each with the browser
 *  chooser the site header has. Until 12 Sep 2026 these were two plain icons —
 *  Visit in the default browser only, and "Admin" opening `/wp-admin/` with no
 *  sign-in at all (owner report). The login is minted per click FOR THIS BLOG
 *  (`blogId`): a token on the main site is invisible to a sub-site's request. */
function SubSiteRow({
  siteId,
  site: s,
  deleteDisabled,
  onDelete,
}: {
  siteId: string;
  site: { id: string; url: string; deleted: boolean };
  deleteDisabled: boolean;
  onDelete: () => void;
}) {
  const loginUrl = async () => {
    try {
      return await wpAdminLoginUrl(siteId, s.id);
    } catch (e) {
      toast.error(`Auto-login unavailable — opening the sub-site's login page instead.\n${String(e)}`);
      return `${s.url.replace(/\/$/, "")}/wp-admin/`;
    }
  };
  const visitMenu = useBrowserMenu(s.url);
  const loginMenu = useBrowserMenu(loginUrl);
  const [signingIn, setSigningIn] = useState(false);
  return (
    <div className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0">
      <span className="rounded bg-rex-surface-3 px-1.5 py-0.5 font-mono text-[0.65625rem] text-rex-text-muted">
        #{s.id}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-rex-text" title={s.url}>
        {s.url}
      </span>
      {s.deleted && (
        <span className="rounded-full bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] text-status-warning-bright">
          archived
        </span>
      )}
      <SplitButton
        size="sm"
        onClick={() => void openExternal(s.url).catch(toastBackendError)}
        menu={visitMenu}
        menuWidth={BROWSER_MENU_WIDTH}
        chevronLabel="Visit in another browser"
      >
        <PreferredBrowserIcon className="h-3.5 w-3.5" />
        Visit
      </SplitButton>
      <SplitButton
        size="sm"
        disabled={signingIn}
        onClick={async () => {
          setSigningIn(true);
          try {
            await openExternal(await loginUrl());
          } catch (e) {
            toastBackendError(e);
          } finally {
            setSigningIn(false);
          }
        }}
        menu={loginMenu}
        menuWidth={BROWSER_MENU_WIDTH}
        chevronLabel="Sign in through another browser"
      >
        <WordPressIcon className="h-3.5 w-3.5" />
        {signingIn ? "Signing in…" : "Magic Login"}
      </SplitButton>
      <button
        className={BTN + " hover:border-status-error-border hover:text-status-error-bright disabled:hover:border-rex-border disabled:hover:text-rex-text"}
        disabled={deleteDisabled}
        title={s.id === "1" ? "Can't delete the main site" : "Delete sub-site"}
        onClick={onDelete}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
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
    onSuccess: (url) => void openExternal(url).catch(toastBackendError),
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
            <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
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
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">{u.email}</div>
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
        <WordPressIcon className="h-3.5 w-3.5" />
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
        <span className="flex h-9 w-12 flex-none items-center justify-center rounded-[5px] border border-rex-border bg-rex-surface-2 text-rex-text-muted">
          <Palette className="h-4 w-4" strokeWidth={1.5} />
        </span>
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[0.78125rem] font-medium text-rex-text">{t.name}</span>
        <span className="block truncate text-[0.6875rem] text-rex-text-muted">
          {t.author && `by ${t.author} · `}
          <span className="font-mono">{t.slug}</span>
        </span>
      </span>
      <span className="flex flex-none items-center gap-2 font-mono text-[0.65625rem] text-rex-text-muted">
        {t.rating > 0 && (
          <span className="flex items-center gap-0.5">
            <Star className="h-3 w-3 fill-current text-status-warning-bright" />
            {(t.rating / 20).toFixed(1)}
          </span>
        )}
        <span>{fmtInstalls(t.activeInstalls)}</span>
      </span>
    </button>
  );
}

export function ThemesPanel({ siteId }: { siteId: string }) {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(false);
  // wp.org live search — same tag-queue pattern as PluginsPanel (debounce,
  // fail fast, manual slug + Enter always works).
  const [pending, setPending] = useState<PendingInstall[]>([]);
  // Queue/remove must never steal focus from the search box (type-to-search).
  const addInputRef = useRef<HTMLInputElement | null>(null);
  const install = useWpInstall(siteId, "theme", () => {
    setPending([]);
    setSlug("");
  });
  const debouncedSlug = useDebounced(slug.trim(), 350);
  const search = useQuery({
    queryKey: ["wporg-themes", debouncedSlug],
    queryFn: () => wpOrgSearchThemes(debouncedSlug),
    enabled: debouncedSlug.length >= 2,
    staleTime: 60_000,
    // A network read to wordpress.org must never ride an alt-tab (#255).
    refetchOnWindowFocus: false,
    retry: false,
  });
  const showSearch = slug.trim().length >= 2;
  const queue = (s: string, icon: string | null) => {
    const v = s.trim();
    if (!v) return;
    setPending((list) => addPending(list, { slug: v, icon }));
    setSlug("");
    addInputRef.current?.focus();
  };
  const installSlugs = [
    ...pending.map((t) => t.slug),
    ...(slug.trim() && !pending.some((t) => t.slug === slug.trim()) ? [slug.trim()] : []),
  ];

  const { themes, isLoading, isError, error, refetch, isFetching } = useWpThemes(siteId);

  // Every action carries what to SAY when it lands: activating a theme or
  // deleting one finishes in a list that may have scrolled, and an action that
  // reports nothing is indistinguishable from one that did nothing.
  const run = useMutation({
    mutationFn: (a: Action) => a.fn(),
    onSuccess: (_r, a) => {
      if (a.done) toast.success(a.done);
      qc.invalidateQueries({ queryKey: ["wp-themes", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });
  // Same streamed update as the plugin panel — a theme download is the same
  // wp-cli upgrader, and was the same silent wait.
  const upd = useUpdateStream("themes", siteId, (names) => wpThemeUpdate(siteId, names), (_r, names) =>
    settleAfterUpdate<WpTheme>(qc, ["wp-themes", siteId], names),
  );
  const busy = run.isPending || upd.pending;

  const [source, setSource] = useState<AddSource>("wporg");
  const gitAssets = useQuery({
    queryKey: ["repo-assets", siteId],
    queryFn: () => repoAssets(siteId),
    staleTime: 30_000,
    // A git read on focus, like its neighbours repo-unmanaged/repo-jobs: opted out.
    refetchOnWindowFocus: false,
  });
  const gitDirs = useMemo(
    () => new Set((gitAssets.data ?? []).filter((a) => a.kind === "theme").map((a) => a.dirName)),
    [gitAssets.data],
  );
  const assetFor = (name: string) =>
    (gitAssets.data ?? []).find((a) => a.kind === "theme" && a.dirName === name);
  const refreshAfterGit = () => {
    qc.invalidateQueries({ queryKey: ["wp-themes", siteId] });
    qc.invalidateQueries({ queryKey: ["repo-assets", siteId] });
    qc.invalidateQueries({ queryKey: ["repo-unmanaged", siteId, "theme"] });
  };
  const unmanaged = useQuery({
    queryKey: ["repo-unmanaged", siteId, "theme"],
    queryFn: () => repoUnmanaged(siteId, "theme"),
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });
  const unmanagedSet = useMemo(
    () => new Set((unmanaged.data ?? []).map((u) => u.dirName)),
    [unmanaged.data],
  );

  /** wp-admin's "Replace current with uploaded", reached the same way: the
   *  install refused because `dir` is already there, and this re-runs the SAME
   *  zip with `--force`.
   *
   *  The confirm exists for a hazard wp-admin does not have. rexenv knows which
   *  of these directories are GIT CHECKOUTS — it put some of them there — and
   *  `--force` unpacks straight over the working tree: uncommitted work, the
   *  branch, `.git` itself. So a tracked (or merely git-looking) target is
   *  named before anything is overwritten; everything else replaces on the
   *  click, which is the confirmation wp-admin's own button is.
   *
   *  The retry keeps the job's OWN source. A zip re-runs as a zip (its paths
   *  are still on disk); a wp.org job re-runs as wp.org — sending it back down
   *  the zip gate would fail `ensure_zip_paths` on a slug and read as a second,
   *  unrelated error. */
  const replaceExisting = async (dir: string) => {
    const job = install.job;
    if (!job) return;
    const tracked = gitDirs.has(dir) || unmanagedSet.has(dir);
    if (
      tracked &&
      !(await confirm({
        title: `Replace ${dir}?`,
        message:
          `${dir} is a git checkout in this site. Replacing it unpacks the zip over the ` +
          `working tree — uncommitted changes, the branch and the repository itself go with it. ` +
          `This cannot be undone from rexenv.`,
        danger: true,
        confirmLabel: "Replace",
      }))
    )
      return;
    try {
      install.start(
        await wpInstallJob(siteId, "theme", job.slugs, activateOnAdd, job.source, true),
      );
    } catch (e) {
      toastBackendError(e);
    }
  };
  const [openRepo, setOpenRepo] = useState<string | null>(null);
  const adoptRepo = async (name: string) => {
    if (
      await confirm({
        title: `Manage "${name}" in rexenv?`,
        message:
          "This folder looks like a git checkout. Adopting records its remote and branch so rexenv can show its repo state — nothing on disk changes.",
        confirmLabel: "Adopt",
      })
    ) {
      try {
        await repoAdopt(siteId, "theme", name);
        refreshAfterGit();
        setOpenRepo(name);
      } catch (e) {
        toastBackendError(e);
      }
    }
  };
  const confirmDelete = async (title: string, name: string): Promise<boolean> => {
    let message: React.ReactNode;
    if (gitDirs.has(name) || unmanagedSet.has(name)) {
      let line: string;
      if (assetFor(name)?.source === "linked") {
        line = "LINKED folder — removes only the link; your original folder stays untouched.";
      } else {
        try {
          const st = await repoAssetStatus(siteId, "theme", name);
          line = st.lossWarning ?? "clean and pushed — nothing at risk.";
        } catch {
          line = "git checkout — anything uncommitted will be lost.";
        }
      }
      message = (
        <div className="space-y-1">
          <div>This deletes a git checkout:</div>
          <div className="font-mono text-[0.71875rem]">{`${name}: ${line}`}</div>
        </div>
      );
    }
    return confirm({ title, message, danger: true, confirmLabel: "Delete" });
  };
  const repoJobs = useQuery({
    queryKey: ["repo-jobs", siteId, "theme"],
    queryFn: () => repoSiteJobs(siteId, "theme"),
    refetchOnWindowFocus: false,
    staleTime: 5_000,
    refetchInterval: (q) =>
      (q.state.data ?? []).some((j) => j.steps.some((st) => st.status === "running"))
        ? 2_500
        : false,
  });
  const gitBusy = (repoJobs.data ?? []).some((j) =>
    j.steps.some((st) => st.status === "running"),
  );

  return (
    <div className="flex flex-col gap-3">
      <div className="relative rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <SourceTabs source={source} onChange={setSource} gitBusy={gitBusy} />
        {source === "git" ? (
          <GitAddPanel siteId={siteId} kind="theme" onInstalled={refreshAfterGit} />
        ) : source === "link" ? (
          <LinkFolderPanel siteId={siteId} kind="theme" onInstalled={refreshAfterGit} />
        ) : source === "zip" ? (
          <ZipAddPanel
            siteId={siteId}
            kind="theme"
            busy={busy || install.running}
            onStarted={install.start}
          />
        ) : (
        <>
        {/* Selected items live ABOVE the input row — the input keeps its full
            width no matter how many are queued (QA). */}
        {pending.length > 0 && (
          <div className="mb-2 flex flex-wrap items-center gap-2">
            {pending.map((t) => (
              <SlugTag
                key={t.slug}
                slug={t.slug}
                icon={t.icon}
                onRemove={() => {
                  setPending((l) => l.filter((x) => x.slug !== t.slug));
                  addInputRef.current?.focus();
                }}
              />
            ))}
          </div>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <input {...TECH_INPUT}
            ref={addInputRef}
            value={slug}
            onChange={(e) => setSlug(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                queue(slug, null);
              } else if (e.key === "Backspace" && slug === "" && pending.length > 0) {
                setPending((l) => l.slice(0, -1));
              }
            }}
            placeholder={pending.length ? "Add another…" : "Search WordPress.org or enter a slug…"}
            className="h-[30px] min-w-[180px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
          />
          {/* Same CHECK class as the list rows: unstyled, this rendered as the
              browser's own ~13px box with no accent colour, visibly smaller
              than every other checkbox on the screen (QA, 11 Aug 2026). */}
          <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
            <input
              type="checkbox"
              checked={activateOnAdd}
              onChange={(e) => setActivateOnAdd(e.target.checked)}
              className={CHECK_INPUT}
            />
            Activate
          </label>
          <button
            className={BTN + " flex items-center gap-1.5"}
            disabled={busy || install.running || installSlugs.length === 0}
            onClick={() => {
              const slugs = installSlugs;
              run.mutate({
                fn: () => wpInstallJob(siteId, "theme", slugs, activateOnAdd).then(install.start),
                // No `done`: the install runs on its own stream and announces
                // itself when it settles (useWpInstall).
              });
            }}
          >
            <Plus className="h-3.5 w-3.5" />
            Install{installSlugs.length > 1 ? ` (${installSlugs.length})` : ""}
          </button>
        </div>
        {showSearch && (
          <div className="absolute left-0 right-0 top-[calc(100%+4px)] z-20 overflow-hidden rounded-lg border border-rex-border-strong bg-rex-surface-1 shadow-xl">
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
                No themes match “{slug.trim()}” — if you know the exact slug, press Enter to queue it.
              </div>
            ) : (
              <div className="max-h-[300px] overflow-y-auto">
                {(search.data ?? []).map((t) => (
                  <WpOrgThemeHit key={t.slug} t={t} onPick={() => queue(t.slug, t.screenshot)} />
                ))}
              </div>
            )}
          </div>
        )}
        </>
        )}
        {/* ONE card for both install sources — wp.org and zip run the same
            job, so a tab switch mid-install must not hide the running work. */}
        {(source === "wporg" || source === "zip") && install.job && (
          <WpInstallCard
            job={install.job}
            lines={install.lines}
            onCancel={() => wpInstallCancel(install.job!.id).catch(toastBackendError)}
            onDismiss={install.dismiss}
            onHoldChange={install.hold}
            onReplace={(dir) => void replaceExisting(dir)}
          />
        )}
      </div>

      {!isLoading && themes.length > 0 && (
        <div className="flex items-center gap-2 px-0.5">
          <div className="font-mono text-[0.6875rem] text-rex-text-muted">
            {themes.length} {themes.length === 1 ? "theme" : "themes"} ·{" "}
            {themes.filter((t) => t.status === "active").length} active
          </div>
          <div className="flex-1" />
          <RefreshButton onClick={refetch} busy={isFetching} what="the theme list" />
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
            <Fragment key={t.name}>
            <ThemeCard
              t={t}
              siteId={siteId}
              git={gitDirs.has(t.name)}
              unmanaged={unmanagedSet.has(t.name)}
              onGitClick={() =>
                gitDirs.has(t.name)
                  ? setOpenRepo((cur) => (cur === t.name ? null : t.name))
                  : adoptRepo(t.name)
              }
              busy={busy}
              onActivate={() => run.mutate({ fn: () => wpThemeActivate(siteId, t.name), done: `Activated ${t.name}` })}
              onUpdate={() => upd.start([t.name])}
              updating={upd.rowUpdate(t.name)}
              onTerminal={() => navigate(`/sites/${siteId}/terminal?theme=${encodeURIComponent(t.name)}`)}
              onDelete={async () => {
                if (await confirmDelete(`Delete theme "${t.name}"?`, t.name))
                  run.mutate({ fn: () => wpThemeDelete(siteId, [t.name]), done: `Deleted ${t.name}` });
              }}
            />
            {openRepo === t.name && assetFor(t.name) && (
              <div className="col-span-full">
                <RepoPanel siteId={siteId} kind="theme" asset={assetFor(t.name)!} />
              </div>
            )}
            </Fragment>
          ))}
        </div>
      )}
    </div>
  );
}

function ThemeCard({
  t,
  siteId,
  git,
  unmanaged,
  onGitClick,
  busy,
  onActivate,
  onUpdate,
  updating,
  onTerminal,
  onDelete,
}: {
  t: WpTheme;
  /** The site the card belongs to — the terminal chevron resolves the theme's
   *  folder from it backend-side. */
  siteId: string;
  git?: boolean;
  unmanaged?: boolean;
  onGitClick?: () => void;
  busy: boolean;
  onActivate: () => void;
  onUpdate: () => void;
  /** Live position in a running update, or null when this card isn't in one. */
  updating?: { fraction: number; phase: string } | null;
  /** Open a shell in this theme's own folder. */
  onTerminal: () => void;
  onDelete: () => void;
}) {
  const active = t.status === "active";
  const updatable = t.update === "available";
  /** The version the update installs — empty until the checked pass lands. */
  const target = updatable ? t.updateVersion : "";
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
            alt={`${t.title || t.name} preview`}
            loading="lazy"
            className="h-full w-full object-cover"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-rex-text-muted">
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
          {/* The theme's own name, like wp-admin — the slug moves to the mono
              line below rather than disappearing: it is the folder name, what
              `theme activate` takes, and what a person greps for. */}
          <span className="min-w-0 flex-1 truncate text-[0.8125rem] font-medium text-rex-text">
            {t.title || t.name}
          </span>
          {git && (
            <button
              type="button"
              onClick={onGitClick}
              className="rounded-full bg-rex-accent-blue-bg px-1.5 py-0.5 font-mono text-[0.625rem] font-medium text-rex-accent-blue transition-colors hover:bg-rex-accent-blue-border"
              title="Git checkout — click for repo state"
            >
              git
            </button>
          )}
          {unmanaged && (
            <button
              type="button"
              onClick={onGitClick}
              className="rounded-full border border-dashed border-rex-accent-blue-border px-1.5 py-0.5 font-mono text-[0.625rem] font-medium text-rex-accent-blue transition-colors hover:border-rex-accent-blue hover:text-rex-accent-blue"
              title="Looks like a git checkout — click to manage it in rexenv"
            >
              git?
            </button>
          )}
          {active && (
            <span className="flex items-center gap-1 rounded-full bg-status-running-bg px-1.5 py-0.5 text-[0.625rem] font-medium text-status-running-bright">
              <Check className="h-3 w-3" />
              Active
            </span>
          )}
          {updatable && !active && (
            <span className="rounded-full bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] font-medium text-status-warning-bright">
              update
            </span>
          )}
        </div>
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
          {t.title && <>{t.name} · </>}v{t.version}
          {/* Drawn only from the checked pass's own target — never guessed
              from the badge. */}
          {target && <span className="text-status-warning-bright"> → {target}</span>}
        </div>
        {updating && (
          <div className="flex flex-col gap-1">
            <ProgressBar fraction={updating.fraction} />
            <span className="truncate font-mono text-[0.625rem] text-rex-text-muted">
              {updating.phase}
            </span>
          </div>
        )}
        <div className="mt-auto flex items-center gap-1.5">
          {!active && (
            <button className={BTN + " flex-1"} disabled={busy} onClick={onActivate}>
              Activate
            </button>
          )}
          {updatable && !updating && (
            <button
              className={BTN + " flex items-center gap-1"}
              disabled={busy}
              onClick={onUpdate}
              title={target ? `Update to ${target}` : "Update"}
            >
              <ArrowUpCircle className="h-3.5 w-3.5" />
            </button>
          )}
          <AssetTerminalButton siteId={siteId} kind="theme" name={t.name} onTerminal={onTerminal} />
          <button
            className={BTN + " hover:border-status-error-border hover:text-status-error-bright disabled:hover:border-rex-border disabled:hover:text-rex-text"}
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
        <span className="block truncate text-[0.6875rem] text-rex-text-muted">
          {p.author && `by ${p.author} · `}
          <span className="font-mono">{p.slug}</span>
        </span>
      </span>
      <span className="flex flex-none items-center gap-2 font-mono text-[0.65625rem] text-rex-text-muted">
        {p.rating > 0 && (
          <span className="flex items-center gap-0.5">
            <Star className="h-3 w-3 fill-current text-status-warning-bright" />
            {(p.rating / 20).toFixed(1)}
          </span>
        )}
        <span>{fmtInstalls(p.activeInstalls)}</span>
      </span>
    </button>
  );
}

// Exported for the DEV WebKit harness (DevGitPanel `?panel=wp-add`) only.
export function PluginsPanel({ siteId }: { siteId: string }) {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<PluginFilter>("all");
  // wp.org live search: the slug input doubles as the search box (wp-admin
  // style). Picked hits queue as tags (multi-install); typing a slug + Enter
  // queues it too, so non-wp.org slugs still work.
  const [pending, setPending] = useState<PendingInstall[]>([]);
  // Queue/remove must never steal focus from the search box (type-to-search).
  const addInputRef = useRef<HTMLInputElement | null>(null);
  const install = useWpInstall(siteId, "plugin", () => {
    setPending([]);
    setSlug("");
  });
  const debouncedSlug = useDebounced(slug.trim(), 350);
  const search = useQuery({
    queryKey: ["wporg-plugins", debouncedSlug],
    queryFn: () => wpOrgSearchPlugins(debouncedSlug),
    enabled: debouncedSlug.length >= 2,
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    retry: false, // offline → fail fast + honest message, no retry spinner
  });
  const showSearch = slug.trim().length >= 2;
  const queue = (s: string, icon: string | null) => {
    const v = s.trim();
    if (!v) return;
    setPending((list) => addPending(list, { slug: v, icon }));
    setSlug("");
    addInputRef.current?.focus();
  };
  // Everything Install applies: queued tags + any un-queued typed slug.
  const installSlugs = [
    ...pending.map((t) => t.slug),
    ...(slug.trim() && !pending.some((t) => t.slug === slug.trim()) ? [slug.trim()] : []),
  ];

  const { plugins, isLoading, isError, error, refetch, isFetching } = useWpPlugins(siteId);

  // wp.org icons for the installed list (same source as the live search), with
  // paid plugins answered by their free counterpart's art (core/wporg.rs).
  // Backend caches per app run; failures just mean letter tiles.
  const slugKey = plugins.map((p) => p.name).sort().join(",");
  const { data: iconMap } = useQuery({
    queryKey: ["wporg-plugin-icons", slugKey],
    queryFn: () => wpOrgPluginIcons(plugins.map((p) => p.name)),
    enabled: plugins.length > 0,
    staleTime: 3_600_000,
    refetchOnWindowFocus: false,
    retry: false,
  });

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
    // BOTH, because the row shows both — and the one a person types is the one
    // they can SEE. This matched the slug alone, so searching "loopback" for
    // the row reading "rexenv loopback DNS" (slug `rexenv-dns`) answered "No
    // plugins match", and so did "yoast" for `wordpress-seo`. A filter that
    // searches a field the row does not display is indistinguishable from a
    // broken filter.
    if (q && !p.name.toLowerCase().includes(q) && !p.title.toLowerCase().includes(q))
      return false;
    return true;
  });

  // Same contract as the themes panel: the action names its own outcome.
  const run = useMutation({
    mutationFn: (a: Action) => a.fn(),
    onSuccess: (_r, a) => {
      if (a.done) toast.success(a.done);
      setSelected(new Set());
      qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] });
    },
    onError: (e) => toastBackendError(e),
  });

  // An update is the one plugin op that takes tens of seconds (WooCommerce and
  // Elementor download tens of MB), so it runs on its own stream rather than
  // the shared `run` above, which could only say "busy".
  const upd = useUpdateStream("plugins", siteId, (names) => wpPluginUpdate(siteId, names), (_r, names) => {
    setSelected(new Set());
    settleAfterUpdate<WpPlugin>(qc, ["wp-plugins", siteId], names);
  });
  const busy = run.isPending || upd.pending;

  const toggleSel = (name: string) =>
    setSelected((s) => {
      const next = new Set(s);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });
  const selNames = [...selected];

  // Select-all over the VISIBLE, selectable rows (must-use/drop-ins can't be
  // acted on, so they never enter the selection).
  const selectable = visible
    .filter((p) => p.status !== "must-use" && p.status !== "dropin")
    .map((p) => p.name);
  const allSelected = selectable.length > 0 && selectable.every((n) => selected.has(n));
  const someSelected = selectable.some((n) => selected.has(n));
  const toggleAll = () => setSelected(allSelected ? new Set() : new Set(selectable));

  const [source, setSource] = useState<AddSource>("wporg");
  const gitAssets = useQuery({
    queryKey: ["repo-assets", siteId],
    queryFn: () => repoAssets(siteId),
    staleTime: 30_000,
    // A git read on focus, like its neighbours repo-unmanaged/repo-jobs: opted out.
    refetchOnWindowFocus: false,
  });
  const gitDirs = useMemo(
    () => new Set((gitAssets.data ?? []).filter((a) => a.kind === "plugin").map((a) => a.dirName)),
    [gitAssets.data],
  );
  const assetFor = (name: string) =>
    (gitAssets.data ?? []).find((a) => a.kind === "plugin" && a.dirName === name);
  const refreshAfterGit = () => {
    qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] });
    qc.invalidateQueries({ queryKey: ["repo-assets", siteId] });
    qc.invalidateQueries({ queryKey: ["repo-unmanaged", siteId, "plugin"] });
  };
  // Manually-cloned checkouts with no provenance row → quiet "git?" chips.
  const unmanaged = useQuery({
    queryKey: ["repo-unmanaged", siteId, "plugin"],
    queryFn: () => repoUnmanaged(siteId, "plugin"),
    staleTime: 30_000,
    refetchOnWindowFocus: false,
  });
  const unmanagedSet = useMemo(
    () => new Set((unmanaged.data ?? []).map((u) => u.dirName)),
    [unmanaged.data],
  );

  /** wp-admin's "Replace current with uploaded", reached the same way: the
   *  install refused because `dir` is already there, and this re-runs the SAME
   *  zip with `--force`.
   *
   *  The confirm exists for a hazard wp-admin does not have. rexenv knows which
   *  of these directories are GIT CHECKOUTS — it put some of them there — and
   *  `--force` unpacks straight over the working tree: uncommitted work, the
   *  branch, `.git` itself. So a tracked (or merely git-looking) target is
   *  named before anything is overwritten; everything else replaces on the
   *  click, which is the confirmation wp-admin's own button is.
   *
   *  The retry keeps the job's OWN source. A zip re-runs as a zip (its paths
   *  are still on disk); a wp.org job re-runs as wp.org — sending it back down
   *  the zip gate would fail `ensure_zip_paths` on a slug and read as a second,
   *  unrelated error. */
  const replaceExisting = async (dir: string) => {
    const job = install.job;
    if (!job) return;
    const tracked = gitDirs.has(dir) || unmanagedSet.has(dir);
    if (
      tracked &&
      !(await confirm({
        title: `Replace ${dir}?`,
        message:
          `${dir} is a git checkout in this site. Replacing it unpacks the zip over the ` +
          `working tree — uncommitted changes, the branch and the repository itself go with it. ` +
          `This cannot be undone from rexenv.`,
        danger: true,
        confirmLabel: "Replace",
      }))
    )
      return;
    try {
      install.start(
        await wpInstallJob(siteId, "plugin", job.slugs, activateOnAdd, job.source, true),
      );
    } catch (e) {
      toastBackendError(e);
    }
  };
  const [openRepo, setOpenRepo] = useState<string | null>(null);
  const adoptRepo = async (name: string) => {
    if (
      await confirm({
        title: `Manage "${name}" in rexenv?`,
        message:
          "This folder looks like a git checkout. Adopting records its remote and branch so rexenv can show its repo state — nothing on disk changes.",
        confirmLabel: "Adopt",
      })
    ) {
      try {
        await repoAdopt(siteId, "plugin", name);
        refreshAfterGit();
        setOpenRepo(name);
      } catch (e) {
        toastBackendError(e);
      }
    }
  };
  /** Delete confirm that NAMES what dies for git checkouts (status-driven:
   *  changed/untracked/unpushed) — a git dir must never vanish generically. */
  const confirmDelete = async (title: string, names: string[]): Promise<boolean> => {
    const gitOnes = names.filter((n) => gitDirs.has(n) || unmanagedSet.has(n));
    let message: React.ReactNode;
    if (gitOnes.length > 0) {
      const lines: string[] = [];
      for (const n of gitOnes) {
        if (assetFor(n)?.source === "linked") {
          // Unlink-only path: nothing is lost — the real checkout stays.
          lines.push(`${n}: LINKED folder — removes only the link; your original folder stays untouched.`);
          continue;
        }
        try {
          const st = await repoAssetStatus(siteId, "plugin", n);
          lines.push(`${n}: ${st.lossWarning ?? "clean and pushed — nothing at risk."}`);
        } catch {
          lines.push(`${n}: git checkout — anything uncommitted will be lost.`);
        }
      }
      message = (
        <div className="space-y-1">
          <div>This deletes a git checkout:</div>
          {lines.map((l) => (
            <div key={l} className="font-mono text-[0.71875rem]">
              {l}
            </div>
          ))}
        </div>
      );
    }
    return confirm({ title, message, danger: true, confirmLabel: "Delete" });
  };
  // Live add-from-Git job for this site? Shared cache with GitAddPanel (it
  // pushes live snapshots in); the poll only carries the dot while the git
  // tab is NOT selected.
  const repoJobs = useQuery({
    queryKey: ["repo-jobs", siteId, "plugin"],
    queryFn: () => repoSiteJobs(siteId, "plugin"),
    refetchOnWindowFocus: false,
    staleTime: 5_000,
    refetchInterval: (q) =>
      (q.state.data ?? []).some((j) => j.steps.some((st) => st.status === "running"))
        ? 2_500
        : false,
  });
  const gitBusy = (repoJobs.data ?? []).some((j) =>
    j.steps.some((st) => st.status === "running"),
  );

  return (
    <div className="flex flex-col gap-3">
      {/* Add: live wp.org search (batch install), or a git repo
          (clone → detect → install → build, streamed). */}
      <div className="relative rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <SourceTabs source={source} onChange={setSource} gitBusy={gitBusy} />
        {source === "git" ? (
          <GitAddPanel siteId={siteId} kind="plugin" onInstalled={refreshAfterGit} />
        ) : source === "link" ? (
          <LinkFolderPanel siteId={siteId} kind="plugin" onInstalled={refreshAfterGit} />
        ) : source === "zip" ? (
          <ZipAddPanel
            siteId={siteId}
            kind="plugin"
            busy={busy || install.running}
            onStarted={install.start}
          />
        ) : (
        <>
        {/* Selected items live ABOVE the input row — the input keeps its full
            width no matter how many are queued (QA). */}
        {pending.length > 0 && (
          <div className="mb-2 flex flex-wrap items-center gap-2">
            {pending.map((t) => (
              <SlugTag
                key={t.slug}
                slug={t.slug}
                icon={t.icon}
                onRemove={() => {
                  setPending((l) => l.filter((x) => x.slug !== t.slug));
                  addInputRef.current?.focus();
                }}
              />
            ))}
          </div>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <input {...TECH_INPUT}
            ref={addInputRef}
            value={slug}
            onChange={(e) => setSlug(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                queue(slug, null);
              } else if (e.key === "Backspace" && slug === "" && pending.length > 0) {
                setPending((l) => l.slice(0, -1));
              }
            }}
            placeholder={pending.length ? "Add another…" : "Search WordPress.org or enter a slug…"}
            className="h-[30px] min-w-[180px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
          />
          {/* Same CHECK class as the list rows: unstyled, this rendered as the
              browser's own ~13px box with no accent colour, visibly smaller
              than every other checkbox on the screen (QA, 11 Aug 2026). */}
          <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
            <input
              type="checkbox"
              checked={activateOnAdd}
              onChange={(e) => setActivateOnAdd(e.target.checked)}
              className={CHECK_INPUT}
            />
            Activate
          </label>
          <button
            className={BTN + " flex items-center gap-1.5"}
            disabled={busy || install.running || installSlugs.length === 0}
            onClick={() => {
              const slugs = installSlugs;
              run.mutate({
                fn: () => wpInstallJob(siteId, "plugin", slugs, activateOnAdd).then(install.start),
                // No `done`: the install runs on its own stream and announces
                // itself when it settles (useWpInstall).
              });
            }}
          >
            <Plus className="h-3.5 w-3.5" />
            Install{installSlugs.length > 1 ? ` (${installSlugs.length})` : ""}
          </button>
        </div>
        {showSearch && (
          <div className="absolute left-0 right-0 top-[calc(100%+4px)] z-20 overflow-hidden rounded-lg border border-rex-border-strong bg-rex-surface-1 shadow-xl">
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
                No plugins match “{slug.trim()}” — if you know the exact slug, press Enter to queue it.
              </div>
            ) : (
              <div className="max-h-[300px] overflow-y-auto">
                {(search.data ?? []).map((p) => (
                  <WpOrgHit key={p.slug} p={p} onPick={() => queue(p.slug, p.icon)} />
                ))}
              </div>
            )}
          </div>
        )}
        </>
        )}
        {/* ONE card for both install sources — wp.org and zip run the same
            job, so a tab switch mid-install must not hide the running work. */}
        {(source === "wporg" || source === "zip") && install.job && (
          <WpInstallCard
            job={install.job}
            lines={install.lines}
            onCancel={() => wpInstallCancel(install.job!.id).catch(toastBackendError)}
            onDismiss={install.dismiss}
            onHoldChange={install.hold}
            onReplace={(dir) => void replaceExisting(dir)}
          />
        )}
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
        <div className="flex-1" />
        <RefreshButton onClick={refetch} busy={isFetching} what="the plugin list" />
      </div>

      {/* Run bar — a multi-plugin update outlives the selection (which clears
          on success), so it gets its own row: how many are done, and which
          plugin the run is actually sitting on. */}
      {upd.updating.length > 1 && (
        <div className="flex items-center gap-3 rounded-lg border border-brand/40 bg-rex-surface-1 p-2.5 text-[0.75rem]">
          <Loader2 className="h-3.5 w-3.5 flex-none animate-spin text-brand" />
          <span className="font-mono text-[0.71875rem] text-rex-text">
            {upd.progress ? `${upd.progress.done} of ${upd.progress.total}` : `0 of ${upd.updating.length}`}
            {upd.progress?.current && (
              <span className="text-rex-text-muted"> · {upd.progress.current}</span>
            )}
          </span>
          <ProgressBar fraction={upd.progress?.fraction ?? 0} className="flex-1" />
          <span className="w-[110px] text-right text-rex-text-muted">
            {upd.progress?.phase ?? "Starting"}
          </span>
        </div>
      )}

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
          <button className={BTN} disabled={busy} onClick={() => run.mutate({ fn: () => wpPluginActivate(siteId, selNames), done: `Activated ${subject(selNames, "plugin")}` })}>
            Activate
          </button>
          <button className={BTN} disabled={busy} onClick={() => run.mutate({ fn: () => wpPluginDeactivate(siteId, selNames), done: `Deactivated ${subject(selNames, "plugin")}` })}>
            Deactivate
          </button>
          <button className={BTN} disabled={busy} onClick={() => upd.start(selNames)}>
            Update
          </button>
          <button
            className={BTN + " hover:border-status-error-border hover:text-status-error-bright"}
            disabled={busy}
            onClick={async () => {
              if (await confirmDelete(`Delete ${selNames.length} plugin(s)?`, selNames))
                run.mutate({ fn: () => wpPluginDelete(siteId, selNames), done: `Deleted ${subject(selNames, "plugin")}` });
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
          <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2 font-mono text-[0.625rem] uppercase tracking-[0.1em] text-rex-text-muted">
            <input
              type="checkbox"
              checked={allSelected}
              ref={(el) => {
                if (el) el.indeterminate = !allSelected && someSelected;
              }}
              onChange={toggleAll}
              disabled={selectable.length === 0}
              aria-label="Select all plugins"
              title="Select all"
              className={CHECK_INPUT}
            />
            <span className="flex-1">Plugin</span>
            <span className="w-[150px]">Status</span>
          </div>
          {visible.map((p) => (
            <Fragment key={p.name}>
            <PluginRow
              p={p}
              siteId={siteId}
              git={gitDirs.has(p.name)}
              unmanaged={unmanagedSet.has(p.name)}
              onGitClick={() =>
                gitDirs.has(p.name)
                  ? setOpenRepo((cur) => (cur === p.name ? null : p.name))
                  : adoptRepo(p.name)
              }
              icon={iconMap?.[p.name] ?? null}
              selected={selected.has(p.name)}
              busy={busy}
              onSelect={() => toggleSel(p.name)}
              onActivate={() => run.mutate({ fn: () => wpPluginActivate(siteId, [p.name]), done: `Activated ${p.name}` })}
              onDeactivate={() => run.mutate({ fn: () => wpPluginDeactivate(siteId, [p.name]), done: `Deactivated ${p.name}` })}
              onUpdate={() => upd.start([p.name])}
              updating={upd.rowUpdate(p.name)}
              onTerminal={() => navigate(`/sites/${siteId}/terminal?plugin=${encodeURIComponent(p.name)}`)}
              onDelete={async () => {
                if (await confirmDelete(`Delete plugin "${p.name}"?`, [p.name]))
                  run.mutate({ fn: () => wpPluginDelete(siteId, [p.name]), done: `Deleted ${p.name}` });
              }}
            />
            {openRepo === p.name && assetFor(p.name) && (
              <RepoPanel siteId={siteId} kind="plugin" asset={assetFor(p.name)!} />
            )}
            </Fragment>
          ))}
          </>
        )}
      </div>
    </div>
  );
}

/** Settle the two caches after a SUCCESSFUL update of `names`, then refetch.
 *
 *  Dropping the checked row alone was not enough, and the reason is timing:
 *  the checked pass on a real site takes tens of seconds to over a minute (it
 *  re-checks every plugin against wp.org AND every premium plugin's own update
 *  API), so a pass that started BEFORE the update is usually still in flight
 *  when the update finishes. Its answer describes the OLD disk, and landing
 *  after the drop it put the badge and the arrow straight back — for as long
 *  as the next check took, which is what "still showing minutes later" was.
 *
 *  So: cancel that in-flight check FIRST, then erase what it and the fast list
 *  claim about the updated items (wp-cli exited 0, so they are at the version
 *  it just installed), and only then invalidate — the refetch is now the only
 *  writer left. */
function settleAfterUpdate<T extends { name: string; update: string; updateVersion: string }>(
  qc: ReturnType<typeof useQueryClient>,
  key: unknown[],
  names: string[],
) {
  const forget = (rows: T[] | undefined) =>
    rows?.map((row) =>
      names.includes(row.name) ? { ...row, update: "none", updateVersion: "" } : row,
    );
  // MEASURED 30 Aug 2026 (wk-checks/wpupdate.js, ledger #250): these two are
  // REDUNDANT, and either alone keeps the badge off — the cancel discards the
  // in-flight result, and the invalidate's newer fetch supersedes it inside
  // react-query. With BOTH gone, a late "available"-with-empty-target check
  // does write the cache and the badge comes back with no arrow. The
  // redundancy is deliberate: the invalidate only refetches while something is
  // observing the query, so the cancel is what holds when nothing is.
  void qc.cancelQueries({ queryKey: key }).then(() => {
    qc.setQueryData<T[]>([...key, "updates"], (old) => old?.filter((r) => !names.includes(r.name)));
    qc.setQueryData<T[]>(key, forget);
    qc.invalidateQueries({ queryKey: key });
  });
}

/** Is `next` a strictly newer version than `have`? Compared segment by numeric
 *  segment ("1.1.11" IS newer than "1.1.3.8" — a string compare says the
 *  opposite). Anything non-numeric (`1.2.0-beta1`) can't be ordered, so a mere
 *  difference counts as newer — the conservative answer for an update offer. */
function isNewerVersion(next: string, have: string): boolean {
  const segs = (v: string) => v.split(/[.\-+_]/).map((s) => (/^\d+$/.test(s) ? Number(s) : NaN));
  const a = segs(next);
  const b = segs(have);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (Number.isNaN(x) || Number.isNaN(y)) return next !== have;
    if (x !== y) return x > y;
  }
  return false;
}

/** The ONE place a row's update verdict is decided, for plugins and themes
 *  alike: a row may claim an update only when the offered version is newer
 *  than the one on disk. A claim of "update to X" over an item already AT X is
 *  unrenderable — which is what makes the badge's return after a finished
 *  update impossible rather than merely unlikely. WP-CLI reports such a claim
 *  whenever its source is stale: an in-flight pre-update check, or a premium
 *  plugin's own updater caching its answer for hours. An empty target (the
 *  fast pass can report `available` with no version) can't be ordered, so it
 *  is left alone — the badge shows with no arrow, as before. */
function verdict<T extends { version: string; update: string; updateVersion: string }>(
  row: T,
  claim: { update: string; updateVersion: string },
): T {
  const honest =
    claim.update === "available" &&
    (claim.updateVersion === "" || isNewerVersion(claim.updateVersion, row.version));
  return honest
    ? { ...row, update: claim.update, updateVersion: claim.updateVersion }
    : { ...row, update: "none", updateVersion: "" };
}

/** What the backend tracker calls the single core item (it has no slug), so
 *  the panel starts the run under the same name the stream reports. */
const CORE_ITEM = "WordPress";

/** Plumbing for a STREAMED wp-cli update (plugins, themes or core): the live
 *  phase stream, the mutation that starts the run, and where each item stands.
 *  Shared because all three are the same wp-cli upgrader wearing a noun —
 *  three copies would have been three places for the bar to start lying. */
function useUpdateStream<T>(
  channel: "plugins" | "themes" | "core",
  siteId: string,
  run: (names: string[]) => Promise<T>,
  onDone: (result: T, names: string[]) => void,
) {
  const qc = useQueryClient();
  // `updating` is the argv order we sent, which is the order wp-cli works in.
  const [updating, setUpdating] = useState<string[]>([]);
  const [progress, setProgress] = useState<WpUpdateProgress | null>(null);
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dead = false;
    onWpUpdate(channel, siteId, setProgress).then((un) => (dead ? un() : (stop = un)));
    return () => {
      dead = true;
      stop?.();
    };
  }, [channel, siteId]);
  const mutation = useMutation({
    mutationFn: (names: string[]) => {
      setUpdating(names);
      setProgress(null);
      return run(names);
    },
    // Braces, NOT `onSuccess: onDone` — react-query AWAITS a promise returned
    // from this callback, so a caller that ended with `qc.invalidateQueries()`
    // (an implicit-return arrow) kept the mutation pending, and the bar sat at
    // 100% for the whole slow wp.org re-check before vanishing. Discarding the
    // return here means no caller can block the bar by accident.
    onSuccess: (result, names) => {
      // wp-cli exits non-zero if any item failed, so reaching here means every
      // requested item updated — the count is safe to say. (A failure lands in
      // onError, which surfaces wp-cli's own message.)
      toast.success(
        channel === "core"
          ? "WordPress core updated"
          : `Updated ${subject(names, channel === "plugins" ? "plugin" : "theme")}`,
      );
      onDone(result, names);
    },
    onError: (e) => toastBackendError(e),
    onSettled: () => {
      setUpdating([]);
      setProgress(null);
      // A PARTIAL failure ("Only updated 2 of 3 plugins") lands in onError,
      // and the two that did update kept their old version and their badge
      // until a manual Refresh. A re-read, never an optimistic write: the
      // cache says what wp-cli says now.
      void qc.invalidateQueries({ queryKey: [`wp-${channel}`, siteId] });
    },
  });
  /** Where one item is in the run — null when it isn't part of it. Items
   *  before the cursor are done, the cursor's carries wp-cli's phase, the rest
   *  are honestly "Queued" (wp-cli has not touched them yet). */
  const rowUpdate = (name: string): { fraction: number; phase: string } | null => {
    const idx = updating.indexOf(name);
    if (idx < 0) return null;
    if (!progress) return { fraction: 0, phase: "Starting" };
    if (idx < progress.done) return { fraction: 1, phase: "Updated" };
    if (name === progress.current) {
      const step = progress.fraction * Math.max(progress.total, 1) - progress.done;
      return { fraction: Math.min(Math.max(step, 0), 1), phase: progress.phase };
    }
    return { fraction: 0, phase: "Queued" };
  };
  return {
    updating,
    progress,
    rowUpdate,
    pending: mutation.isPending,
    start: (names: string[]) => mutation.mutate(names),
  };
}

/** A determinate bar for a real, reported position — every caller feeds it a
 *  fraction wp-cli announced (items done + the current item's step), never an
 *  elapsed-time guess. */
function ProgressBar({ fraction, className }: { fraction: number; className?: string }) {
  const pct = Math.round(Math.min(Math.max(fraction, 0), 1) * 100);
  return (
    <div
      className={cn("h-1 overflow-hidden rounded-full bg-rex-surface-2", className)}
      role="progressbar"
      aria-valuenow={pct}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div
        className="h-full rounded-full bg-brand transition-[width] duration-300"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

function PluginRow({
  p,
  siteId,
  git,
  unmanaged,
  onGitClick,
  icon,
  selected,
  busy,
  onSelect,
  onActivate,
  onDeactivate,
  onUpdate,
  updating,
  onTerminal,
  onDelete,
}: {
  p: WpPlugin;
  /** The site the row belongs to — the terminal chevron resolves the plugin's
   *  folder from it backend-side. */
  siteId: string;
  git?: boolean;
  unmanaged?: boolean;
  onGitClick?: () => void;
  icon: string | null;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onActivate: () => void;
  onDeactivate: () => void;
  onUpdate: () => void;
  /** Live position in a running update, or null when this row isn't in one. */
  updating?: { fraction: number; phase: string } | null;
  /** Open a shell in this plugin's own folder. */
  onTerminal: () => void;
  onDelete: () => void;
}) {
  const active = p.status === "active" || p.status === "active-network";
  const updatable = p.update === "available";
  /** The version the update installs — empty until the checked pass lands. */
  const target = updatable ? p.updateVersion : "";
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
        aria-label={`Select ${p.name}`}
        className={CHECK_INPUT}
      />
      {/* Icon spans the title + slug lines, wp-admin style; letter tile when
          nothing on wp.org answers for the plugin — a custom one, an mu-plugin or
          drop-in, a paid plugin with no free counterpart — or icons are loading. */}
      {icon ? (
        <img src={icon} alt="" className="h-8 w-8 flex-none rounded-[6px] object-cover" />
      ) : (
        <span className="flex h-8 w-8 flex-none items-center justify-center rounded-[6px] border border-rex-border bg-rex-surface-2 font-mono text-[0.75rem] font-bold text-rex-text-muted">
          {(p.title || p.name).slice(0, 1).toUpperCase() || "?"}
        </span>
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[0.8125rem] font-medium text-rex-text">
            {p.title || p.name}
          </span>
          {git && (
            <button
              type="button"
              onClick={onGitClick}
              className="rounded-full bg-rex-accent-blue-bg px-1.5 py-0.5 font-mono text-[0.625rem] font-medium text-rex-accent-blue transition-colors hover:bg-rex-accent-blue-border"
              title="Git checkout — click for repo state"
            >
              git
            </button>
          )}
          {unmanaged && (
            <button
              type="button"
              onClick={onGitClick}
              className="rounded-full border border-dashed border-rex-accent-blue-border px-1.5 py-0.5 font-mono text-[0.625rem] font-medium text-rex-accent-blue transition-colors hover:border-rex-accent-blue hover:text-rex-accent-blue"
              title="Looks like a git checkout — click to manage it in rexenv"
            >
              git?
            </button>
          )}
          {updatable && (
            <span className="rounded-full bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] font-medium text-status-warning-bright">
              update
            </span>
          )}
        </div>
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
          {p.name}
          {/* Drop-ins/mu often have no version — show nothing, never a bare "v". */}
          {p.version && <span> · v{p.version}</span>}
          {/* The arrow only appears once the update-check pass supplied a real
              target — never invent one from the badge alone. */}
          {p.version && target && <span className="text-status-warning-bright"> → {target}</span>}
        </div>
      </div>
      {/* Updating beats the button: the click is spent, and what the user
          needs now is which wp-cli step this plugin is on. */}
      {updating ? (
        <div className="flex w-[132px] flex-none flex-col gap-1">
          <ProgressBar fraction={updating.fraction} />
          <span className="truncate font-mono text-[0.625rem] text-rex-text-muted">
            {updating.phase}
          </span>
        </div>
      ) : (
        updatable && (
          <button
            className={BTN + " flex items-center gap-1"}
            disabled={busy}
            onClick={onUpdate}
            title={target ? `Update to ${target}` : "Update"}
          >
            <ArrowUpCircle className="h-3.5 w-3.5" />
          </button>
        )
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
      {/* Must-use plugins and drop-ins load from a FILE in the content dir and
          have no folder of their own — offering a terminal "in this plugin"
          would land somewhere that isn't it. */}
      {!immutable && (
        <AssetTerminalButton siteId={siteId} kind="plugin" name={p.name} onTerminal={onTerminal} />
      )}
      <button
        className={cn(
          BTN,
          immutable
            ? "cursor-not-allowed opacity-45"
            : "hover:border-status-error-border hover:text-status-error-bright",
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
