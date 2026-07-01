import { create } from "zustand";

export type ToastKind = "error" | "success" | "info";

export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

interface ToastState {
  toasts: Toast[];
  push: (message: string, kind: ToastKind) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (message, kind) => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, message, kind }] }));
    const ttl = kind === "error" ? 7000 : 4000;
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), ttl);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/**
 * Imperative toast API — call from anywhere (mutation `onError`, etc.). Replaces
 * `window.alert`, which WKWebView (Tauri) doesn't reliably show.
 */
export const toast = {
  error: (message: string) => useToastStore.getState().push(message, "error"),
  success: (message: string) => useToastStore.getState().push(message, "success"),
  info: (message: string) => useToastStore.getState().push(message, "info"),
};
