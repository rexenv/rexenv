/**
 * Mock data for the static shell (Phase 1 task 0.6). Replaced by real IPC data
 * as backend tasks land. Kept in one place so it's easy to delete later.
 */
import type { AdminerStatus, AppInfo, AppUpdateState, DbStatus, GlobalStatus, MailDetail, MailList, PhpSetting, PhpVersion, PlatformWords, ServiceInfo, Site, SiteServing , ResolverTldStatus } from "@/types";

/** macOS's platform words — the browser shell's answer, and `usePlatformWords`' answer until the backend's
 *  arrives. Must match `src-tauri/src/platform/words.rs` `MACOS` (a test holds them together). */
export const mockPlatformWords: PlatformWords = {
  reveal: "Show in Finder",
  fileManager: "Finder",
  loginItem: "rexenv launches when you sign in to your Mac (a macOS login item).",
  gitInstall: "On macOS it ships with the Xcode Command Line Tools — install them, then hit Re-detect:\n$ xcode-select --install",
  nodeInstall: "$ brew install node",
  bunInstall: "$ brew install oven-sh/bun/bun",
  nativeBuild: "That needs the Xcode Command Line Tools — install them, then retry:\n$ xcode-select --install",
  cliInstall: "Put the rex command on your PATH to manage rexenv from the terminal. One admin prompt.",
  cliStale: "points elsewhere (an old copy or another tool) — reinstall to point it at this app.",
  cliInstalled: "rex installed — run it from any terminal",
  trustStore: "login keychain",
  privilegedPrompt: "asks for your password once",
  osName: "macOS",
  caTarget: "your Mac",
  elevationNote: "macOS will also ask for your password",
  host: "this Mac",
  trayHome: "menu bar",
  appSearch: "/Applications and ~/Applications",
  windowControlsInContent: true,
};

export const mockAppInfo: AppInfo = {
  name: "rexenv",
  version: "0.1.0",
  tauriVersion: "2",
  commit: "dev",
  builtAt: "unknown",
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
    dbName: "wp_acme_rex",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: true,
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
    dbName: "lv_portfolio_rex",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: true,
  },
  {
    id: "3",
    name: "Blog Network",
    domain: "network.rex",
    type: "wordpress",
    status: "stopped",
    phpVersion: "8.1",
    // FrankenPHP on purpose: the dev route must render the read-only PHP
    // picker + "Fixed by FrankenPHP" note (the stored 8.1 is NOT what serves).
    webServer: "frankenphp",
    ssl: true,
    path: "~/Sites/network",
    createdAt: "2026-06-03 12:00:00",
    multisite: "subdirectory",
    dbName: "wp_network_rex",
  dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: true,
  },
  {
    // A site with NO tunnel, on purpose: the Tunnels page needs an IDLE card or
    // "this one says Live" is a claim about a screen where nothing else could
    // have been rendered. It is the control for the tunnel-health probe, and
    // the state most users' sites are in.
    id: "4",
    name: "Docs",
    domain: "docs.rex",
    type: "php",
    status: "running",
    phpVersion: "8.3",
    webServer: "nginx",
    ssl: true,
    path: "~/Sites/docs",
    createdAt: "2026-06-04 09:00:00",
    multisite: "none",
    dbName: "",
    dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: true,
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
  { name: "PostgreSQL", running: false, pid: null, port: 15432, cpuPercent: 0.0, ramMb: 0, kind: "database", version: "18.6.0" },
  { name: "Mailpit", running: true, pid: 1240, port: 11025, cpuPercent: 0.0, ramMb: 12, kind: "mail", version: "1.20.0" },
  { name: "Nginx", running: true, pid: 1236, port: 18088, cpuPercent: 0.0, ramMb: 7, kind: "web", version: "1.30.3", isRouter: false },
  { name: "Caddy", running: false, pid: null, port: 443, cpuPercent: 0.0, ramMb: 0, kind: "web", version: "2.11.4", isRouter: true },
];

/** The services the dev/WebKit harness renders: `?busy=8.3` gives that pool the busy-workers
 *  note the backend sends (ledger #608). Absent → no note, which is the ordinary state and MUST
 *  render nothing — the harness probe asserts both ways. */
export function mockServicesView(): ServiceInfo[] {
  const busy = (new URLSearchParams(window.location.search).get("busy") ?? "")
    .split(",")
    .map((m) => m.trim())
    .filter(Boolean);
  return mockServices.map((s) =>
    s.running && busy.some((m) => s.name === `PHP-FPM ${m}`)
      ? { ...s, busyNote: "all 10 workers busy — requests are queuing" }
      : s,
  );
}

/** Shaped like production, including the awkward rows: 8.0 and 7.4 ship but have
 *  NO Xdebug (neither static build exports `_OnUpdateBool`, so no `.so` can
 *  dlopen into them), which is the case the dev UI must render correctly and the
 *  one a tidy all-true fixture would hide.
 *
 *  **Four update states, because a fixture where every row is the same state
 *  proves only that one state renders.** 8.1 is POST-UPDATE — `patch` is 8.1.35,
 *  above what the app pins, with nothing offered: the state the row spent a
 *  release rendering wrong, because every field was compared against the pin
 *  instead of what the minor will run. 8.2 has `upstream === updatable`, where
 *  the chip must vanish and only the button remain — "8.2.32 exists" beside a
 *  button offering 8.2.32 reads as two versions. 8.3 has all three at once
 *  (serving behind, a button, and a chip naming something newer than both).
 *  8.4/8.5 are upstream-only, the chip's actual reason to exist. 8.0 is the only
 *  row NOT installed, so the Install path has a fixture at all.
 *
 *  **7.4 leads the list and its port breaks the pattern on purpose.** `fpmPort`
 *  is `9700 + major*10 + minor`, so 7.4 → **9774**, BELOW the contiguous
 *  9780–9785 block. A fixture that quietly renumbered it to 9779 would look
 *  tidier and would hide any layout that assumes the ports ascend with the list
 *  or sit in one range. Production is not tidy here; neither is this. */
export const mockPhpVersions: PhpVersion[] = [
  { minor: "7.4", patch: "7.4.33", serving: null, upstream: null, updatable: null, upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9774, installed: true, isDefault: false, xdebugSupported: false, xdebugUnavailableReason: "Xdebug isn't available for PHP 7.4 — its static build exports no Zend symbols, so it can't load any extension (rexenv's own 7.4.33 build, 14 Aug 2026). No version of Xdebug can change that. Switch the site to PHP 8.1 or newer first.", xdebugVersion: null, postgresSupported: false, updateCost: null, eolSince: "2022-11-28" },
  { minor: "8.0", patch: "8.0.30", serving: null, upstream: null, updatable: null, upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9780, installed: false, isDefault: false, xdebugSupported: false, xdebugUnavailableReason: "Xdebug isn't available for PHP 8.0 — its static build exports no Zend symbols, so it can't load any extension (static-php.dev's 8.0.30 build, Nov 2024). No version of Xdebug can change that. Switch the site to PHP 8.1 or newer first.", xdebugVersion: null, postgresSupported: false, updateCost: null, eolSince: "2023-11-26" },
  { minor: "8.1", patch: "8.1.35", serving: null, upstream: null, updatable: null, upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9781, installed: true, isDefault: false, xdebugSupported: true, xdebugUnavailableReason: null, xdebugVersion: "3.5.3", postgresSupported: true, updateCost: null, eolSince: "2025-12-31" },
  { minor: "8.2", patch: "8.2.32", serving: null, upstream: "8.2.34", updatable: "8.2.33", upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9782, installed: true, isDefault: false, xdebugSupported: true, xdebugUnavailableReason: null, xdebugVersion: "3.5.3", postgresSupported: true, updateCost: "PHP 8.2.33 is upstream's build and cannot reach PostgreSQL — no site here uses it, but PostgreSQL will stop being offered for new sites on this version.", eolSince: null },
  { minor: "8.3", patch: "8.3.32", serving: "8.3.31", upstream: "8.3.33", updatable: "8.3.33", upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9783, installed: true, isDefault: true, xdebugSupported: true, xdebugUnavailableReason: null, xdebugVersion: "3.5.3", postgresSupported: true, updateCost: "PHP 8.3.33 is upstream's build and cannot reach PostgreSQL. shop.rex uses a PostgreSQL database and would stop reaching it.", eolSince: null },
  { minor: "8.4", patch: "8.4.23", serving: null, upstream: "8.4.24", updatable: null, upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9784, installed: true, isDefault: false, xdebugSupported: true, xdebugUnavailableReason: null, xdebugVersion: "3.5.3", postgresSupported: true, updateCost: null, eolSince: null },
  { minor: "8.5", patch: "8.5.8", serving: null, upstream: "8.5.9", updatable: null, upstreamCheckedAt: "2026-08-17 09:12:00", fpmPort: 9785, installed: true, isDefault: false, xdebugSupported: true, xdebugUnavailableReason: null, xdebugVersion: "3.5.3", postgresSupported: true, updateCost: null, eolSince: null },
];

/** The Adminer version row.
 *
 *  Carries an OFFER by default, because the state with a button is the one that
 *  renders something and therefore the one worth looking at — and because
 *  upstream really is ahead: rexenv pins 5.4.2 while Adminer shipped 6.0.1.
 *  `?adminer=` in the dev harness swaps the other states in. */
export const mockAdminerStatus: AdminerStatus = {
  staged: "5.4.2",
  effective: "5.4.2",
  updatable: "6.0.1",
};

/** The browser-dev answer for the update card.
 *
 *  Deliberately the "nothing to offer, and we looked" state rather than a
 *  standing offer: a fixture that always offers an update is the friendly-fake
 *  shape this project keeps getting bitten by — it makes the one state that
 *  must be right (no button, honest footer) the one nobody ever looks at. The
 *  dev harness swaps the other states in by query parameter. */
export const mockAppUpdateState: AppUpdateState = {
  running: "0.1.0",
  enabled: true,
  autoCheck: true,
  offered: null,
  noOfferReason: "the newest signed release is 0.1.0 and this is 0.1.0",
  checkedAt: null,
  skipped: null,
  installedPending: null,
};

/** Nothing offered in the browser mock, so nothing to be ready for. */
export const mockAppUpdateReadiness = null;

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
  { key: "postgres", label: "PostgreSQL", port: 15432, version: "18.6.0", running: false, pid: null, cpuPercent: 0.0, ramMb: 0 },
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

/** Resolver drift for the dev/WebKit harness: `?drift=test,dev` in the URL is
 *  the fixture. Absent/empty → `[]`, which is the ordinary state and MUST
 *  render nothing (the #306 rule) — the harness probe asserts both ways. */
export function mockResolverDrift(): string[] {
  const raw = new URLSearchParams(window.location.search).get("drift") ?? "";
  return raw
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
}

/** Who owns a TLD's resolver file, for the dev/WebKit harness: `?foreign=test,dev`
 *  names TLDs whose `/etc/resolver/<tld>` "belongs to Valet"; anything else is
 *  `absent`, which MUST render nothing (the consent card is fixture-driven, never
 *  always-on). A takeover through the mock flips the TLD to `borrowed` for the
 *  page's lifetime, so the card has to disappear on the same evidence the real
 *  app would have — a card that hid itself on the click rather than on the
 *  re-read would pass a frozen fixture. */
const mockTakenOver = new Set<string>();
export function mockResolverTldStatus(tld: string): ResolverTldStatus {
  const foreign = (new URLSearchParams(window.location.search).get("foreign") ?? "")
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
  const ourContent = "nameserver 127.0.0.1\nport 15353\n";
  const path = `/etc/resolver/${tld}`;
  if (mockTakenOver.has(tld)) {
    return { tld, owner: "borrowed", path, theirContent: "nameserver 127.0.0.1\n", ourContent, rexenvSites: 0 };
  }
  if (foreign.includes(tld)) {
    return { tld, owner: "foreign", path, theirContent: "nameserver 127.0.0.1\n", ourContent, rexenvSites: 0 };
  }
  return { tld, owner: "absent", path, theirContent: null, ourContent, rexenvSites: 0 };
}
export function mockResolverTakeOver(tld: string): void {
  mockTakenOver.add(tld);
}

/** Extra domains per site id, for the dev shell (v42).
 *
 *  MUTABLE on purpose: the L2 probe adds and removes through the same wrappers
 *  the app uses, and a fixture that answered the same list forever would let a
 *  card that ignores the reply pass — which is precisely the honest-UI claim
 *  ("the screen never shows a name the server did not confirm") the probe is
 *  there to hold.
 */
const mockDomains: Record<string, string[]> = {
  "1": ["acme.rex", "shop.acme.rex"],
};

export function mockSiteDomains(id: string): string[] {
  const site = mockSites.find((s) => s.id === id);
  return mockDomains[id] ?? (site ? [site.domain] : []);
}

/** Refuses what `core::sites::validate_alias` refuses, naming the site — the
 *  dev shell must not accept another site's primary and render it, which is the
 *  honest-UI violation the card exists to prevent, passing its own probe. */
export function mockAddSiteDomain(id: string, domain: string): string[] {
  const list = mockSiteDomains(id);
  if (list.includes(domain)) throw new Error(`this site already answers on "${domain}"`);
  for (const other of mockSites) {
    if (other.id === id) continue;
    const names = mockSiteDomains(other.id);
    if (names[0] === domain)
      throw new Error(
        `"${domain}" is already the domain of the site "${other.name}" — one hostname can only reach one site`,
      );
    if (names.includes(domain))
      throw new Error(`"${domain}" is already an extra domain of the site "${other.name}"`);
  }
  mockDomains[id] = [...list, domain];
  return mockDomains[id];
}

/** The primary (index 0) is not removable here, exactly as in the backend. */
export function mockRemoveSiteDomain(id: string, domain: string): string[] {
  const [primary, ...extras] = mockSiteDomains(id);
  mockDomains[id] = [primary, ...extras.filter((d) => d !== domain)];
  return mockDomains[id];
}

/** The dev shell's extra-domain map, mirroring `all_site_domains`: keyed by site
 *  id, sites with none absent. Site 1 has one, so the Sites row's `+N` marker
 *  renders — a fixture where every site had none would leave that marker
 *  unexercised, which is how the PHP-avatar contrast bug hid for months. */
export function mockAllSiteDomains(): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const [id, list] of Object.entries(mockDomains)) {
    const extras = list.slice(1);
    if (extras.length > 0) out[id] = extras;
  }
  return out;
}
