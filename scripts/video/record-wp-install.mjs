// Pilot: "One-click WordPress" — New site → WordPress → name it → Install,
// the provision card streaming the real phases, the site landing in the list.
// Narrated: the scene waits on each line, so the pacing follows the voice.
import { openStage } from "./lib.mjs";

// `text` is the subtitle; `say` is what the voice reads when it must differ.
const narration = {
  intro: {
    text: "Here's how to create a WordPress site in rexenv — in one step.",
    say: "Here's how to create a WordPress site in rex env, in one step.",
  },
  newSite: { text: "From the Sites screen, click New site." },
  type: { text: "Choose WordPress, then Continue." },
  name: {
    text: "Give your site a name — rexenv fills in the .rex domain for you.",
    say: "Give your site a name. Rex env fills in the dot rex domain for you.",
  },
  stack: { text: "PHP version, web server and database are already set. Keep the defaults, or pick your own." },
  admin: {
    text: "Next, your WordPress admin: admin and admin by default, or generate a strong password. Add your email too.",
  },
  install: { text: "Now click Install WordPress." },
  provision: {
    text: "rexenv starts the database, downloads WordPress, writes the config and installs it — and you can watch every step.",
    say: "Rex env starts the database, downloads WordPress, writes the config, and installs it. And you can watch every step.",
  },
  done: {
    text: "That's it — your site is live at client-shop.rex, with a trusted HTTPS certificate.",
    say: "That's it. Your site is live at client shop dot rex, with a trusted HTTPS certificate.",
  },
  outro: {
    text: "rexenv — your whole local stack, without Docker.",
    say: "Rex env. Your whole local stack, without Docker.",
  },
};

const s = await openStage({ narration });
const { app } = s;

// ── Title card ────────────────────────────────────────────────────────────
await s.title("One-click WordPress", "A fresh local WordPress site — HTTPS, database and admin — in one step");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── New site ──────────────────────────────────────────────────────────────
await s.say("newSite");
await s.caption("Click <b>New site</b>");
await s.click(app.getByRole("button", { name: "New site" }), { ms: 1100 });

const dialog = app.locator("div.w-\\[516px\\]");
await s.say("type");
await s.caption("Choose <b>WordPress</b>");
await s.click(dialog.getByRole("button", { name: /^WordPress/ }), { fx: 0.3 });
await s.wait(300);
await s.click(dialog.getByRole("button", { name: "Continue" }));

// ── Name ──────────────────────────────────────────────────────────────────
const selects = dialog.locator("select");
const nameArea = await s.union([dialog.getByRole("button", { name: "New folder" }), selects.nth(2)], 22);
await s.say("name");
await s.caption("Give it a name — the <code>.rex</code> domain fills itself in");
await s.camera(nameArea, 0.78);
await s.click(dialog.getByPlaceholder("my-site"));
await s.type("Client Shop");

await s.say("stack");
await s.caption("PHP, web server and database — keep the defaults or pick your own");
await s.moveTo(selects.nth(0), { ms: 700 });
await s.wait(900);
await s.moveTo(selects.nth(1), { ms: 600 });
await s.wait(900);
await s.moveTo(selects.nth(2), { ms: 600 });

// ── Admin ─────────────────────────────────────────────────────────────────
const email = dialog.getByPlaceholder("you@example.com");
const adminArea = await s.union([dialog.getByText("WordPress install", { exact: true }), dialog.getByPlaceholder("••••••••")], 22);
await s.say("admin");
await s.caption("Your WordPress admin — <code>admin</code> / <code>admin</code> by default, or Generate a strong one", "top");
await s.camera(adminArea, 0.78);
await s.moveTo(dialog.getByRole("button", { name: "Generate" }), { ms: 800 });
await s.wait(900);
await s.click(email);
await s.type("hello@client-shop.rex", 55);

// ── Install ───────────────────────────────────────────────────────────────
await s.voiceDone();
await s.camera(null);
await s.say("install");
await s.caption("Then <b>Install WordPress</b>", "top");
await s.click(dialog.getByRole("button", { name: "Install WordPress" }), { ms: 800 });

const card = dialog.locator('[data-probe="provision-card"]');
await card.waitFor();
await s.say("provision", 100);
await s.caption("rexenv starts the database, downloads WordPress and installs it — live");
await s.moveTo(card, { fx: 0.55, fy: 1.6, ms: 600 });
await s.camera(await s.box(card), 0.62);
// The dialog closes itself when the job settles ok.
await dialog.waitFor({ state: "detached", timeout: 30_000 });
await s.voiceDone(0);
await s.caption(null);
await s.camera(null);

// ── Done ──────────────────────────────────────────────────────────────────
await app.getByText("client-shop.rex", { exact: true }).waitFor();
const row = await app.evaluate(() => {
  let el = [...document.querySelectorAll("span, div")].find((e) => e.childElementCount === 0 && e.textContent === "client-shop.rex");
  while (el && el.getBoundingClientRect().width < 700) el = el.parentElement;
  const r = el.getBoundingClientRect();
  return { x: r.x, y: r.y, w: r.width, h: r.height };
});
await s.spotlight(row);
await s.say("done");
await s.caption("Done — <code>https://client-shop.rex</code> is live, with a trusted HTTPS certificate");
await s.moveTo(app.getByText("Client Shop", { exact: true }), { ms: 900 });
await s.voiceDone(900);
await s.caption(null);
await s.spotlight(null);
await s.stage(() => window.stage.showCursor(false));
await s.wait(300);
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("wp-install")}`);
