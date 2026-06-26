import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { getAppInfo, isTauri } from "@/lib/ipc";

/** The persistent app frame: sidebar + routed main area. */
export function AppShell() {
  // Smoke-test the typed IPC bridge (task 0.5) when running under Tauri.
  useEffect(() => {
    if (!isTauri()) return;
    getAppInfo()
      .then((info) => console.info("[ipc] app_info", info))
      .catch((err) => console.error("[ipc] app_info failed", err));
  }, []);

  return (
    <div className="flex h-full bg-rex-bg text-rex-text">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col bg-rex-bg">
        <Outlet />
      </main>
    </div>
  );
}
