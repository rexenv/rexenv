// Tier 1 · "Share a site": Tunnels → share Agency Blog → the public link goes
// live → the admin/admin warning → copy the link → stop sharing.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "Need to show a client your local site? Share it with a public HTTPS link — no deploy.",
  },
  open: {
    text: "Open Tunnels. Sharing makes a free, temporary link through Cloudflare, straight to the site on your Mac.",
  },
  start: { text: "Flip the switch next to the site." },
  live: {
    text: "A few seconds later there's a public link, and rexenv checks it's reachable — here it's Live.",
    say: "A few seconds later there's a public link, and rex env checks that it's reachable. Here, it's live.",
  },
  warn: {
    text: "This WordPress site still accepts admin / admin, so rexenv warns you: change the password, or stop sharing.",
    say: "This WordPress site still accepts admin, admin. So rex env warns you: change the password, or stop sharing.",
  },
  copy: {
    text: "Copy the link and send it. While it's shared, WordPress uses the public address — and the site keeps working on .rex.",
    say: "Copy the link, and send it. While it's shared, WordPress uses the public address, and the site keeps working on dot rex.",
  },
  stop: {
    text: "Links last only while sharing is on. Stop sharing — or quit rexenv — and the link is gone.",
    say: "Links last only while sharing is on. Stop sharing, or quit rex env, and the link is gone.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "share" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Share a site", "A public HTTPS link to your local site — free, no deploy");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Tunnels ──────────────────────────────────────────────────────────────
await s.say("open");
await s.caption("Open <b>Tunnels</b>");
await s.click(app.getByRole("link", { name: /^Tunnels/ }), { ms: 1000 });
await app.getByText("free, temporary public link").waitFor();
await s.wait(300);
await s.camera(await s.containerOf(app.getByText("free, temporary public link"), 500, 10), 0.68);
await s.voiceDone(200);
await s.camera(null);

// ── Start ────────────────────────────────────────────────────────────────
const toggle = app.getByRole("switch", { name: "Start sharing Agency Blog" });
await s.say("start");
await s.caption("Share <code>agency-blog.rex</code>");
await s.click(toggle, { ms: 1000 });
await s.voiceDone(100);

const url = app.getByText("https://harbor-lemon-quiet-meadow.trycloudflare.com");
await url.waitFor({ timeout: 15_000 });
await s.say("live", 100);
await s.caption("A public HTTPS link — checked every 30 s");
// The card moves to "Shared now" and grows its warning as the start lands:
// frame it only once it has settled (Live).
await app.getByText("Live", { exact: true }).waitFor({ timeout: 15_000 });
await s.wait(300);
await s.camera(await s.containerOf(url, 760, 10), 0.8);
await s.moveTo(app.getByText("Live", { exact: true }), { ms: 800 });
await s.voiceDone(200);

// ── The admin/admin warning ──────────────────────────────────────────────
await s.say("warn");
await s.caption("Still on <code>admin / admin</code>? rexenv says so");
const warning = app.getByText(/still accepts the default/);
await s.spotlight(await s.containerOf(warning, 480));
await s.moveTo(warning, { ms: 800 });
await s.voiceDone(200);
await s.spotlight(null);

// ── Copy ─────────────────────────────────────────────────────────────────
await s.say("copy");
await s.caption("Copy the link, send it to your client");
await s.click(app.getByRole("button", { name: "Copy" }), { ms: 800 });
await s.voiceDone(300);

// ── Stop ─────────────────────────────────────────────────────────────────
await s.say("stop");
await s.caption("<b>Stop sharing</b> — the link goes dead");
await s.click(app.getByRole("button", { name: "Stop sharing" }), { ms: 800 });
await url.waitFor({ state: "detached", timeout: 15_000 });
await s.camera(null);
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("share")}`);
