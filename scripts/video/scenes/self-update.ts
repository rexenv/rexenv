/** In-app self-update. A (demo) 0.8.11 is offered; the consent and restart
 *  sentences are the Rust ones for macOS, not installed via Homebrew
 *  (`core/app_update.rs` + `platform/words.rs`). The download shows as item
 *  `rexenv-0.8.11` on `download-progress`; apply resolves with the restart
 *  notice, and OK calls `app_update_restart`. */
import type { SceneCtx } from "../demo-backend";
import type { AppUpdateState, DownloadItem } from "@/types";

const V = "0.8.11";
const SIZE = 31_400_000;
const CONSENT =
  `Downloads rexenv ${V} (31 MB), checks its signature and checksum, replaces rexenv.app in one step, then ASKS before it closes and reopens on ${V}. ` +
  "Your sites, databases and DNS keep running throughout — services outlive the app. Open terminals and running jobs close with it, exactly as they do when you quit. " +
  "macOS may ask again for permissions it had granted this copy: rexenv has no Apple developer signature yet, so each build is a new identity to it.";
const NOTICE =
  `rexenv will now close and open again on ${V}. Your sites, databases and DNS keep running throughout — services outlive the app. ` +
  "Open terminals and running jobs close with it. If you dismiss this, the new version starts the next time you open rexenv.";

export default function selfUpdate(ctx: SceneCtx) {
  const stamp = new Date(Date.now() - 20 * 60_000).toISOString().slice(0, 19).replace("T", " ");
  const state: AppUpdateState = {
    running: "0.8.10",
    enabled: true,
    autoCheck: true,
    offered: {
      version: V,
      url: `https://github.com/rexenv/homebrew-tap/releases/download/v${V}/rexenv_${V}_universal.app.tar.gz`,
      sha256: "4f1c9a7e2b0d8c6e5a3f1b9d7c2e0a8f6d4b2c0e8a6f4d2b0c8e6a4f2d0b8c6e",
      sizeBytes: SIZE,
      notes: "",
      publishedAt: "2026-09-30T00:00:00Z",
    },
    noOfferReason: null,
    checkedAt: stamp,
    skipped: null,
    installedPending: null,
    checkRefusal: null,
  };
  let seq = 1;
  const item: DownloadItem = { id: `rexenv-${V}`, name: "rexenv", version: V, label: `rexenv ${V}`, phase: "pending", downloadedBytes: 0, totalBytes: SIZE, bytesPerSec: null, error: null };
  const snap = (active: boolean) => ({ batch: active ? { action: `Update rexenv to ${V}`, done: 0, failed: 0, total: 1 } : null, items: [structuredClone(item)], seq });
  const publish = (active = true) => {
    seq += 1;
    void ctx.emit("download-progress", snap(active));
  };
  return {
    app_update_state: () => state,
    app_update_check: async () => {
      await ctx.sleep(900);
      return state;
    },
    app_update_readiness: () => ({ refusal: null, consent: CONSENT, homebrew: false }),
    downloads_state: () => (item.phase === "pending" ? { batch: null, items: [], seq } : snap(false)),
    app_update_apply: async () => {
      item.phase = "downloading";
      item.bytesPerSec = 9_000_000;
      publish();
      while (item.downloadedBytes < SIZE) {
        await ctx.sleep(200);
        item.downloadedBytes = Math.min(SIZE, item.downloadedBytes + 1_800_000);
        publish();
      }
      item.phase = "preparing";
      item.bytesPerSec = null;
      publish();
      await ctx.sleep(1400);
      item.phase = "done";
      publish(false);
      return { swapped: true, version: V, notice: NOTICE };
    },
    app_update_restart: () => null,
    app_update_skip: () => state,
  };
}
