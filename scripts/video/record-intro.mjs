// "Introducing rexenv" — the ~85 s launch video: why rexenv exists and what is
// distinctive about it, at a fast pace. One recording across many scenes
// (switchScene), jobs replayed at 20 % length, the voice at +15 %. Each beat
// starts its line, then loads its screen, so the load plays under the voice.
//   node record-intro.mjs                 landscape 1920×1080
//   node record-intro.mjs portrait        the vertical 1080×1920 cut
import { openStage } from "./lib.mjs";

const PORTRAIT = process.argv[2] === "portrait";
const NAME = PORTRAIT ? "intro-vertical" : "intro";

const narration = {
  hook: { text: "rexenv runs your whole local stack — in one native app.", say: "Rex env runs your whole local stack, in one native app." },
  why: { text: "Local development shouldn't mean Docker's weight, one framework, or one operating system." },
  answer: { text: "rexenv is native, open source, and runs on Mac, Windows and Linux.", say: "Rex env is native, open source, and runs on Mac, Windows and Linux." },
  movein: { text: "Already on Local, Valet or Herd? Import your sites where they live — databases copied, your old setup untouched.", say: "Already on Local, Valet or Herd? Import your sites where they live. Databases copied, your old setup untouched." },
  wp: { text: "One click for WordPress — then plugins, themes, users and whole multisite networks, without opening wp-admin.", say: "One click for WordPress. Then plugins, themes, users and whole multisite networks, without opening WP admin." },
  git: { text: "Start sites and plugins straight from Git. Install, build and activate in a few clicks — then pull and push, all inside rexenv.", say: "Start sites and plugins straight from Git. Install, build and activate in a few clicks. Then pull and push, all inside rex env." },
  ai: { text: "AI agents get disposable scratch sites of their own. Your sites stay read-only until you say so — and every call is logged.", say: "AI agents get disposable scratch sites of their own. Your sites stay read-only until you say so. And every call is logged." },
  safe: { text: "Mail is caught, even through SMTP plugins. Public links warn you about default logins. And system changes always ask first." },
  stack: { text: "PHP 7.4 to 8.5, three web servers, four databases side by side — and a CLI for all of it.", say: "PHP seven four to eight five, three web servers, four databases side by side, and a CLI for all of it." },
  close: { text: "rexenv. Your whole local stack — free, open source, no Docker.", say: "Rex env. Your whole local stack. Free, open source, and no Docker." },
};

const s = await openStage({ narration, orient: PORTRAIT ? "portrait" : "landscape", speed: 0.2, rate: "+15%", gap: 90, switchSettle: 80 });
const { app, page } = s;
const FAST = { ms: 300, pre: 70, post: 120 };
const tap = (loc, o = {}) => s.click(loc, { ...FAST, ...o });
// Beat timings in the log, against the cue times: where a beat outruns its line.
const T0 = Date.now();
const mark = (label) => console.log(`${((Date.now() - T0) / 1000).toFixed(1)}s ${label}`);

// ── Hook: the whole stack, one app ───────────────────────────────────────
await s.layout("split");
await s.stage(() => window.stage.showWindow());
await s.stage(() => window.stage.showCursor());
await s.headline("Your whole local stack.<br><em>One app.</em>", "Sites · PHP · databases · mail · HTTPS · sharing");
await s.say("hook", 0);
for (const nav of ["Services", "Databases", "Mail", "Sites"]) await tap(app.getByRole("link", { name: new RegExp(`^${nav}`) }), { ms: 380 });
await s.voiceDone();

// ── Why ──────────────────────────────────────────────────────────────────
await s.headline(null);
await s.stage(() => window.stage.showWindow(false));
await s.stage(() => window.stage.showCursor(false));
await s.say("why");
const whyMs = s.dur("why") * 1000;
await s.title("No Docker.", "", true);
await s.wait(whyMs * 0.34);
await s.title("Any framework.", "WordPress · Laravel · plain PHP", true);
await s.wait(whyMs * 0.33);
await s.title("Any OS.", "", true);
await s.voiceDone();
await s.say("answer");
const ansMs = s.dur("answer") * 1000;
await s.title("Native.", "", true);
await s.wait(ansMs * 0.25);
await s.title("Open source.", "Apache 2.0", true);
await s.wait(ansMs * 0.3);
await s.title("macOS · Windows · Linux", "", true);
await s.voiceDone();

// ── Move in ──────────────────────────────────────────────────────────────
// Each beat: headline + line first, then the screen loads under the voice.
await s.title(null, "", true);
await s.headline("Move in.<br><em>Nothing moves.</em>", "Import from Local, Valet &amp; Herd — in place, databases copied.");
await s.say("movein");
await s.switchScene("import", "/import", "text=Import from Valet, Herd or Local", { tookover: "1" });
await s.stage(() => window.stage.showCursor());
await tap(app.locator('div:has(> span:has-text("Select sites to import")) > input[type=checkbox]'));
await tap(app.getByRole("button", { name: /^Import 3$/ }));
await app.getByText("Cancel after current").waitFor();
await s.camera(await s.containerOf(app.getByText("Cancel after current"), 500, 16), 0.62);
mark("movein acted");
await s.voiceDone();

// ── WordPress ────────────────────────────────────────────────────────────
await s.headline("WordPress,<br><em>fully managed.</em>", "Plugins, themes, users, multisite — no wp-admin.");
await s.say("wp");
await s.switchScene("wp-manager", "/sites/1/wordpress", "text=Yoast SEO");
const search = app.getByPlaceholder(/Search WordPress\.org or enter a slug/);
await tap(search);
await s.type("woo", 45);
const hit = app.getByText("WooCommerce", { exact: true }).first();
await hit.waitFor();
await tap(hit);
await tap(app.getByRole("button", { name: /^Install/ }));
await app.getByText("Installed woocommerce").waitFor({ timeout: 15_000 });
await tap(app.getByRole("button", { name: /^Network/ }));
mark("wp acted");
await s.voiceDone();

// ── Git ──────────────────────────────────────────────────────────────────
await s.headline("<em>Git-native.</em>", "Sites and plugins from Git — build, pull, push, stash.");
await s.say("git");
await s.switchScene("repo-panel", "/sites/1/wordpress", "text=Yoast SEO");
await tap(app.getByRole("button", { name: "From Git" }).first());
await tap(app.getByPlaceholder(/owner\/repo/).first());
await s.type("acme/booking-widget", 12);
await tap(app.getByRole("button", { name: "Fetch" }).first());
await tap(app.getByRole("button", { name: /^Add plugin/ }));
for (const name of ["composer install", "pnpm install", "pnpm run build"]) {
  await tap(app.getByRole("button", { name, exact: true }), { ms: 220 });
  await app.getByRole("button", { name: `✓ ${name}`, exact: true }).waitFor({ timeout: 10_000 });
}
await tap(app.getByRole("button", { name: "Activate plugin" }));
mark("git acted");
await s.voiceDone();

// ── AI with guardrails ───────────────────────────────────────────────────
await s.headline("AI agents,<br><em>with guardrails.</em>", "Scratch sites of their own. Your sites stay read-only until you say so.");
await s.say("ai");
await s.switchScene("mcp-agent", "/settings?section=agents", "text=Enable the MCP endpoint");
await tap(app.getByRole("switch", { name: "Enable the MCP endpoint" }).or(app.getByLabel("Enable the MCP endpoint")).first());
await app.getByText("MCP endpoint enabled").waitFor({ timeout: 8000 });
void app.evaluate(() => window.__scene.agentWorks());
// Open the fold while it is still empty; the calls land in it on the feed's
// next poll (every 4 s). Meanwhile "read-only until you say so" is the dial.
const fold = app.getByRole("button", { name: /Recent activity/ });
await tap(fold);
const readRadio = app.getByRole("radio", { name: /^Read/ });
await s.box(readRadio);
await s.camera(await s.containerOf(readRadio, 560, 30), 0.72);
await app.getByText("denied", { exact: true }).first().waitFor({ timeout: 10_000 });
// "…and every call is logged": the rows sit below the scroller's edge.
const lastRow = app.getByText("scratch_create_site", { exact: true });
await s.box(lastRow);
await s.camera(await s.union([fold, lastRow], 14), 0.62);
mark("ai acted");
await s.voiceDone();

// ── Safe by default: three screens of one app, through its sidebar ───────
await s.headline("Safe<br><em>by default.</em>", "Mail caught · public links checked · system changes ask first.");
await s.say("safe");
await s.switchScene("mail,share,import", "/mail", "text=Mailpit");
await tap(app.getByRole("button", { name: /message: .*New order #1042/ }));
await s.wait(500);
await tap(app.getByRole("link", { name: /^Tunnels/ }));
await tap(app.getByRole("switch", { name: "Start sharing Agency Blog" }));
const warn = app.getByText(/still accepts the default/);
await warn.waitFor({ timeout: 10_000 });
await s.spotlight(await s.containerOf(warn, 480));
await s.wait(700);
await s.spotlight(null);
await tap(app.getByRole("link", { name: /^Import/ }));
const consent = app.getByText(".test is managed by Valet or Herd");
await consent.waitFor({ timeout: 10_000 });
await s.camera(await s.union([consent, app.getByRole("button", { name: "Take over .test" })], 18), 0.66);
mark("safe acted");
await s.voiceDone();

// ── Every version, every server ──────────────────────────────────────────
await s.headline("Every version.<br><em>Every server.</em>", "PHP 7.4–8.5 · Nginx · FrankenPHP · Apache · MySQL · MariaDB · PostgreSQL · Redis");
await s.say("stack");
await s.switchScene("php,database", "/settings?section=services", "text=PHP versions");
const php74 = app.getByText("PHP 7.4", { exact: true });
await s.box(php74);
await s.camera(await s.union([php74, app.getByText("PHP 8.5", { exact: true })], 16), 0.7);
await s.wait(500);
await s.camera(null);
await tap(app.getByRole("link", { name: /^Databases/ }));
await app.getByText("127.0.0.1:13306").first().waitFor();
await s.wait(450);
await s.stage(() => window.stage.showWindow(false));
await s.term.show(PORTRAIT ? { title: "Terminal — zsh", x: 40, y: 815, w: 1000, h: 520, font: 20 } : { title: "Terminal — zsh", x: 640, y: 250, w: 1180, h: 560, font: 22 });
await s.term.type("rex status", { perChar: 30 });
await s.term.print(
  [
    "DNS      answering (agent, udp 15353) · resolver installed · CA trusted",
    "NAME         STATE        PID    PORT    CPU%     RAM",
    "Caddy        running    41210     443     0.3      38 MB",
    "Nginx        running    41212   18088     0.1      12 MB",
    "PHP-FPM 8.3  running    41230    9783     0.8      64 MB",
    "MySQL        running    41250   13306     1.2     410 MB",
  ],
  60,
);
mark("stack acted");
// The table is the point: hold it on screen even when the line is over.
await Promise.all([s.voiceDone(), s.wait(1100)]);
await s.term.hide();

// ── Close ────────────────────────────────────────────────────────────────
await s.headline(null);
await s.stage(() => window.stage.showCursor(false));
await s.title(
  "rexenv",
  "Free & open source · macOS · Windows · Linux",
  false,
  '<div><span>macOS · Linux</span>curl -fsSL https://rexenv.rex.bd/install.sh | bash</div><div><span>Windows</span>irm https://rexenv.rex.bd/install.ps1 | iex</div>',
);
await s.say("close");
await s.voiceDone(1500);

console.log(`saved ${await s.save(NAME)}`);
