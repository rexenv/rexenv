/**
 * Mock data for the static shell (Phase 1 task 0.6). Replaced by real IPC data
 * as backend tasks land. Kept in one place so it's easy to delete later.
 */
import type { DbStatus, GlobalStatus, MailDetail, MailList, PhpVersion, ServiceInfo, Site } from "@/types";

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
    multisite: "none",
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
    multisite: "none",
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
    multisite: "subdirectory",
  },
];

export const mockServices: ServiceInfo[] = [
  { name: "MySQL", running: true, pid: 1234, port: 13306, cpuPercent: 0.4, ramMb: 480 },
  { name: "PHP-FPM", running: true, pid: 1235, port: 9783, cpuPercent: 0.1, ramMb: 28 },
  { name: "Nginx", running: true, pid: 1236, port: 8088, cpuPercent: 0.0, ramMb: 7 },
  { name: "Caddy", running: false, pid: null, port: 443, cpuPercent: 0.0, ramMb: 0 },
];

export const mockPhpVersions: PhpVersion[] = [
  { minor: "8.1", patch: "8.1.34", fpmPort: 9781, installed: false, isDefault: false },
  { minor: "8.2", patch: "8.2.31", fpmPort: 9782, installed: true, isDefault: false },
  { minor: "8.3", patch: "8.3.31", fpmPort: 9783, installed: true, isDefault: true },
];

export const mockDatabases: DbStatus[] = [
  { key: "mysql", label: "MySQL", port: 13306, version: "8.4.6", running: true, pid: 1234, cpuPercent: 0.4, ramMb: 480 },
  { key: "postgres", label: "PostgreSQL", port: 15432, version: "18.4.0", running: false, pid: null, cpuPercent: 0.0, ramMb: 0 },
];

export const mockGlobalStatus: GlobalStatus = {
  summary: "partial",
  running: 3,
  total: 12,
  cpuPercent: 14,
  ramMb: 612,
  ramTotalMb: 16384,
};

export const mockMailList: MailList = {
  total: 3,
  unread: 2,
  messages: [
    {
      id: "m1",
      from: { name: "Acme Store", address: "wordpress@acme.test" },
      to: [{ name: "", address: "admin@acme.test" }],
      subject: "[Acme Store] Password reset request",
      created: "2026-06-28T09:14:00+06:00",
      read: false,
      snippet: "Someone has requested a password reset for the following account…",
    },
    {
      id: "m2",
      from: { name: "", address: "noreply@portfolio.test" },
      to: [{ name: "", address: "hello@portfolio.test" }],
      subject: "New contact form submission",
      created: "2026-06-28T08:02:00+06:00",
      read: false,
      snippet: "Name: Jane Doe — Message: Loved your work, let's talk…",
    },
    {
      id: "m3",
      from: { name: "Acme Store", address: "wordpress@acme.test" },
      to: [{ name: "", address: "customer@example.test" }],
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
    html: `<p>${s.snippet}</p><p>This is a <b>mock</b> HTML body shown in the browser-only dev build.</p>`,
    headers: [
      { name: "From", value: s.from.address },
      { name: "To", value: s.to.map((t) => t.address).join(", ") },
      { name: "Subject", value: s.subject },
      { name: "Content-Type", value: "text/html" },
    ],
  };
}
