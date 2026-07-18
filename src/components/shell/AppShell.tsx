import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";

/** The persistent app frame: sidebar + routed main area. */
export function AppShell() {
  return (
    <div className="flex h-full bg-rex-bg text-rex-text">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col bg-rex-bg">
        <Outlet />
      </main>
    </div>
  );
}
