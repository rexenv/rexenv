import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { App } from "./App";
import { initTheme } from "./lib/theme";
import { initWindowFocus } from "./lib/window-focus";
import "./styles/globals.css";

// Apply the saved theme before first paint (no flash); keeps "system" in sync.
initTheme();
// Focus = the NATIVE window's focus, not the webview's idea of it — what makes
// `refetchOnWindowFocus` mean "the user came back from wp-admin / a terminal".
initWindowFocus();

// WebView2 (Windows) offers its OWN context menu — Back / Refresh / Save as / Print /
// More tools — over the app's UI (seen on the first installed copy, 19 Sep 2026);
// WKWebView shows none. Suppress it everywhere except where a native menu is the
// feature: text fields, editable regions and the terminal (xterm's own copy/paste).
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (t?.closest("input, textarea, [contenteditable=''], [contenteditable='true'], .xterm")) return;
  e.preventDefault();
});

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </React.StrictMode>,
);
