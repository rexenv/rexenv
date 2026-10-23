import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  adminerUpdateApply,
  appUpdateApply,
  appUpdateReadiness,
  appUpdateRestart,
  appUpdateSetAutoCheck,
  appUpdateState,
  phpUpdateApply,
  phpmyadminUpdateApply,
  updatesCheckNow,
  updatesInventory,
  updatesSkip,
} from "@/lib/ipc";
import { Checkbox } from "@/components/ui/checkbox";
import { acknowledge } from "@/components/ui/dialog";
import { toast, toastBackendError } from "@/lib/toast";
import { agoLabel } from "@/routes/Sites";
import { APP_UPDATE_CHECK_MUTATION, CHECK_FLAG } from "@/lib/useCheckForUpdates";
import { fmtBytes, pctOf, Track } from "@/components/shell/DownloadPanel";
import { DB_CONSOLE_QUERY } from "@/lib/dbconsole";
import type { DownloadsSnapshot, UpdateKind, UpdateOffer } from "@/types";

/**
 * Settings → Updates: every kind of thing newer than what runs here — rexenv itself, a
 * PHP patch per installed minor, Adminer, phpMyAdmin — in ONE list of rows of ONE shape,
 * each with its own Install, and the switch and footer that govern the checks
 * (`docs/PLAN-unified-updates.md` T4). It replaced the About card that knew only about
 * the app, while PHP and the consoles offered themselves on their own screens in their
 * own words.
 *
 * # The three sentences this page refuses to blur
 *
 * "Never checked", "checked and there is nothing", and "the check failed" are different
 * facts, and only the middle one is good news. The footer says which, from backend values
 * written only after a successful check — so a failure keeps yesterday's timestamp instead
 * of ageing into a lie. It never says "up to date": that is unprovable before a check has
 * ever succeeded, and only ever true of the instant the check ran.
 *
 * # What the page is not allowed to author
 *
 * Every row's consent sentence and its "what pressing Install does" come from Rust
 * (`core::update_hub`, `core::app_update`), because a sentence describing a rule must live
 * beside the rule; `the_consent_sentence_has_one_source` fails the build if this file ever
 * spells the app's out. Refusals come from Rust too, with their copy-paste fix, and where one
 * exists the row renders it AS a command and shows no button.
 *
 * # Installs
 *
 * Each row installs through its family's own command — the same four the family screens
 * use — and reports the effect the backend MEASURED ("PHP 8.3 is now on 8.3.28" only when a
 * pool was restarted). The app's install swaps and then ASKS before restarting; only the
 * dialog's OK restarts (`appupdaterestart.js`). Progress for the app rides the download hub's
 * snapshot, read passively from the cache (`useDownloads` is mounted once, by the footer).
 */
export function UpdatesSection({ downloads: downloadsOverride }: { downloads?: DownloadsSnapshot } = {}) {
  const qc = useQueryClient();
  const { data: inv } = useQuery({ queryKey: ["updates"], queryFn: updatesInventory, staleTime: Infinity });
  const { data: st } = useQuery({ queryKey: ["app-update"], queryFn: appUpdateState });
  const { data: ready } = useQuery({
    queryKey: ["app-update-readiness"],
    queryFn: appUpdateReadiness,
    staleTime: 60_000,
  });
  // The download hub's snapshot, read PASSIVELY from the cache the footer's single
  // `useDownloads()` mount keeps fresh (mounting a second one is what that hook's doc
  // forbids). `enabled: false` never runs the fn; it exists so TanStack does not log a
  // "no queryFn" error on every render — which the About card did, unseen, for a month.
  const cached = useQuery<DownloadsSnapshot>({
    queryKey: ["downloads"],
    queryFn: () => qc.getQueryData<DownloadsSnapshot>(["downloads"]) ?? { batch: null, items: [], seq: 0 },
    enabled: false,
  }).data;
  const downloads = downloadsOverride ?? cached ?? { batch: null, items: [], seq: 0 };
  const [failed, setFailed] = useState(false);

  const refreshAll = () => {
    void qc.invalidateQueries({ queryKey: ["updates"] });
    void qc.invalidateQueries({ queryKey: ["app-update"] });
    void qc.invalidateQueries({ queryKey: ["app-update-readiness"] });
    void qc.invalidateQueries({ queryKey: ["php-versions"] });
    void qc.invalidateQueries({ queryKey: ["adminer-status"] });
    void qc.invalidateQueries({ queryKey: DB_CONSOLE_QUERY });
  };

  const check = useMutation({
    // Named so the TopBar pill can SHOW this check in flight without running one.
    mutationKey: APP_UPDATE_CHECK_MUTATION,
    mutationFn: updatesCheckNow,
    onMutate: () => setFailed(false),
    onSuccess: (fresh) => {
      qc.setQueryData(["updates"], fresh);
      void qc.invalidateQueries({ queryKey: ["app-update"] });
      void qc.invalidateQueries({ queryKey: ["app-update-readiness"] });
      void qc.invalidateQueries({ queryKey: ["php-versions"] });
      void qc.invalidateQueries({ queryKey: ["adminer-status"] });
      void qc.invalidateQueries({ queryKey: DB_CONSOLE_QUERY });
    },
    onError: (e) => {
      // A failed check writes nothing: the footer says the check failed and the
      // timestamps keep pointing at the last one that did not.
      setFailed(true);
      toastBackendError(e);
    },
  });

  // "Check for Updates…" (app menu, tray, TopBar) arrives as `?check=1`: run THIS page's
  // check once — the same mutation, copy and deadline as "Check now" — then drop the flag
  // so a reload or a back-navigation does not check again. The ref absorbs StrictMode's
  // double effect; a later arrival with the flag is a new request and runs again.
  const [params, setParams] = useSearchParams();
  const asked = params.get(CHECK_FLAG) === "1";
  const answered = useRef(false);
  const enabled = st?.enabled;
  const { mutate: runCheck, isPending: checking } = check;
  useEffect(() => {
    if (!asked) {
      answered.current = false;
      return;
    }
    if (enabled === undefined || answered.current) return;
    answered.current = true;
    if (enabled && !checking) runCheck();
    setParams(
      (p) => {
        const next = new URLSearchParams(p);
        next.delete(CHECK_FLAG);
        return next;
      },
      { replace: true },
    );
  }, [asked, enabled, checking, runCheck, setParams]);

  const autoCheck = useMutation({
    mutationFn: appUpdateSetAutoCheck,
    onSuccess: (fresh) => {
      qc.setQueryData(["app-update"], fresh);
      void qc.invalidateQueries({ queryKey: ["updates"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const skip = useMutation({
    mutationFn: ({ kind, version }: { kind: UpdateKind; version: string | null }) => updatesSkip(kind, version),
    onSuccess: (fresh) => {
      qc.setQueryData(["updates"], fresh);
      void qc.invalidateQueries({ queryKey: ["app-update"] });
    },
    onError: (e) => toastBackendError(e),
  });

  // The app's install does NOT take the window away: it swaps, and then the app says what
  // is about to happen and waits to be told go (owner ruling, 20 Sep 2026). Dismissing the
  // dialog is allowed and the sentence says what that means, because the swapped build is
  // already on disk either way.
  const applyApp = useMutation({
    mutationFn: appUpdateApply,
    onError: (e) => toastBackendError(e),
    onSuccess: async (outcome) => {
      refreshAll();
      const go = await acknowledge({
        title: `rexenv ${outcome.version} is installed`,
        // Rust's sentence (`core::app_update::restart_sentence`): it describes what a
        // restart does, which is a rule, and a copy here is a copy that drifts.
        message: outcome.notice,
        confirmLabel: "OK",
      });
      if (go) await appUpdateRestart().catch(toastBackendError);
    },
  });

  // The other kinds install through their family's own command and say the MEASURED
  // effect — "recorded" and "now running" are different facts (DESIGN).
  const [applying, setApplying] = useState<ReadonlySet<string>>(new Set());
  const rowKey = (o: UpdateOffer) => JSON.stringify(o.kind);
  const applyOther = useMutation({
    mutationFn: async (o: UpdateOffer) => {
      switch (o.kind.kind) {
        case "php": {
          const out = await phpUpdateApply(o.kind.minor, o.offered);
          return out.restarted
            ? `PHP ${o.kind.minor} is now on ${out.patch}`
            : `PHP ${o.kind.minor} will use ${out.patch} — nothing was running to restart`;
        }
        case "adminer": {
          const fresh = await adminerUpdateApply(o.offered);
          qc.setQueryData(["adminer-status"], fresh);
          return `Adminer is now on ${fresh.staged ?? fresh.effective}`;
        }
        case "phpMyAdmin": {
          const fresh = await phpmyadminUpdateApply(o.offered);
          qc.setQueryData(DB_CONSOLE_QUERY, fresh);
          return `phpMyAdmin is now on ${fresh.phpmyadminStaged ?? fresh.phpmyadminEffective}`;
        }
        case "app":
          throw new Error("the app installs through applyApp");
      }
    },
    onMutate: (o) => setApplying((s) => new Set(s).add(rowKey(o))),
    onSuccess: (sentence) => {
      toast.success(sentence);
      refreshAll();
    },
    onError: (e) => toastBackendError(e),
    onSettled: (_d, _e, o) =>
      setApplying((s) => {
        const next = new Set(s);
        next.delete(rowKey(o));
        return next;
      }),
  });

  if (!st?.enabled) return null;

  const offers = inv?.offers ?? [];
  const pending = offers.filter((o) => !o.skipped);
  const appOffer = offers.find((o) => o.kind.kind === "app") ?? null;
  const appItem = appOffer ? downloads.items.find((i) => i.id === `rexenv-${appOffer.offered}`) : undefined;
  const appInstalling =
    applyApp.isPending || appItem?.phase === "downloading" || appItem?.phase === "preparing";
  const restartPending = inv?.restartPending ?? st.installedPending;
  // The APP's phase, named as the About card named it — the harness and the WebKit checks
  // read it off `data-phase`, and the states that must be right (nothing offered, a refusal,
  // a failed check) are the ones nobody looks at (ledger #771 for `installed` vs `pending`).
  const appPhase = appInstalling
    ? "installing"
    : restartPending
      ? st.restartDeclined
        ? "installed"
        : "pending"
      : appOffer?.skipped
        ? "skipped"
        : appOffer && ready?.refusal
          ? "refused"
          : appOffer
            ? "offered"
            : "none";

  const checkedAt = inv?.checkedAt ?? st.checkedAt;
  // Three different facts, three sentences: the server could not be reached; the server
  // answered and rexenv REFUSED what it published (Rust's sentence, as sent — a refusal used
  // to read as "couldn't reach", and a machine stuck on a replayed serial showed that line
  // for a week); or when the last accepted check ran.
  const footer = failed
    ? "Couldn't reach the update server just now, so nothing here says whether anything newer exists."
    : inv?.checkProblem
      ? inv.checkProblem
      : st.checkRefusal
        ? st.checkRefusal
        : checkedAt
          ? `Release lists from rexenv, checked ${agoLabel(checkedAt)}.`
          : "Not checked yet, so nothing here says whether anything newer exists.";

  const headline = restartPending ? (
    <>
      rexenv <span className="font-mono text-rex-text-bright">{restartPending}</span> is installed —
      it takes effect the next time rexenv opens.
    </>
  ) : pending.length > 1 ? (
    <>
      <span className="font-mono text-rex-text-bright">{pending.length}</span> updates are ready to install.
    </>
  ) : pending.length === 1 ? (
    <>
      <span className="font-mono text-rex-text-bright">{pending[0].display}</span>
      {pending[0].sizeBytes != null && (
        <>
          {" · "}
          <span className="font-mono">{fmtBytes(pending[0].sizeBytes)}</span>
        </>
      )}{" "}
      has been published.
    </>
  ) : (
    <>
      rexenv <span className="font-mono text-rex-text-bright">{st.running}</span> is what you are
      running, and nothing newer is offered.
    </>
  );

  return (
    <div
      className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5"
      data-probe="app-update"
      data-running={st.running}
      data-offered={appOffer && !appOffer.skipped ? appOffer.offered : ""}
      data-checked-at={checkedAt ?? ""}
      data-skipped={st.skipped ?? ""}
      data-phase={appPhase}
      data-pending={pending.length}
      data-auto-check={String(st.autoCheck)}
      data-installed-pending={restartPending ?? ""}
      data-restart-declined={String(st.restartDeclined)}
      data-check-refusal={st.checkRefusal ?? ""}
    >
      <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px] last:border-b-0">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Updates</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">{headline}</div>
        </div>
        <button
          onClick={() => check.mutate()}
          disabled={check.isPending || appInstalling}
          className="flex h-8 flex-none items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover disabled:opacity-60"
        >
          {check.isPending && (
            <span className="h-3 w-3 rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
          )}
          {check.isPending ? "Checking…" : "Check now"}
        </button>
      </div>

      {/* One row per offer, ONE shape, in Update-all order (the app last). */}
      {offers.map((o) => {
        const isApp = o.kind.kind === "app";
        const busy = isApp ? appInstalling : applying.has(rowKey(o));
        const refusal = isApp && !o.skipped ? ready?.refusal ?? null : null;
        return (
          <div
            key={rowKey(o)}
            data-probe="update-row"
            data-kind={o.kind.kind}
            data-offered={o.offered}
            data-skipped={String(o.skipped)}
            className="border-t border-rex-border-subtle py-[15px]"
          >
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="text-[0.84375rem] font-medium text-rex-text">{o.label}</span>
              <span className="font-mono text-[0.71875rem] text-rex-text-muted">
                {o.installed} → <span className="text-rex-text-bright">{o.offered}</span>
                {o.sizeBytes != null && ` · ${fmtBytes(o.sizeBytes)}`}
              </span>
            </div>

            {/* Skipped: the version is remembered, not a flag — so a LATER release still
                offers itself, and this row says which one was set aside. */}
            {o.skipped ? (
              <div className="mt-2 flex items-center gap-3 text-[0.75rem] text-rex-text-muted">
                <span>
                  <span className="font-mono">{o.display}</span> was set aside. A newer{" "}
                  {isApp ? "release" : "version"} will still offer itself.
                </span>
                <button
                  onClick={() => skip.mutate({ kind: o.kind, version: null })}
                  className="text-rex-text-bright underline-offset-2 hover:underline"
                >
                  Undo
                </button>
              </div>
            ) : refusal ? (
              /* A refusal is rendered where the button would have been, with its fix as a
                 command: a fix that IS a command is the command. */
              <div className="mt-2 text-[0.75rem] leading-[1.55] text-rex-text-muted">
                {refusal.split("\n$ ")[0]}
                {refusal.includes("\n$ ") && (
                  <pre className="mt-2 overflow-x-auto rounded-[9px] bg-rex-well px-3 py-2 font-mono text-[0.71875rem] text-rex-text-bright">
                    {refusal.split("\n$ ")[1]}
                  </pre>
                )}
              </div>
            ) : isApp && appInstalling ? (
              /* Downloading: the hub's own numbers, and the bar FREEZES where the work
                 stopped rather than snapping to 100 or resetting. */
              <div className="mt-2">
                <div className="mb-2 flex items-baseline justify-between gap-3">
                  <span className="text-[0.75rem] text-rex-text-muted">
                    {appItem?.phase === "preparing" ? "Installing…" : "Downloading…"}
                  </span>
                  <span className="font-mono text-[0.71875rem] text-rex-text-muted">
                    {appItem && appItem.totalBytes != null
                      ? `${fmtBytes(appItem.downloadedBytes)} / ${fmtBytes(appItem.totalBytes)}`
                      : appItem
                        ? fmtBytes(appItem.downloadedBytes)
                        : ""}
                  </span>
                </div>
                <Track pct={appItem ? pctOf(appItem) : null} state={appItem?.phase === "failed" ? "error" : "run"} />
              </div>
            ) : isApp && restartPending ? null : (
              <>
                {/* The consent sentence sits IN FRONT of the button that starts the thing
                    it describes. Served from Rust — see the file header. */}
                {o.consent && (
                  <div className="mt-2 text-[0.75rem] leading-[1.55] text-rex-text-muted">{o.consent}</div>
                )}
                <div className="mt-3 flex items-center gap-3">
                  <button
                    onClick={() => (isApp ? applyApp.mutate() : applyOther.mutate(o))}
                    disabled={busy}
                    title={o.effect}
                    className="flex h-8 items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-brand-light transition-colors hover:bg-rex-surface-2-hover disabled:opacity-60"
                  >
                    {busy && (
                      <span className="h-3 w-3 rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
                    )}
                    {busy ? `Installing ${o.display}…` : `Install ${o.display}`}
                  </button>
                  <button
                    onClick={() => skip.mutate({ kind: o.kind, version: o.offered })}
                    disabled={busy}
                    className="text-[0.75rem] text-rex-text-muted underline-offset-2 hover:underline disabled:opacity-60"
                  >
                    Skip this version
                  </button>
                </div>
              </>
            )}
          </div>
        );
      })}

      {/* Installed, quit cancelled. The swap already happened, so there is no button:
          pressing one would re-download bytes that are on disk. The ONLY state whose fix
          is "quit and open it again". Read from a stored row, so closing the window does
          not lose it. */}
      {appPhase === "installed" && (
        <div className="border-t border-rex-border-subtle py-[15px] text-[0.75rem] leading-[1.55] text-rex-text-muted">
          You chose to keep rexenv running, so the new version is waiting rather than lost.
          Quit rexenv and open it again when you are ready — nothing needs downloading a second
          time, and your sites, databases and DNS keep running either way.
        </div>
      )}

      {/* The switch that stops the REQUESTS — both signed documents — not just this page:
          the setting is read in Rust before any network call. It sits beside the footer
          that reports the last check, because "when did it last look" and "may it look"
          are the same question asked twice. */}
      <div className="flex items-start gap-[10px] border-t border-rex-border-subtle py-[13px] text-[0.75rem] leading-[1.5] text-rex-text-muted">
        <span className="mt-[1px]">
          <Checkbox id="app-update-auto" checked={st.autoCheck} onCheckedChange={(v) => autoCheck.mutate(v)} />
        </span>
        <label htmlFor="app-update-auto" className="flex-1 cursor-pointer">
          Check for new releases automatically — rexenv, PHP, Adminer and phpMyAdmin alike.{" "}
          {st.autoCheck
            ? "rexenv asks GitHub every few hours and on launch. Nothing installs without you."
            : "Turned off — rexenv contacts nothing on its own. “Check now” still works."}
        </label>
      </div>

      <div className="border-t border-rex-border-subtle py-[13px] text-[0.75rem] leading-[1.5] text-rex-text-muted">
        {footer}
      </div>
    </div>
  );
}
