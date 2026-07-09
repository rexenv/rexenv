import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { Check, ChevronRight, Globe, Lock, RotateCw, Shield } from "lucide-react";
import { coreBinariesPlan, prefetchCoreBinaries, retryDownload, systemSetup } from "@/lib/ipc";
import { useDownloads } from "@/lib/useDownloads";
import { Track, pctOf } from "@/components/shell/DownloadPanel";
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

      {/* step content */}
      <div className="relative z-10 flex flex-1 flex-col items-center justify-center px-12 text-center">
        {step === 0 && <Welcome />}
        {step === 1 && <Install />}
        {step === 2 && <Domains />}
        {step === 3 && <Done />}
      </div>

      {/* footer: skip · step label · primary */}
      <div className="relative z-10 flex flex-none items-center justify-between gap-3 px-[34px] pb-[30px]">
        <div className="min-w-[90px]">
          {step === 0 && (
            <button
              onClick={finish}
              className="px-1 py-2 text-[13px] text-rex-text-dim transition-colors hover:text-rex-text-bright"
            >
              Skip setup
            </button>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-label">{meta.label}</div>
        <div className="flex min-w-[90px] justify-end">
          <button
            onClick={next}
            className="flex h-10 items-center gap-2 rounded-[11px] bg-primary px-[18px] text-[13.5px] font-semibold text-white shadow-glow-primary transition-[filter] hover:brightness-110"
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
      <svg width="48" height="48" viewBox="0 0 24 24" className="relative block">
        <path
          d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
          style={{ fill: "var(--rex-brand)", stroke: "var(--rex-brand-hover)" }}
          strokeWidth="0.8"
          strokeLinejoin="round"
        />
        <circle cx="3" cy="8.4" r="1.5" style={{ fill: "var(--rex-crown-gem)" }} />
        <circle cx="12" cy="5" r="1.8" style={{ fill: "var(--rex-crown-gem-bright)" }} />
        <circle cx="21" cy="8.4" r="1.5" style={{ fill: "var(--rex-crown-gem)" }} />
      </svg>
    </div>
  );
}

function Welcome() {
  return (
    <div className="flex flex-col items-center">
      <CrownHero />
      <div className="font-display text-[54px] font-semibold leading-none tracking-[-0.03em] text-rex-text-hero [text-shadow:0_2px_30px_rgba(124,92,255,.25)]">
        rexenv
      </div>
      <div className="mt-[18px] font-display text-[19px] font-medium tracking-[-0.01em] text-rex-text-bright">
        Your local development environment — fast, native, all in one.
      </div>
      <div className="mt-3 max-w-[380px] text-[13.5px] leading-[1.55] text-rex-text-muted">
        Run every server, site, and database from one calm command room. Let's get you set up — it
        takes about a minute.
      </div>
    </div>
  );
}

function StepHeading({ title, subtitle }: { title: string; subtitle: ReactNode }) {
  return (
    <>
      <div className="font-display text-[27px] font-semibold tracking-[-0.02em] text-rex-text-hero">
        {title}
      </div>
      <div className="mt-[9px] text-[13.5px] leading-[1.55] text-rex-text-muted">{subtitle}</div>
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
          className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border font-mono text-[10px] font-bold"
          style={chipStyle(planned.name)}
        >
          {(CHIP[planned.name] ?? { abbr: planned.name.slice(0, 2) }).abbr}
        </span>
        <span className="flex-1 truncate text-left text-[13.5px] font-medium text-rex-text">
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
            className="flex flex-none items-center gap-1 rounded-[6px] border border-rex-border-strong bg-rex-surface-2 px-2 py-1 font-mono text-[10px] uppercase tracking-[0.08em] text-status-error-bright transition-[filter] hover:brightness-110 disabled:opacity-60"
          >
            <RotateCw className={retrying ? "h-[10px] w-[10px] animate-rex-spin" : "h-[10px] w-[10px]"} strokeWidth={2.2} />
            Retry
          </button>
        ) : (
          <span className="flex flex-none items-center gap-1 font-mono text-[10.5px] uppercase tracking-[0.1em] text-rex-text-dim">
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

function Install() {
  const [plan, setPlan] = useState<PlannedDownload[] | null>(null);
  const downloads = useDownloads();
  const fired = useRef(false);
  // Auto-prefetch on entering the step — fire WITHOUT awaiting: progress
  // arrives via download-progress events, failures land on their rows, and
  // continuing (or skipping) onboarding never cancels the backend downloads.
  useEffect(() => {
    void coreBinariesPlan().then(setPlan).catch(() => setPlan([]));
    if (!fired.current) {
      fired.current = true;
      void prefetchCoreBinaries().catch(() => {
        // Row-level errors already carry the details; nothing extra to do.
      });
    }
  }, []);

  const items = new Map(downloads.items.map((i) => [i.id, i]));
  const ready =
    plan !== null &&
    plan.every((p) => {
      const phase = items.get(p.id)?.phase ?? (p.cached ? "cached" : "pending");
      return phase === "cached" || phase === "done";
    });

  return (
    <div className="w-full max-w-[440px]">
      <StepHeading
        title="Bundled core components"
        subtitle="rexenv ships its own runtimes, so nothing touches your system setup. They're downloading now — you can keep going while that runs in the background."
      />
      <div className="mt-[26px] flex flex-col gap-[10px] text-left">
        {plan === null ? (
          <div className="py-4 text-center font-mono text-[11px] text-rex-text-dim">
            Checking components…
          </div>
        ) : (
          plan.map((p) => <InstallRow key={p.id} planned={p} item={items.get(p.id)} />)
        )}
      </div>
      <div className="mt-6 text-center font-mono text-[11px] text-rex-text-dim">
        {ready ? "All components ready · no system changes" : "Downloads continue in the background · no system changes"}
      </div>
    </div>
  );
}

function StatusPill({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <span className="inline-flex items-center gap-[7px] rounded-full border border-rex-border bg-rex-surface-1 py-1.5 pl-[10px] pr-3">
      <span className="flex">{icon}</span>
      <span className="text-[12px] text-rex-text-bright">{label}</span>
    </span>
  );
}

function Domains() {
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [error, setError] = useState("");
  // Real privileged setup: install the .test resolver (admin prompt) + trust the
  // local CA (keychain dialog), via core::setup::run_system_setup.
  const run = async () => {
    setState("busy");
    setError("");
    try {
      await systemSetup();
      setState("done");
    } catch (e) {
      setError(String(e));
      setState("error");
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
            <span className="font-mono text-brand-tint">https://anything.test</span>, rexenv adds a
            private certificate authority to your Mac and points{" "}
            <span className="font-mono text-rex-text-bright">.test</span> domains to your machine.
            Nothing leaves your computer.
          </>
        }
      />
      {(state === "idle" || state === "error") && (
        <div className="mt-[22px]">
          <button
            onClick={() => void run()}
            className="inline-flex h-[42px] items-center gap-2 rounded-[11px] bg-primary px-[22px] text-[14px] font-semibold text-white shadow-glow-primary transition-[filter] hover:brightness-110"
          >
            {state === "error" ? "Try again" : "Set up domains & SSL"}
          </button>
          <div className="mt-3 font-mono text-[10.5px] text-rex-text-faint">
            macOS will ask for permission (resolver + certificate)
          </div>
          {state === "error" && (
            <div className="mx-auto mt-3 max-w-[380px] text-[12px] leading-[1.5] text-status-error-bright">
              {error}
            </div>
          )}
        </div>
      )}
      {state === "busy" && (
        <div className="mt-[22px] inline-flex h-[42px] items-center gap-[9px] rounded-[11px] border border-rex-border bg-rex-surface-1 px-5">
          <span className="h-[14px] w-[14px] rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
          <span className="text-[13px] text-rex-text-bright">
            Configuring certificate authority & DNS…
          </span>
        </div>
      )}
      {state === "done" && (
        <div className="mt-[22px] inline-flex items-center gap-[9px] rounded-[11px] border border-status-running-border bg-status-running-bg px-[18px] py-[11px]">
          <Check className="h-[17px] w-[17px] text-status-running" strokeWidth={2.2} />
          <span className="text-[13px] font-medium text-rex-text">Domains & SSL are ready</span>
        </div>
      )}
    </div>
  );
}

function DoneChip({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 font-mono text-[11px] text-rex-text-muted">
      <Check className="h-[13px] w-[13px] text-status-running" strokeWidth={2.4} />
      {label}
    </span>
  );
}

function Done() {
  return (
    <div className="flex flex-col items-center">
      <div className="relative mb-6 flex h-[78px] w-[78px] items-center justify-center rounded-full border border-status-running-border bg-gradient-to-br from-rex-success-chip-from to-rex-success-chip-to shadow-[0_14px_38px_rgba(63,185,80,0.26)]">
        <div
          className="pointer-events-none absolute -inset-3 rounded-full blur-[7px]"
          style={{ background: "radial-gradient(circle,rgba(63,185,80,.32),transparent 68%)" }}
        />
        <Check className="relative h-10 w-10 text-status-running" strokeWidth={2.4} />
      </div>
      <div className="font-display text-[34px] font-semibold leading-[1.1] tracking-[-0.025em] text-rex-text-hero">
        Your kingdom is ready
      </div>
      <div className="mt-3 max-w-[400px] text-[14px] leading-[1.55] text-rex-text-muted">
        Everything's installed and your local domains work over HTTPS. Create your first site and
        rexenv will serve it instantly.
      </div>
      <div className="mt-[18px] flex items-center gap-[14px]">
        <DoneChip label="Core components" />
        <DoneChip label="Domains & SSL" />
      </div>
    </div>
  );
}
