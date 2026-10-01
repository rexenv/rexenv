// Tier 1 · "Mail catching": the Mail screen grouped by site → a WordPress
// password-reset mail arrives → read it → an HTML order mail → mark all read.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Every email your local sites send is caught by rexenv — nothing reaches a real inbox.",
    say: "Every email your local sites send is caught by rex env. Nothing reaches a real inbox.",
  },
  screen: { text: "Open Mail. Messages are grouped by the site that sent them — WordPress, Laravel or plain PHP." },
  arrive: { text: "Say you click “Lost your password” on Shop Staging. Seconds later, the reset email is here." },
  open: { text: "Open it to read the message and the reset link — no real mailbox needed." },
  html: { text: "HTML emails render too, like this WooCommerce order. Scripts are stripped, and links open in your browser." },
  smtp: {
    text: "WordPress sites are caught even when an SMTP plugin is configured, so a local site doesn't email real people.",
    say: "WordPress sites are caught even when an SMTP plugin is configured, so a local site doesn't email real people.",
  },
  read: { text: "Mark all read, and the next email your site sends stands out." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "mail", path: "/mail", ready: "text=Mailpit", timezoneId: "America/Los_Angeles" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Mail catching", "Every email your sites send — caught on your Mac, never delivered");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── The inbox ────────────────────────────────────────────────────────────
await s.say("screen");
await s.caption("Grouped by site");
await s.moveTo(app.getByText("Shop Staging", { exact: true }).first(), { ms: 1000 });
await s.wait(900);
await s.moveTo(app.getByText("Booking API", { exact: true }).first(), { ms: 900 });
await s.voiceDone(200);

// ── A reset mail arrives ─────────────────────────────────────────────────
await app.evaluate(() => window.__scene.deliver());
await s.say("arrive");
await s.caption("“Lost your password?” → the reset email, caught");
const resetRow = app.getByRole("button", { name: /message: .*Password Reset/ });
await resetRow.waitFor({ timeout: 8000 });
await s.spotlight(await s.box(resetRow));
await s.moveTo(resetRow, { ms: 900 });
await s.voiceDone(200);
await s.spotlight(null);

await s.say("open");
await s.caption("Read it right here");
await s.click(resetRow);
await s.wait(500);
const body = app.getByText("To reset your password, visit the following address:");
await body.waitFor();
await s.camera(await s.containerOf(body, 380, 20), 0.8);
await s.moveTo(app.getByText(/wp-login\.php\?/), { ms: 900 });
await s.voiceDone(300);
await s.camera(null);

// ── HTML mail ────────────────────────────────────────────────────────────
await s.say("html");
await s.caption("HTML email, rendered safely");
await s.click(app.getByRole("button", { name: /message: .*New order #1042/ }), { ms: 900 });
await s.wait(600);
// The preview is its own iframe: aim at the frame element (app coordinates),
// never at an element inside it (those are the preview's own coordinates).
await s.moveTo(app.locator('iframe[title="HTML preview"]'), { ms: 900, fx: 0.3, fy: 0.62 });
await s.voiceDone(300);

await s.say("smtp");
await s.caption("SMTP plugins are caught too");
await s.moveTo(app.getByText(/Mailpit · :/), { ms: 900 });
await s.voiceDone(300);

// ── Mark all read ────────────────────────────────────────────────────────
await s.say("read");
await s.caption("<b>Mark all read</b> — the next email stands out");
await s.click(app.getByRole("button", { name: "Mark all read" }), { ms: 900 });
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("mail")}`);
