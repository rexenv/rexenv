// Tier 2 · "Debugging": Xdebug for one site, WP_DEBUG from Tools, the debug
// log in the Logs tab, and the Terminal tab with the site's PHP and WP-CLI.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Debugging a local site: Xdebug, WordPress debug logs and a terminal — all built in." },
  xdebug: { text: "In the site's Settings, switch on Xdebug. Only this site moves to a debug PHP pool — every other site keeps full speed." },
  ide: { text: "Point your IDE at port 9003, and start a session with the browser helper or XDEBUG_SESSION=1.", say: "Point your IDE at port nine thousand and three, and start a session with the browser helper, or X debug session equals one." },
  wpdebug: { text: "For WordPress, Tools has the debug switches. WP_DEBUG turns on the recommended set: log on, display off.", say: "For WordPress, Tools has the debug switches. WP debug turns on the recommended set: log on, display off." },
  logs: { text: "The Logs tab then shows debug.log as it grows — errors in red, warnings in amber.", say: "The Logs tab then shows debug dot log as it grows. Errors in red, warnings in amber." },
  terminal: { text: "And the Terminal tab opens a shell in the site's folder, with the site's own PHP and WP-CLI already on the PATH.", say: "And the Terminal tab opens a shell in the site's folder, with the site's own PHP and WP CLI already on the path." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "debugging" });
const { app, page } = s;

await s.title("Debugging", "Xdebug, debug logs and a terminal — per site");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Xdebug ───────────────────────────────────────────────────────────────
await s.say("xdebug");
await s.caption("Site → Settings → <b>Xdebug</b>");
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 1000 });
await s.click(app.getByRole("button", { name: "Settings", exact: true }).last(), { ms: 800 });
const xd = app.getByRole("switch", { name: "Toggle Xdebug" }).or(app.getByLabel("Toggle Xdebug"));
await s.box(xd);
await s.camera(await s.containerOf(xd, 560, 12), 0.75);
await s.click(xd, { ms: 900 });
await app.getByText(/Xdebug on — set your IDE to listen on port 9003/).waitFor({ timeout: 10_000 });
await s.voiceDone(200);

await s.say("ide");
await s.caption("IDE on port <code>9003</code> · <code>?XDEBUG_SESSION=1</code>");
await s.moveTo(app.getByText(/Your IDE listens on port/), { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── WP_DEBUG ─────────────────────────────────────────────────────────────
await s.say("wpdebug");
await s.caption("WordPress → Tools → <b>WP_DEBUG</b>");
await s.click(app.getByRole("button", { name: "WordPress", exact: true }).first(), { ms: 800 });
await s.click(app.getByRole("button", { name: "Tools", exact: true }), { ms: 700 });
const wpd = app.getByRole("switch", { name: "Toggle WP_DEBUG", exact: true });
await s.box(wpd);
await s.camera(await s.containerOf(wpd, 380, 12), 0.7);
await s.click(wpd, { ms: 800 });
await s.voiceDone(300);
await s.camera(null);

// ── Logs ─────────────────────────────────────────────────────────────────
await s.say("logs");
await s.caption("<b>Logs</b> → WordPress debug log, live");
await s.click(app.getByRole("button", { name: "Logs", exact: true }).first(), { ms: 800 });
const line = app.getByText(/Undefined array key "price"/);
await line.waitFor({ timeout: 10_000 });
await s.camera(await s.containerOf(line, 700, 10), 0.85);
await app.evaluate(() => window.__scene.logMore());
await app.getByText(/Uncaught Error: Call to undefined function/).waitFor({ timeout: 8000 });
await s.moveTo(app.getByText(/Uncaught Error: Call to undefined function/), { ms: 800, fx: 0.3 });
await s.voiceDone(300);
await s.camera(null);

// ── Terminal ─────────────────────────────────────────────────────────────
await s.say("terminal");
await s.caption("<b>Terminal</b> — this site's PHP and <code>wp</code> on the PATH");
await s.click(app.getByRole("button", { name: "Terminal", exact: true }).first(), { ms: 800 });
const xterm = app.locator(".xterm");
await xterm.waitFor();
await s.wait(800);
const tb = await s.box(xterm);
// xterm draws at 12 px: frame the top of it, where the session is.
await s.camera({ x: tb.x, y: tb.y - 30, w: Math.min(tb.w, 760), h: 250 }, 0.9);
await s.click(xterm, { ms: 600, fx: 0.3, fy: 0.1 });
await s.type("wp --version", 70);
await page.keyboard.press("Enter");
await s.wait(700);
await s.type("wp plugin list", 70);
await page.keyboard.press("Enter");
await s.voiceDone(1200);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("debugging")}`);
