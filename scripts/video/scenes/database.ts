/** Databases: the four engines on rexenv's own ports (MySQL 13306, MariaDB
 *  13307, PostgreSQL 15432, Redis 16379 — `core/db.rs`), their offered
 *  versions (`core/binaries.rs`), Adminer pinned at 6.1.1, and the Database
 *  Browser pointed at the stand-in page (see adminer/index.html). */
import type { SceneCtx } from "../demo-backend";
import type { DbStatus } from "@/types";
import { mockPlatformWords } from "@/lib/mock";
import { wpHandlers } from "../wp-fixtures";

export default function database(ctx: SceneCtx) {
  const dbs: DbStatus[] = [
    { key: "mysql", label: "MySQL", port: 13306, version: "8.4.6", running: true, pid: 1402, cpuPercent: 0.4, ramMb: 412 },
    { key: "mariadb", label: "MariaDB", port: 13307, version: "12.3.2", running: true, pid: 1418, cpuPercent: 0.1, ramMb: 118 },
    { key: "postgres", label: "PostgreSQL", port: 15432, version: "18.6.0", running: true, pid: 1433, cpuPercent: 0.1, ramMb: 64 },
    { key: "redis", label: "Redis", port: 16379, version: "8.8.0", running: true, pid: 1447, cpuPercent: 0.0, ramMb: 9 },
  ];
  return {
    ...wpHandlers(ctx),
    platform_words: () => ({ ...mockPlatformWords, dbBrowserOrigin: "/scripts/video/adminer" }),
    databases_status: () => dbs,
    db_engine_versions: () => ({
      mysql: ["8.4.6", "8.0.44"],
      mariadb: ["12.3.2", "11.4.12"],
      postgres: ["18.6.0", "17.11.0", "16.15.0"],
      redis: ["8.8.0"],
    }),
    set_db_engine_version: async (a: Record<string, unknown> | undefined) => {
      const d = dbs.find((x) => x.key === a?.key)!;
      d.running = false;
      await ctx.sleep(1800);
      Object.assign(d, { version: String(a?.version), running: true, pid: d.pid! + 40 });
      return null;
    },
    adminer_status: () => ({ staged: "6.1.1", effective: "6.1.1", pinned: "6.1.1", updatable: null }),
    adminer_update_check: () => ({ staged: "6.1.1", effective: "6.1.1", pinned: "6.1.1", updatable: null }),
  };
}
