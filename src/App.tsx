import { useEffect, useRef } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";
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
import {
  dnsStatus,
  initError,
  onAboutMenu,
  onServiceHealth,
  onTrayRoute,
  startupNotices,
  appUpdateState,
  onCheckUpdatesMenu,
} from "@/lib/ipc";
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

/** The macOS app menu's "About rexenv" routes to Settings → About — the app's
 *  own About screen, which (unlike the native panel) carries the commit, the
 *  build date and the bundled licences. Lives at the app root so the item works
 *  from any screen. */
function AboutMenuWatch() {
  const navigate = useNavigate();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onAboutMenu(() => navigate("/settings?section=about")).then((f) => {
      if (disposed) f();
      else unlisten = f;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);
  return null;
}

/** The tray menu's Services / Databases / Mail / Tunnels items. Same shape as
 *  `AboutMenuWatch` and for the same reason: mounted at the app root so a menu
 *  item works from whatever screen the window was left on — including a window
 *  that was hidden for hours, since closing it now hides rather than quits. */
function TrayRouteWatch() {
  const navigate = useNavigate();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onTrayRoute((path) => navigate(path))
      .then((f) => {
        if (disposed) f();
        else unlisten = f;
      })
      // Outside the app shell the event API rejects; an unhandled rejection
      // is a console error on every /dev route for a listener that has
      // nothing to listen to there.
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);
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

/** Says once, per app run, that a newer rexenv can be installed.
 *
 *  This is a menu-bar app: the window is usually closed, so the card alone can
 *  go unread for weeks. A toast is the quietest surface that still reaches
 *  someone — Sparkle's rule for dockless apps is not to steal focus, and an
 *  `info` toast with an action does not.
 *
 *  ONCE per version per run, tracked here rather than persisted: a version the
 *  user has seen and not acted on should not nag at every navigation, and a
 *  version they skipped is already withdrawn by the backend, so it never
 *  arrives. */
function UpdateWatch() {
  const navigate = useNavigate();
  const told = useRef<string | null>(null);
  const { data: st } = useQuery({
    queryKey: ["app-update"],
    queryFn: appUpdateState,
    // No refetch loop: the launch check and the 6 h poller are the backend's
    // cadence, and this reads whatever the card's own query already has.
    staleTime: Infinity,
  });
  const offered = st?.offered?.version ?? null;
  useEffect(() => {
    if (!offered || told.current === offered) return;
    told.current = offered;
    toast.info(`rexenv ${offered} can be installed`, {
      label: "Open About",
      onClick: () => navigate("/settings?section=about"),
    });
  }, [offered, navigate]);
  return null;
}

/** The app menu's "Check for Updates…": go to the card AND run a check.
 *
 *  Both, because someone who chose that item is ASKING — landing them on a card
 *  showing yesterday's answer would be a menu item that technically worked.
 *  Mounted at the app root so it fires from any screen. */
function CheckUpdatesMenuWatch() {
  const navigate = useNavigate();
  const qc = useQueryClient();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onCheckUpdatesMenu(() => {
      navigate("/settings?section=about");
      // Invalidate rather than run the check here: the card owns that mutation
      // and its failure copy, and a second caller would be a second place that
      // decides what a failed check looks like.
      void qc.invalidateQueries({ queryKey: ["app-update"] });
      void qc.invalidateQueries({ queryKey: ["app-update-readiness"] });
    }).then((f) => {
      if (disposed) f();
      else unlisten = f;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate, qc]);
  return null;
}

/** Drains the notices the launch sweeps queued before this window existed.
 *
 *  Runs ONCE per app run, deliberately: the sweeps happen in `setup()`, so an
 *  emitted event would have been emitted to nobody, and a share rexenv stopped
 *  on the user's behalf is the one thing they must not have to find in a log.
 *  Draining is what keeps a reload from re-toasting what they already read. */
function StartupNoticeHost() {
  const shown = useRef(false);
  const navigate = useNavigate();
  useEffect(() => {
    if (shown.current) return; // StrictMode double-mount must not drain twice
    shown.current = true;
    startupNotices()
      .then((list) => {
        for (const n of list) {
          // There is no "warning" toast kind, and `error` would be a lie — the
          // app did the right thing. A `warn` notice gets the ACTION form
          // instead: it names where to look and stays up 10s rather than 4,
          // which is the difference between telling someone and technically
          // having told them.
          if (n.level === "warn") {
            toast.info(n.message, { label: "Open Tunnels", onClick: () => navigate("/tunnels") });
          } else {
            toast.info(n.message);
          }
        }
      })
      .catch(() => {
        // A failed drain loses a courtesy, never a record: the same facts are
        // in rexenv.log, and a toast is not worth an error screen.
      });
    // `navigate` is stable for the app's lifetime and the ref guard makes a
    // re-run a no-op anyway; it is listed so the hooks rule stays a rule.
  }, [navigate]);
  return null;
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
      <StartupNoticeHost />
      <UpdateWatch />
      <CheckUpdatesMenuWatch />
      <AboutMenuWatch />
      <TrayRouteWatch />
    </>
  );
}
