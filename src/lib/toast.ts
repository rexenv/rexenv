import { create } from "zustand";

export type ToastKind = "error" | "success" | "info";

/** Optional action button on a toast (e.g. "Show in Finder" on a DB export). */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
  /** Optional copyable shell command rendered as a code block (e.g. the
   *  "free this port" one-liner from a port-conflict error). */
  command?: string;
  action?: ToastAction;
}

interface ToastState {
  toasts: Toast[];
  push: (message: string, kind: ToastKind, command?: string, action?: ToastAction) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (message, kind, command, action) => {
    const id = nextId++;
    // Cap the visible stack: beyond 5 the oldest yields (its timer still
    // clears it from state) — an unbounded stack overflows the window top.
    set((s) => ({ toasts: [...s.toasts, { id, message, kind, command, action }].slice(-5) }));
    // Toasts carrying a command stay long enough to read + copy it; ones with
    // an action button long enough to click it.
    const ttl = command ? 30000 : action ? 10000 : kind === "error" ? 7000 : 4000;
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), ttl);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/**
 * Imperative toast API — call from anywhere (mutation `onError`, etc.). Replaces
 * `window.alert`, which WKWebView (Tauri) doesn't reliably show.
 */
export const toast = {
  error: (message: string, command?: string) =>
    useToastStore.getState().push(message, "error", command),
  success: (message: string, action?: ToastAction) =>
    useToastStore.getState().push(message, "success", undefined, action),
  info: (message: string) => useToastStore.getState().push(message, "info"),
};

/**
 * Show a backend error, extracting an embedded suggested command if present.
 * Backend contract (`core/ports::ensure_free`): when an error carries a fix-it
 * shell command, it is the LAST line and starts with `"$ "`.
 */
export function toastBackendError(e: unknown) {
  const raw = String(e);
  const nl = raw.lastIndexOf("\n$ ");
  if (nl !== -1) {
    toast.error(raw.slice(0, nl).trim(), raw.slice(nl + 3).trim());
  } else {
    toast.error(raw);
  }
}
