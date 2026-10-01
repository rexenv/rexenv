// Tier 3 · "Updates": the Settings badge → About → the consent sentence →
// Install → download + install progress → the restart question.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "rexenv updates itself — and tells you exactly what will happen first.", say: "Rex env updates itself, and tells you exactly what will happen first." },
  badge: { text: "When a new version is out, Settings shows it. Open Settings → About." },
  consent: { text: "Before anything installs, it says what it will do: download, check the signature and checksum, and swap the app in one step." },
  install: { text: "Click Install. Your sites, databases and DNS keep running the whole time." },
  restart: { text: "When it's done, rexenv asks before it restarts — then reopens on the new version.", say: "When it's done, rex env asks before it restarts. Then it reopens on the new version." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "self-update" });
const { app } = s;

await s.title("Updates", "Signed, checked, and your sites keep running");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

await s.say("badge");
await s.caption("Settings shows <b>0.8.11</b> → About");
const settings = app.getByRole("link", { name: /^Settings/ });
await s.spotlight(await s.box(settings));
await s.moveTo(settings, { ms: 900 });
await s.wait(500);
await s.spotlight(null);
await s.click(settings, { ms: 300 });
await s.click(app.getByRole("button", { name: "About", exact: true }), { ms: 800 });
const install = app.getByRole("button", { name: /^Install rexenv 0\.8\.11/ });
await install.waitFor({ timeout: 10_000 });
await s.voiceDone(200);

await s.say("consent");
await s.caption("Signature + checksum checked, then one swap");
const consent = app.getByText(/checks its signature and checksum/);
await s.box(consent);
await s.camera(await s.containerOf(consent, 560, 14), 0.75);
await s.moveTo(consent, { ms: 900, fx: 0.4 });
await s.voiceDone(200);

await s.say("install");
await s.caption("<b>Install</b> — services keep running");
await s.click(install, { ms: 800 });
await app.getByText("rexenv 0.8.11 is installed").waitFor({ timeout: 30_000 });
await s.camera(null);
await s.voiceDone(200);

await s.say("restart");
await s.caption("It asks before it restarts");
await s.moveTo(app.getByRole("button", { name: "OK", exact: true }), { ms: 900 });
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("self-update")}`);
