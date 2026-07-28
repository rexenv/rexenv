import { useEffect } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle } from "lucide-react";
import { AppShell } from "@/components/shell/AppShell";
import { Sites } from "@/routes/Sites";
import { SiteDetail } from "@/routes/SiteDetail";
import { Services } from "@/routes/Services";
import { Databases } from "@/routes/Databases";
import { Mail } from "@/routes/Mail";
import { Tunnels } from "@/routes/Tunnels";
import { Import } from "@/routes/Import";
import { Settings } from "@/routes/Settings";
import { Onboarding } from "@/routes/Onboarding";
import { DevGitPanel } from "@/routes/DevGitPanel";
import { DevUiReview } from "@/routes/DevUiReview";
import { dnsStatus, initError, onServiceHealth } from "@/lib/ipc";
import { toast, toastBackendError } from "@/lib/toast";
import { Toaster } from "@/components/ui/toaster";
import { DialogHost } from "@/components/ui/dialog";

/** First launch: route to onboarding until system setup is complete for THIS
 *  user — the .rex resolver (system-wide) AND the local-CA trust (per-user
 *  login keychain). Checking only the resolver skipped the trust step for any
 *  second macOS account (the resolver file already existed), leaving HTTPS
 *  broken there. Reflects real state (dns_status), so it keeps prompting until
 *  both are done. */
/** Global listener for backend health-watchdog events: toast what happened
 *  (auto-restart / edge down / gave up) and refresh the status queries so the
 *  Services view + sidebar footer flip immediately, not on the next poll. */
function HealthWatch() {
  const qc = useQueryClient();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onServiceHealth((events) => {
      for (const e of events) {
        if (e.action === "restarted") {
          toast.info(`${e.service} stopped unexpectedly — restarted automatically`);
        } else if (
          e.action === "adopted" ||
          e.action === "edge-restarting" ||
          e.action === "edge-unblocked"
        ) {
          // edge-restarting: the KeepAlive daemon is already bringing the edge back —
          // informational, not a failure the user must act on.
          toast.info(`${e.service}: ${e.detail}`);
        } else {
          // Same "\n$ <cmd>" extraction as backend errors: a health event whose
          // detail ends with a fix-it command gets the copyable command block.
          toastBackendError(`${e.service}: ${e.detail}`);
        }
      }
      qc.invalidateQueries({ queryKey: ["services"] });
      qc.invalidateQueries({ queryKey: ["global-status"] });
      qc.invalidateQueries({ queryKey: ["databases"] });
    }).then((f) => {
      if (disposed) f();
      else unlisten = f;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [qc]);
  return null;
}

function FirstRunGate() {
  const { data, isLoading } = useQuery({ queryKey: ["dns-status"], queryFn: dnsStatus });
  if (isLoading) return null;
  const needsSetup = data && (!data.resolverInstalled || !data.caTrusted);
  return <Navigate to={needsSetup ? "/onboarding" : "/sites"} replace />;
}

/** Terminal screen when the backend failed to initialize (DB/CA) — task 1.2 / H3.
 *  Calls no AppState-backed commands, so it can't trip the "state not managed" panic. */
function FatalError({ message }: { message: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-rex-bg px-10 text-center">
      <div className="flex h-[56px] w-[56px] items-center justify-center rounded-2xl border border-status-error-border bg-status-error-bg">
        <AlertCircle className="h-7 w-7 text-status-error-bright" strokeWidth={1.8} />
      </div>
      <div>
        <div className="text-[1.1875rem] font-semibold text-rex-text">rexenv couldn't start</div>
        <div className="mt-2 max-w-[460px] whitespace-pre-wrap text-[0.8125rem] leading-[1.6] text-rex-text-muted">
          {message}
        </div>
      </div>
    </div>
  );
}

export function App() {
  // Gate the whole app on backend init (task 1.2 / H3): if the DB/CA failed to load,
  // AppState is absent, so show a terminal error screen and drive NO AppState commands
  // (which would otherwise panic). `init_error` is always managed, so this never panics.
  const { data: fatal, isLoading } = useQuery({ queryKey: ["init-error"], queryFn: initError });
  if (isLoading) return null;
  if (fatal) return <FatalError message={fatal} />;
  return (
    <>
      <Routes>
        <Route path="/" element={<FirstRunGate />} />
        <Route path="/onboarding" element={<Onboarding />} />
        {import.meta.env.DEV && <Route path="/dev/git-panel" element={<DevGitPanel />} />}
        {import.meta.env.DEV && <Route path="/dev/ui-review" element={<DevUiReview />} />}
        <Route element={<AppShell />}>
          <Route path="/sites" element={<Sites />} />
          <Route path="/sites/:id" element={<SiteDetail />} />
          <Route path="/sites/:id/:tab" element={<SiteDetail />} />
          <Route path="/services" element={<Services />} />
          <Route path="/databases" element={<Databases />} />
          <Route path="/mail" element={<Mail />} />
          <Route path="/tunnels" element={<Tunnels />} />
          <Route path="/import" element={<Import />} />
          <Route path="/settings" element={<Settings />} />
        </Route>
        <Route path="*" element={<Navigate to="/sites" replace />} />
      </Routes>
      {/* App-wide in-app dialogs + toasts (WKWebView lacks window.confirm/alert). */}
      <DialogHost />
      <Toaster />
      <HealthWatch />
    </>
  );
}
