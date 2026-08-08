import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import { Button } from "@/components/ui/button";
import { TECH_INPUT } from "@/lib/utils";
import { confirmPhraseMatches, TypeToConfirm } from "@/components/ui/type-to-confirm";

/** Shared overlay + centered card for in-app modals. WKWebView (Tauri) doesn't
 *  reliably support window.alert/confirm/prompt, so we use these instead.
 *  Exported for dialogs whose shape ConfirmDialog can't express (e.g. the
 *  connected-site delete, which needs two consequence-naming actions). */
export function Overlay({
  onClose,
  children,
  cardClassName = "w-[380px]",
}: {
  onClose: () => void;
  children: React.ReactNode;
  /** Width/sizing of the card — override for dialogs that need more room
   *  than the 380px confirm-dialog default (e.g. the licenses viewer). */
  cardClassName?: string;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  return createPortal(
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        className={`${cardClassName} rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl`}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** In-app replacement for `window.confirm`. Pass `confirmPhrase` for the
 *  irreversible ones: the confirm button then stays disabled until the user
 *  types (or pastes — the phrase carries a copy button) that exact string, so
 *  a delete can't be one stray Enter away. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  danger = false,
  confirmPhrase,
  onConfirm,
  onCancel,
}: {
  title: string;
  message?: React.ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  confirmPhrase?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const [typed, setTyped] = useState("");
  const gated = confirmPhrase !== undefined;
  const match = gated && confirmPhraseMatches(typed, confirmPhrase);
  return (
    <Overlay onClose={onCancel}>
      <div className="text-[0.9375rem] font-semibold text-rex-text">{title}</div>
      {message && <div className="mt-2 text-[0.8125rem] leading-[1.55] text-rex-text-muted">{message}</div>}
      {gated && <TypeToConfirm phrase={confirmPhrase} value={typed} onChange={setTyped} autoFocus />}
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          variant={danger ? "danger" : "primary"}
          disabled={gated && !match}
          onClick={onConfirm}
          autoFocus={!gated}
        >
          {confirmLabel}
        </Button>
      </div>
    </Overlay>
  );
}

/** In-app replacement for `window.prompt` — a single text input. */
export function PromptDialog({
  title,
  label,
  initialValue = "",
  placeholder,
  submitLabel = "Save",
  mono = false,
  onSubmit,
  onCancel,
}: {
  title: string;
  label?: string;
  initialValue?: string;
  placeholder?: string;
  submitLabel?: string;
  mono?: boolean;
  onSubmit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initialValue);
  const trimmed = value.trim();
  const submit = () => {
    if (trimmed) onSubmit(trimmed);
  };
  return (
    <Overlay onClose={onCancel}>
      <div className="text-[0.9375rem] font-semibold text-rex-text">{title}</div>
      {label && <label className="mb-1.5 mt-3 block text-[0.75rem] text-rex-text-muted">{label}</label>}
      <input {...TECH_INPUT}
        autoFocus
        value={value}
        placeholder={placeholder}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
        className={`mt-${label ? "0" : "3"} h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 text-[0.8125rem] text-rex-text outline-none focus:border-brand ${
          mono ? "font-mono text-[0.78125rem]" : ""
        }`}
      />
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!trimmed} onClick={submit}>
          {submitLabel}
        </Button>
      </div>
    </Overlay>
  );
}

// ── Imperative API ──────────────────────────────────────────────────────────
// Promise-based confirm()/promptText() so call sites can replace window.confirm/
// window.prompt with a minimal `await`. A single <DialogHost/> (mounted in App)
// renders the active request.

type ConfirmReq = {
  kind: "confirm";
  title: string;
  message?: React.ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  resolve: (v: boolean) => void;
};
type PromptReq = {
  kind: "prompt";
  title: string;
  label?: string;
  initialValue?: string;
  placeholder?: string;
  submitLabel?: string;
  mono?: boolean;
  resolve: (v: string | null) => void;
};
type Req = ConfirmReq | PromptReq;

const useDialogStore = create<{ current: Req | null; open: (r: Req) => void; close: () => void }>((set) => ({
  current: null,
  open: (r) => set({ current: r }),
  close: () => set({ current: null }),
}));

/** In-app `window.confirm` — resolves true (confirmed) / false (cancelled). */
export function confirm(opts: Omit<ConfirmReq, "kind" | "resolve">): Promise<boolean> {
  return new Promise((resolve) => useDialogStore.getState().open({ kind: "confirm", resolve, ...opts }));
}

/** In-app `window.prompt` — resolves the entered text, or null if cancelled. */
export function promptText(opts: Omit<PromptReq, "kind" | "resolve">): Promise<string | null> {
  return new Promise((resolve) => useDialogStore.getState().open({ kind: "prompt", resolve, ...opts }));
}

/** Renders the active imperative dialog — mount once (App). */
export function DialogHost() {
  const current = useDialogStore((s) => s.current);
  const close = useDialogStore((s) => s.close);
  if (!current) return null;
  if (current.kind === "confirm") {
    return (
      <ConfirmDialog
        title={current.title}
        message={current.message}
        confirmLabel={current.confirmLabel}
        danger={current.danger}
        onConfirm={() => {
          current.resolve(true);
          close();
        }}
        onCancel={() => {
          current.resolve(false);
          close();
        }}
      />
    );
  }
  return (
    <PromptDialog
      title={current.title}
      label={current.label}
      initialValue={current.initialValue}
      placeholder={current.placeholder}
      submitLabel={current.submitLabel}
      mono={current.mono}
      onSubmit={(v) => {
        current.resolve(v);
        close();
      }}
      onCancel={() => {
        current.resolve(null);
        close();
      }}
    />
  );
}
