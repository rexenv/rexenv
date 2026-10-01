/** First run on a fresh Mac: no sites, the resolver and CA not set up yet,
 *  and the six core components downloading while the user clicks through.
 *  The plan rows are macOS's real first-run plan (`core/downloads.rs`, pins in
 *  `core/binaries.rs`); the batch action is "First-run setup"
 *  (`commands/downloads.rs`). */
import type { SceneCtx } from "../demo-backend";
import type { DownloadItem, DownloadsSnapshot } from "@/types";

const PLAN: Array<{ name: string; version: string; label: string; mb: number }> = [
  { name: "caddy", version: "2.11.4", label: "Caddy (edge router)", mb: 17.4 },
  { name: "nginx", version: "1.30.4", label: "Nginx (web server)", mb: 4.1 },
  { name: "mysql", version: "8.4.6", label: "MySQL 8.4", mb: 118 },
  { name: "mailpit", version: "1.30.3", label: "Mailpit (mail catcher)", mb: 12.8 },
  { name: "adminer", version: "6.1.1", label: "Adminer (DB browser)", mb: 0.5 },
  { name: "php-fpm", version: "8.3.32", label: "PHP 8.3 (FPM)", mb: 21.6 },
];

export default function onboarding(ctx: SceneCtx) {
  let setUp = false;
  let seq = 1;
  const items: DownloadItem[] = PLAN.map((p) => ({
    id: `${p.name}-${p.version}`,
    name: p.name,
    version: p.version,
    label: p.label,
    phase: "pending",
    downloadedBytes: 0,
    totalBytes: Math.round(p.mb * 1024 * 1024),
    bytesPerSec: null,
    error: null,
  }));
  const snapshot = (): DownloadsSnapshot => {
    const done = items.filter((i) => i.phase === "done").length;
    const active = done < items.length;
    return {
      batch: active ? { action: "First-run setup", done, failed: 0, total: items.length } : null,
      items: structuredClone(items),
      seq: seq,
    };
  };
  const publish = () => {
    seq += 1;
    void ctx.emit("download-progress", snapshot());
  };

  let running: Promise<void> | null = null;
  // Two at a time, the big MySQL archive included, ~12 s end to end — the
  // Hub's real order: the plan's order, preparing after each download.
  const download = async () => {
    const queue = [...items];
    const worker = async () => {
      for (let it = queue.shift(); it; it = queue.shift()) {
        const total = it.totalBytes ?? 0;
        const rate = Math.max(total / (it.name === "mysql" ? 7.5 : 2.2), 2 * 1024 * 1024);
        it.phase = "downloading";
        it.bytesPerSec = rate;
        publish();
        while (it.downloadedBytes < total) {
          await ctx.sleep(200);
          it.downloadedBytes = Math.min(total, it.downloadedBytes + rate * 0.2);
          publish();
        }
        it.phase = "preparing";
        it.bytesPerSec = null;
        publish();
        await ctx.sleep(500);
        it.phase = "done";
        publish();
      }
    };
    await Promise.all([worker(), worker()]);
  };

  return {
    list_sites: () => [],
    sites_resources: () => [],
    dns_status: () => ({
      running: true,
      mode: "agent",
      port: 15353,
      resolverInstalled: setUp,
      resolverPath: "/etc/resolver/rex",
      caTrusted: setUp,
    }),
    core_binaries_plan: () =>
      PLAN.map((p) => ({ id: `${p.name}-${p.version}`, name: p.name, version: p.version, label: p.label, cached: false })),
    prefetch_core_binaries: () => (running ??= download()),
    downloads_state: () => snapshot(),
    // The two macOS prompts (admin password for /etc/resolver/rex, then the
    // login keychain) happen inside this call; the video narrates them.
    system_setup: async () => {
      await ctx.sleep(3200);
      setUp = true;
      return null;
    },
  };
}
