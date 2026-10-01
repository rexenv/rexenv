// Tier 2 · "WordPress multisite": WordPress → Network → convert to a subdomain
// network → add sub-sites → visit / Magic Login.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Run a whole WordPress network from one install — multisite, with subdomains that just work." },
  network: { text: "Open the site's WordPress tab, then Network. Choose subdomains or subdirectories." },
  dns: {
    text: "With subdomains, DNS, the HTTPS certificate and routing are handled for you — every .rex name already points at your Mac.",
    say: "With subdomains, DNS, the HTTPS certificate and routing are handled for you. Every dot rex name already points at your Mac.",
  },
  convert: { text: "Convert to multisite. rexenv updates wp-config and turns the site into a network.", say: "Convert to multisite. Rex env updates WP config, and turns the site into a network." },
  create: { text: "Now add a sub-site: type a slug, and shop.agency-blog.rex exists — with HTTPS, nothing to set up.", say: "Now add a sub-site. Type a slug, and shop dot agency blog dot rex exists. With HTTPS, and nothing to set up." },
  manage: { text: "Visit any sub-site or sign straight in, and manage network plugins, themes and super admins below." },
  dialog: { text: "You can also switch multisite on when you create a WordPress site — it's one toggle in New site." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "multisite" });
const { app } = s;

await s.title("WordPress multisite", "A whole network from one install");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Network tab ──────────────────────────────────────────────────────────
await s.say("network");
await s.caption("Site → WordPress → <b>Network</b>");
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 1000 });
await s.click(app.getByRole("button", { name: "WordPress", exact: true }).first(), { ms: 800 });
await s.click(app.getByRole("button", { name: /^Network/ }), { ms: 800 });
const convertBtn = app.getByRole("button", { name: "Convert to multisite" });
await convertBtn.waitFor();
await s.moveTo(app.getByText("Subdirectory", { exact: true }), { ms: 800 });
await s.wait(500);
await s.moveTo(app.getByText("Subdomain", { exact: true }), { ms: 700 });
await s.voiceDone(200);

await s.say("dns");
await s.caption("<code>*.agency-blog.rex</code> — DNS, HTTPS and routing, automatic");
const note = app.getByText(/DNS, HTTPS certificate and routing are handled automatically/);
await s.camera(await s.containerOf(note, 500, 16), 0.75);
await s.moveTo(note, { ms: 800 });
await s.voiceDone(200);

// ── Convert ──────────────────────────────────────────────────────────────
await s.say("convert");
await s.caption("<b>Convert to multisite</b>");
await s.click(convertBtn, { ms: 800 });
await s.camera(null);
await s.click(app.getByRole("button", { name: "Convert", exact: true }), { ms: 800 });
await app.getByText("Converted to subdomain multisite").waitFor({ timeout: 10_000 });
await s.voiceDone(200);

// ── A sub-site ───────────────────────────────────────────────────────────
await s.say("create");
await s.caption("Add a sub-site: <code>shop</code> → <code>shop.agency-blog.rex</code>");
const slug = app.getByPlaceholder(/slug \(→/);
await slug.waitFor();
await s.camera(await s.containerOf(slug, 560, 14), 0.7);
await s.click(slug, { ms: 800 });
await s.type("shop", 120);
await s.click(app.getByRole("button", { name: "Create", exact: true }), { ms: 700 });
const sub = app.getByText("https://shop.agency-blog.rex/");
await sub.waitFor({ timeout: 10_000 });
await s.spotlight(await s.containerOf(sub, 500));
await s.voiceDone(200);
await s.spotlight(null);

await s.say("manage");
await s.caption("Visit · Magic Login · network plugins &amp; themes");
await s.moveTo(app.getByRole("button", { name: "Magic Login" }).last(), { ms: 900 });
await s.camera(null);
await s.wait(400);
await s.moveTo(app.getByText("Super admins"), { ms: 1000 });
await s.voiceDone(200);

// ── The New site toggle ──────────────────────────────────────────────────
await s.say("dialog");
await s.caption("Or from the start: New site → <b>Multisite network</b>");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 800 });
await s.click(app.getByRole("button", { name: "New site" }), { ms: 800 });
const dialog = app.locator("div.w-\\[516px\\]");
await s.click(dialog.getByRole("button", { name: /^WordPress/ }), { fx: 0.3, ms: 600 });
await s.click(dialog.getByRole("button", { name: "Continue" }), { ms: 500 });
const toggle = dialog.getByRole("switch", { name: "Enable multisite" }).or(dialog.getByLabel("Enable multisite"));
await s.box(toggle);
await s.camera(await s.containerOf(toggle, 440, 12), 0.7);
await s.click(toggle, { ms: 700 });
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("multisite")}`);
