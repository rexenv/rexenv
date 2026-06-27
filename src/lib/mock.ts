/**
 * Mock data for the static shell (Phase 1 task 0.6). Replaced by real IPC data
 * as backend tasks land. Kept in one place so it's easy to delete later.
 */
import type { GlobalStatus, ServiceInfo, Site } from "@/types";

export const mockSites: Site[] = [
  {
    id: "1",
    name: "Acme Store",
    domain: "acme.test",
    type: "wordpress",
    status: "running",
    phpVersion: "8.3",
    webServer: "nginx",
    ssl: true,
    path: "~/Sites/acme",
    createdAt: "2026-06-01 10:00:00",
  },
  {
    id: "2",
    name: "Portfolio",
    domain: "portfolio.test",
    type: "laravel",
    status: "running",
    phpVersion: "8.2",
    webServer: "nginx",
    ssl: true,
    path: "~/Sites/portfolio",
    createdAt: "2026-06-02 11:00:00",
  },
  {
    id: "3",
    name: "Blog Network",
    domain: "network.test",
    type: "wordpress",
    status: "stopped",
    phpVersion: "8.1",
    webServer: "apache",
    ssl: true,
    path: "~/Sites/network",
    createdAt: "2026-06-03 12:00:00",
  },
];

export const mockServices: ServiceInfo[] = [
  { name: "MySQL", running: true, pid: 1234, port: 13306, cpuPercent: 0.4, ramMb: 480 },
  { name: "PHP-FPM", running: true, pid: 1235, port: 9783, cpuPercent: 0.1, ramMb: 28 },
  { name: "Nginx", running: true, pid: 1236, port: 8088, cpuPercent: 0.0, ramMb: 7 },
  { name: "Caddy", running: false, pid: null, port: 443, cpuPercent: 0.0, ramMb: 0 },
];

export const mockGlobalStatus: GlobalStatus = {
  summary: "partial",
  running: 3,
  total: 12,
  cpuPercent: 14,
  ramMb: 612,
  ramTotalMb: 16384,
};
