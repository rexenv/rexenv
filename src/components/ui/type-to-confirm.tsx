import { TECH_INPUT } from "@/lib/utils";
import { CopyButton } from "@/components/ui/copy-button";

/** True when what the user typed satisfies a type-the-domain gate.
 *  Trimmed on purpose: the phrase is now copyable, and a pasted value that
 *  drags a trailing space along is the SAME intent — failing it would only
 *  teach people that the gate is broken. */
export function confirmPhraseMatches(typed: string, phrase: string): boolean {
  return typed.trim() === phrase;
}

/** The type-the-domain gate shared by every destructive confirm (reset,
 *  db-import overwrite, delete). ONE component so all three read the same and
 *  all three get the copy button — the phrase is shown with a copy button so
 *  the user can paste instead of retyping from memory. */
export function TypeToConfirm({
  phrase,
  value,
  onChange,
  disabled = false,
  autoFocus = false,
  className = "mt-4",
}: {
  phrase: string;
  value: string;
  onChange: (v: string) => void;
  disabled?: boolean;
  autoFocus?: boolean;
  /** Spacing above the block — call sites differ. */
  className?: string;
}) {
  return (
    <div className={className}>
      {/* The copy button sits INSIDE the sentence, not flexed to the card's
          right edge: a long domain wraps the line, and a right-aligned button
          then floats away from the value it copies. */}
      <div className="text-[0.78125rem] leading-[1.6] text-rex-text-muted">
        Type <span className="font-mono text-rex-text">{phrase}</span>
        <CopyButton
          value={phrase}
          title={`Copy ${phrase}`}
          className="mx-0.5 align-text-bottom"
          probe="confirm-copy"
        />{" "}
        to
        confirm:
      </div>
      <input
        {...TECH_INPUT}
        data-probe="confirm-input"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={phrase}
        disabled={disabled}
        autoFocus={autoFocus}
        className="mt-1.5 h-[32px] w-full rounded-md border border-rex-border bg-rex-surface-2 px-2.5 font-mono text-[0.78125rem] text-rex-text outline-none focus:border-status-error-border"
      />
    </div>
  );
}
