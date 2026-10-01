// "The rexenv tour" — every feature in one ~3.5 min video, at the intro's pace:
// one beat per feature (~8 s), each beat's line starting as its screen loads.
// The beats reuse the tutorials' scenes and steps, cut to the one action that
// shows the feature. Jobs replay at 20 % length, the voice at +15 %.
//   node record-tour.mjs
import { openStage } from "./lib.mjs";

const NAME = "tour";

const narration = {
  open: { text: "This is rexenv: every feature, in about three and a half minutes.", say: "This is rex env. Every feature, in about three and a half minutes." },
  install: { text: "Install it with one command. On first launch, rexenv downloads its own servers and sets up local domains and HTTPS, once.", say: "Install it with one command. On first launch, rex env downloads its own servers, and sets up local domains and HTTPS, once." },
  wp: { text: "Create a WordPress site: pick WordPress, name it, install. The database, WordPress itself and HTTPS are done for you." },
  laravel: { text: "Laravel and plain PHP sites get a real database too, with the installer, the .env and the migrations handled.", say: "Laravel and plain PHP sites get a real database too, with the installer, the dot env file, and the migrations handled." },
  git: { text: "Or start from a Git repository. rexenv clones it, installs the dependencies, runs the migrations and builds, with the log live.", say: "Or start from a Git repository. Rex env clones it, installs the dependencies, runs the migrations and builds, with the log live." },
  folder: { text: "Already have a project folder? Link it where it is. rexenv detects the framework, serves the right folder, and never copies or moves a file.", say: "Already have a project folder? Link it where it is. Rex env detects the framework, serves the right folder, and never copies or moves a file." },
  import: { text: "Moving from Local, Valet or Herd? Import your sites where they live, with their databases." },
  blueprint: { text: "Save your starter plugins and theme as a blueprint. Any new WordPress site can then start from it, set up the same way." },
  wpmanage: { text: "The WordPress tab handles plugins and themes. Search WordPress.org, install and update, without wp-admin.", say: "The WordPress tab handles plugins and themes. Search WordPress dot org, install and update, without WP admin." },
  users: { text: "Users too. And Magic Login opens wp-admin in your browser, already signed in.", say: "Users too. And Magic Login opens WP admin in your browser, already signed in." },
  multisite: { text: "Turn any site into a multisite network. With subdomains, DNS and the certificate are handled for you." },
  repo: { text: "Plugins and themes can come from Git too. Fetch a repository, then install, build and activate in a few clicks, and pull and push from the same panel." },
  debug: { text: "Debugging is built in: Xdebug per site, a live debug log, and a terminal in the site's folder with its PHP and WP-CLI ready.", say: "Debugging is built in: Xdebug per site, a live debug log, and a terminal in the site's folder, with its PHP and WP CLI ready." },
  php: { text: "Each site runs its own PHP version, from 7.4 to 8.5, and rexenv tells you when one is past end of life.", say: "Each site runs its own PHP version, from seven four to eight five, and rex env tells you when one is past end of life." },
  servers: { text: "Nginx by default. Switch a site to FrankenPHP, or to Apache when it needs .htaccess.", say: "Nginx by default. Switch a site to FrankenPHP, or to Apache when it needs dot H T access." },
  db: { text: "MySQL, MariaDB, PostgreSQL and Redis run on their own ports, with a database browser built in." },
  domains: { text: "Every site gets a .rex domain with trusted HTTPS. Add more names, and the certificate is reissued.", say: "Every site gets a dot rex domain with trusted HTTPS. Add more names, and the certificate is reissued." },
  stop: { text: "Stop one site without touching the others, and start it again in one click." },
  mail: { text: "Every email your sites send lands in Mail, password resets and orders included. WordPress mail is caught even with an SMTP plugin set up." },
  share: { text: "Share a site with a temporary public link. rexenv warns you when it still accepts admin / admin.", say: "Share a site with a temporary public link. Rex env warns you when it still accepts admin, admin." },
  cli: { text: "The rex command does it all from the terminal: status, sites, PHP, databases, and a doctor.", say: "The rex command does it all from the terminal: status, sites, PHP, databases, and a doctor." },
  ai: { text: "AI agents like Claude Code get their own scratch sites. Your sites stay read-only until you say so, and every call is logged." },
  tray: { text: "Close the window and everything keeps running. The menu-bar icon has your sites, mail and tunnels." },
  update: { text: "And rexenv updates itself: it checks the signature and checksum, installs while your sites keep running, and asks before it restarts.", say: "And rex env updates itself. It checks the signature and checksum, installs while your sites keep running, and asks before it restarts." },
  close: { text: "That's rexenv. Your whole local stack: free, open source, and no Docker.", say: "That's rex env. Your whole local stack. Free, open source, and no Docker." },
};

const s = await openStage({ narration, speed: 0.2, rate: "+15%", gap: 90, switchSettle: 80 });
const { app, page } = s;
const tap = (loc, o = {}) => s.click(loc, { ms: 300, pre: 70, post: 120, ...o });
const T0 = Date.now();
const mark = (label) => console.log(`${((Date.now() - T0) / 1000).toFixed(1)}s ${label}`);
const dialog = app.locator("div.w-\\[516px\\]");
const card = app.locator('[data-probe="provision-card"]');

/** A beat: its headline and line first, then its screen loads under the voice. */
async function beat(key, h, sub, scene, path, ready, params) {
  await s.headline(h, sub);
  await s.say(key);
  if (path) await s.switchScene(scene, path, ready, params);
  await s.stage(() => window.stage.showCursor());
}
async function end(key) {
  mark(key);
  await s.voiceDone();
}
/** New site → a type → Continue. */
async function newSite(kind) {
  await tap(app.getByRole("button", { name: "New site" }));
  await tap(dialog.getByRole("button", { name: kind }), { fx: 0.3 });
  await tap(dialog.getByRole("button", { name: "Continue" }));
}
/** Frame the provision card's live log. */
async function watchJob() {
  await card.waitFor();
  await tap(card.getByText("Show log"));
  await s.camera(await s.containerOf(card.getByText("Hide log"), 470, 6), 0.8);
}

// ── Open ─────────────────────────────────────────────────────────────────
await s.title("The rexenv tour", "Every feature, in about three and a half minutes");
await s.say("open", 0);
await s.voiceDone();
await s.title(null);
await s.layout("split");

// ── Get started ──────────────────────────────────────────────────────────
await s.headline("Install<br><em>in one command.</em>", "Then a one-time setup: components, local domains, HTTPS.");
await s.say("install");
await s.term.show({ title: "Terminal — zsh", x: 640, y: 250, w: 1180, h: 560, font: 22 });
await s.term.type("curl -fsSL https://rexenv.rex.bd/install.sh | bash", { perChar: 18 });
await s.term.print(["rexenv: latest release: 0.8.11", "rexenv: downloading rexenv_0.8.11_universal.app.tar.gz"], 150);
await s.term.bar(700);
await s.term.print(["rexenv: checksum ok", "rexenv: installed rexenv 0.8.11: /Applications/rexenv.app"], 150);
await s.term.hide();
await s.term.clear();
await s.switchScene("onboarding", "/onboarding", "text=Get started");
await s.stage(() => window.stage.showCursor());
await tap(app.getByRole("button", { name: "Get started" }));
await app.getByText("Caddy (edge router)").waitFor();
await s.wait(500);
await tap(app.getByRole("button", { name: "Continue" }));
await tap(app.getByRole("button", { name: "Set up domains & SSL" }));
await app.getByText("Domains & SSL are ready").waitFor({ timeout: 20_000 });
await end("install");

// ── Create sites ─────────────────────────────────────────────────────────
await beat("wp", "WordPress<br><em>in one click.</em>", "Database, WordPress and HTTPS, done for you.", null, "/sites", "text=Agency Blog", { speed: "0.6" });
await newSite(/^WordPress/);
await tap(dialog.getByPlaceholder("my-site"));
await s.type("Client Shop", 30);
await tap(dialog.getByRole("button", { name: "Install WordPress" }));
await card.waitFor();
await s.camera(await s.box(card), 0.62);
await end("wp");

await beat("laravel", "Laravel<br><em>and plain PHP.</em>", "A real database, .env and migrations included.", "new-sites", "/sites", "text=Agency Blog", { speed: "0.6" });
await newSite(/^Laravel/);
await tap(dialog.getByPlaceholder("my-site"));
await s.type("Invoices", 30);
await tap(dialog.getByRole("button", { name: "Create site" }));
await watchJob();
await end("laravel");

await beat("git", "Start<br><em>from Git.</em>", "Clone, install, migrate and build, with the log live.", "git-site", "/sites", "text=Agency Blog");
await newSite(/^Laravel/);
await tap(dialog.getByRole("button", { name: "From Git" }));
await tap(dialog.getByPlaceholder("https://github.com/you/your-app"));
await s.type("acme/storefront", 14);
await tap(dialog.getByRole("button", { name: "Fetch" }));
await dialog.getByText("Branch or tag").waitFor();
await tap(dialog.getByRole("button", { name: "Create site" }));
await watchJob();
await end("git");

await beat("folder", "Link<br><em>a folder.</em>", "Served where it is. Nothing copied or moved.", "link-folder", "/sites", "text=Agency Blog");
await newSite(/^Laravel/);
await tap(dialog.getByRole("button", { name: "Existing folder" }));
await tap(dialog.getByRole("button", { name: "Choose folder…" }));
const detected = dialog.getByText("Detected", { exact: false }).first();
await detected.waitFor();
await s.camera(await s.containerOf(detected, 440, 50), 0.72);
await tap(dialog.getByRole("button", { name: "Link site" }));
await end("folder");

await beat("import", "Move in.<br><em>Nothing moves.</em>", "Import from Local, Valet &amp; Herd, databases copied.", "import", "/import", "text=Import from Valet, Herd or Local", { tookover: "1" });
await tap(app.locator('div:has(> span:has-text("Select sites to import")) > input[type=checkbox]'));
await tap(app.getByRole("button", { name: /^Import 3$/ }));
await app.getByText("Cancel after current").waitFor();
await s.camera(await s.containerOf(app.getByText("Cancel after current"), 500, 16), 0.62);
await end("import");

await beat("blueprint", "<em>Blueprints.</em>", "Your starter plugins and theme, for any new site.", "blueprints", "/settings", "text=Blueprints");
const bpName = app.getByPlaceholder("Blueprint name");
await s.box(bpName);
await s.camera(await s.containerOf(bpName, 560, 14), 0.72);
await tap(bpName);
await s.type("Shop starter", 30);
await tap(app.getByPlaceholder(/Plugin slugs/));
await s.type("woocommerce", 25);
await tap(app.getByPlaceholder(/Theme slugs/));
await s.type("storefront", 25);
await tap(app.getByRole("button", { name: "Add blueprint" }));
await app.getByText("Shop starter", { exact: true }).waitFor({ timeout: 8000 });
await end("blueprint");

// ── WordPress ────────────────────────────────────────────────────────────
await beat("wpmanage", "WordPress,<br><em>fully managed.</em>", "Plugins and themes without wp-admin.", "wp-manager", "/sites/1/wordpress", "text=Yoast SEO");
await tap(app.getByPlaceholder(/Search WordPress\.org or enter a slug/));
await s.type("woo", 45);
const hit = app.getByText("WooCommerce", { exact: true }).first();
await hit.waitFor();
await tap(hit);
await tap(app.getByRole("button", { name: /^Install/ }));
await app.getByText("Installed woocommerce").waitFor({ timeout: 15_000 });
const upd = app.getByTitle("Update to 26.2");
await tap(upd);
await app.getByText("Updated wordpress-seo").waitFor({ timeout: 15_000 });
await end("wpmanage");

await beat("users", "Users and<br><em>Magic Login.</em>", "Straight into wp-admin, already signed in.");
await tap(app.getByRole("button", { name: /^Users/ }));
const magic = app.getByRole("button", { name: /Magic Login/ }).first();
await magic.waitFor();
await s.spotlight(await s.box(magic));
await tap(magic);
await end("users");
await s.spotlight(null);

await beat("multisite", "<em>Multisite</em><br>in a click.", "Subdomain DNS and certificates handled for you.", "multisite", "/sites/1/wordpress", "text=Plugins");
await tap(app.getByRole("button", { name: /^Network/ }));
await tap(app.getByRole("button", { name: "Convert to multisite" }));
await tap(app.getByRole("button", { name: "Convert", exact: true }));
const slug = app.getByPlaceholder(/slug \(→/);
await slug.waitFor({ timeout: 10_000 });
await s.camera(await s.containerOf(slug, 560, 14), 0.7);
await tap(slug);
await s.type("shop", 40);
await tap(app.getByRole("button", { name: "Create", exact: true }));
await app.getByText("https://shop.agency-blog.rex/").waitFor({ timeout: 10_000 });
await end("multisite");

await beat("repo", "<em>Git-native</em><br>plugins.", "Install, build, activate. Then pull, push, stash.", "repo-panel", "/sites/1/wordpress", "text=Yoast SEO");
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
await end("repo");

await beat("debug", "Debug<br><em>in place.</em>", "Xdebug, the live debug.log, a terminal with WP-CLI.", "debugging", "/sites/1", "text=Overview", { wpdebug: "1" });
await tap(app.getByRole("button", { name: "Settings", exact: true }).last());
const xd = app.getByRole("switch", { name: "Toggle Xdebug" }).or(app.getByLabel("Toggle Xdebug"));
await tap(xd);
await app.getByText(/Xdebug on — set your IDE to listen on port 9003/).waitFor({ timeout: 10_000 });
await tap(app.getByRole("button", { name: "Logs", exact: true }).first());
await app.getByText(/Undefined array key "price"/).waitFor({ timeout: 10_000 });
await app.evaluate(() => window.__scene.logMore());
await s.wait(300);
await tap(app.getByRole("button", { name: "Terminal", exact: true }).first());
const xterm = app.locator(".xterm");
await xterm.waitFor();
const tb = await s.box(xterm);
await s.camera({ x: tb.x, y: tb.y - 30, w: Math.min(tb.w, 760), h: 250 }, 0.9);
await tap(xterm, { fx: 0.3, fy: 0.1 });
await s.type("wp plugin list", 18);
await page.keyboard.press("Enter");
await end("debug");

// ── The stack ────────────────────────────────────────────────────────────
await beat("php", "PHP<br><em>7.4 to 8.5.</em>", "Per site, with end-of-life warnings.", "php", "/sites/1", "text=Overview");
const picker = app.locator("select").first();
await picker.waitFor();
await s.moveTo(picker, { ms: 300 });
await s.stage(() => window.stage.press());
await picker.selectOption("7.4");
await tap(app.getByRole("button", { name: "Switch", exact: true }));
const eol = app.getByText(/stopped receiving upstream security fixes/);
await eol.waitFor({ timeout: 10_000 });
await s.camera(await s.containerOf(eol, 700, 12), 0.72);
await end("php");

await beat("servers", "Nginx, Apache<br><em>or FrankenPHP.</em>", "Chosen per site.", "servers", "/sites", "text=Landing Page");
await tap(app.getByText("Landing Page", { exact: true }));
const ws = app.locator("select").nth(1);
await ws.waitFor();
await s.moveTo(ws, { ms: 300 });
await s.stage(() => window.stage.press());
await ws.selectOption("frankenphp");
await tap(app.getByRole("button", { name: "Switch", exact: true }));
const fixed = app.getByText(/Fixed by FrankenPHP/);
await fixed.waitFor({ timeout: 10_000 });
await s.camera(await s.containerOf(fixed, 700, 12), 0.72);
await end("servers");

await beat("db", "Four<br><em>databases.</em>", "MySQL · MariaDB · PostgreSQL · Redis, each on its own port.", "database", "/databases", "text=127.0.0.1:13306");
const top = await s.containerOf(app.getByText(/127\.0\.0\.1:13306/), 600);
const bottom = await s.containerOf(app.getByText(/127\.0\.0\.1:16379/), 600);
await s.camera({ x: top.x, y: top.y, w: top.w, h: bottom.y + bottom.h - top.y }, 0.8);
await s.wait(900);
await s.camera(null);
await tap(app.getByRole("button", { name: "Browse" }).first());
await app.locator('iframe[title="Adminer"]').waitFor();
await end("db");

await beat("domains", "<em>.rex</em> domains,<br>real HTTPS.", "Add more names; the certificate follows.", "domains", "/sites/1", "text=Overview");
await tap(app.getByRole("button", { name: "Settings", exact: true }).last());
const add = app.getByPlaceholder("another.rex");
await s.box(add);
await s.camera(await s.containerOf(add, 520, 14), 0.72);
await tap(add);
await s.type("agencyblog.test", 30);
await tap(app.getByRole("button", { name: "Add domain" }));
await app.getByText(/Extra domain added — certificate re-issued/).waitFor({ timeout: 8000 });
await end("domains");

await beat("stop", "Stop<br><em>one site.</em>", "The others keep serving.", "site-stop", "/sites", "text=Shop Staging");
await tap(app.getByText("Shop Staging", { exact: true }));
await tap(app.getByRole("button", { name: "Stop site" }));
const stopped = app.getByText("Stopped by you").first();
await stopped.waitFor({ timeout: 8000 });
await s.spotlight(await s.box(stopped));
await s.wait(700);
await s.spotlight(null);
await tap(app.getByRole("button", { name: "Start site" }));
await app.getByText("shop-staging.rex is serving again.").waitFor({ timeout: 8000 });
await end("stop");

// ── Everyday ─────────────────────────────────────────────────────────────
await beat("mail", "Mail,<br><em>caught.</em>", "Every email your sites send, in one inbox.", "mail", "/mail", "text=Mailpit");
await app.evaluate(() => window.__scene.deliver());
const resetRow = app.getByRole("button", { name: /message: .*Password Reset/ });
await resetRow.waitFor({ timeout: 8000 });
await tap(resetRow);
await s.wait(600);
await tap(app.getByRole("button", { name: /message: .*New order #1042/ }));
await end("mail");

await beat("share", "Share<br><em>a public link.</em>", "Temporary, and checked for admin / admin.", "share", "/tunnels", "text=Share publicly");
await tap(app.getByRole("switch", { name: "Start sharing Agency Blog" }));
const warn = app.getByText(/still accepts the default/);
await warn.waitFor({ timeout: 15_000 });
await s.spotlight(await s.containerOf(warn, 480));
await end("share");
await s.spotlight(null);

await s.headline("The <em>rex</em><br>command.", "Everything the app does, from any terminal.");
await s.say("cli");
await s.stage(() => window.stage.showWindow(false));
await s.term.show({ title: "Terminal — zsh", x: 640, y: 250, w: 1180, h: 560, font: 22 });
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
  50,
);
await s.wait(500);
await s.term.type("rex site php shop.rex 8.3", { perChar: 25 });
await s.term.print(["switching shop.rex to PHP 8.3…"], 400);
await s.term.print(['<span class="ok">✓</span> shop.rex — PHP 8.3 on nginx'], 100);
await end("cli");
await s.term.hide();
await s.term.clear();

await beat("ai", "AI agents,<br><em>with guardrails.</em>", "Scratch sites of their own. Your sites stay read-only until you say so.", "mcp-agent", "/settings?section=agents", "text=Enable the MCP endpoint");
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
const lastRow = app.getByText("scratch_create_site", { exact: true });
await s.box(lastRow);
await s.camera(await s.union([fold, lastRow], 14), 0.62);
await end("ai");

// The menu bar is drawn on the stage (it is native, outside the webview); its
// items and order are core/tray.rs's, as in the tray tutorial.
const MENU = ["~All running · 7 services", "-", "~Start all", "Stop all", "-", "Sites\t›", "All sites…", "Services", "Databases", "Mail", "Tunnels", "-", "MCP server", "-", "About rexenv", "Open rexenv", "Quit rexenv"];
const SITES = ["landing.rex", "booking-api.rex", "shop-staging.rex", "agency-blog.rex"];
const I = { sites: 3, mail: 7, tunnels: 8, open: 11 };
const st = (fn, a) => page.evaluate(fn, a);
const glideTo = async (pt, ms = 500) => {
  if (pt) await st(([x, y, m]) => window.stage.scursor.moveTo(x, y, m), [pt.x, pt.y, ms]);
};
await s.headline("Close it.<br><em>It keeps running.</em>", "Sites, mail and tunnels from the menu bar.");
await s.say("tray");
await s.switchScene(null, "/sites", "text=Agency Blog");
await st(() => window.stage.tray.bar(true));
await s.stage(() => window.stage.showCursor(false));
await st(() => window.stage.scursor.show(true));
await glideTo(await st(() => window.stage.tray.closeButtonAt()), 600);
await st(() => window.stage.scursor.press());
await s.stage(() => window.stage.showWindow(false));
await s.wait(400);
await glideTo(await st(() => window.stage.tray.iconAt()), 600);
await st(() => window.stage.scursor.press());
await st((m) => window.stage.tray.open(m), MENU);
await s.wait(300);
await glideTo(await st((i) => window.stage.tray.hl(i), I.sites), 400);
await st(([m, sub]) => window.stage.tray.open(m, { at: 3, items: sub }), [MENU, SITES]);
await st((i) => window.stage.tray.hl(i), I.sites);
await glideTo(await st(() => window.stage.tray.hl(1, 1)), 400);
await s.wait(500);
await st((m) => window.stage.tray.open(m), MENU);
await glideTo(await st((i) => window.stage.tray.hl(i), I.mail), 400);
await s.wait(300);
await glideTo(await st((i) => window.stage.tray.hl(i), I.tunnels), 300);
await end("tray");
await st(() => window.stage.tray.close());
await st(() => window.stage.scursor.show(false));
await st(() => window.stage.tray.bar(false));

await beat("update", "Updates<br><em>itself.</em>", "Signature and checksum checked; it asks before restarting.", "self-update", "/settings?section=about", "text=Install rexenv 0.8.11");
const install = app.getByRole("button", { name: /^Install rexenv 0\.8\.11/ });
const consent = app.getByText(/checks its signature and checksum/);
await s.box(consent);
await s.camera(await s.containerOf(consent, 560, 14), 0.72);
await s.wait(200);
await tap(install);
await app.getByText("rexenv 0.8.11 is installed").waitFor({ timeout: 30_000 });
await end("update");

// ── Close ────────────────────────────────────────────────────────────────
await s.headline(null);
await s.camera(null);
await s.stage(() => window.stage.showWindow(false));
await s.stage(() => window.stage.showCursor(false));
await s.title(
  "rexenv",
  "Free & open source · macOS · Windows · Linux",
  false,
  // Recorded on macOS: what the other builds lack is said on screen, not left
  // for a viewer to find (a caption would sit under the title card).
  '<div><span>macOS · Linux</span>curl -fsSL https://rexenv.rex.bd/install.sh | bash</div><div><span>Windows</span>irm https://rexenv.rex.bd/install.ps1 | iex</div>' +
    "<small>Shown on macOS. Apache, MariaDB, Redis and Xdebug are macOS-only for now.</small>",
);
await s.say("close");
await s.voiceDone(1800);

console.log(`saved ${await s.save(NAME)}`);
