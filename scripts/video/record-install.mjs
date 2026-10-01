// Tier 1 · "Install rexenv": the one-command install in Terminal, then the
// four-step first run (components, domains & SSL, done) to the empty Sites
// screen. The terminal lines are install.sh's own `say` lines (rexenv/homebrew-tap).
import { openStage } from "./lib.mjs";

const VERSION = "0.8.10";

const narration = {
  intro: {
    text: "Let's install rexenv and get it ready for your first site — it takes about a minute.",
    say: "Let's install rex env and get it ready for your first site. It takes about a minute.",
  },
  terminal: { text: "On a Mac, it's one command. Open Terminal and paste it." },
  installed: {
    text: "It downloads rexenv, checks the checksum, puts it in Applications and starts it. No Docker, no Homebrew.",
    say: "It downloads rex env, checks the checksum, puts it in Applications, and starts it. No Docker, and no Homebrew.",
  },
  welcome: {
    text: "rexenv opens with a short setup. Click Get started.",
    say: "Rex env opens with a short setup. Click Get started.",
  },
  components: {
    text: "rexenv ships its own servers — Caddy, Nginx, MySQL, PHP, Mailpit. They download in the background, and nothing touches your system.",
    say: "Rex env ships its own servers: Caddy, Nginx, MySQL, PHP and Mailpit. They download in the background, and nothing touches your system setup.",
  },
  domains: {
    text: "Next, local domains and HTTPS: a private certificate authority, and .rex domains that point to your Mac.",
    say: "Next, local domains and HTTPS. Rex env adds a private certificate authority, and points dot rex domains to your Mac. Nothing leaves your computer.",
  },
  password: {
    text: "macOS asks for your password twice — once for the DNS resolver, once for the keychain.",
    say: "Mac OS asks for your password twice. Once for the DNS resolver, and once for the keychain.",
  },
  ready: { text: "Domains and SSL are ready. Continue." },
  done: { text: "Everything is installed, and your local domains work over HTTPS." },
  sites: {
    text: "Now create your first site — the next video shows how.",
    say: "Now create your first site. The next video shows how.",
  },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "onboarding", path: "/onboarding", ready: "text=Get started" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("Install rexenv", "One command, then a one-minute setup");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(500);

// ── Terminal ─────────────────────────────────────────────────────────────
await s.term.show({ title: "Terminal — zsh" });
await s.wait(500);
await s.say("terminal");
await s.term.type("curl -fsSL https://rexenv.rex.bd/install.sh | bash");
await s.wait(400);
await s.term.print([`rexenv: latest release: ${VERSION}`], 350);
await s.term.print([`rexenv: downloading rexenv_${VERSION}_universal.app.tar.gz`], 200);
await s.term.bar(2200);
await s.say("installed");
await s.term.print(["rexenv: checksum ok"], 700);
await s.term.print([`rexenv: installed rexenv ${VERSION}: /Applications/rexenv.app`], 700);
await s.term.print(["rexenv: started. rexenv lives in the menu bar (the crowned-R icon)."], 400);
await s.term.idle();
await s.voiceDone(400);
await s.term.hide();
await s.wait(400);

// ── Welcome ──────────────────────────────────────────────────────────────
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());
await s.say("welcome");
await s.caption("Click <b>Get started</b>");
await s.click(app.getByRole("button", { name: "Get started" }), { ms: 1000 });

// ── Components ───────────────────────────────────────────────────────────
await s.say("components");
await s.caption("Core components download in the background");
const rows = app.getByText("Caddy (edge router)");
await rows.waitFor();
const list = await s.union([app.getByText("Caddy (edge router)"), app.getByText("PHP 8.3 (FPM)")], 40);
await s.camera(list, 0.8);
await s.moveTo(app.getByText("MySQL 8.4"), { ms: 900, fx: 1.4 });
await s.voiceDone(300);
await s.camera(null);
await s.click(app.getByRole("button", { name: "Continue" }));

// ── Domains & SSL ────────────────────────────────────────────────────────
await s.say("domains");
await s.caption("Local domains &amp; HTTPS — one setup, done once");
await s.moveTo(app.getByRole("button", { name: "Set up domains & SSL" }), { ms: 900 });
await s.voiceDone(200);
await s.click(app.getByRole("button", { name: "Set up domains & SSL" }));
await s.say("password", 100);
await s.caption("macOS asks for your password: DNS resolver, then keychain");
await app.getByText("Domains & SSL are ready").waitFor({ timeout: 20_000 });
await s.voiceDone(200);
await s.say("ready");
await s.caption("Domains &amp; SSL are ready");
await s.click(app.getByRole("button", { name: "Continue" }), { ms: 900 });

// ── All set ──────────────────────────────────────────────────────────────
await app.getByText("Your kingdom is ready").waitFor({ timeout: 30_000 });
await s.say("done");
await s.caption("✓ Core components &nbsp;·&nbsp; ✓ Domains &amp; SSL");
await s.moveTo(app.getByText("Your kingdom is ready"), { ms: 800 });
await s.voiceDone(300);
await s.click(app.getByRole("button", { name: "Create your first site" }), { ms: 900 });

// ── Sites ────────────────────────────────────────────────────────────────
await app.getByText("No sites yet").waitFor();
await s.say("sites");
await s.caption("Next: create your first site");
const cta = app.getByRole("button", { name: "Create your first site" });
await s.spotlight(await s.box(cta));
await s.moveTo(cta, { ms: 900 });
await s.voiceDone(900);
await s.caption(null);
await s.spotlight(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("install")}`);
