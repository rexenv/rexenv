// Tier 1 · "WordPress manager": the WordPress tab → search wp.org → install
// WooCommerce (activated) → update Yoast → themes → users → Magic Login.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Plugins, themes and users for every WordPress site — right inside rexenv.",
    say: "Plugins, themes and users for every WordPress site, right inside rex env.",
  },
  tab: { text: "Open a site's WordPress tab: everything installed, with updates flagged." },
  search: { text: "Search WordPress.org, pick a plugin, and click Install — with Activate ticked, it's switched on too." },
  install: {
    text: "rexenv runs WP-CLI for you, and shows each step as it happens.",
    say: "Rex env runs WP CLI for you, and shows each step as it happens.",
  },
  update: { text: "Yoast SEO has an update. One click — and you watch it download, unpack and install." },
  themes: { text: "Themes work the same way: install, activate, update." },
  users: { text: "And users: add one, change a role, or set a new password." },
  magic: {
    text: "Magic Login opens wp-admin in your browser, already signed in — through a one-time link that only works on your own Mac.",
    say: "Magic Login opens WP admin in your browser, already signed in. Through a one-time link, that only works on your own Mac.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "wp-manager" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("WordPress manager", "Plugins, themes & users — without opening wp-admin");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── The WordPress tab ────────────────────────────────────────────────────
await s.say("tab");
await s.caption("Site → <b>WordPress</b> tab");
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 1000 });
await s.click(app.getByRole("button", { name: "WordPress", exact: true }).first(), { ms: 800 });
const yoast = app.getByText("Yoast SEO", { exact: true });
await yoast.waitFor();
await s.wait(600);
await s.moveTo(app.getByText("update", { exact: true }).first(), { ms: 900 });
await s.voiceDone(200);

// ── Search + install ─────────────────────────────────────────────────────
await s.say("search");
await s.caption("Search WordPress.org → <b>Install</b>");
const search = app.getByPlaceholder(/Search WordPress\.org or enter a slug/);
const bar = await s.containerOf(search, 560, 10);
await s.camera({ ...bar, h: bar.h + 230 }, 0.8);
await s.click(search, { ms: 800 });
await s.type("woo", 140);
const hit = app.getByText("WooCommerce", { exact: true }).first();
await hit.waitFor({ timeout: 8000 });
await s.wait(700);
await s.click(hit, { ms: 800 });
await s.wait(400);
await s.moveTo(app.getByText("Activate", { exact: true }).first(), { ms: 600 });
await s.wait(300);
await s.click(app.getByRole("button", { name: /^Install/ }), { ms: 700 });
await s.voiceDone(100);

await s.say("install", 100);
await s.caption("WP-CLI, step by step");
const card = app.getByText(/plugin install · woocommerce/);
await card.waitFor();
await s.camera(await s.containerOf(card, 560, 10), 0.75);
await app.getByText("Installed woocommerce").waitFor({ timeout: 20_000 });
await s.voiceDone(300);
// A successful install card clears itself after 3 s — and the list moves up
// under it. Aim at anything below only once it has gone.
await card.waitFor({ state: "detached", timeout: 8000 });
await s.camera(null);

// ── Update ───────────────────────────────────────────────────────────────
await s.say("update");
await s.caption("One-click updates");
const upd = app.getByTitle("Update to 26.2");
await s.box(upd);
await s.camera(await s.containerOf(upd, 560, 8), 0.7);
await s.click(upd, { ms: 900 });
await app.getByText("Updated wordpress-seo").waitFor({ timeout: 20_000 });
await s.voiceDone(300);
await s.camera(null);

// ── Themes ───────────────────────────────────────────────────────────────
await s.say("themes");
await s.caption("<b>Themes</b>");
await s.click(app.getByRole("button", { name: /^Themes/ }), { ms: 900 });
await app.getByText("Twenty Twenty-Five").waitFor();
await s.moveTo(app.getByText("Twenty Twenty-Four"), { ms: 900 });
await s.voiceDone(200);

// ── Users ────────────────────────────────────────────────────────────────
await s.say("users");
await s.caption("<b>Users</b>");
await s.click(app.getByRole("button", { name: /^Users/ }), { ms: 900 });
await app.getByText("nadia", { exact: true }).waitFor();
await s.moveTo(app.getByText("nadia", { exact: true }), { ms: 900 });
await s.voiceDone(200);

// ── Magic Login ──────────────────────────────────────────────────────────
await s.say("magic");
await s.caption("<b>Magic Login</b> — signed in to wp-admin, one click");
const magic = app.getByRole("button", { name: /Magic Login/ }).first();
await s.spotlight(await s.box(magic));
await s.click(magic, { ms: 1000 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("wp-manager")}`);
