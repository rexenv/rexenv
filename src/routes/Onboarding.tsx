import { usePlatformWords } from "@/lib/usePlatformWords";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronRight, Globe, Loader2, Lock, RotateCw, Shield } from "lucide-react";
import { coreBinariesPlan, dnsStatus, prefetchCoreBinaries, retryDownload, setupEdgeConflict, systemSetup } from "@/lib/ipc";
import { onTitleBarMouseDown } from "@/lib/window-drag";
import { useDownloads } from "@/lib/useDownloads";
import { Track, pctOf } from "@/components/shell/DownloadPanel";
import { RexLogo } from "@/components/common/RexLogo";
import type { DownloadItem, DownloadPhase, PlannedDownload } from "@/types";

/**
 * First-run onboarding — a 4-step wizard (Welcome → Install → Domains & SSL →
 * Done). The ONE place the Space Grotesk display face + a bolder violet brand
 * moment are used (DESIGN_BRIEF Block 12).
 */
const STEP_COUNT = 4;

const STEP_META = [
  { label: "Welcome", primary: "Get started" },
  { label: "Step 2 of 4 · Install", primary: "Continue" },
  { label: "Step 3 of 4 · Domains & SSL", primary: "Continue" },
  { label: "All set", primary: "Create your first site" },
];

export function Onboarding() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const finish = () => navigate("/sites");
  const next = () => (step >= STEP_COUNT - 1 ? finish() : setStep(step + 1));
  const meta = STEP_META[step];

  // The Domains & SSL step is MANDATORY: without the resolver + trusted CA no
  // site loads, so Continue stays locked until real state says both are done
  // (same ["dns-status"] source as FirstRunGate; Domains invalidates it after
  // a successful setup run).
  const { data: dns } = useQuery({ queryKey: ["dns-status"], queryFn: dnsStatus });
  const setupDone = !!dns?.resolverInstalled && !!dns?.caTrusted;
  const locked = step === 2 && !setupDone;

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-[radial-gradient(130%_100%_at_50%_-10%,var(--rex-hero-bg-from),var(--rex-hero-bg-to)_60%)]">
      {/* dotted texture + violet top aura */}
      <div
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage: "radial-gradient(circle at 1px 1px,var(--rex-texture-dot) 1px,transparent 0)",
          backgroundSize: "22px 22px",
        }}
      />
      <div
        className="pointer-events-none absolute -top-[120px] left-1/2 h-[280px] w-[420px] animate-rex-aura rounded-full blur-[20px] motion-reduce:animate-none"
        style={{ background: "radial-gradient(circle,rgba(124,92,255,.22),transparent 70%)" }}
      />

      {/* Title-bar drag strip — onboarding renders outside AppShell, so it has
          none of the shell's drag-region headers; without this the window
          can't be moved. Covers the traffic-light band + progress dots (no
          interactive elements there). */}
      <div
        onMouseDown={onTitleBarMouseDown}
        className="drag-region absolute inset-x-0 top-0 z-20 h-[60px]"
      />

      {/* progress dots */}
      <div className="relative z-10 flex flex-none items-center justify-center gap-[7px] pt-[46px]">
        {Array.from({ length: STEP_COUNT }).map((_, i) => (
          <span
            key={i}
            className="h-[6px] rounded-full transition-all duration-300"
            style={{ width: i === step ? "26px" : "6px", background: i <= step ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
          />
        ))}
      </div>

      {/* step content — scrollable so the pinned footer's Continue stays
          reachable at any window height: min-h-0 lets this area shrink instead
          of pushing the footer out of the clipped root; the inner min-h-full +
          justify-center centers when there's room yet scrolls from the top
          when there isn't (justify-center directly on a scroll container
          would make the top unreachable). */}
      <div className="relative z-10 min-h-0 flex-1 overflow-y-auto px-12 text-center">
        <div className="flex min-h-full flex-col items-center justify-center py-5">
          {step === 0 && <Welcome />}
          {step === 1 && <Install />}
          {step === 2 && <Domains />}
          {step === 3 && <OnboardingDone />}
        </div>
      </div>

      {/* footer: skip · step label · primary */}
      <div className="relative z-10 flex flex-none items-center justify-between gap-3 px-[34px] pb-[30px]">
        {/* No "Skip setup": skipping lands in an app where no site can load
            (resolver/CA missing) — the same hole the locked Continue closes. */}
        <div className="min-w-[90px]" />
        <div className="font-mono text-[0.6875rem] text-rex-text-muted">{meta.label}</div>
        <div className="flex min-w-[90px] justify-end">
          <button
            onClick={next}
            disabled={locked}
            title={locked ? "Finish the domains & SSL setup to continue" : undefined}
            className="flex h-10 items-center gap-2 rounded-[11px] bg-primary px-[18px] text-[0.84375rem] font-semibold text-white shadow-glow-primary transition-[filter] hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-45 disabled:shadow-none disabled:hover:brightness-100"
          >
            {meta.primary}
            {step < STEP_COUNT - 1 && <ChevronRight className="h-[15px] w-[15px]" strokeWidth={2.2} />}
          </button>
        </div>
      </div>
    </div>
  );
}

function CrownHero() {
  return (
    <div className="relative mb-[26px] flex h-[88px] w-[88px] animate-rex-float items-center justify-center rounded-[24px] border border-rex-hero-chip-border bg-gradient-to-br from-rex-hero-chip-from to-rex-hero-chip-to shadow-[0_16px_44px_rgba(124,92,255,.34)] motion-reduce:animate-none">
      <div
        className="pointer-events-none absolute -inset-[14px] rounded-full blur-[8px]"
        style={{ background: "radial-gradient(circle,rgba(124,92,255,.4),transparent 68%)" }}
      />
      <RexLogo className="relative block h-[48px] w-auto" />
    </div>
  );
}

function Welcome() {
  return (
    <div className="flex flex-col items-center">
      <CrownHero />
      <div className="font-display text-[3.375rem] font-semibold leading-none tracking-[-0.03em] text-rex-text-hero [text-shadow:0_2px_30px_rgba(124,92,255,.25)]">
        rexenv
      </div>
      <div className="mt-[18px] font-display text-[1.1875rem] font-medium tracking-[-0.01em] text-rex-text-bright">
        Your local development environment — fast, native, all in one.
      </div>
      <div className="mt-3 max-w-[380px] text-[0.84375rem] leading-[1.55] text-rex-text-muted">
        Run every server, site, and database from one calm command room. Let's get you set up — it
        takes about a minute.
      </div>
    </div>
  );
}

function StepHeading({ title, subtitle }: { title: string; subtitle: ReactNode }) {
  return (
    <>
      <div className="font-display text-[1.6875rem] font-semibold tracking-[-0.02em] text-rex-text-hero">
        {title}
      </div>
      <div className="mt-[9px] text-[0.84375rem] leading-[1.55] text-rex-text-muted">{subtitle}</div>
    </>
  );
}

/** Chip visuals per binary (accent tokens only). Fallback: periwinkle. */
const CHIP: Record<string, { abbr: string; tone: "periwinkle" | "teal" | "blue" | "amber" | "red" }> = {
  php: { abbr: "PHP", tone: "periwinkle" },
  "php-fpm": { abbr: "PHP", tone: "periwinkle" },
  nginx: { abbr: "Nx", tone: "teal" },
  caddy: { abbr: "Cf", tone: "blue" },
  mysql: { abbr: "My", tone: "amber" },
  mailpit: { abbr: "Mp", tone: "red" },
  adminer: { abbr: "Ad", tone: "blue" },
  frankenphp: { abbr: "Fp", tone: "periwinkle" },
};

function chipStyle(name: string) {
  const tone = (CHIP[name] ?? { tone: "periwinkle" as const }).tone;
  return {
    background: `var(--rex-accent-${tone}-bg)`,
    borderColor: `var(--rex-accent-${tone}-border)`,
    color: `var(--rex-accent-${tone})`,
  };
}

/** One core component row: static plan info + the live hub item overlaid. The
 *  Track keeps identical geometry through every phase (no layout shift). */
function InstallRow({ planned, item }: { planned: PlannedDownload; item?: DownloadItem }) {
  const [retrying, setRetrying] = useState(false);
  const phase: DownloadPhase = item?.phase ?? (planned.cached ? "cached" : "pending");
  const pct = item ? pctOf(item) : null;
  const status =
    phase === "cached" || phase === "done"
      ? "Ready"
      : phase === "pending"
        ? "Queued"
        : phase === "preparing"
          ? "Preparing…"
          : phase === "failed"
            ? "Failed"
            : pct != null
              ? `${pct}%`
              : "…";
  const trackState =
    phase === "failed"
      ? ("error" as const)
      : phase === "done" || phase === "cached"
        ? ("ok" as const)
        : phase === "pending"
          ? ("idle" as const)
          : ("run" as const);

  return (
    <div className="rounded-[10px] border border-rex-border-subtle bg-rex-surface-1 px-3 py-2.5">
      <div className="mb-2 flex items-center gap-2.5">
        <span
          className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border font-mono text-[0.625rem] font-bold"
          style={chipStyle(planned.name)}
        >
          {(CHIP[planned.name] ?? { abbr: planned.name.slice(0, 2) }).abbr}
        </span>
        <span className="flex-1 truncate text-left text-[0.84375rem] font-medium text-rex-text">
          {planned.label}
        </span>
        {phase === "failed" ? (
          <button
            onClick={() => {
              setRetrying(true);
              void retryDownload(planned.name, planned.version).finally(() => setRetrying(false));
            }}
            disabled={retrying}
            title={item?.error ?? undefined}
            className="flex flex-none items-center gap-1 rounded-[6px] border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[0.625rem] uppercase tracking-[0.08em] text-status-error-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
          >
            <RotateCw className={retrying ? "h-[10px] w-[10px] animate-rex-spin" : "h-[10px] w-[10px]"} strokeWidth={2.2} />
            Retry
          </button>
        ) : (
          <span className="flex flex-none items-center gap-1 font-mono text-[0.65625rem] uppercase tracking-[0.1em] text-rex-text-muted">
            {(phase === "cached" || phase === "done") && (
              <Check className="h-[12px] w-[12px] text-status-running" strokeWidth={2.4} />
            )}
            {status}
          </span>
        )}
      </div>
      <Track pct={pct} state={trackState} />
    </div>
  );
}

/** The core-component set as the wizard sees it: the static plan with the live
 *  hub item overlaid per row, and ONE verdict derived from all of them.
 *
 *  Shared by the Install step and the final step, because the final step used
 *  to be a static "Everything's installed ✓ Core components" — rendered, on the
 *  first clean-VM smoke test (18 Sep 2026), over six rows that had all FAILED
 *  (a dead DNS relay, every download gave up). The step's own copy promised
 *  "create your first site and rexenv will serve it instantly", and the site
 *  card then had to say the components were missing. A summary that cannot
 *  fail is not a summary; this one is the same fact the rows show. */
function useCoreComponents() {
  const [plan, setPlan] = useState<PlannedDownload[] | null>(null);
  const downloads = useDownloads();
  useEffect(() => {
    void coreBinariesPlan().then(setPlan).catch(() => setPlan([]));
  }, []);
  const items = new Map(downloads.items.map((i) => [i.id, i]));
  const phaseOf = (p: PlannedDownload): DownloadPhase =>
    items.get(p.id)?.phase ?? (p.cached ? "cached" : "pending");
  const rows = plan ?? [];
  const done = rows.filter((p) => phaseOf(p) === "cached" || phaseOf(p) === "done");
  const failed = rows.filter((p) => phaseOf(p) === "failed");
  const verdict: "checking" | "ready" | "failed" | "downloading" =
    plan === null ? "checking" : failed.length > 0 ? "failed" : done.length === rows.length ? "ready" : "downloading";
  return { plan, items, total: rows.length, done: done.length, failed, verdict };
}

function Install() {
  const { plan, items, verdict } = useCoreComponents();
  const fired = useRef(false);
  // Auto-prefetch on entering the step — fire WITHOUT awaiting: progress
  // arrives via download-progress events, failures land on their rows, and
  // continuing (or skipping) onboarding never cancels the backend downloads.
  useEffect(() => {
    if (!fired.current) {
      fired.current = true;
      void prefetchCoreBinaries().catch(() => {
        // Row-level errors already carry the details; nothing extra to do.
      });
    }
  }, []);
  const ready = verdict === "ready";

  return (
    <div className="w-full max-w-[440px]">
      <StepHeading
        title="Bundled core components"
        subtitle="rexenv ships its own runtimes, so nothing touches your system setup. They're downloading now — you can keep going while that runs in the background."
      />
      <div className="mt-[26px] flex flex-col gap-[10px] text-left">
        {plan === null ? (
          <div className="py-4 text-center font-mono text-[0.6875rem] text-rex-text-muted">
            Checking components…
          </div>
        ) : (
          plan.map((p) => <InstallRow key={p.id} planned={p} item={items.get(p.id)} />)
        )}
      </div>
      <div className="mt-6 text-center font-mono text-[0.6875rem] text-rex-text-muted">
        {ready ? "All components ready · no system changes" : "Downloads continue in the background · no system changes"}
      </div>
    </div>
  );
}

function StatusPill({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <span className="inline-flex items-center gap-[7px] rounded-full border border-rex-border bg-rex-surface-1 py-1.5 pl-[10px] pr-3">
      <span className="flex">{icon}</span>
      <span className="text-[0.75rem] text-rex-text-bright">{label}</span>
    </span>
  );
}

function Domains() {
  const words = usePlatformWords();
  const qc = useQueryClient();
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [error, setError] = useState("");
  // Real privileged setup: install the .rex backbone resolver (admin prompt) +
  // trust the local CA (keychain dialog), via core::setup::run_system_setup.
  // Other TLDs (.test included) install on demand when their first site is made.
  const run = async () => {
    setState("busy");
    setError("");
    try {
      await systemSetup();
      setState("done");
    } catch (e) {
      setError(String(e));
      setState("error");
    } finally {
      // Unlock (or keep locked) the wizard's Continue from real state — also
      // refreshes FirstRunGate's view after a partial run (e.g. resolver
      // installed but the keychain dialog cancelled).
      void qc.invalidateQueries({ queryKey: ["dns-status"] });
    }
  };
  return (
    <div className="w-full max-w-[460px]">
      <div className="mb-[22px] flex justify-center gap-2.5">
        <StatusPill
          icon={<Shield className="h-[15px] w-[15px] text-rex-accent-teal" strokeWidth={1.7} />}
          label="Local CA"
        />
        <StatusPill
          icon={<Globe className="h-[15px] w-[15px] text-rex-accent-blue" strokeWidth={1.7} />}
          label="Local DNS"
        />
        <StatusPill
          icon={<Lock className="h-[15px] w-[15px] text-status-running" strokeWidth={1.7} />}
          label="HTTPS"
        />
      </div>
      <StepHeading
        title="Set up local domains & SSL"
        subtitle={
          <>
            So your sites work at{" "}
            <span className="font-mono text-brand-tint">https://anything.rex</span>, rexenv adds a
            private certificate authority to {words.caTarget} and points{" "}
            <span className="font-mono text-rex-text-bright">.rex</span> domains to your machine.
            Nothing leaves your computer.
          </>
        }
      />
      {(state === "idle" || state === "error") && (
        <div className="mt-[22px]">
          <button
            onClick={() => void run()}
            className="inline-flex h-[42px] items-center gap-2 rounded-[11px] bg-primary px-[22px] text-[0.875rem] font-semibold text-white shadow-glow-primary transition-[filter] hover:brightness-110"
          >
            {state === "error" ? "Try again" : "Set up domains & SSL"}
          </button>
          <div className="mt-3 font-mono text-[0.65625rem] text-rex-text-muted">
            {words.osName} will ask for permission (resolver + certificate)
          </div>
          {state === "error" && (
            <div className="mx-auto mt-3 max-w-[380px] text-[0.75rem] leading-[1.5] text-status-error-bright">
              {error}
            </div>
          )}
        </div>
      )}
      {state === "busy" && (
        <div className="mt-[22px] inline-flex h-[42px] items-center gap-[9px] rounded-[11px] border border-rex-border bg-rex-surface-1 px-5">
          <span className="h-[14px] w-[14px] rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
          <span className="text-[0.8125rem] text-rex-text-bright">
            Configuring certificate authority & DNS…
          </span>
        </div>
      )}
      {state === "done" && (
        <div className="mt-[22px] inline-flex items-center gap-[9px] rounded-[11px] border border-status-running-border bg-status-running-bg px-[18px] py-[11px]">
          <Check className="h-[17px] w-[17px] text-status-running" strokeWidth={2.2} />
          <span className="text-[0.8125rem] font-medium text-rex-text">Domains & SSL are ready</span>
        </div>
      )}
    </div>
  );
}

function DoneChip({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 font-mono text-[0.6875rem] text-rex-text-muted">
      <Check className="h-[13px] w-[13px] text-status-running" strokeWidth={2.4} />
      {label}
    </span>
  );
}

/** The :443 warning, shown ONLY when something else already answers it.
 *
 *  Placed on the last step because that is where onboarding promises "create
 *  your first site and rexenv will serve it instantly" — the one sentence a
 *  foreign proxy makes false. It WARNS and never blocks: nothing in onboarding
 *  needs :443, and someone evaluating rexenv with Herd running is in a
 *  deliberate state, not a broken one. The thing that actually needs the port
 *  tells them again when they reach it (the provision card, the watchdog,
 *  `rex doctor`).
 *
 *  Nothing listening returns null from the backend and renders NOTHING — at
 *  onboarding that is the ordinary case, not a problem.
 *
 *  Every clause is LOAD-BEARING and guarded (`the_onboarding_edge_notice_says
 *  _what_it_costs_and_that_continuing_is_fine`, core/proxy.rs). "You can finish
 *  setting up" is the one a trim reads as reassurance: it is the warn-not-block
 *  decision made visible, and without it this is a wall. */
function EdgeConflictNotice() {
  const { data } = useQuery({ queryKey: ["setup-edge-conflict"], queryFn: setupEdgeConflict });
  const words = usePlatformWords();
  if (!data) return null;
  const holder = data.app ?? data.holder ?? "Another app";
  const quit = data.app ?? "it";
  return (
    <div className="mt-6 w-full max-w-[440px] rounded-[11px] border border-status-warning-border bg-status-warning-bg px-3.5 py-3 text-left">
      <div className="text-[0.78125rem] font-medium text-rex-text">
        {holder} is answering HTTPS on {words.host}.
      </div>
      <div className="mt-1 text-[0.71875rem] leading-[1.55] text-rex-text-muted">
        rexenv needs port 443 to serve sites, so they won't load while {quit === "it" ? "it" : quit} has
        it. Nothing here depends on it — you can finish setting up and quit {quit} whenever you like.
      </div>
      {data.fix && <CommandLine command={data.fix} />}
    </div>
  );
}

/** The copyable fix, same shape as the toast's command block. */
function CommandLine({ command }: { command: string }) {
  return (
    <code className="mt-2 block truncate rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5 font-mono text-[0.6875rem] text-rex-text">
      $ {command}
    </code>
  );
}

/** The last step tells the truth the Install step's rows tell: ready, still
 *  downloading, or failed — and only the first one gets "Everything's
 *  installed". See `useCoreComponents` for the run that made this a rule. */
export function OnboardingDone() {
  const core = useCoreComponents();
  const [retrying, setRetrying] = useState(false);
  const retryAll = () => {
    setRetrying(true);
    void Promise.allSettled(core.failed.map((p) => retryDownload(p.name, p.version))).finally(() =>
      setRetrying(false),
    );
  };
  const copy =
    core.verdict === "failed"
      ? `Your local domains work over HTTPS, but ${core.failed.length} of ${core.total} core components failed to download. Retry them here or from the footer — a site can't start until they land.`
      : core.verdict === "downloading"
        ? `Your local domains work over HTTPS. Core components are still downloading (${core.done} of ${core.total} ready) — create your first site now and it starts as soon as they land.`
        : "Everything's installed and your local domains work over HTTPS. Create your first site and rexenv will serve it instantly.";
  const ready = core.verdict === "ready";
  return (
    <div className="flex flex-col items-center">
      {ready ? (
        <div className="relative mb-6 flex h-[78px] w-[78px] items-center justify-center rounded-full border border-status-running-border bg-gradient-to-br from-rex-success-chip-from to-rex-success-chip-to shadow-[0_14px_38px_rgba(63,185,80,0.26)]">
          <div
            className="pointer-events-none absolute -inset-3 rounded-full blur-[7px]"
            style={{ background: "radial-gradient(circle,rgba(63,185,80,.32),transparent 68%)" }}
          />
          <Check className="relative h-10 w-10 text-status-running" strokeWidth={2.4} />
        </div>
      ) : (
        // The green medal is the ready state's; over a failed or half-done set it
        // would say in icon what the copy below no longer says in words.
        <div className="relative mb-6 flex h-[78px] w-[78px] items-center justify-center rounded-full border border-rex-border bg-rex-surface-1">
          {core.verdict === "failed" ? (
            <RotateCw className="relative h-9 w-9 text-status-error-bright" strokeWidth={2.2} />
          ) : (
            <Loader2 className="relative h-9 w-9 animate-rex-spin text-rex-text-muted motion-reduce:animate-none" strokeWidth={2.2} />
          )}
        </div>
      )}
      <div className="font-display text-[2.125rem] font-semibold leading-[1.1] tracking-[-0.025em] text-rex-text-hero">
        {ready ? "Your kingdom is ready" : "Nearly there"}
      </div>
      <div className="mt-3 max-w-[400px] text-[0.875rem] leading-[1.55] text-rex-text-muted">
        {copy}
      </div>
      <EdgeConflictNotice />
      <div className="mt-[18px] flex items-center gap-[14px]">
        {core.verdict === "ready" ? (
          <DoneChip label="Core components" />
        ) : core.verdict === "failed" ? (
          <button
            onClick={retryAll}
            disabled={retrying}
            title={core.failed.map((p) => `${p.label}: ${core.items.get(p.id)?.error ?? "failed"}`).join("\n")}
            className="inline-flex items-center gap-1.5 rounded-[6px] border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[0.6875rem] text-status-error-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
          >
            <RotateCw className={retrying ? "h-[12px] w-[12px] animate-rex-spin" : "h-[12px] w-[12px]"} strokeWidth={2.2} />
            Core components · {core.failed.length} failed · Retry
          </button>
        ) : (
          <span className="inline-flex items-center gap-1.5 font-mono text-[0.6875rem] text-rex-text-muted">
            <Loader2 className="h-[13px] w-[13px] animate-rex-spin motion-reduce:animate-none" strokeWidth={2.2} />
            Core components · {core.verdict === "checking" ? "checking…" : `${core.done} of ${core.total} ready`}
          </span>
        )}
        <DoneChip label="Domains & SSL" />
      </div>
    </div>
  );
}
