// Tier 3 · "Blueprints": save a WordPress setup in Settings, then start a new
// site from it — the provision job's "applying blueprint" phase installs it.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Building the same kind of WordPress site again and again? Save the setup as a blueprint." },
  create: { text: "In Settings → Blueprints, give it a name, list the plugins and themes, and choose single site or multisite." },
  use: { text: "Then in New site, pick it under Start from blueprint." },
  apply: { text: "rexenv installs WordPress, then applies the blueprint — every plugin and theme, activated the way you set.", say: "Rex env installs WordPress, then applies the blueprint. Every plugin and theme, activated the way you set." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "blueprints", path: "/settings", ready: "text=Blueprints" });
const { app } = s;

await s.title("Blueprints", "Your WordPress starter kit — plugins, themes, multisite, in one pick");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

await s.say("create");
await s.caption("Settings → <b>Blueprints</b> → Add blueprint");
const nameIn = app.getByPlaceholder("Blueprint name");
await s.box(nameIn);
await s.camera(await s.containerOf(nameIn, 560, 14), 0.75);
await s.click(nameIn, { ms: 800 });
await s.type("Shop starter", 80);
await s.click(app.getByPlaceholder(/Plugin slugs/), { ms: 600 });
await s.type("woocommerce", 70);
await s.click(app.getByPlaceholder(/Theme slugs/), { ms: 600 });
await s.type("storefront", 70);
await s.click(app.getByRole("button", { name: "Add blueprint" }), { ms: 700 });
await app.getByText("Shop starter", { exact: true }).waitFor({ timeout: 8000 });
await s.voiceDone(200);
await s.camera(null);

await s.say("use");
await s.caption("New site → WordPress → <b>Start from blueprint</b>");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 800 });
await s.click(app.getByRole("button", { name: "New site" }), { ms: 800 });
const dialog = app.locator("div.w-\\[516px\\]");
await s.click(dialog.getByRole("button", { name: /^WordPress/ }), { fx: 0.3, ms: 700 });
await s.click(dialog.getByRole("button", { name: "Continue" }), { ms: 500 });
const bp = dialog.locator("select").first();
await s.moveTo(bp, { ms: 700 });
await s.stage(() => window.stage.press());
const id = await bp.evaluate((el) => [...el.options].find((o) => o.textContent === "Shop starter")?.value);
await bp.selectOption(id);
await s.click(dialog.getByPlaceholder("my-site"), { ms: 700 });
await s.type("Candle Shop", 80);
await s.voiceDone(200);

await s.say("apply");
await s.caption("…then <b>applying blueprint</b>: WooCommerce + Storefront");
await s.click(dialog.getByRole("button", { name: "Install WordPress" }), { ms: 700 });
const card = dialog.locator('[data-probe="provision-card"]');
await card.waitFor();
await s.click(card.getByText("Show log"), { ms: 600 });
await s.camera(await s.containerOf(card.getByText("Hide log"), 470, 6), 0.85);
await dialog.waitFor({ state: "detached", timeout: 60_000 });
await s.camera(null);
await s.voiceDone(200);
const row = app.getByText("candle-shop.rex", { exact: true });
await row.waitFor();
await s.spotlight(await s.containerOf(row, 700));
await s.wait(1600);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("blueprints")}`);
