// Tier 3 · "The menu-bar app": close the window (nothing stops), the tray
// menu (status, Start/Stop all, Sites, screens, Open/Quit), reopen. The menu
// bar and menu are DRAWN (native, outside the webview); items are core/tray.rs's.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "rexenv lives in your menu bar — closing its window doesn't stop anything.", say: "Rex env lives in your menu bar. Closing its window doesn't stop anything." },
  close: { text: "Close the window: your sites, databases and DNS keep running, and rex and AI agents can still reach it.", say: "Close the window. Your sites, databases and DNS keep running, and the rex command and AI agents can still reach it." },
  menu: { text: "Click the icon in the menu bar. The first line is the same status as the app's footer." },
  sites: { text: "Start or stop everything, jump to a site, or straight to Mail or Tunnels." },
  open: { text: "Open rexenv brings the window back, right where you left it.", say: "Open rex env brings the window back, right where you left it." },
  quit: { text: "Only Quit really quits — and it asks first if a site is still shared publicly." },
  login: { text: "Turn on “Start rexenv at login”, and it comes up quietly in the menu bar with your sites running.", say: "Turn on, start rex env at login, and it comes up quietly in the menu bar, with your sites running." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const MENU = [
  "~All running · 7 services",
  "-",
  "~Start all",
  "Stop all",
  "-",
  "Sites\t›",
  "All sites…",
  "Services",
  "Databases",
  "Mail",
  "Tunnels",
  "-",
  "MCP server",
  "-",
  "About rexenv",
  "Open rexenv",
  "Quit rexenv",
];
const SITES = ["landing.rex", "booking-api.rex", "shop-staging.rex", "agency-blog.rex"];
// Indexes among the non-separator items.
const I = { status: 0, stopAll: 2, sites: 3, mail: 7, tunnels: 8, open: 11, quit: 12 };

const s = await openStage({ narration });
const { page } = s;
const st = (fn, a) => page.evaluate(fn, a);
const glideTo = async (pt, ms = 700) => {
  if (pt) await st(([x, y, m]) => window.stage.scursor.moveTo(x, y, m), [pt.x, pt.y, ms]);
};

await s.title("The menu-bar app", "Close the window — everything keeps running");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await st(() => window.stage.tray.bar(true));
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await st(() => window.stage.scursor.show(true));

// ── Close the window ─────────────────────────────────────────────────────
await s.say("close");
await s.caption("Close the window — nothing stops");
await glideTo(await st(() => window.stage.tray.closeButtonAt()), 1000);
await st(() => window.stage.scursor.press());
await s.stage(() => window.stage.showWindow(false));
await s.voiceDone(200);

// ── The menu ─────────────────────────────────────────────────────────────
await s.say("menu");
await s.caption("The menu-bar icon");
await glideTo(await st(() => window.stage.tray.iconAt()), 900);
await st(() => window.stage.scursor.press());
await st((m) => window.stage.tray.open(m), MENU);
await s.wait(400);
await glideTo(await st((i) => window.stage.tray.hl(i), I.status), 500);
await s.voiceDone(200);

await s.say("sites");
await s.caption("Start / Stop all · Sites · Mail · Tunnels");
await glideTo(await st((i) => window.stage.tray.hl(i), I.stopAll), 500);
await s.wait(500);
await glideTo(await st((i) => window.stage.tray.hl(i), I.sites), 500);
await st(([m, sub]) => window.stage.tray.open(m, { at: 3, items: sub }), [MENU, SITES]);
await st((i) => window.stage.tray.hl(i), I.sites);
await s.wait(300);
await glideTo(await st(() => window.stage.tray.hl(1, 1)), 600);
await s.wait(700);
await st((m) => window.stage.tray.open(m), MENU);
await glideTo(await st((i) => window.stage.tray.hl(i), I.mail), 600);
await s.wait(400);
await glideTo(await st((i) => window.stage.tray.hl(i), I.tunnels), 400);
await s.voiceDone(200);

// ── Open rexenv ──────────────────────────────────────────────────────────
await s.say("open");
await s.caption("<b>Open rexenv</b>");
await glideTo(await st((i) => window.stage.tray.hl(i), I.open), 700);
await st(() => window.stage.scursor.press());
await st(() => window.stage.tray.close());
await s.stage(() => window.stage.showWindow(true));
await s.voiceDone(200);

await s.say("quit");
await s.caption("Only <b>Quit rexenv</b> quits");
await glideTo(await st(() => window.stage.tray.iconAt()), 700);
await st(() => window.stage.scursor.press());
await st((m) => window.stage.tray.open(m), MENU);
await glideTo(await st((i) => window.stage.tray.hl(i), I.quit), 800);
await s.voiceDone(300);
await st(() => window.stage.tray.close());

await s.say("login");
await s.caption("Settings → Services → <b>Start rexenv at login</b>");
await st(() => window.stage.scursor.show(false));
await s.stage(() => window.stage.showCursor(true));
await s.click(s.app.getByRole("link", { name: /^Settings/ }), { ms: 900 });
await s.click(s.app.getByRole("button", { name: "Services", exact: true }), { ms: 700 });
const login = s.app.getByText("Start rexenv at login", { exact: true });
await s.box(login);
await s.spotlight(await s.containerOf(login, 560, 4));
await s.moveTo(login, { ms: 800 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await st(() => window.stage.tray.bar(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("tray")}`);
