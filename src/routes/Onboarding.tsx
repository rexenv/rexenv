import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";

/**
 * First-run onboarding — the ONE place the Space Grotesk display face and a
 * bolder violet brand moment are used (per DESIGN_BRIEF Block 12).
 */
export function Onboarding() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-6 bg-rex-bg px-8 text-center">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl border border-rex-border-strong bg-gradient-to-br from-[#20232C] to-[#13151B] shadow-glow-primary">
        <svg width="34" height="34" viewBox="0 0 24 24" className="block">
          <path
            d="M3 8.4 L8 12.6 L12 5 L16 12.6 L21 8.4 L19.1 18.7 L4.9 18.7 Z"
            fill="#7C5CFF"
            stroke="#7C5CFF"
            strokeWidth="1.1"
            strokeLinejoin="round"
          />
          <circle cx="12" cy="5" r="1.5" fill="#C9BCFF" />
        </svg>
      </div>
      <div>
        <h1 className="font-display text-[32px] font-semibold tracking-[-0.02em] text-rex-text">
          rexenv
        </h1>
        <p className="mt-2 max-w-sm text-[15px] text-rex-text-muted">
          Your local development environment — fast, native, all in one.
        </p>
      </div>
      <Button variant="primary" size="lg" asChild>
        <Link to="/sites">Create your first site</Link>
      </Button>
    </div>
  );
}
