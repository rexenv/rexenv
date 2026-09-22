import { useEffect, useState, useSyncExternalStore } from "react";
import { useSearchParams } from "react-router-dom";
import { toast, toastBackendError } from "@/lib/toast";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { confirm, Overlay } from "@/components/ui/dialog";
// Bundled verbatim at build time (`?raw`) so the app can show its own legal
// text offline — the About row must not depend on a website or the repo
// being reachable (or public).
import licenseText from "../../LICENSE?raw";
import noticesText from "../../THIRD-PARTY-NOTICES.md?raw";
import { ResolverConsentFor, ResolverHandBackRow } from "@/routes/Import";
import { agoLabel } from "@/routes/Sites";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpCircle, ArrowUpRight, Bot, CheckCircle2, ChevronRight, Code, Download, FileText, FolderOpen, Github, Globe, Info, Lock, ScrollText, Server, Settings as SettingsIcon, Settings2, Shield, ShieldCheck, Star, Trash2, type LucideIcon } from "lucide-react";
import { CHECK_INPUT, cn, TECH_INPUT } from "@/lib/utils";
import { eolNote, eolWhen } from "@/lib/php";
import { TopBar } from "@/components/shell/TopBar";
import { AppUpdateCard } from "@/components/settings/AppUpdateCard";
import { Button } from "@/components/ui/button";
import { CopyButton } from "@/components/ui/copy-button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { RexLogo } from "@/components/common/RexLogo";
import { AgentsMcpCard } from "@/components/mcp/AgentsMcpCard";
import { AppPicker, type AppChoice } from "@/components/ui/app-picker";
import { useBrowsers } from "@/lib/useBrowser";
import {
  applyPhpSettings,
  autostartStatus,
  cliInstall,
  cliStatus,
  defaultTld,
  deleteBlueprint,
  dnsStatus,
  repairResolver,
  unresolvableTlds,
  firefoxTrustStatus,
  getAppInfo,
  getPhpSettings,
  getSetting,
  mailCatchAll,
  setMailCatchAll,
  listBlueprints,
  listEditors,
  listPhpVersions,
  openExternal,
  pickFolder,
  regenerateCerts,
  saveBlueprint,
  setAutostart,
  setDefaultPhpVersion,
  setDefaultTld,
  setPhpVersionInstalled,
  setSetting,
  sitesFolder,
  tldPolicy,
  trustCaInFirefox,
  allowTldsInFirefox,
  trustLocalCa,
  wpCliPackages,
  scanValetImport,
  uninstallSystem, phpUpdateApply, phpUpdateCheck } from "@/lib/ipc";
import { getStoredTheme, setTheme, subscribeTheme, type Theme } from "@/lib/theme";
import type { AppInfo, Blueprint, MultisiteMode, PhpSetting, PhpVersion, DownloadsSnapshot } from "@/types";

const SITES_DIR_KEY = "sites_dir";
// Mirrors commands::services::AUTO_START_SETTING — the opt-in "run Start all
// when rexenv opens" behavior (login-start when combined with app autostart).
const AUTO_START_KEY = "start_services_on_launch";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
      <div className="mb-3.5 text-[0.875rem] font-semibold text-rex-text">{title}</div>
      {children}
    </div>
  );
}

/** A styled success notice (the design's toast look, inline). */
function Notice({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2.5 rounded-[11px] border border-rex-border-strong border-l-[3px] border-l-status-running bg-rex-surface-2 px-3 py-2.5">
      <CheckCircle2 className="h-4 w-4 flex-none text-status-running-bright" strokeWidth={2} />
      <span className="font-mono text-[0.71875rem] text-rex-text-bright">{children}</span>
    </div>
  );
}

function ThemeSetting() {
  const words = usePlatformWords();
  // Shared store (lib/theme) — stays in sync with the sidebar ThemeToggle.
  const theme = useSyncExternalStore(subscribeTheme, getStoredTheme);
  const choose = setTheme; // persists + applies + notifies immediately
  // Light stays selectable — it shipped in §4.4 (the comp's "soon" badge is stale).
  // Preview swatches DEPICT each theme, so they are intentionally literal —
  // routing them through theme vars would make the Light tile flip in dark mode.
  const TILES: { value: Theme; label: string; preview: string; bars?: "dark" | "light" }[] = [
    { value: "dark", label: "Dark", preview: "linear-gradient(135deg,#15171D,#0D0E12)", bars: "dark" },
    { value: "light", label: "Light", preview: "linear-gradient(135deg,#F4F5F8,#E2E5EC)", bars: "light" },
    { value: "system", label: "System", preview: "linear-gradient(115deg,#15171D 0 50%,#E2E5EC 50% 100%)" },
  ];
  return (
    <>
      <div className="mb-[14px] text-[0.78125rem] text-rex-text-muted">
        Choose how rexenv looks. System follows your {words.osName} appearance.
      </div>
      <div className="grid grid-cols-3 gap-[10px]">
        {TILES.map((t) => {
          const selected = theme === t.value;
          return (
            <button
              key={t.value}
              onClick={() => choose(t.value)}
              className={cn(
                "overflow-hidden rounded-[11px] border text-left transition-colors",
                selected ? "border-brand" : "border-rex-border-subtle hover:border-rex-border-strong",
              )}
            >
              <div
                className="relative h-[62px] border-b border-rex-border-subtle"
                style={{ background: t.preview }}
              >
                {t.bars && (
                  <>
                    <span
                      className="absolute left-[9px] top-[9px] h-[6px] w-[30px] rounded-[3px]"
                      style={{ background: t.bars === "light" ? "#C2C6D0" : "#2A2E38" }}
                    />
                    <span
                      className="absolute left-[9px] top-[20px] h-[6px] w-[46px] rounded-[3px]"
                      style={{ background: t.bars === "light" ? "#D6D9E0" : "#1E222A" }}
                    />
                  </>
                )}
                <span className="absolute bottom-[9px] right-[9px] h-[18px] w-[18px] rounded-[5px] bg-brand" />
              </div>
              <div className="flex items-center justify-between px-[11px] py-[9px]">
                <span className="text-[0.78125rem] font-medium text-rex-text">{t.label}</span>
                <CheckCircle2
                  className="h-[15px] w-[15px]"
                  style={{ color: selected ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
                  strokeWidth={2}
                />
              </div>
            </button>
          );
        })}
      </div>
    </>
  );
}

/** General prefs card: compact Default-PHP select + Sites-folder picker. */
function GeneralPrefsCard() {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data: versions = [] } = useQuery({ queryKey: ["php-versions"], queryFn: listPhpVersions });
  const installed = versions.filter((v) => v.installed);
  const currentPhp = versions.find((v) => v.isDefault)?.minor ?? installed[0]?.minor ?? "";
  const setDefault = useMutation({
    mutationFn: (minor: string) => setDefaultPhpVersion(minor),
    onSuccess: () => {
      // Services' PHP rows show the default too — keep both views in sync.
      void qc.invalidateQueries({ queryKey: ["php-versions"] });
      void qc.invalidateQueries({ queryKey: ["services"] });
    },
    onError: (e) => toastBackendError(e),
  });

  // "Open in editor" target (Sites row menu). Auto = first detected editor.
  const { data: editors = [] } = useQuery({ queryKey: ["editors"], queryFn: listEditors });
  const { data: preferredEditor } = useQuery({
    queryKey: ["setting", "preferred_editor"],
    queryFn: () => getSetting("preferred_editor"),
  });
  const currentEditor =
    editors.find((e) => e.id === preferredEditor)?.id ?? editors[0]?.id ?? "";
  const saveEditor = useMutation({
    mutationFn: (id: string) => setSetting("preferred_editor", id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["setting", "preferred_editor"] }),
    onError: (e) => toastBackendError(e),
  });

  // Where every http(s) link opens. "" = the OS default handler, which is also
  // what an uninstalled preference falls back to — so the empty choice is a
  // real, first-class entry rather than the absence of one.
  const browsers = useBrowsers();
  const { data: preferredBrowser } = useQuery({
    queryKey: ["setting", "preferred_browser"],
    queryFn: () => getSetting("preferred_browser"),
  });
  const systemDefaultBrowser = browsers.find((b) => b.systemDefault);
  const browserChoices: AppChoice[] = [
    {
      id: "",
      name: "System default",
      icon: systemDefaultBrowser?.icon,
      hint: systemDefaultBrowser ? `· ${systemDefaultBrowser.name}` : undefined,
    },
    ...browsers.map((b) => ({ id: b.id, name: b.name, icon: b.icon })),
  ];
  // A stored browser that has since been uninstalled reads as "System default"
  // — which is exactly what it now DOES (open_external falls back per open).
  const currentBrowser = browsers.some((b) => b.id === preferredBrowser)
    ? preferredBrowser!
    : "";
  const saveBrowser = useMutation({
    mutationFn: (id: string) => setSetting("preferred_browser", id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["setting", "preferred_browser"] }),
    onError: (e) => toastBackendError(e),
  });

  const { data: resolved } = useQuery({ queryKey: ["sites-folder"], queryFn: sitesFolder });
  // The RAW setting (null/blank = using the computed default) — drives the
  // "Reset to default" affordance, which is only shown for a custom folder.
  const { data: customDir } = useQuery({
    queryKey: ["setting", SITES_DIR_KEY],
    queryFn: () => getSetting(SITES_DIR_KEY),
  });
  const saveFolder = useMutation({
    mutationFn: (v: string) => setSetting(SITES_DIR_KEY, v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites-folder"] });
      qc.invalidateQueries({ queryKey: ["setting", SITES_DIR_KEY] });
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
      <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px]">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Default PHP version</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            New sites use this version unless you pick another.
          </div>
        </div>
        <select
          value={currentPhp}
          onChange={(e) => setDefault.mutate(e.target.value)}
          className="h-[34px] rounded-[9px] border border-rex-border-strong bg-rex-well px-3 font-mono text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand"
        >
          {installed.map((v) => (
            <option key={v.minor} value={v.minor}>
              {v.minor}
            </option>
          ))}
        </select>
      </div>
      <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px]">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Code editor</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            "Open in editor" opens a site's folder as a project here.
          </div>
        </div>
        {editors.length === 0 ? (
          <span
            className="font-mono text-[0.71875rem] text-rex-text-muted"
            title={`Looked in ${words.appSearch} for VS Code, Cursor, PhpStorm, Windsurf, Zed, Sublime Text, WebStorm, VSCodium, Nova and TextMate.`}
          >
            No code editor detected
          </span>
        ) : (
          <AppPicker
            ariaLabel="Code editor"
            value={currentEditor}
            choices={editors.map((e) => ({ id: e.id, name: e.name, icon: e.icon }))}
            onChange={(id) => saveEditor.mutate(id)}
            fallbackIcon={<Code className="h-4 w-4 text-rex-text-muted" />}
          />
        )}
      </div>
      <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px]">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Web browser</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            Every link rexenv opens — sites, wp-admin, Mailpit, tunnels — goes here.
          </div>
        </div>
        {browsers.length === 0 ? (
          <span
            className="font-mono text-[0.71875rem] text-rex-text-muted"
            title={`Looked in ${words.appSearch} for Safari, Chrome, Firefox, Brave, Edge, Arc, Opera, Vivaldi, Chromium and friends.`}
          >
            No browser detected
          </span>
        ) : (
          <AppPicker
            ariaLabel="Web browser"
            value={currentBrowser}
            choices={browserChoices}
            onChange={(id) => saveBrowser.mutate(id)}
            fallbackIcon={<Globe className="h-4 w-4 text-rex-text-muted" />}
          />
        )}
      </div>
      <div className="flex items-center gap-[14px] py-[15px]">
        <div className="min-w-0 flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Sites folder</div>
          <div className="mt-0.5 truncate font-mono text-[0.71875rem] text-rex-text-muted">
            {resolved ?? "…"}
          </div>
        </div>
        {!!customDir?.trim() && (
          <button
            onClick={() => saveFolder.mutate("")}
            disabled={saveFolder.isPending}
            title="Use the default folder for new sites — existing sites stay where they are"
            className="flex h-8 flex-none items-center rounded-[9px] px-[9px] text-[0.75rem] text-rex-text-muted transition-colors hover:text-rex-text-bright disabled:opacity-40"
          >
            Reset to default
          </button>
        )}
        <button
          onClick={async () => {
            const v = await pickFolder("Choose your sites folder", resolved ?? undefined);
            if (v && v.trim()) saveFolder.mutate(v.trim());
          }}
          className="flex h-8 flex-none items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover"
        >
          <FolderOpen className="h-3.5 w-3.5" />
          Choose…
        </button>
      </div>
    </div>
  );
}

/**
 * One icon-only action in a PHP row.
 *
 * Icon-only because the four text buttons this replaces ("Make default",
 * "Settings", "Remove", plus an "Installed" word that was not a control at
 * all) read as one undifferentiated grey sentence and pushed every row to two
 * lines. An icon carries the verb faster ONLY if it is the conventional one —
 * gear for settings, trash for remove, star for default — and only if the
 * label still exists for the reader who is unsure: `title` on hover, and
 * `aria-label` for anyone who never sees the icon at all.
 */
function PhpIconAction({
  icon,
  label,
  danger,
  active,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  danger?: boolean;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] text-rex-text-muted transition-colors",
        "hover:bg-rex-hover hover:text-rex-text disabled:pointer-events-none disabled:opacity-40",
        active && "bg-rex-hover text-rex-text",
        danger && "hover:text-status-error-bright",
      )}
    >
      {icon}
    </button>
  );
}

function PhpVersionRow({
  v,
  busy,
  expanded,
  onToggle,
  onMakeDefault,
  onUpdate,
  onExpand,
}: {
  v: PhpVersion;
  busy: boolean;
  expanded: boolean;
  onToggle: (installed: boolean) => void;
  onMakeDefault: () => void;
  onUpdate: () => void;
  onExpand: () => void;
}) {
  const words = usePlatformWords();
  return (
    <div className="border-b border-rex-border-subtle last:border-b-0">
      {/* flex-wrap: at the 980px min window the action cluster is wider than
          the row can spare — let it reflow under the version name instead of
          crushing it. The icon cluster is narrow enough that the ordinary row
          now stays on ONE line, which is the whole point of the icons. */}
      <div
        className="flex flex-wrap items-center gap-x-3 gap-y-1.5 py-2"
        // The L2 harness asserts PER STATE (post-update, button-and-chip,
        // chip-only, not-installed) and cannot tell the rows apart from text —
        // "8.2.32" appears in a chip, a button and a tooltip.
        data-probe="php-row"
        data-minor={v.minor}
        data-updatable={v.updatable ?? ""}
        data-upstream={v.upstream ?? ""}
        data-installed={v.installed ? "1" : ""}
      >
        {/* Installed/not is a STATE, so it reads as one: a dot, not the word
            "Installed" sitting in the button row pretending to be pressable.
            The dot is titled, because a colour alone is not a fact anyone can
            read off a screenshot. */}
        <span
          title={
            v.installed
              ? `PHP ${v.minor} is installed and runs its own ${words.poolKind}.`
              : `PHP ${v.minor} is not installed.`
          }
          className={cn(
            "h-[7px] w-[7px] flex-none rounded-full",
            v.installed ? "bg-status-running" : "border border-rex-border-strong",
          )}
        />
        {/* A FLEX row with wrapping, not inline spans with `ml-2`. Five chips
            never fit 9rem, and as inline content the badges wrapped INTERNALLY —
            "EOL" on one line and "November 2022" on the next, each carrying half
            the pill's border, which reads as a rendering bug rather than as a
            long label. Each chip is `whitespace-nowrap` so it wraps as a UNIT,
            and the gap survives the wrap where a left margin does not. */}
        <div
          data-probe="php-row-chips"
          className="flex min-w-0 flex-1 basis-[13rem] flex-wrap items-center gap-x-2 gap-y-1"
        >
          <span className="whitespace-nowrap font-mono text-[0.8125rem] text-rex-text">
            PHP {v.minor}
          </span>
          <span className="whitespace-nowrap font-mono text-[0.6875rem] text-rex-text-muted">
            {v.patch}
          </span>
          {/* The pool is running bytes this build does not pin — say so, rather
              than rendering the pin and letting it read as what is serving. Only
              ever present when the two genuinely disagree (core sends `null`
              otherwise), so the ordinary row grows nothing. */}
          {v.serving && (
            <span
              className="whitespace-nowrap font-mono text-[0.6875rem] text-rex-accent-amber"
              title={`PHP ${v.minor} is set to ${v.patch}. The running pool is still on ${v.serving} — it restarts on the next launch, or on Start all.`}
            >
              serving {v.serving}
            </span>
          )}
          {/* An upstream FACT, not an offer. rexenv installs verified builds
              from static-php.dev, which trails php.net by weeks, so this can
              name a version rexenv has no build of — "exists" stays true where
              "update available" would not (docs/archive/PLAN-binary-updates.md §13).
              Suppressed when the Update button already names the same version:
              a chip saying 8.2.32 "exists" next to a button offering 8.2.32
              reads as two different versions, and the whole point of the chip
              is that it is the one there is NO button for. */}
          {v.upstream && v.upstream !== v.updatable && (
            <span
              className="whitespace-nowrap font-mono text-[0.6875rem] text-rex-text-muted"
              title={
                v.updatable
                  ? `php.net lists ${v.upstream} as the newest ${v.minor} release. rexenv can install ${v.updatable} today — ${v.upstream} arrives once a verified build of it is published.`
                  : `php.net lists ${v.upstream} as the newest ${v.minor} release. rexenv installs verified builds, so it appears here once one is published.`
              }
            >
              · {v.upstream} exists
            </span>
          )}
          {v.isDefault && (
            <span
              title="New sites use this version unless they pick their own."
              className="flex items-center gap-1 whitespace-nowrap rounded border border-brand/40 bg-brand/10 px-1.5 py-0.5 text-[0.625rem] font-medium text-brand"
            >
              <Star className="h-[9px] w-[9px] fill-current" strokeWidth={2} />
              Default
            </span>
          )}
          {/* An offered runtime that receives no security fixes says so HERE,
              where it is chosen — not in a doc. rexenv shipped 8.0 from Nov
              2023 and 8.1 from Dec 2025 with no tell at all. */}
          {v.eolSince && (
            <span
              className="whitespace-nowrap rounded border border-status-warning-border bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] font-medium text-status-warning-bright"
              title={eolNote(v.minor, v.eolSince)}
            >
              EOL {eolWhen(v.eolSince)}
            </span>
          )}
          {/* An update that TAKES SOMETHING AWAY gets a visible tell, not just a
              tooltip: the offer beside it is a version number, which looks
              identical whether it is strictly better or not. The sentence comes
              from core — one source, because the rule (upstream's builds have no
              PostgreSQL driver) is not obvious enough to restate here. */}
          {v.updateCost && (
            <span
              className="whitespace-nowrap rounded border border-status-warning-border bg-status-warning-bg px-1.5 py-0.5 text-[0.625rem] font-medium text-status-warning-bright"
              title={v.updateCost}
              data-update-cost="postgres"
            >
              costs PostgreSQL
            </span>
          )}
        </div>
        {v.installed ? (
          <div className="flex flex-none items-center gap-1">
            {/* The one control that installs bytes this build was not shipped
                with. Present ONLY when a VERIFIED manifest offers a newer patch
                for a minor the user actually has — never for `upstream`, which
                is php.net saying a release exists and which rexenv may have no
                build of. Stays a WORDED button among the icons on purpose: it
                is the only action here that downloads ~100 MB and restarts a
                pool serving live sites, and it names the version it will move
                to. No auto-update, ever. */}
            {v.updatable && (
              <Button
                size="sm"
                variant="secondary"
                disabled={busy}
                onClick={onUpdate}
                title={
                  // The cost, when there is one, goes in front of the mechanics:
                  // an update that REMOVES something is not the same offer, and
                  // the sentence is core's (`php::update_cost`) rather than a
                  // second copy of the rule living here.
                  (v.updateCost ? `${v.updateCost}\n\n` : "") +
                  `Download PHP ${v.updatable}, restart the ${v.minor} pool onto it, and put it back on ${v.patch} if it does not come up. Your sites keep their ${v.minor} setting either way.`
                }
                // Selected STRUCTURALLY by the WebKit probe. It matched
                // `/^Update to /` on the label, which this button has not said
                // for some time — and nothing noticed, because no fixture row
                // carried `updatable`, so the branch never ran. A guard that
                // cannot fire and is wrong about what it looks for is two
                // defects agreeing with each other (ledger #337's shape).
                data-probe="php-update"
                className="mr-1 h-[26px] gap-1.5 px-2.5 text-[0.75rem] text-brand-light"
              >
                <ArrowUpCircle className="h-3.5 w-3.5" strokeWidth={1.8} />
                {busy ? "…" : v.updatable}
              </Button>
            )}
            {/* New sites use the default version; let the user move it (§4.4).
                The default row keeps its chip and shows NO star button — there
                is nothing to press, and a lit star that does nothing is the
                oldest way to lie about a control. */}
            {!v.isDefault && (
              <PhpIconAction
                icon={<Star className="h-[15px] w-[15px]" strokeWidth={1.7} />}
                label={`Make PHP ${v.minor} the default for new sites`}
                disabled={busy}
                onClick={onMakeDefault}
              />
            )}
            <PhpIconAction
              icon={<Settings2 className="h-[15px] w-[15px]" strokeWidth={1.7} />}
              label={`PHP ${v.minor} settings — memory_limit, upload size, execution time`}
              active={expanded}
              onClick={onExpand}
            />
            {/* The default cannot be removed: something has to serve new sites.
                Hidden rather than disabled — a greyed trash on one row out of
                seven asks the reader to work out why. */}
            {!v.isDefault && (
              <PhpIconAction
                icon={<Trash2 className="h-[15px] w-[15px]" strokeWidth={1.7} />}
                label={`Remove PHP ${v.minor} and stop its pool`}
                danger
                disabled={busy}
                onClick={() => onToggle(false)}
              />
            )}
          </div>
        ) : (
          <Button
            size="sm"
            variant="primary"
            disabled={busy}
            onClick={() => onToggle(true)}
            className="h-[26px] flex-none gap-1.5 px-2.5 text-[0.75rem]"
          >
            <Download className="h-3.5 w-3.5" strokeWidth={1.8} />
            {busy ? "…" : "Install"}
          </Button>
        )}
      </div>
      {expanded && <PhpIniSettingsEditor minor={v.minor} />}
    </div>
  );
}

/** Per-version ini overrides for the shared php-fpm pool (memory_limit etc.).
 *  Empty field = rexenv's default (shown as placeholder), which is written into
 *  the pool config too. Save validates on the backend, gates the rewritten pool config on
 *  `php-fpm -t`, restarts the pool, and reloads nginx so its upload body limit
 *  tracks upload_max_filesize/post_max_size. */
function PhpIniSettingsEditor({ minor }: { minor: string }) {
  const qc = useQueryClient();
  const { data: settings = [], isLoading } = useQuery({
    queryKey: ["php-settings", minor],
    queryFn: () => getPhpSettings(minor),
  });
  // Draft edits keyed by ini key; a key absent from the draft shows the stored value.
  const [draft, setDraft] = useState<Record<string, string>>({});
  const current = (s: PhpSetting) => draft[s.key] ?? s.value ?? "";
  const dirty = settings.some((s) => current(s) !== (s.value ?? ""));
  const apply = useMutation({
    mutationFn: () =>
      applyPhpSettings(
        minor,
        settings
          .map((s) => ({ key: s.key, value: current(s).trim() }))
          .filter((s) => s.value !== ""),
      ),
    onSuccess: () => {
      toast.success(`PHP ${minor} settings applied — pool restarted`);
      setDraft({});
      qc.invalidateQueries({ queryKey: ["php-settings", minor] });
    },
    onError: (e) => toastBackendError(e),
  });

  if (isLoading) {
    return <div className="pb-3 text-[0.75rem] text-rex-text-muted">Loading…</div>;
  }
  return (
    <div className="mb-2.5 rounded-[9px] border border-rex-border-subtle bg-rex-well/50 p-3">
      <div className="grid grid-cols-2 gap-x-4 gap-y-2.5 sm:grid-cols-3">
        {settings.map((s) => (
          <div key={s.key}>
            <label className="mb-1 block font-mono text-[0.6875rem] text-rex-text-muted">
              {s.key}
            </label>
            <input {...TECH_INPUT}
              value={current(s)}
              placeholder={s.default}
              onChange={(e) => setDraft((d) => ({ ...d, [s.key]: e.target.value }))}
              className="h-[30px] w-full rounded-[7px] border border-rex-border-strong bg-rex-well px-[9px] font-mono text-[0.75rem] text-rex-text outline-none transition-colors placeholder:text-rex-text-muted focus:border-brand"
            />
          </div>
        ))}
      </div>
      <div className="mt-3 flex items-center gap-3">
        <Button variant="primary" disabled={!dirty || apply.isPending} onClick={() => apply.mutate()}>
          {apply.isPending ? "Applying…" : "Save & restart pool"}
        </Button>
        <div className="text-[0.6875rem] leading-snug text-rex-text-muted">
          Empty = rexenv's default (placeholder), written to the pool. Applies to every nginx-served site on PHP {minor};
          FrankenPHP sites use their own embedded PHP. Requests are still recycled after 5&nbsp;min
          wall-clock unless max_execution_time is set higher (0 keeps the 5-min cap).
        </div>
      </div>
    </div>
  );
}

export function PhpVersionsSetting() {
  const words = usePlatformWords();
  const qc = useQueryClient();
  const { data: versions = [], isLoading } = useQuery({
    queryKey: ["php-versions"],
    queryFn: listPhpVersions,
  });
  // Refresh the SIGNED manifest when this section opens, so a manifest published
  // since launch is offered without a restart. The launch path does this too; this
  // is the "I just published one" case.
  //
  // Its own query rather than a call inside the one above: a failure here must not
  // fail the LIST. `retry: false` because a poll nobody asked for should not
  // hammer, and a stale window so re-opening Settings does not re-fetch.
  useQuery({
    queryKey: ["php-update-check"],
    queryFn: async () => {
      const rows = await phpUpdateCheck();
      // The check returns the fresh rows; publish them to the list's cache rather
      // than invalidating, which would re-run the list query for data we hold.
      qc.setQueryData(["php-versions"], rows);
      return rows;
    },
    retry: false,
    staleTime: 5 * 60 * 1000,
  });
  const toggle = useMutation({
    mutationFn: ({ minor, installed }: { minor: string; installed: boolean }) =>
      setPhpVersionInstalled(minor, installed),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["php-versions"] }),
    onError: (e) => toastBackendError(e),
  });
  const makeDefault = useMutation({
    mutationFn: (minor: string) => setDefaultPhpVersion(minor),
    onSuccess: () => {
      // Services' PHP rows show the default too — keep both views in sync.
      void qc.invalidateQueries({ queryKey: ["php-versions"] });
      void qc.invalidateQueries({ queryKey: ["services"] });
    },
    onError: (e) => toastBackendError(e),
  });
  // An apply takes minutes (a ~100 MB download, then a pool restart), so the
  // minors in flight are tracked as a SET rather than read off the mutation.
  // `update.variables` holds only the LAST call's arguments: start 8.2, start
  // 8.3, and 8.2's button re-enables while its download is still running.
  const [applying, setApplying] = useState<ReadonlySet<string>>(new Set());
  const update = useMutation({
    mutationFn: ({ minor, patch }: { minor: string; patch: string }) =>
      phpUpdateApply(minor, patch),
    onSuccess: (out, v) => {
      // "recorded" and "now running" are different facts. When no pool was
      // running there is nothing to be "now on", and saying so names a process
      // that does not exist — the backend reports which one happened.
      toast.success(
        out.restarted
          ? `PHP ${v.minor} is now on ${out.patch}`
          : `PHP ${v.minor} will use ${out.patch} — nothing was running to restart`,
      );
      void qc.invalidateQueries({ queryKey: ["php-versions"] });
      void qc.invalidateQueries({ queryKey: ["services"] });
    },
    onError: (e) => toastBackendError(e),
    onSettled: (_d, _e, v) =>
      setApplying((s) => {
        const next = new Set(s);
        next.delete(v.minor);
        return next;
      }),
  });
  const startUpdate = (minor: string, patch: string) => {
    setApplying((s) => new Set(s).add(minor));
    update.mutate({ minor, patch });
  };
  const busyFor = (minor: string) =>
    (toggle.isPending && toggle.variables?.minor === minor) ||
    (makeDefault.isPending && makeDefault.variables === minor) ||
    applying.has(minor);
  const [expanded, setExpanded] = useState<string | null>(null);

  if (isLoading) {
    return <div className="text-[0.78125rem] text-rex-text-muted">Loading…</div>;
  }
  // The rows the note is FOR: an upstream version with no button beside it.
  // Conditioning on `v.upstream` alone printed "a newer patch arrives with a
  // rexenv update" on a screen with an Update button on it.
  const anyUnbuildableUpstream = versions.some((v) => v.upstream && v.upstream !== v.updatable);
  return (
    <div>
      {/* Stated BEFORE the chips it explains. "8.3.33 exists" with no button
          beside it reads as a half-built feature unless the reader already knows
          there are two projects involved — which the first person to see it did
          not. Rendered only when a chip is actually on screen, so the ordinary
          case gains no paragraph. */}
      {anyUnbuildableUpstream && (
        <div
          data-probe="php-upstream-note"
          className="mb-2.5 rounded border border-rex-border bg-rex-well px-2.5 py-2 text-[0.6875rem] leading-relaxed text-rex-text-muted"
        >
          <span className="text-rex-text">“exists” is not a button.</span> rexenv installs
          checksum-verified builds, which are published some weeks after php.net announces a
          release — so a version can exist upstream with nothing here to install yet.
        </div>
      )}
      {versions.map((v) => (
        <PhpVersionRow
          key={v.minor}
          v={v}
          busy={busyFor(v.minor)}
          expanded={expanded === v.minor}
          onToggle={(installed) => toggle.mutate({ minor: v.minor, installed })}
          onMakeDefault={() => makeDefault.mutate(v.minor)}
          onUpdate={() => v.updatable && startUpdate(v.minor, v.updatable)}
          onExpand={() => setExpanded((e) => (e === v.minor ? null : v.minor))}
        />
      ))}
      {/* The legend, not a tooltip. Icons are only faster than words once the
          reader has met them; hover text reaches the person who already
          suspects what the icon does, and nobody else. Three icons, named
          once, under the rows they belong to. */}
      <div className="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-[0.6875rem] text-rex-text-muted">
        <span className="flex items-center gap-1.5">
          <Star className="h-3 w-3" strokeWidth={1.7} /> make default
        </span>
        <span className="flex items-center gap-1.5">
          <Settings2 className="h-3 w-3" strokeWidth={1.7} /> ini settings
        </span>
        <span className="flex items-center gap-1.5">
          <Trash2 className="h-3 w-3" strokeWidth={1.7} /> remove
        </span>
      </div>
      <div className="mt-1.5 text-[0.6875rem] text-rex-text-muted">
        Installed versions each run a {words.poolKind}; new sites use the default. A site can pick its own
        version in its detail view.
      </div>
      {/* A check that finds nothing must still visibly have run — the Import
          screen's "scanned 12s ago" honesty. Never says "up to date": that is
          unprovable before the first successful check, and false whenever
          static-php.dev lags php.net (which it does, by weeks). The WHY moved
          ABOVE the rows: as a footer under seven rows it did not reach the first
          person to read an "exists" chip, who took the missing button for a
          broken feature. An explanation has to precede the thing it explains. */}
      <div className="mt-1 text-[0.6875rem] text-rex-text-muted">
        {versions[0]?.upstreamCheckedAt
          ? `Release list from php.net, checked ${agoLabel(versions[0].upstreamCheckedAt)}.`
          : "Couldn't reach php.net yet, so nothing here says whether a newer patch exists."}
      </div>
    </div>
  );
}

function ActionRow({
  title,
  desc,
  busy,
  label,
  busyLabel,
  onClick,
}: {
  title: string;
  desc: string;
  busy: boolean;
  label: string;
  busyLabel?: string;
  onClick: () => void;
}) {
  return (
    <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px] last:border-b-0">
      <div className="flex-1">
        <div className="text-[0.84375rem] font-medium text-rex-text">{title}</div>
        <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">{desc}</div>
      </div>
      <button
        onClick={onClick}
        disabled={busy}
        className="flex h-8 flex-none items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover disabled:opacity-60"
      >
        {busy && (
          <span className="h-3 w-3 rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
        )}
        {busy ? (busyLabel ?? label) : label}
      </button>
    </div>
  );
}

function DnsSslSetting() {
  const words = usePlatformWords();
  const qc = useQueryClient();
  // Poll on the Services cadence: the shared ["dns-status"] key is only
  // refetched by MOUNTED observers, and Services (the other poller) unmounts
  // when this page shows — without an interval here the tile freezes at
  // mount-time state and never reflects an agent → in-process fallback.
  const { data: dns } = useQuery({
    queryKey: ["dns-status"],
    queryFn: dnsStatus,
    refetchInterval: 5000,
  });
  // The card copy shows the USER'S configured TLD (changeable below), not a literal.
  const { data: tld = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });
  const [msg, setMsg] = useState<string | null>(null);

  const trust = useMutation({
    mutationFn: trustLocalCa,
    onSuccess: () => {
      setMsg(`Local CA re-trusted in your ${words.trustStore}.`);
      void qc.invalidateQueries({ queryKey: ["dns-status"] });
    },
    onError: (e) => toastBackendError(e),
  });
  const regen = useMutation({
    mutationFn: regenerateCerts,
    onSuccess: (n) => setMsg(`Regenerated ${n} site certificate${n === 1 ? "" : "s"}.`),
    onError: (e) => toastBackendError(e),
  });

  const dnsActive = !!dns?.running && !!dns?.resolverInstalled;
  // Mode is part of the status, not a technical label: agent = the goal state
  // (survives app quits), in-process = honest DEGRADED fallback (DNS dies with
  // the app — amber, never green), down = fault. See DnsStatus.mode.
  const dnsDegraded = dnsActive && dns?.mode === "in-process";

  // A TLD one of this machine's sites ANSWERS on that cannot resolve here. The
  // card's own line covers the DEFAULT TLD, which says nothing about a site —
  // or an extra domain — on a second one whose resolver went away: nginx serves
  // it, the certificate covers it, and the browser cannot find it. Empty is the
  // ordinary answer and renders nothing.
  const { data: unresolvable = [] } = useQuery({
    queryKey: ["unresolvable-tlds"],
    queryFn: unresolvableTlds,
  });
  const repair = useMutation({
    mutationFn: (tld: string) => repairResolver(tld),
    onSuccess: (tld) => {
      void qc.invalidateQueries({ queryKey: ["unresolvable-tlds"] });
      toast.success(`.${tld} resolves here again — sites on it should load now.`);
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <>
      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
        <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
          Status
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="flex items-center gap-[11px] rounded-[11px] border border-rex-border-subtle bg-rex-well px-[14px] py-[13px]">
            <span className="relative inline-flex h-[9px] w-[9px] flex-none">
              {dnsActive && !dnsDegraded && (
                <span className="absolute inset-0 rounded-full bg-status-running opacity-50 animate-rex-ping motion-reduce:animate-none" />
              )}
              <span
                className="relative h-[9px] w-[9px] rounded-full"
                style={{
                  background: !dnsActive
                    ? "var(--rex-stopped)"
                    : dnsDegraded
                      ? "var(--rex-warning-bright)"
                      : "var(--rex-running)",
                  boxShadow: dnsActive && !dnsDegraded ? "var(--rex-glow-run)" : "none",
                }}
              />
            </span>
            <div className="min-w-0">
              <div className="text-[0.8125rem] font-medium text-rex-text">DNS resolver</div>
              <div className="mt-px font-mono text-[0.65625rem] text-rex-text-muted">
                *.{tld} → 127.0.0.1 · {dnsActive ? (dnsDegraded ? "in-process" : "agent") : "inactive"}
              </div>
              {dnsActive && (
                <div
                  className="mt-0.5 text-[0.65625rem] leading-snug"
                  style={{
                    color: dnsDegraded ? "var(--rex-warning-bright)" : "var(--rex-text-muted)",
                  }}
                >
                  {dnsDegraded
                    ? "Running inside the app — DNS stops when you quit rexenv. Restart the app to retry the always-on agent."
                    : "Always on — resolves even when rexenv is closed."}
                </div>
              )}
            </div>
          </div>
          <div className="flex items-center gap-[11px] rounded-[11px] border border-rex-border-subtle bg-rex-well px-[14px] py-[13px]">
            <Lock
              className={cn(
                "h-[18px] w-[18px] flex-none",
                dns?.caTrusted ? "text-status-running-bright" : "text-rex-text-muted",
              )}
              strokeWidth={1.8}
            />
            <div className="min-w-0">
              <div className="text-[0.8125rem] font-medium text-rex-text">Local CA</div>
              <div className="mt-px font-mono text-[0.65625rem] text-rex-text-muted">
                {dns?.caTrusted ? `trusted · ${words.trustStore}` : "not trusted — use Re-trust below"}
              </div>
            </div>
          </div>
        {unresolvable.length > 0 && (
          <div className="mt-3 rounded-[11px] border border-status-warning-border bg-rex-well px-[14px] py-[11px]">
            <div className="text-[0.8125rem] font-medium text-rex-text">
              {unresolvable.length === 1 ? "A TLD your sites use" : "TLDs your sites use"} can't be
              resolved on {words.host}
            </div>
            {/* Said plainly because everything ELSE about these sites is fine:
                nginx serves them, the edge routes them, the certificate covers
                them — and the browser still cannot find them. Without this line
                the app looks healthy while a site does not load. */}
            <div className="mt-0.5 text-[0.71875rem] leading-snug text-rex-text-muted">
              Their sites are served correctly; the name just doesn't reach this machine.
            </div>
            <div className="mt-2 flex flex-col gap-1.5">
              {unresolvable.map((u) => (
                <div key={u.tld} className="flex flex-col gap-2">
                  <div className="flex items-center justify-between gap-3">
                    <span className="font-mono text-[0.71875rem] text-rex-text">
                      .{u.tld}
                      <span className="ml-2 text-rex-text-muted">
                        {/* Two causes, two fixes: telling someone to install a
                            file another tool already owns sends them in a circle. */}
                        {u.foreign
                          ? `another tool owns its ${words.routesLabel}`
                          : `no ${words.routesLabel}`}
                      </span>
                    </span>
                    {/* Repair puts OUR file back where there is none; it refuses
                        a foreign file by design, so offering it here for one was
                        a button that could only fail (5 Sep 2026). A foreign TLD
                        gets the takeover consent instead — same card as Import. */}
                    {!u.foreign && (
                      <Button
                        variant="secondary"
                        disabled={repair.isPending}
                        onClick={() => repair.mutate(u.tld)}
                      >
                        {repair.isPending ? "Repairing…" : "Repair"}
                      </Button>
                    )}
                  </div>
                  {u.foreign && (
                    <ResolverConsentFor
                      tld={u.tld}
                      alternative={
                        <>
                          Or keep their file and move these sites to another ending via Change
                          domain — <span className="font-mono">.rex</span> always works.
                        </>
                      }
                    />
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
        </div>
      </div>

      <DefaultTldCard />

      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
        <ActionRow
          title="Re-trust local CA"
          desc={`Reinstall rexenv's certificate authority in your ${words.trustStore}.`}
          busy={trust.isPending}
          label="Re-trust"
          onClick={() => {
            setMsg(null);
            trust.mutate();
          }}
        />
        <ActionRow
          title="Regenerate certificates"
          desc="Issue fresh SSL certs for every local site."
          busy={regen.isPending}
          label="Regenerate"
          busyLabel="Working…"
          onClick={() => {
            setMsg(null);
            regen.mutate();
          }}
        />
      </div>
      <BorrowedResolverCard />
      <FirefoxTrustCard />
      {msg && <Notice>{msg}</Notice>}
    </>
  );
}

/** Firefox has its OWN certificate store (NSS): the keychain trust Safari and
 *  Chrome honor is invisible to it unless its OS-roots import pref is on
 *  (default since Firefox 120). Offers the one-click per-profile pref fix and
 *  the manual CA import as a fallback. Hidden when Firefox was never run. */
function FirefoxTrustCard() {
  const qc = useQueryClient();
  const { data: ff } = useQuery({ queryKey: ["firefox-trust"], queryFn: firefoxTrustStatus });
  const [copied, setCopied] = useState(false);

  const force = useMutation({
    mutationFn: trustCaInFirefox,
    onSuccess: (s) => {
      toast.success(
        `HTTPS trust enabled in ${s.profiles} Firefox profile${s.profiles === 1 ? "" : "s"} — restart Firefox to apply.`,
      );
      void qc.invalidateQueries({ queryKey: ["firefox-trust"] });
    },
    onError: (e) => toastBackendError(e),
  });
  const typing = useMutation({
    mutationFn: allowTldsInFirefox,
    onSuccess: () => {
      toast.success("Typed addresses will open in Firefox — restart Firefox to apply.");
      void qc.invalidateQueries({ queryKey: ["firefox-trust"] });
    },
    onError: (e) => toastBackendError(e),
  });

  if (!ff?.installed) return null;
  const endings = ff.tlds.map((t) => `name.${t}`).join(", ");
  const typingDone = ff.typing >= ff.profiles;
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
      {/* Firefox is the one browser with a setting for this: a per-TLD pref in
          user.js (`core::firefox`). rexenv also writes it when a TLD's route is
          installed and on Re-trust; this row is for a Firefox added later. */}
      <ActionRow
        title="Open typed addresses in Firefox"
        desc={
          typingDone
            ? `Typing ${endings} opens the site in all ${ff.profiles} profile${ff.profiles === 1 ? "" : "s"} — restart Firefox if it still searches.`
            : `Firefox searches a typed ${endings} instead of opening it — let it treat rexenv's endings as addresses.`
        }
        busy={typing.isPending}
        label={typingDone ? "Re-apply" : "Enable"}
        onClick={() => typing.mutate()}
      />
      <ActionRow
        title="Trust HTTPS in Firefox"
        desc={
          ff.forced >= ff.profiles
            ? `Enabled in all ${ff.profiles} profile${ff.profiles === 1 ? "" : "s"} — restart Firefox if sites still warn.`
            : "Firefox uses its own certificate store — enable its system-roots import so rexenv HTTPS works there too."
        }
        busy={force.isPending}
        label={ff.forced >= ff.profiles ? "Re-apply" : "Enable"}
        onClick={() => force.mutate()}
      />
      <div className="border-t border-rex-border-subtle py-[13px]">
        <div className="text-[0.75rem] text-rex-text-muted">
          Still warning? Import the CA manually: Firefox Settings → Privacy &amp; Security →
          Certificates → View Certificates → Authorities → Import, pick the file below and check
          “Trust this CA to identify websites”.
        </div>
        <div className="mt-2 flex items-center gap-2">
          <code className="min-w-0 flex-1 truncate rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5 font-mono text-[0.6875rem] text-rex-text">
            {ff.caPath}
          </code>
          <button
            onClick={() => {
              void navigator.clipboard?.writeText(ff.caPath);
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            }}
            className="h-8 flex-none rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover"
          >
            {copied ? "Copied" : "Copy path"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** `rex` CLI install card. macOS: one symlink on PATH (may cost one admin prompt —
 *  /usr/local/bin is root-owned on most Macs); the link tracks the bundle, so
 *  app updates need no re-install, and a moved bundle shows as "points elsewhere".
 *  Windows: a copy in rexenv's own folder, that folder on the user's Path, no
 *  prompt (#634) — `onPath` says when the folder has left the Path. The words
 *  are the platform's. Hidden when the sidecar isn't next to the app binary
 *  (bare `cargo run` — `tauri dev` and the packaged app always have it). */
function CliCard() {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data: cli } = useQuery({ queryKey: ["cli-status"], queryFn: cliStatus });

  const install = useMutation({
    mutationFn: cliInstall,
    onSuccess: (s) => {
      toast.success(`${words.cliInstalled} (${s.linkPath})`);
      void qc.invalidateQueries({ queryKey: ["cli-status"] });
    },
    onError: (e) => toastBackendError(e),
  });

  if (!cli?.available) return null;
  const desc =
    cli.current && cli.onPath === false
      ? "Installed, but its folder is no longer on your Path — reinstall to add it back."
      : cli.current
        ? "Installed — manage rexenv from any terminal: rex status, rex start, rex site create."
        : cli.installed
          ? `${cli.linkPath} ${words.cliStale}`
          : words.cliInstall;
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
      <ActionRow
        title="Command-line tool"
        desc={desc}
        busy={install.isPending}
        label={cli.installed ? "Reinstall" : "Install"}
        busyLabel="Installing…"
        onClick={() => install.mutate()}
      />
      <div className="border-t border-rex-border-subtle py-[13px]">
        <code
          className="block truncate rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5 font-mono text-[0.6875rem] text-rex-text"
          title={`${cli.linkPath} → ${cli.bundledPath ?? "?"}`}
        >
          {cli.linkPath} → {cli.bundledPath ?? "?"}
        </code>
      </div>
    </div>
  );
}

/** The tell for #228: rexenv pins the command set of every `wp` it runs, so a
 *  user's global WP-CLI packages no longer extend it. Rendered ONLY when a
 *  packages dir exists that would have contributed — on almost every machine
 *  this is nothing at all.
 *
 *  Every clause below is LOAD-BEARING and guarded (`the_settings_tell_says_what
 *  _changed_and_what_still_works`, core/wp_packages.rs). Read the guard's
 *  reasons before shortening anything here — in particular the last sentence,
 *  which reads as reassurance and is the entire relief valve: without it this is
 *  a capability removal rather than a scoped change.
 *
 *  `names` EMPTY means "could not be named", NEVER "none" — so the copy drops to
 *  a variant that claims no count. "The 0 packages" would be worse than not
 *  rendering: it invites the reader to conclude something false about their own
 *  machine. */
const PACKAGES_SCOPE = "rexenv runs `wp` with only the commands it bundles.";
const PACKAGES_REASON =
  "are not loaded into the commands rexenv runs for you, so a command does the same thing here as on a machine that never installed one.";
const PACKAGES_TERMINAL =
  "They still work in rexenv's terminal: `wp` there is your command line, not ours.";

export function WpCliPackagesCard() {
  const { data } = useQuery({ queryKey: ["wp-cli-packages"], queryFn: wpCliPackages });
  if (!data) return null;
  const named = data.names.length > 0;
  return (
    <div
      data-probe="wp-cli-packages"
      className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5 py-[15px]"
    >
      <div className="text-[0.8125rem] font-medium text-rex-text">WP-CLI packages</div>
      <div className="mt-1 text-[0.71875rem] leading-relaxed text-rex-text-muted">
        {PACKAGES_SCOPE.replace("`wp`", "wp")}{" "}
        {named ? (
          <>
            The {data.names.length} package{data.names.length === 1 ? "" : "s"} in{" "}
            <span className="font-mono text-rex-text">{data.dir}</span> —{" "}
            {data.names.map((n, i) => (
              <span key={n}>
                {i > 0 && ", "}
                <span className="font-mono text-rex-text">{n}</span>
              </span>
            ))}{" "}
            — {PACKAGES_REASON}
          </>
        ) : (
          <>
            The packages in <span className="font-mono text-rex-text">{data.dir}</span>{" "}
            {PACKAGES_REASON}
          </>
        )}
      </div>
      <div className="mt-1.5 text-[0.71875rem] leading-relaxed text-rex-text-muted">
        {PACKAGES_TERMINAL.replace("`wp`", "wp")}
      </div>
    </div>
  );
}

/** Default-TLD picker (configurable TLD v1): new sites are created under this
 *  TLD. Existing sites keep their domain (re-point one via Change domain).
 *  Blocked TLDs are refused by the BACKEND — the inline feedback here mirrors
 *  the same `tld_policy` classification, it doesn't enforce anything. */
function DefaultTldCard() {
  const words = usePlatformWords();
  const qc = useQueryClient();
  const { data: current = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });
  const [input, setInput] = useState<string | null>(null); // null = untouched
  const value = (input ?? current).trim().replace(/^\./, "").toLowerCase();
  const dirty = input !== null && value !== current;

  const { data: policy } = useQuery({
    queryKey: ["tld-policy", value],
    queryFn: () => tldPolicy(value),
    enabled: value !== "",
  });

  const save = useMutation({
    mutationFn: () => setDefaultTld(value),
    onSuccess: (stored) => {
      setInput(null);
      toast.success(`New sites will now be created under .${stored}`);
      void qc.invalidateQueries({ queryKey: ["default-tld"] });
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
      <div className="flex items-center gap-[14px]">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Default domain ending</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            New sites are created under this TLD. Existing sites keep their domain — re-point
            one from its page via Change domain.
          </div>
        </div>
        <div className="flex h-9 w-[150px] items-center rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] transition-colors has-[input:focus]:border-brand">
          <span className="flex-none font-mono text-[0.78125rem] text-rex-text-muted">.</span>
          <input
            {...TECH_INPUT}
            value={input ?? current}
            onChange={(e) => setInput(e.target.value.toLowerCase())}
            onKeyDown={(e) => {
              if (e.key === "Enter" && dirty && policy?.allowed && !save.isPending) save.mutate();
            }}
            aria-label="Default TLD for new sites"
            className="min-w-0 flex-1 bg-transparent font-mono text-[0.78125rem] text-rex-text outline-none focus-visible:shadow-none"
          />
        </div>
        <Button
          variant="secondary"
          disabled={!dirty || !policy?.allowed || save.isPending}
          onClick={() => save.mutate()}
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
      {policy && !policy.allowed && value !== "" && (
        <div className="mt-2.5 text-[0.71875rem] text-status-error-bright">{policy.reason}</div>
      )}
      {/* Saving the default is a setting, not a resolver write — but the FIRST
          site created under it hits `ensure_resolver`, which refuses a TLD
          Valet/Herd still owns. Saying so here, with the takeover in reach, beats
          a create dialog that fails with "rexenv can take that TLD over" and no
          button (5 Sep 2026). Save stays enabled: the setting is theirs to make. */}
      {policy?.allowed && (
        <div className="mt-2.5 empty:hidden">
          <ResolverConsentFor
            tld={value}
            alternative={
              <>
                Or leave it alone — new sites under <span className="font-mono">.{value}</span>{" "}
                won't resolve on {words.host} until rexenv answers it.
              </>
            }
          />
        </div>
      )}
      {policy?.allowed && policy.warn && (
        <div className="mt-2.5 text-[0.71875rem] text-status-warning-bright">
          <span className="font-mono">.{value}</span> may shadow a real internet TLD on this
          machine{value === "rex" ? " — and ICANN could delegate it for real use in the future" : ""}.
          The reserved-for-testing TLDs (<span className="font-mono">.test</span>) can never
          collide.
        </div>
      )}
      <div className="mt-2.5 text-[0.71875rem] text-rex-text-muted">
        <span className="font-mono">.rex</span> is rexenv's home TLD — its route is set up
        during onboarding and rexenv's own tools use it. Any other TLD (including{" "}
        <span className="font-mono">.test</span>) {words.privilegedPrompt}, when its first
        site is created.
      </div>
      {/* Browsers decide URL-or-search from the public TLD list BEFORE any DNS
          lookup, so a bare `acme.rex` never reaches rexenv's resolver in these.
          Nothing on this side can change that; the trailing slash can. */}
      <div className="mt-2.5 text-[0.71875rem] text-rex-text-muted" data-probe="typed-address-hint">
        Typing an address? {words.searchingBrowsers} search a bare{" "}
        <span className="font-mono">name.{current}</span> instead of opening it — add a slash (
        <span className="font-mono">name.{current}/</span>) or <span className="font-mono">https://</span>,
        or open the site from rexenv.
      </div>
    </div>
  );
}

function PrefRow({
  title,
  desc,
  on,
  onToggle,
  label,
}: {
  title: string;
  desc: string;
  on: boolean;
  onToggle: () => void;
  label: string;
}) {
  return (
    <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px] last:border-b-0">
      <div className="flex-1">
        <div className="text-[0.84375rem] font-medium text-rex-text">{title}</div>
        <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">{desc}</div>
      </div>
      <StartStopToggle running={on} variant="setting" onToggle={onToggle} label={label} />
    </div>
  );
}

/** Services prefs: app-autostart + login-start toggles + a (UI-only) idle-stop
 *  toggle. Copy is deliberately literal about the mechanics: the app OPENS at
 *  login (a macOS login item — you'll see it launch), and the second toggle
 *  makes that launch also start the stack. Both on = sites back after a reboot
 *  without a click. */
function ServicePrefsCard() {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data: enabled } = useQuery({ queryKey: ["autostart"], queryFn: autostartStatus });
  const toggle = useMutation({
    mutationFn: (on: boolean) => setAutostart(on),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["autostart"] }),
    onError: (e) => toastBackendError(e),
  });
  const { data: autoStart } = useQuery({
    queryKey: ["setting", AUTO_START_KEY],
    queryFn: () => getSetting(AUTO_START_KEY),
  });
  const autoStartOn = autoStart === "true";
  const toggleAutoStart = useMutation({
    mutationFn: (on: boolean) => setSetting(AUTO_START_KEY, on ? "true" : "false"),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["setting", AUTO_START_KEY] }),
    onError: (e) => toastBackendError(e),
  });
  const [stopIdle, setStopIdle] = useState(false);

  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
      <PrefRow
        title="Open rexenv at login"
        desc={words.loginItem}
        on={!!enabled}
        onToggle={() => toggle.mutate(!enabled)}
        label="Open rexenv at login"
      />
      <PrefRow
        title="Start services when rexenv opens"
        desc="Runs Start all automatically on launch — with 'Open rexenv at login' on, your sites come back after a reboot without a click. Never downloads or prompts at login."
        on={autoStartOn}
        onToggle={() => toggleAutoStart.mutate(!autoStartOn)}
        label="Start services when rexenv opens"
      />
      <PrefRow
        title="Stop idle services automatically"
        desc="Free up memory by pausing services no running site is using."
        on={stopIdle}
        onToggle={() => {
          setStopIdle((v) => !v);
          toast.info("Idle-service auto-stop isn't wired yet (UI only).");
        }}
        label="Stop idle services automatically"
      />
    </div>
  );
}

/**
 * The mail catch-all.
 *
 * Its own card rather than a row in Service prefs because the copy has to carry
 * two facts a one-line toggle cannot: turning it OFF lets a local site mail the
 * real world, and turning it ON is not absolute — a site that has already run
 * `php artisan config:cache`, an HTTP-API mailer, and the user's own terminal
 * are all beyond reach. An honest-UI promise: the screen states the limit
 * instead of letting a developer infer it from a message that reached a customer.
 *
 * The backend does not merely record the flip — it installs or removes the
 * WordPress mu-plugin and restarts the running pools — so the toggle is left in
 * its pending state until that resolves rather than snapping to a value that is
 * not yet true of the machine.
 */
function MailCatchAllCard() {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data: on } = useQuery({ queryKey: ["mail-catch-all"], queryFn: mailCatchAll });
  const toggle = useMutation({
    mutationFn: (next: boolean) => setMailCatchAll(next),
    onSuccess: (_r, next) => {
      qc.invalidateQueries({ queryKey: ["mail-catch-all"] });
      toast.info(
        next
          ? "Mail from your sites is caught in Mailpit again."
          : `Your sites now send mail for real. Anything they mail will leave ${words.host}.`,
      );
    },
    onError: (e) => toastBackendError(e),
  });
  const enabled = on ?? true;
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
      <PrefRow
        title="Catch all outgoing mail"
        desc="Every site's mail goes to Mailpit instead of the internet — even when a site is configured for a real SMTP provider (a WordPress SMTP plugin, or a Laravel .env). Turn it off only to test a live provider on purpose."
        on={enabled}
        onToggle={() => toggle.mutate(!enabled)}
        label="Catch all outgoing mail"
      />
      {enabled ? (
        <div className="border-b border-rex-border-subtle py-[11px] text-[0.75rem] text-rex-text-muted last:border-b-0">
          Not everything can be caught: a Laravel app that has run{" "}
          <span className="font-mono text-[0.6875rem]">php artisan config:cache</span> reads its
          baked config, a plugin that mails through a provider&rsquo;s HTTP API never touches
          PHP&rsquo;s mailer, and commands you run in your own terminal are outside rexenv.
        </div>
      ) : null}
    </div>
  );
}

/** Default ports card (UI-only — rexenv's ports are fixed today; see CLAUDE.md). */
function DefaultPortsCard() {
  const [http, setHttp] = useState("80");
  const [https, setHttps] = useState("443");
  const [mysql, setMysql] = useState("13306");
  const input = (label: string, value: string, onChange: (v: string) => void) => (
    <div>
      <label className="mb-1.5 block text-[0.75rem] text-rex-text-muted">{label}</label>
      <input {...TECH_INPUT}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-[34px] w-full rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] font-mono text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand"
      />
    </div>
  );
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 p-5">
      <div className="mb-[14px] font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
        Default ports
      </div>
      <div className="grid grid-cols-3 gap-3">
        {input("HTTP", http, setHttp)}
        {input("HTTPS", https, setHttps)}
        {input("MySQL", mysql, setMysql)}
      </div>
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
    onError: (e) => toastBackendError(e),
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteBlueprint(id),
    onSuccess: invalidate,
    onError: (e) => toastBackendError(e),
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
      <p className="text-[0.75rem] text-rex-text-muted">
        Reusable WordPress setups — pick one in <span className="font-mono">New site</span> to auto-install its
        plugins/themes and apply multisite.
      </p>

      {blueprints.length > 0 && (
        <div className="overflow-hidden rounded-lg border border-rex-border">
          {blueprints.map((b) => (
            <div key={b.id} className="flex items-center gap-2 border-b border-rex-border-subtle px-3 py-2 last:border-b-0">
              <div className="min-w-0 flex-1">
                <div className="truncate text-[0.78125rem] text-rex-text">{b.name}</div>
                <div className="truncate font-mono text-[0.65625rem] text-rex-text-muted">
                  {b.spec.multisite !== "none" ? `multisite:${b.spec.multisite} · ` : ""}
                  {b.spec.plugins.length} plugin(s){b.spec.wpDebug ? " · WP_DEBUG" : ""}
                </div>
              </div>
              <Button variant="ghost" disabled={remove.isPending} onClick={() => remove.mutate(b.id)} className="hover:text-status-error-bright">
                Delete
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Add */}
      <div className="flex flex-col gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <div className="flex items-center gap-2">
          <input {...TECH_INPUT}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Blueprint name"
            className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.75rem] text-rex-text outline-none focus:border-brand"
          />
          <select value={multisite} onChange={(e) => setMultisite(e.target.value as MultisiteMode)} className={SELECT}>
            <option value="none">Single site</option>
            <option value="subdomain">Subdomain MS</option>
            <option value="subdirectory">Subdirectory MS</option>
          </select>
        </div>
        <input {...TECH_INPUT}
          value={plugins}
          onChange={(e) => setPlugins(e.target.value)}
          placeholder="Plugin slugs (comma-separated, e.g. woocommerce, jetpack)"
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        <input {...TECH_INPUT}
          value={themes}
          onChange={(e) => setThemes(e.target.value)}
          placeholder="Theme slugs (comma-separated)"
          className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[0.75rem] text-rex-text outline-none focus:border-brand"
        />
        <div className="flex items-center justify-between">
          <label className="flex cursor-pointer items-center gap-1.5 text-[0.75rem] text-rex-text-muted">
            <input
              type="checkbox"
              checked={wpDebug}
              onChange={(e) => setWpDebug(e.target.checked)}
              className={CHECK_INPUT}
            />
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
  "h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 text-[0.75rem] text-rex-text outline-none focus:border-brand";

function UninstallSetting() {
  const [msg, setMsg] = useState<string | null>(null);
  const words = usePlatformWords();
  const run = useMutation({
    mutationFn: uninstallSystem,
    onSuccess: (r) => {
      // Say what actually happened to each resolver file. A file we BORROWED
      // from Valet/Herd is handed back, not deleted — and if our backup of it
      // was gone we must say so rather than imply a clean restore.
      const parts = ["Services stopped, local CA untrusted."];
      // "resolvers" was a plural the platform word cannot make (`file under /etc/resolver` does not
      // pluralise into a sentence), so the TLDs are listed after it instead.
      if (r.removed.length)
        parts.push(`Removed rexenv's ${words.routesLabel} for: ${r.removed.map((t) => `.${t}`).join(", ")}.`);
      if (r.restored.length)
        parts.push(`Handed back to Valet/Herd: ${r.restored.map((t) => `.${t}`).join(", ")}.`);
      if (r.leftAlone.length)
        parts.push(`Left alone (already reclaimed): ${r.leftAlone.map((t) => `.${t}`).join(", ")}.`);
      if (r.backupMissing.length)
        parts.push(
          `Couldn't find our backup of the ${r.backupMissing.map((t) => `.${t}`).join(", ")} ${words.routesLabel}, so rexenv's version was removed — the other tool will need to put its own back.`,
        );
      parts.push("You can now quit and delete rexenv.");
      setMsg(parts.join(" "));
    },
    onError: (e) => toastBackendError(e),
  });

  return (
    <div className="flex flex-col gap-3">
      <p className="text-[0.75rem] text-rex-text-muted">
        Reverse the system-level changes rexenv made — stop all services, remove every rexenv{" "}
        {words.routesLabel} (<span className="font-mono">.rex</span> plus any other TLDs you added), and
        untrust the local HTTPS certificate authority. Your site files and databases are{" "}
        <span className="font-medium">not</span> deleted.
      </p>
      <div className="flex items-center justify-between rounded-lg border border-status-error/40 bg-status-error/5 p-3">
        <span className="text-[0.78125rem] text-rex-text">Remove rexenv's system changes</span>
        <Button
          variant="ghost"
          disabled={run.isPending}
          onClick={async () => {
            setMsg(null);
            if (
              await confirm({
                title: "Remove rexenv's system changes?",
                message:
                  `This stops all services, deletes every rexenv ${words.routesLabel} (.rex and any other TLDs), and untrusts the local CA (${words.elevationNote}). Your sites and databases are kept.`,
                danger: true,
                confirmLabel: "Remove",
              })
            )
              run.mutate();
          }}
          className="border border-status-error/50 text-status-error-bright hover:bg-status-error/10"
        >
          {run.isPending ? "Removing…" : "Remove"}
        </Button>
      </div>
      {msg && <Notice>{msg}</Notice>}
    </div>
  );
}

/** The violet brand medallion (used by About). */
function CrownBadge({ size }: { size: number }) {
  return (
    <div
      className="flex flex-none items-center justify-center rounded-xl border border-[var(--rex-crown-border)] bg-gradient-to-br from-[var(--rex-crown-chip-from)] to-[var(--rex-crown-chip-to)] shadow-glow-crown"
      style={{ width: size, height: size }}
    >
      <RexLogo className="block w-auto" style={{ height: size * 0.5 }} />
    </div>
  );
}

/** In-app viewer for the legal text bundled with this exact binary: the
 *  NOTICE line, THIRD-PARTY-NOTICES.md, then the full Apache-2.0 LICENSE.
 *  Rendered from `?raw` imports, so it always matches what shipped. */
function LicensesDialog({ onClose }: { onClose: () => void }) {
  return (
    <Overlay onClose={onClose} cardClassName="w-[680px] max-w-[92vw]">
      <div className="flex items-center justify-between">
        <div className="text-[0.9375rem] font-semibold text-rex-text">Licenses &amp; credits</div>
        <Button variant="secondary" size="sm" onClick={onClose}>
          Close
        </Button>
      </div>
      <div className="mt-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">
        rexenv © 2026 Linkon Miyan — Apache License 2.0. Everything below ships inside this
        app bundle; the server binaries rexenv downloads at runtime carry their own licenses.
      </div>
      <pre className="mt-4 max-h-[62vh] overflow-y-auto whitespace-pre-wrap rounded-lg border border-rex-border-subtle bg-rex-well p-4 font-mono text-[0.6875rem] leading-[1.6] text-rex-text-muted">
        {noticesText}
        {"\n\n" + "─".repeat(72) + "\n\n"}
        {licenseText}
      </pre>
    </Overlay>
  );
}

/** The identity of THIS binary, as a copyable block. Facts only — every row is
 *  read from `app_info`, never composed from what the UI hopes shipped.
 *
 *  The commit + build date are not trivia: a stale install once looked exactly
 *  like a logic bug, and "is the app running the code I just changed?" has to
 *  be answerable without a terminal. They used to be one cramped line under the
 *  version; here they are legible AND copyable into a bug report. */
function BuildFactsCard({ info }: { info: AppInfo }) {
  const rows: [string, string][] = [
    ["Version", `v${info.version}`],
    ["Commit", info.commit],
    ["Built", info.builtAt.replace("T", " ").replace("Z", " UTC")],
    ["Platform", info.platform],
    ["Tauri", `v${info.tauriVersion}`],
  ];
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5 py-3">
      <div className="flex items-center justify-between py-1">
        <div className="text-[0.78125rem] font-medium text-rex-text">Build</div>
        <CopyButton
          value={rows.map(([k, v]) => `${k}: ${v}`).join("\n")}
          title="Copy build info"
        />
      </div>
      <dl className="mt-1">
        {rows.map(([k, v]) => (
          <div
            key={k}
            className="flex items-baseline justify-between gap-4 border-t border-rex-border-subtle py-[9px]"
          >
            <dt className="text-[0.78125rem] text-rex-text-muted">{k}</dt>
            <dd className="truncate font-mono text-[0.6875rem] text-rex-text" title={v}>
              {v}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/**
 * Where a user reads what changed.
 *
 * The website's changelog, not a GitHub releases page: `rexenv/rexenv` is private
 * (its release page is a 404 to anyone not signed in) and the tap's releases page
 * is an artefact list, not a changelog. This link therefore does NOT move when the
 * repo goes public — which is the point of pointing at the site rather than at
 * whichever repo happens to host releases this month.
 */
const CHANGELOG_URL = "https://rexenv.rex.bd/docs/changelog/";

/** The About section — identity, version, links, credits. */

function AboutSetting() {
  const words = usePlatformWords();
  // Read the download hub's snapshot PASSIVELY from the cache StatusFooter's
  // single `useDownloads()` mount keeps fresh — mounting a second one is what
  // that hook's own doc forbids. An empty snapshot before the footer has seeded
  // it renders the card without a bar, which is correct: nothing is downloading.
  const downloads = useQuery<DownloadsSnapshot>({
    queryKey: ["downloads"],
    enabled: false,
  }).data ?? { batch: null, items: [], seq: 0 };
  const { data: info } = useQuery({ queryKey: ["app-info"], queryFn: getAppInfo });
  const [showLicenses, setShowLicenses] = useState(false);
  // A string opens externally (arrow-out icon, URL shown); a function runs
  // in-app (chevron icon). The icon is the promise — keep it truthful.
  const linkRow = (
    icon: React.ReactNode,
    color: string,
    label: string,
    target: string | (() => void),
  ) => (
    <button
      onClick={() =>
        typeof target === "string" ? void openExternal(target).catch(toastBackendError) : target()
      }
      className="flex w-full items-center gap-3 border-b border-rex-border-subtle py-[14px] text-left transition-opacity last:border-b-0 hover:opacity-80"
    >
      <span className="flex flex-none" style={{ color }}>
        {icon}
      </span>
      <span className="flex-1 text-[0.84375rem] text-rex-text">{label}</span>
      {typeof target === "string" && (
        <span className="font-mono text-[0.6875rem] text-rex-text-muted">
          {target.replace(/^https?:\/\//, "")}
        </span>
      )}
      {typeof target === "string" ? (
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
          <div className="font-display text-[1.375rem] font-semibold tracking-[-0.02em] text-rex-text">
            rexenv
          </div>
          <div className="mt-1 font-mono text-[0.71875rem] text-rex-text-muted">
            {info && `v${info.version} · ${info.platform}`}
          </div>
        </div>
        <div className="max-w-[380px] text-[0.78125rem] leading-[1.55] text-rex-text-muted">
          A calm, fast command room for your local kingdom — every server, site, and database in one place.
        </div>
      </div>

      {/* Updates first, because it is the only card here that can be ACTED on;
          the build facts below it answer "which build is this?" once you know
          there is a question. */}
      <AppUpdateCard downloads={downloads} />

      {/* The build, spelled out and COPYABLE. The header line is for a glance;
          this is for a bug report, where "v0.3.0" alone is not enough to tell
          two builds apart and retyping a commit from a screenshot is how the
          wrong build gets diagnosed. */}
      {info && <BuildFactsCard info={info} />}

      <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5">
        {linkRow(
          <FileText className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "var(--rex-accent-blue)",
          "Documentation",
          "https://rexenv.rex.bd/docs",
        )}
        {linkRow(
          <ScrollText className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "var(--rex-accent-periwinkle)",
          "Changelog",
          CHANGELOG_URL,
        )}
        {linkRow(
          <Github className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "var(--rex-text-bright)",
          "GitHub",
          "https://github.com/rexenv",
        )}
        {linkRow(
          <ShieldCheck className="h-[17px] w-[17px]" strokeWidth={1.7} />,
          "var(--rex-accent-teal)",
          "Licenses & credits",
          () => setShowLicenses(true),
        )}
      </div>

      {showLicenses && <LicensesDialog onClose={() => setShowLicenses(false)} />}

      <div className="text-center text-[0.71875rem] leading-[1.6] text-rex-text-muted">
        Built on open source — {words.bundledTools}.
        <br />
        Made for developers who run their kingdom locally.
      </div>
    </>
  );
}

type Section = "general" | "dns" | "services" | "agents" | "about";

const SECTIONS: { key: Section; label: string; icon: LucideIcon }[] = [
  { key: "general", label: "General", icon: SettingsIcon },
  { key: "dns", label: "DNS & SSL", icon: Shield },
  { key: "services", label: "Services", icon: Server },
  { key: "agents", label: "AI agents", icon: Bot },
  { key: "about", label: "About", icon: Info },
];

export function Settings() {
  // `?section=` is a deep link, not decoration: the macOS app menu's "About
  // rexenv" lands here (see App.tsx), and it must land on About from whatever
  // screen the user was on. Unknown values fall back to General rather than
  // rendering an empty pane.
  const [params] = useSearchParams();
  const wanted = SECTIONS.find((s) => s.key === params.get("section"))?.key;
  const [section, setSection] = useState<Section>(wanted ?? "general");
  useEffect(() => {
    if (wanted) setSection(wanted);
  }, [wanted]);
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
                  "flex items-center gap-2.5 rounded-[9px] px-[11px] py-[9px] text-left text-[0.8125rem] transition-colors",
                  active
                    ? "bg-brand-active text-brand-tint"
                    : "text-rex-text-muted hover:bg-rex-hover hover:text-rex-text",
                )}
              >
                <Icon className="h-4 w-4 flex-none" strokeWidth={1.7} />
                <span className="flex-1">{s.label}</span>
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
                <GeneralPrefsCard />
                <CliCard />
                <WpCliPackagesCard />
                <Card title="Blueprints">
                  <BlueprintsSetting />
                </Card>
              </>
            )}
            {section === "dns" && <DnsSslSetting />}
            {section === "services" && (
              <>
                <ServicePrefsCard />
                <MailCatchAllCard />
                <DefaultPortsCard />
                <Card title="PHP versions">
                  <PhpVersionsSetting />
                </Card>
                <Card title="Uninstall">
                  <UninstallSetting />
                </Card>
              </>
            )}
            {section === "agents" && <AgentsMcpCard />}
            {section === "about" && <AboutSetting />}
          </div>
        </div>
      </div>
    </>
  );
}

/**
 * Resolver files rexenv BORROWED from Valet/Herd, each with its return path.
 *
 * Shown here and not only in the importer because the borrow outlives the
 * import: someone who took `.test` over months ago should be able to find the
 * "hand it back" button without remembering which screen took it.
 */
function BorrowedResolverCard() {
  // DNS only: the resolver files rexenv borrowed. The "N sites can be imported"
  // nudge and the leftover-dumps card that used to sit beside this moved to the
  // Import page (owner, 12 Sep 2026) — importing is not part of DNS & SSL.
  const { data } = useQuery({ queryKey: ["valet-scan"], queryFn: scanValetImport });
  const borrowed = (data?.tlds ?? []).filter((t) => t.owner === "borrowed" || t.owner === "drifted");
  if (borrowed.length === 0) return null;
  return (
    <div className="mt-3 flex flex-col gap-2">
      {borrowed.map((t) => (
        <ResolverHandBackRow key={t.tld} tld={t} />
      ))}
    </div>
  );
}

