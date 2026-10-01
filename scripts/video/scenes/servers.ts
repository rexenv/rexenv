/** Web servers and Redis. `set_site_web_server` switches a site in place (no
 *  docroot/cert/DB rebuild — `commands/sites.rs`); FrankenPHP embeds PHP 8.5.8
 *  (`core/binaries.rs`). Redis 8.8.0 is one shared server on 127.0.0.1:16379,
 *  started from Services (`start_database {key:"redis"}`), never by Start all. */
import type { SceneCtx } from "../demo-backend";
import type { DbStatus, ServiceInfo } from "@/types";
import { mockDatabases } from "@/lib/mock";

type Args = Record<string, unknown> | undefined;

export default function servers(ctx: SceneCtx) {
  const redis: ServiceInfo = { name: "Redis", running: false, pid: null, port: 16379, cpuPercent: 0, ramMb: 0, kind: "database", version: "8.8.0", serviceKey: "redis" };
  const dbs = (): DbStatus[] => [
    ...mockDatabases.map((d) => ({ ...d, running: true, pid: d.pid ?? 1402, ramMb: d.ramMb || 96 })),
    { key: "redis", label: "Redis", port: 16379, version: "8.8.0", running: redis.running, pid: redis.pid, cpuPercent: 0, ramMb: redis.ramMb },
  ];
  const svcs = () => [...ctx.services.map((s) => (s.kind === "database" && !s.serviceKey ? { ...s, serviceKey: s.name.toLowerCase() } : s)), redis];
  return {
    services_status: () => svcs(),
    list_services: () => svcs(),
    databases_status: () => dbs(),
    db_engine_versions: () => ({ mysql: ["8.4.6", "8.0.44"], postgres: ["18.6.0", "17.11.0", "16.15.0"], redis: ["8.8.0"] }),
    adminer_status: () => ({ staged: "6.1.1", effective: "6.1.1", pinned: "6.1.1", updatable: null }),
    adminer_update_check: () => ({ staged: "6.1.1", effective: "6.1.1", pinned: "6.1.1", updatable: null }),
    start_database: async (a: Args) => {
      await ctx.sleep(1200);
      if (a?.key === "redis") Object.assign(redis, { running: true, pid: 41377, ramMb: 9 });
      return null;
    },
    set_site_web_server: async (a: Args) => {
      await ctx.sleep(1500);
      const s = ctx.sites.find((x) => x.id === a?.id)!;
      s.webServer = a?.server as typeof s.webServer;
      return s;
    },
    frankenphp_embedded_php: () => "8.5.8",
  };
}
