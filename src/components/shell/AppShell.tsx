import { Outlet } from "react-router-dom";
import { SiteSwitcherHost } from "@/components/sites/SiteSwitcher";
import { Sidebar } from "./Sidebar";

/** The persistent app frame: sidebar + routed main area. */
export function AppShell() {
  return (
    <div className="flex h-full bg-rex-bg text-rex-text">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col bg-rex-bg">
        <Outlet />
      </main>
      {/* Mod+K on every route inside the shell — never on onboarding, which has
          no sites to switch between. */}
      <SiteSwitcherHost />
    </div>
  );
}
