// Tier 3 · "Domains": the default ending for new sites (.local refused, .test
// saved) and an extra name on a site.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Every site gets a .rex domain with HTTPS. You can pick another ending — and give a site more than one name.", say: "Every site gets a dot rex domain, with HTTPS. You can pick another ending, and give a site more than one name." },
  tld: { text: "In Settings → DNS & SSL, set the default ending for new sites. Some are refused — .local belongs to Bonjour, printers and AirDrop.", say: "In Settings, DNS and SSL, set the default ending for new sites. Some are refused: dot local belongs to Bonjour, printers and AirDrop." },
  test: { text: ".test is always safe. Existing sites keep their domain, and the first site on a new ending asks for your password once.", say: "Dot test is always safe. Existing sites keep their domain, and the first site on a new ending asks for your password once." },
  alias: { text: "To give a site another name, open its Settings → Domains and add one. The certificate is re-issued to cover it.", say: "To give a site another name, open its Settings, then Domains, and add one. The certificate is re-issued to cover it." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "domains", path: "/settings", ready: "text=DNS & SSL" });
const { app } = s;

await s.title("Domains", "Pick your ending — .rex, .test and more — and add extra names");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

await s.say("tld");
await s.caption("Settings → DNS &amp; SSL → <b>Default domain ending</b>");
await s.click(app.getByRole("button", { name: "DNS & SSL", exact: true }), { ms: 900 });
const input = app.getByLabel("Default TLD for new sites");
await s.box(input);
await s.camera(await s.containerOf(input, 520, 14), 0.72);
await s.click(input, { ms: 700 });
await s.page.keyboard.press("Meta+A");
await s.type("local", 110);
await app.getByText(/is used by Bonjour\/mDNS/).waitFor();
await s.voiceDone(200);

await s.say("test");
await s.caption("<code>.test</code> — saved for new sites");
await s.page.keyboard.press("Meta+A");
await s.type("test", 110);
await s.click(app.getByRole("button", { name: "Save", exact: true }), { ms: 700 });
await app.getByText("New sites will now be created under .test").waitFor({ timeout: 8000 });
await s.voiceDone(200);
await s.camera(null);

await s.say("alias");
await s.caption("Site → Settings → <b>Domains</b> → Add domain");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 800 });
await s.click(app.getByText("Agency Blog", { exact: true }), { ms: 800 });
await s.click(app.getByRole("button", { name: "Settings", exact: true }).last(), { ms: 800 });
const add = app.getByPlaceholder("another.rex");
await s.box(add);
await s.camera(await s.containerOf(add, 520, 14), 0.75);
await s.click(add, { ms: 700 });
await s.type("agencyblog.test", 70);
await s.click(app.getByRole("button", { name: "Add domain" }), { ms: 700 });
await app.getByText(/Extra domain added — certificate re-issued/).waitFor({ timeout: 8000 });
await s.voiceDone(900);
await s.camera(null);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("domains")}`);
