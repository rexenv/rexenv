import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "@/components/shell/AppShell";
import { Sites } from "@/routes/Sites";
import { SiteDetail } from "@/routes/SiteDetail";
import { Services } from "@/routes/Services";
import { Databases } from "@/routes/Databases";
import { Mail } from "@/routes/Mail";
import { Tunnels } from "@/routes/Tunnels";
import { Settings } from "@/routes/Settings";
import { Onboarding } from "@/routes/Onboarding";

export function App() {
  return (
    <Routes>
      <Route path="/onboarding" element={<Onboarding />} />
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/sites" replace />} />
        <Route path="/sites" element={<Sites />} />
        <Route path="/sites/:id" element={<SiteDetail />} />
        <Route path="/sites/:id/:tab" element={<SiteDetail />} />
        <Route path="/services" element={<Services />} />
        <Route path="/databases" element={<Databases />} />
        <Route path="/mail" element={<Mail />} />
        <Route path="/tunnels" element={<Tunnels />} />
        <Route path="/settings" element={<Settings />} />
      </Route>
      <Route path="*" element={<Navigate to="/sites" replace />} />
    </Routes>
  );
}
