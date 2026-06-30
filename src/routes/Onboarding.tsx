import { useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { Check, ChevronRight, Globe, Lock, Shield } from "lucide-react";

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
    <div className="relative flex h-full flex-col overflow-hidden bg-[radial-gradient(130%_100%_at_50%_-10%,#1A1530,#0D0E12_60%)]">
      {/* dotted texture + violet top aura */}
      <div
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage: "radial-gradient(circle at 1px 1px,rgba(255,255,255,.02) 1px,transparent 0)",
          backgroundSize: "22px 22px",
        }}
      />
      <div
        className="pointer-events-none absolute -top-[120px] left-1/2 h-[280px] w-[420px] -translate-x-1/2 rounded-full blur-[20px]"
        style={{ background: "radial-gradient(circle,rgba(124,92,255,.22),transparent 70%)" }}
      />

      {/* progress dots */}
      <div className="relative z-10 flex flex-none items-center justify-center gap-[7px] pt-[46px]">
        {Array.from({ length: STEP_COUNT }).map((_, i) => (
          <span
            key={i}
            className="h-[6px] rounded-full transition-all duration-300"
            style={{ width: i === step ? "26px" : "6px", background: i <= step ? "#7C5CFF" : "#2A2E38" }}
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
    <div className="relative mb-[26px] flex h-[88px] w-[88px] animate-rex-float items-center justify-center rounded-[24px] border border-[#34304E] bg-gradient-to-br from-[#23263180] to-[#13151Bc0] shadow-[0_16px_44px_rgba(124,92,255,.34)] motion-reduce:animate-none">
      <div
        className="pointer-events-none absolute -inset-[14px] rounded-full blur-[8px]"
        style={{ background: "radial-gradient(circle,rgba(124,92,255,.4),transparent 68%)" }}
      />
      <svg width="48" height="48" viewBox="0 0 24 24" className="relative block">
        <path
          d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
          fill="#7C5CFF"
          stroke="#8E72FF"
          strokeWidth="0.8"
          strokeLinejoin="round"
        />
        <circle cx="3" cy="8.4" r="1.5" fill="#B9A6FF" />
        <circle cx="12" cy="5" r="1.8" fill="#D7CCFF" />
        <circle cx="21" cy="8.4" r="1.5" fill="#B9A6FF" />
      </svg>
    </div>
  );
}

function Welcome() {
  return (
    <div className="flex flex-col items-center">
      <CrownHero />
      <div className="font-display text-[54px] font-semibold leading-none tracking-[-0.03em] text-[#F2F0FA] [text-shadow:0_2px_30px_rgba(124,92,255,.25)]">
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
      <div className="font-display text-[27px] font-semibold tracking-[-0.02em] text-[#F2F0FA]">
        {title}
      </div>
      <div className="mt-[9px] text-[13.5px] leading-[1.55] text-rex-text-muted">{subtitle}</div>
    </>
  );
}

const INSTALL_ROWS = [
  { abbr: "PHP", name: "PHP 8.3 runtime", size: "62 MB", bg: "rgba(125,128,185,0.17)", border: "rgba(125,128,185,0.32)", color: "#A7AADD" },
  { abbr: "Nx", name: "Nginx web server", size: "8 MB", bg: "rgba(45,156,143,0.13)", border: "rgba(45,156,143,0.28)", color: "#5FBFA8" },
  { abbr: "Cf", name: "Edge router", size: "41 MB", bg: "rgba(74,134,170,0.15)", border: "rgba(74,134,170,0.30)", color: "#7DB8D8" },
];

function Install() {
  // Shell: simulate staggered downloads so the bars + states are demonstrable.
  const [pct, setPct] = useState([0, 0, 0]);
  useEffect(() => {
    const id = setInterval(() => {
      setPct((prev) => {
        const i = prev.findIndex((p) => p < 100);
        if (i === -1) return prev;
        const next = [...prev];
        next[i] = Math.min(100, next[i] + 12);
        return next;
      });
    }, 130);
    return () => clearInterval(id);
  }, []);
  const allDone = pct.every((p) => p >= 100);

  return (
    <div className="w-full max-w-[440px]">
      <StepHeading
        title="Installing core components"
        subtitle="rexenv bundles its own runtimes, so nothing touches your system setup. This downloads once."
      />
      <div className="mt-[26px] flex flex-col gap-[14px] text-left">
        {INSTALL_ROWS.map((r, i) => {
          const p = pct[i];
          const done = p >= 100;
          const active = !done && (i === 0 || pct[i - 1] >= 100);
          return (
            <div key={r.abbr}>
              <div className="mb-[7px] flex items-center gap-2.5">
                <span
                  className="flex h-[26px] w-[26px] flex-none items-center justify-center rounded-[7px] border font-mono text-[10px] font-bold"
                  style={{ background: r.bg, borderColor: r.border, color: r.color }}
                >
                  {r.abbr}
                </span>
                <span className="flex-1 text-[13.5px] font-medium text-rex-text">{r.name}</span>
                <span className="flex items-center gap-[7px]">
                  {done ? (
                    <Check className="h-4 w-4 text-status-running" strokeWidth={2.2} />
                  ) : active ? (
                    <span className="h-[13px] w-[13px] rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
                  ) : null}
                  <span
                    className="min-w-[74px] text-right font-mono text-[11px]"
                    style={{ color: done ? "#3FB950" : "#6E7681" }}
                  >
                    {done ? "Installed" : active ? `${p}%` : r.size}
                  </span>
                </span>
              </div>
              <div className="h-[6px] overflow-hidden rounded-full border border-rex-border-subtle bg-rex-surface-1">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-brand-strong to-brand-light shadow-glow-primary transition-[width] duration-300"
                  style={{ width: `${p}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
      <div className="mt-6 text-center font-mono text-[11px] text-rex-text-dim">
        {allDone
          ? "All components installed · bundled, no system changes"
          : "Downloading bundled runtimes…"}
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
  const [state, setState] = useState<"idle" | "busy" | "done">("idle");
  // Shell: simulate the privileged setup. TODO: wire to run_system_setup.
  const run = () => {
    setState("busy");
    setTimeout(() => setState("done"), 1800);
  };
  return (
    <div className="w-full max-w-[460px]">
      <div className="mb-[22px] flex justify-center gap-2.5">
        <StatusPill
          icon={<Shield className="h-[15px] w-[15px] text-[#5FBFA8]" strokeWidth={1.7} />}
          label="Local CA"
        />
        <StatusPill
          icon={<Globe className="h-[15px] w-[15px] text-[#7DB8D8]" strokeWidth={1.7} />}
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
      {state === "idle" && (
        <div className="mt-[22px]">
          <button
            onClick={run}
            className="inline-flex h-[42px] items-center gap-2 rounded-[11px] bg-primary px-[22px] text-[14px] font-semibold text-white shadow-glow-primary transition-[filter] hover:brightness-110"
          >
            Set up domains & SSL
          </button>
          <div className="mt-3 font-mono text-[10.5px] text-rex-text-faint">
            macOS will ask for your password once
          </div>
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
      <div className="relative mb-6 flex h-[78px] w-[78px] items-center justify-center rounded-full border border-status-running-border bg-gradient-to-br from-[#1A3322] to-[#13201A] shadow-[0_14px_38px_rgba(63,185,80,0.26)]">
        <div
          className="pointer-events-none absolute -inset-3 rounded-full blur-[7px]"
          style={{ background: "radial-gradient(circle,rgba(63,185,80,.32),transparent 68%)" }}
        />
        <Check className="relative h-10 w-10 text-status-running" strokeWidth={2.4} />
      </div>
      <div className="font-display text-[34px] font-semibold leading-[1.1] tracking-[-0.025em] text-[#F2F0FA]">
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
