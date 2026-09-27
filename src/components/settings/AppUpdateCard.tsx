import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  appUpdateApply,
  appUpdateRestart,
  appUpdateCheck,
  appUpdateReadiness,
  appUpdateSetAutoCheck,
  appUpdateSkip,
  appUpdateState,
} from "@/lib/ipc";
import { Checkbox } from "@/components/ui/checkbox";
import { acknowledge } from "@/components/ui/dialog";
import { toastBackendError } from "@/lib/toast";
import { agoLabel } from "@/routes/Sites";
import { fmtBytes, pctOf, Track } from "@/components/shell/DownloadPanel";
import type { DownloadsSnapshot } from "@/types";

/**
 * Settings → About: what rexenv knows about its own updates, and the one button
 * that acts on it.
 *
 * # The three sentences this card refuses to blur
 *
 * "Never checked", "checked and there is nothing", and "the check failed" are
 * different facts, and only the middle one is good news. The footer says which,
 * from ONE backend value written only after a successful check — so a failure
 * keeps yesterday's timestamp instead of ageing into a lie. It never says "up to
 * date": that is unprovable before a check has ever succeeded, and only ever
 * true of the instant the check ran.
 *
 * # What the card is not allowed to author
 *
 * The consent sentence comes from Rust (`app_update_readiness`), because a
 * sentence describing a rule must live beside the rule; a copy here is a copy
 * that drifts, and `the_consent_sentence_has_one_source` fails the build if this
 * file ever spells it out. Refusals come from Rust too, with their copy-paste
 * fix, and where one exists the card renders it AS a command and shows no
 * button — pressing it could not work, and offering it would be a lie.
 *
 * # Progress
 *
 * Read passively from the download hub's cache. The apply reports into the same
 * hub every pinned binary uses, so the footer indicator and the download panel
 * already show it; this card reads that snapshot rather than mounting a second
 * `useDownloads`, which the hook's own doc forbids.
 */
export function AppUpdateCard({ downloads }: { downloads: DownloadsSnapshot }) {
  const qc = useQueryClient();
  const { data: st } = useQuery({ queryKey: ["app-update"], queryFn: appUpdateState });
  const { data: ready } = useQuery({
    queryKey: ["app-update-readiness"],
    queryFn: appUpdateReadiness,
    staleTime: 60_000,
  });
  const [failed, setFailed] = useState(false);

  const check = useMutation({
    mutationFn: appUpdateCheck,
    onMutate: () => setFailed(false),
    onSuccess: (fresh) => {
      qc.setQueryData(["app-update"], fresh);
      void qc.invalidateQueries({ queryKey: ["app-update-readiness"] });
    },
    onError: (e) => {
      // A failed check writes nothing: the footer says the check failed and the
      // timestamp keeps pointing at the last one that did not.
      setFailed(true);
      toastBackendError(e);
    },
  });

  const autoCheck = useMutation({
    mutationFn: appUpdateSetAutoCheck,
    onSuccess: (fresh) => qc.setQueryData(["app-update"], fresh),
    onError: (e) => toastBackendError(e),
  });

  const skip = useMutation({
    mutationFn: appUpdateSkip,
    onSuccess: (fresh) => qc.setQueryData(["app-update"], fresh),
    onError: (e) => toastBackendError(e),
  });

  // The install does NOT take the window away any more: it swaps, and then the
  // app says what is about to happen and waits to be told go (owner ruling,
  // 20 Sep 2026 — an app that restarts itself mid-sentence is a surprise, not a
  // feature). Dismissing the dialog is allowed and the sentence says what that
  // means, because the swapped build is already on disk either way.
  const apply = useMutation({
    mutationFn: appUpdateApply,
    onError: (e) => toastBackendError(e),
    onSuccess: async (outcome) => {
      const go = await acknowledge({
        title: `rexenv ${outcome.version} is installed`,
        // The sentence is Rust's (`core::app_update::restart_sentence`): it
        // describes what a restart does, which is a rule, and a copy here is a
        // copy that drifts (the guard in copy_scan enforces it).
        message: outcome.notice,
        confirmLabel: "OK",
      });
      if (go) await appUpdateRestart().catch(toastBackendError);
    },
  });

  if (!st?.enabled) return null;

  const offered = st.offered;
  const item = offered
    ? downloads.items.find((i) => i.id === `rexenv-${offered.version}`)
    : undefined;
  const installing = apply.isPending || item?.phase === "downloading" || item?.phase === "preparing";
  const phase = installing
    ? "installing"
    : st.installedPending
      ? "installed"
      : st.skipped && st.skipped === offered?.version
        ? "skipped"
        : ready?.refusal
          ? "refused"
          : offered
            ? "offered"
            : "none";

  // Three different facts, three sentences: the server could not be reached; the
  // server answered and rexenv REFUSED what it published (Rust's sentence, as
  // sent — a refusal used to read as "couldn't reach", and a machine stuck on a
  // replayed serial showed that line for a week); or when the last accepted
  // check ran.
  const footer = failed
    ? "Couldn't reach the update server just now, so nothing here says whether a newer rexenv exists."
    : st.checkRefusal
      ? st.checkRefusal
      : st.checkedAt
      ? `Release list from rexenv, checked ${agoLabel(st.checkedAt)}.`
      : "Not checked yet, so nothing here says whether a newer rexenv exists.";

  return (
    <div
      className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5"
      data-probe="app-update"
      data-running={st.running}
      data-offered={offered?.version ?? ""}
      data-checked-at={st.checkedAt ?? ""}
      data-skipped={st.skipped ?? ""}
      data-phase={phase}
      data-auto-check={String(st.autoCheck)}
      data-installed-pending={st.installedPending ?? ""}
      data-check-refusal={st.checkRefusal ?? ""}
    >
      <div className="flex items-center gap-[14px] border-b border-rex-border-subtle py-[15px] last:border-b-0">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Updates</div>
          <div className="mt-0.5 text-[0.75rem] text-rex-text-muted">
            {offered ? (
              <>
                rexenv <span className="font-mono text-rex-text-bright">{offered.version}</span>
                {" · "}
                <span className="font-mono">{fmtBytes(offered.sizeBytes)}</span> has been
                published.
              </>
            ) : st.installedPending ? (
              <>
                rexenv{" "}
                <span className="font-mono text-rex-text-bright">{st.installedPending}</span> is
                installed — it takes effect the next time rexenv opens.
              </>
            ) : (
              <>
                rexenv <span className="font-mono text-rex-text-bright">{st.running}</span> is what
                you are running.
              </>
            )}
          </div>
        </div>
        <button
          onClick={() => check.mutate()}
          disabled={check.isPending || installing}
          className="flex h-8 flex-none items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-surface-2-hover disabled:opacity-60"
        >
          {check.isPending && (
            <span className="h-3 w-3 rounded-full border-2 border-brand/30 border-t-brand animate-rex-spin motion-reduce:animate-none" />
          )}
          {check.isPending ? "Checking…" : "Check now"}
        </button>
      </div>

      {/* Downloading: the hub's own numbers, and the bar FREEZES where the work
          stopped rather than snapping to 100 or resetting. */}
      {offered && installing && (
        <div className="border-t border-rex-border-subtle py-[15px]">
          <div className="mb-2 flex items-baseline justify-between gap-3">
            <span className="text-[0.75rem] text-rex-text-muted">
              {item?.phase === "preparing" ? "Installing…" : "Downloading…"}
            </span>
            <span className="font-mono text-[0.71875rem] text-rex-text-muted">
              {item && item.totalBytes != null
                ? `${fmtBytes(item.downloadedBytes)} / ${fmtBytes(item.totalBytes)}`
                : item
                  ? fmtBytes(item.downloadedBytes)
                  : ""}
            </span>
          </div>
          <Track
            pct={item ? pctOf(item) : null}
            state={item?.phase === "failed" ? "error" : "run"}
          />
        </div>
      )}

      {/* The consent sentence sits IN FRONT of the button that starts the thing
          it describes. Served from Rust — see the file header. */}
      {offered && !installing && phase === "offered" && ready && (
        <div className="border-t border-rex-border-subtle py-[15px]">
          <div className="text-[0.75rem] leading-[1.55] text-rex-text-muted">{ready.consent}</div>
          <div className="mt-3 flex items-center gap-3">
            <button
              onClick={() => apply.mutate()}
              className="flex h-8 items-center gap-[7px] rounded-[9px] border border-rex-border-strong bg-rex-surface-2 px-[13px] text-[0.78125rem] font-medium text-brand-light transition-colors hover:bg-rex-surface-2-hover"
            >
              Install rexenv {offered.version}
            </button>
            <button
              onClick={() => skip.mutate(offered.version)}
              className="text-[0.75rem] text-rex-text-muted underline-offset-2 hover:underline"
            >
              Skip this version
            </button>
          </div>
        </div>
      )}

      {/* Installed, quit cancelled. The swap already happened, so there is no
          button: pressing one would re-download bytes that are on disk. The card
          says what is true and what closes it — the ONLY state whose fix is
          "quit and open it again". Read from a stored row, so closing the window
          does not lose it. */}
      {phase === "installed" && (
        <div className="border-t border-rex-border-subtle py-[15px] text-[0.75rem] leading-[1.55] text-rex-text-muted">
          You chose to keep rexenv running, so the new version is waiting rather than lost.
          Quit rexenv and open it again when you are ready — nothing needs downloading a second
          time, and your sites, databases and DNS keep running either way.
        </div>
      )}

      {/* Skipped: the version is remembered, not a flag — so a LATER release
          still offers itself, and this row says which one was set aside. */}
      {phase === "skipped" && (
        <div className="flex items-center gap-3 border-t border-rex-border-subtle py-[15px] text-[0.75rem] text-rex-text-muted">
          <span>
            rexenv <span className="font-mono">{st.skipped}</span> was set aside. A newer release
            will still offer itself.
          </span>
          <button
            onClick={() => skip.mutate(undefined)}
            className="text-rex-text-bright underline-offset-2 hover:underline"
          >
            Undo
          </button>
        </div>
      )}

      {/* A refusal is rendered where the button would have been, with its fix as
          a command: a fix that IS a command is the command. */}
      {phase === "refused" && ready?.refusal && (
        <div className="border-t border-rex-border-subtle py-[15px] text-[0.75rem] leading-[1.55] text-rex-text-muted">
          {ready.refusal.split("\n$ ")[0]}
          {ready.refusal.includes("\n$ ") && (
            <pre className="mt-2 overflow-x-auto rounded-[9px] bg-rex-well px-3 py-2 font-mono text-[0.71875rem] text-rex-text-bright">
              {ready.refusal.split("\n$ ")[1]}
            </pre>
          )}
        </div>
      )}

      {/* The switch that stops the REQUESTS, not just this card — the setting is
          read in Rust before any network call. It sits beside the footer that
          reports the last check, because "when did it last look" and "may it
          look" are the same question asked twice. */}
      <div className="flex items-start gap-[10px] border-t border-rex-border-subtle py-[13px] text-[0.75rem] leading-[1.5] text-rex-text-muted">
        <span className="mt-[1px]">
          <Checkbox
            id="app-update-auto"
            checked={st.autoCheck}
            onCheckedChange={(v) => autoCheck.mutate(v)}
          />
        </span>
        <label htmlFor="app-update-auto" className="flex-1 cursor-pointer">
          Check for new releases automatically.{" "}
          {st.autoCheck
            ? "rexenv asks GitHub every few hours and on launch. Nothing installs without you."
            : "Turned off — rexenv contacts nothing on its own. \u201cCheck now\u201d still works."}
        </label>
      </div>

      <div className="border-t border-rex-border-subtle py-[13px] text-[0.75rem] leading-[1.5] text-rex-text-muted">
        {footer}
      </div>
    </div>
  );
}
