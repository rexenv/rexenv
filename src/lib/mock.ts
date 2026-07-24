/**
 * Mock data for the static shell (Phase 1 task 0.6). Replaced by real IPC data
 * as backend tasks land. Kept in one place so it's easy to delete later.
 */
import type { AppInfo, DbStatus, GlobalStatus, MailDetail, MailList, PhpSetting, PhpVersion, ServiceInfo, Site, SiteServing } from "@/types";

export const mockAppInfo: AppInfo = {
  name: "rexenv",
  version: "0.1.0",
  tauriVersion: "2",
  platform: "macOS · Apple silicon",
};

export const mockSites: Site[] = [
  {
    id: "1",
    name: "Acme Store",
    domain: "acme.rex",
    type: "wordpress",
    status: "running",
    phpVersion: "8.3",
    webServer: "nginx",
    ssl: true,
    path: "~/Sites/acme",
    createdAt: "2026-06-01 10:00:00",
    multisite: "none",
    dbName: "wp_acme_test",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
  },
  {
    id: "2",
    name: "Portfolio",
    domain: "portfolio.rex",
    type: "laravel",
    status: "running",
    phpVersion: "8.2",
    webServer: "nginx",
    ssl: true,
    path: "~/Sites/portfolio",
    createdAt: "2026-06-02 11:00:00",
    multisite: "none",
    dbName: "wp_portfolio_test",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
  },
  {
    id: "3",
    name: "Blog Network",
    domain: "network.rex",
    type: "wordpress",
    status: "stopped",
    phpVersion: "8.1",
    webServer: "apache",
    ssl: true,
    path: "~/Sites/network",
    createdAt: "2026-06-03 12:00:00",
    multisite: "subdirectory",
    dbName: "wp_network_test",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
  },
];

// Mirrors each mock site's own status so the browser preview shows per-site
// serving (backend derives this from live upstream + edge state).
export const mockSitesServing: SiteServing[] = mockSites.map((s) => ({
  domain: s.domain,
  serving: s.status === "running",
}));

export const mockServices: ServiceInfo[] = [
  { name: "PHP-FPM 8.3", running: true, pid: 1235, port: 9783, cpuPercent: 0.1, ramMb: 28, kind: "php", version: "8.3.4", isDefault: true },
  { name: "PHP-FPM 8.2", running: false, pid: null, port: 9782, cpuPercent: 0.0, ramMb: 0, kind: "php", version: "8.2.18" },
  { name: "MySQL", running: true, pid: 1234, port: 13306, cpuPercent: 0.4, ramMb: 480, kind: "database", version: "8.4.6" },
  { name: "PostgreSQL", running: false, pid: null, port: 15432, cpuPercent: 0.0, ramMb: 0, kind: "database", version: "18.4.0" },
  { name: "Mailpit", running: true, pid: 1240, port: 11025, cpuPercent: 0.0, ramMb: 12, kind: "mail", version: "1.20.0" },
  { name: "Nginx", running: true, pid: 1236, port: 18088, cpuPercent: 0.0, ramMb: 7, kind: "web", version: "1.30.3", isRouter: false },
  { name: "Caddy", running: false, pid: null, port: 443, cpuPercent: 0.0, ramMb: 0, kind: "web", version: "2.11.4", isRouter: true },
];

export const mockPhpVersions: PhpVersion[] = [
  { minor: "8.0", patch: "8.0.30", fpmPort: 9780, installed: false, isDefault: false },
  { minor: "8.1", patch: "8.1.34", fpmPort: 9781, installed: false, isDefault: false },
  { minor: "8.2", patch: "8.2.31", fpmPort: 9782, installed: true, isDefault: false },
  { minor: "8.3", patch: "8.3.31", fpmPort: 9783, installed: true, isDefault: true },
  { minor: "8.4", patch: "8.4.23", fpmPort: 9784, installed: false, isDefault: false },
  { minor: "8.5", patch: "8.5.8", fpmPort: 9785, installed: false, isDefault: false },
];

/** Whitelisted keys + PHP compiled defaults (mirrors core::php::SETTINGS). */
export const mockPhpSettings: PhpSetting[] = [
  { key: "memory_limit", value: null, default: "128M" },
  { key: "upload_max_filesize", value: null, default: "2M" },
  { key: "post_max_size", value: null, default: "8M" },
  { key: "max_execution_time", value: null, default: "30" },
  { key: "max_input_time", value: null, default: "-1" },
  { key: "max_input_vars", value: null, default: "1000" },
];

export const mockDatabases: DbStatus[] = [
  { key: "mysql", label: "MySQL", port: 13306, version: "8.4.6", running: true, pid: 1234, cpuPercent: 0.4, ramMb: 480 },
  { key: "postgres", label: "PostgreSQL", port: 15432, version: "18.4.0", running: false, pid: null, cpuPercent: 0.0, ramMb: 0 },
];

// Derived from mockServices so the browser mock mirrors the backend's single
// source of truth: running/total/summary reflect live SERVICES, not site rows.
export const mockGlobalStatus: GlobalStatus = {
  summary: mockServices.every((s) => s.running)
    ? "all"
    : mockServices.some((s) => s.running)
      ? "partial"
      : "stopped",
  running: mockServices.filter((s) => s.running).length,
  total: mockServices.length,
  cpuPercent: 14,
  cpuCores: 8,
  ramMb: 612,
  ramTotalMb: 16384,
};

export const mockMailList: MailList = {
  total: 3,
  unread: 2,
  messages: [
    {
      id: "m1",
      from: { name: "Acme Store", address: "wordpress@acme.rex" },
      to: [{ name: "", address: "admin@acme.rex" }],
      subject: "[Acme Store] Password reset request",
      created: "2026-06-28T09:14:00+06:00",
      read: false,
      snippet: "Someone has requested a password reset for the following account…",
    },
    {
      id: "m2",
      from: { name: "", address: "noreply@portfolio.rex" },
      to: [{ name: "", address: "hello@portfolio.rex" }],
      subject: "New contact form submission",
      created: "2026-06-28T08:02:00+06:00",
      read: false,
      snippet: "Name: Jane Doe — Message: Loved your work, let's talk…",
    },
    {
      id: "m3",
      from: { name: "Acme Store", address: "wordpress@acme.rex" },
      to: [{ name: "", address: "customer@example.rex" }],
      subject: "Your order has shipped",
      created: "2026-06-27T17:40:00+06:00",
      read: true,
      snippet: "Order #1024 is on its way. Tracking: 1Z999…",
    },
  ],
};

export function mockMailDetail(id: string): MailDetail {
  const s = mockMailList.messages.find((m) => m.id === id) ?? mockMailList.messages[0];
  return {
    id: s.id,
    from: s.from,
    to: s.to,
    cc: [],
    subject: s.subject,
    date: s.created,
    text: `${s.snippet}\n\nThis is mock message body text shown in the browser-only dev build.`,
    // wp_mail() sends plain-text only — mirror that on one message so the
    // preview's text-fallback path is exercisable in the dev build.
    html:
      s.id === "m2"
        ? ""
        : `<p>${s.snippet}</p><p>This is a <b>mock</b> HTML body shown in the browser-only dev build.</p>`,
    headers: [
      { name: "From", value: s.from.address },
      { name: "To", value: s.to.map((t) => t.address).join(", ") },
      { name: "Subject", value: s.subject },
      { name: "Content-Type", value: "text/html" },
    ],
  };
}
