// Tier 2 · "Serve an existing folder": New site → Existing folder → Choose
// folder → detected Laravel, serving public/ → Link site → the external tag.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Already have a project folder? Serve it right where it is — nothing is copied or moved." },
  choose: { text: "Click New site, pick Existing folder, and choose your project." },
  detect: {
    text: "rexenv looks inside and knows what it is: a Laravel app, served from its public folder. It installs nothing into it.",
    say: "Rex env looks inside and knows what it is: a Laravel app, served from its public folder. It installs nothing into it.",
  },
  keep: { text: "The folder stays yours — keep your own git workflow, and deleting the site never deletes the folder." },
  link: { text: "Click Link site — seconds later it's served at crm.rex, with HTTPS.", say: "Click Link site. Seconds later it's served at crm dot rex, with HTTPS." },
  tag: { text: "Linked sites carry an “external” tag, so you always know which folders are your own." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "link-folder" });
const { app } = s;

await s.title("Serve an existing folder", "Your project, where it already lives");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Choose ───────────────────────────────────────────────────────────────
await s.say("choose");
await s.caption("New site → <b>Existing folder</b> → Choose folder…");
await s.click(app.getByRole("button", { name: "New site" }), { ms: 1000 });
const dialog = app.locator("div.w-\\[516px\\]");
await s.click(dialog.getByRole("button", { name: /^Laravel/ }), { fx: 0.3 });
await s.wait(300);
await s.click(dialog.getByRole("button", { name: "Continue" }));
await s.wait(300);
await s.click(dialog.getByRole("button", { name: "Existing folder" }), { ms: 700 });
await s.click(dialog.getByRole("button", { name: "Choose folder…" }), { ms: 700 });
await s.voiceDone(200);

// ── Detected ─────────────────────────────────────────────────────────────
const detected = dialog.getByText("Detected", { exact: false }).first();
await detected.waitFor();
await s.say("detect");
await s.caption("Detected <b>Laravel</b> · serving <code>public/</code> · adopted as-is");
await s.camera(await s.containerOf(detected, 440, 50), 0.8);
await s.moveTo(detected, { ms: 800 });
await s.voiceDone(200);

await s.say("keep");
await s.caption("Your folder, your git — never deleted with the site");
await s.moveTo(dialog.getByText(/The folder stays where it is/), { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── Link ─────────────────────────────────────────────────────────────────
await s.say("link");
await s.caption("<b>Link site</b>");
await s.click(dialog.getByRole("button", { name: "Link site" }), { ms: 800 });
await dialog.waitFor({ state: "detached", timeout: 20_000 });
await s.voiceDone(200);

// ── The external tag ─────────────────────────────────────────────────────
const crm = app.getByText("crm.rex", { exact: true });
await crm.waitFor();
await s.say("tag");
await s.caption("<code>external</code> — served from your own folder");
const rowRect = await s.containerOf(crm, 700);
await s.spotlight(rowRect);
await s.moveTo(app.getByText("external", { exact: true }).first(), { ms: 900 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("link-folder")}`);
