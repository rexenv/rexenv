// Tier 2 · "The rex command line": install rex from Settings, then a terminal
// session — status, site list, site create, PHP switch, db export, doctor —
// and the app showing the site the terminal made. Output formats are the CLI's
// own (`cli/src/main.rs`); PIDs, percentages and sizes are illustrative.
import { openStage } from "./lib.mjs";

const narration = {
  intro: {
    text: "rexenv comes with a command-line tool, rex — it remote-controls the running app.",
    say: "Rex env comes with a command-line tool, called rex. It remote-controls the running app.",
  },
  install: { text: "Install it from Settings → General → Command-line tool. One admin prompt, and rex is on your PATH." },
  status: { text: "rex status shows the whole stack: DNS, the certificate authority, and every service with its port." },
  list: { text: "rex site list shows every site — type, PHP, server, database and state." },
  create: { text: "Create a site from the terminal. WordPress is the default, and rex shows each step as it runs." },
  php: { text: "Switch its PHP version…" },
  export: { text: "…or export its database, straight to your Downloads folder." },
  doctor: { text: "And rex doctor checks DNS, the HTTPS edge, ports and services — and tells you what to fix." },
  app: { text: "Everything rex does happens in the app too — there's shop.rex, made from the terminal.", say: "Everything rex does happens in the app too. There's shop dot rex, made from the terminal." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const pad = (s, n) => String(s).padEnd(n);
const lpad = (s, n) => String(s).padStart(n);

const s = await openStage({ narration, scene: "rex-cli", path: "/settings", ready: "text=Command-line tool" });
const { app } = s;

// ── Title ────────────────────────────────────────────────────────────────
await s.title("The rex command line", "Drive rexenv from any terminal");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Install rex ──────────────────────────────────────────────────────────
await s.say("install");
await s.caption("Settings → General → <b>Command-line tool</b> → Install");
const card = app.getByText("Command-line tool", { exact: true });
await s.box(card);
await s.camera(await s.containerOf(card, 500, 12), 0.7);
await s.click(app.getByRole("button", { name: "Install", exact: true }), { ms: 900 });
await app.getByText(/rex installed — run it from any terminal/).waitFor({ timeout: 10_000 });
await s.voiceDone(300);
await s.camera(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.stage(() => window.stage.showWindow(false));
await s.wait(500);

// ── Terminal ─────────────────────────────────────────────────────────────
await s.term.show({ title: "Terminal — zsh", x: 200, y: 120, w: 1520, h: 820, font: 22 });
await s.wait(500);

await s.say("status");
await s.term.type("rex status");
const svc = [
  ["Caddy", 41210, 443, 0.3, 38],
  ["Nginx", 41212, 18088, 0.1, 12],
  ["PHP-FPM 8.3", 41230, 9783, 0.8, 64],
  ["MySQL", 41250, 13306, 1.2, 410],
  ["Mailpit", 41260, 18025, 0.0, 22],
];
await s.term.print(
  [
    "DNS      answering (agent, udp 15353) · resolver installed · CA trusted",
    `${pad("NAME", 11)}  ${pad("STATE", 8)} ${lpad("PID", 7)}  ${lpad("PORT", 6)}  ${lpad("CPU%", 6)}  ${lpad("RAM", 6)}`,
    ...svc.map(([n, pid, port, cpu, ram]) => `${pad(n, 11)}  ${pad("running", 8)} ${lpad(pid, 7)}  ${lpad(port, 6)}  ${lpad(cpu.toFixed(1), 6)}  ${lpad(ram, 6)} MB`),
  ],
  90,
);
await s.voiceDone(300);

await s.say("list");
await s.term.type("rex site list");
const sites = [
  ["agency-blog.rex", "Agency Blog", "wordpress", "8.3", "nginx", "mysql", "serving"],
  ["booking-api.rex", "Booking API", "laravel", "8.4", "nginx", "mysql", "serving"],
  ["landing.rex", "Landing Page", "php", "8.3", "nginx", "mysql", "serving"],
  ["shop-staging.rex", "Shop Staging", "wordpress", "8.2", "nginx", "mysql", "serving"],
];
const row = (r) => `${pad(r[0], 16)}  ${pad(r[1], 12)}  ${pad(r[2], 9)}  ${pad(r[3], 5)}  ${pad(r[4], 10)}  ${pad(r[5], 7)}  ${pad(r[6], 10)}`;
await s.term.print([row(["DOMAIN", "NAME", "TYPE", "PHP", "SERVER", "DB", "STATE"]) + "  ALSO", ...sites.map(row)], 90);
await s.voiceDone(300);

await s.say("create");
await s.term.clear();
await s.term.type("rex site create shop.rex --php 8.4");
await s.term.print(["creating shop.rex… (WordPress sites install on first create — this can take a minute)"], 300);
await s.term.print(
  [
    "  [  4%] preparing site (domain, certificate)",
    "  [ 12%] downloading binaries",
    "  [ 22%] starting database",
    "  [ 48%] downloading WordPress core",
    "  [ 66%] writing wp-config + creating database",
    "  [ 88%] installing WordPress",
    "  [ 99%] starting to serve",
  ],
  700,
);
await app.evaluate(() => window.__scene.createShop());
await s.term.print(['<span class="ok">✓</span> created shop.rex (wordpress, PHP 8.4, nginx, mysql) → https://shop.rex'], 200);
await s.voiceDone(300);

await s.say("php");
await s.term.type("rex site php shop.rex 8.3");
await s.term.print(["switching shop.rex to PHP 8.3…"], 900);
await s.term.print(['<span class="ok">✓</span> shop.rex — PHP 8.3 on nginx'], 100);
await s.voiceDone(200);

await s.say("export");
await s.term.type("rex db export shop.rex");
await s.term.print(["exporting shop.rex…"], 900);
await s.term.print(['<span class="ok">✓</span> exported → /Users/demo/Downloads/shop.rex-db.sql'], 100);
await s.voiceDone(300);

await s.say("doctor");
await s.term.clear();
await s.term.type("rex doctor");
await s.term.print(
  [
    "rexenv 0.8.10 (macOS · Apple silicon)",
    ...[
      ["DNS", "agent (always on) · resolver installed · CA trusted"],
      ["Resolvers", "no TLD taken back by another tool"],
      ["TLDs", "in use every TLD your sites answer on resolves here"],
      ["Edge", "answering as rexenv on :443"],
      ["Services", "5/5 running"],
      ["Ports", "no foreign holders on rexenv ports"],
      ["CLI", "/usr/local/bin/rex → this app"],
    ].map(([l, m]) => `<span class="ok">✓</span> ${pad(l, 9)} ${m}`),
  ],
  220,
);
await s.term.idle();
await s.voiceDone(400);
await s.term.hide();
await s.wait(400);

// ── Back in the app ──────────────────────────────────────────────────────
await s.stage(() => window.stage.showWindow());
await s.stage(() => window.stage.showCursor());
await s.say("app");
await s.caption("Made in the terminal, served by the app");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 900 });
const shop = app.getByText("shop.rex", { exact: true });
await shop.waitFor();
await s.spotlight(await s.containerOf(shop, 700));
await s.moveTo(shop, { ms: 900 });
await s.voiceDone(900);
await s.spotlight(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("rex-cli")}`);
