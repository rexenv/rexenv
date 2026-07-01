import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "@/components/ui/button";

/** Shared overlay + centered card for in-app modals. WKWebView (Tauri) doesn't
 *  reliably support window.alert/confirm/prompt, so we use these instead. */
function Overlay({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
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
        className="w-[380px] rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** In-app replacement for `window.confirm`. */
export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  danger = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  message?: React.ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Overlay onClose={onCancel}>
      <div className="text-[15px] font-semibold text-rex-text">{title}</div>
      {message && <div className="mt-2 text-[13px] leading-[1.55] text-rex-text-muted">{message}</div>}
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant={danger ? "danger" : "primary"} onClick={onConfirm} autoFocus>
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
      <div className="text-[15px] font-semibold text-rex-text">{title}</div>
      {label && <label className="mb-1.5 mt-3 block text-[12px] text-rex-text-muted">{label}</label>}
      <input
        autoFocus
        value={value}
        placeholder={placeholder}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
        className={`mt-${label ? "0" : "3"} h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 text-[13px] text-rex-text outline-none focus:border-brand ${
          mono ? "font-mono text-[12.5px]" : ""
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
