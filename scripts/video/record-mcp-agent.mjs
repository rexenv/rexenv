// Tier 2 · "AI agents (MCP)": turn the endpoint on, the connect command, an
// agent working (a scratch site, a refused change to your site), the Agent
// access dial, and the Agent scratch section on Sites.
import { openStage } from "./lib.mjs";

const narration = {
  intro: { text: "Let an AI agent like Claude Code work with your sites — through rexenv's built-in MCP endpoint.", say: "Let an AI agent like Claude Code work with your sites, through rex env's built-in MCP endpoint." },
  enable: { text: "It's off by default. In Settings → AI agents, turn the endpoint on — the card says exactly what an agent can see and do." },
  connect: { text: "Copy the connect command into your terminal — or add rexenv to Cursor or VS Code.", say: "Copy the connect command into your terminal. Or add rex env to Cursor, or VS Code." },
  read: { text: "Agents start at Read: they can look at your sites, and make disposable scratch sites of their own to work in." },
  feed: { text: "Every call is listed. Here the agent built a scratch site and activated a plugin in it — and was refused when it tried to update a plugin on your own site." },
  dial: { text: "To let it change your sites, turn Agent access up — for this session, seven days, or always." },
  scratch: { text: "Scratch sites get their own section, and clean themselves up after a day unused — Keep one to make it yours." },
  caveat: { text: "Code an agent runs still runs as you, with your permissions — so turn the endpoint off when you're not using it." },
  outro: { text: "rexenv — your whole local stack, without Docker.", say: "Rex env. Your whole local stack, without Docker." },
};

const s = await openStage({ narration, scene: "mcp-agent", path: "/settings", ready: "text=AI agents" });
const { app } = s;

await s.title("AI agents (MCP)", "Let Claude Code and friends work with your sites — on your terms");
await s.wait(700);
await s.say("intro");
await s.voiceDone(300);
await s.title(null);
await s.wait(400);
await s.stage(() => window.stage.showWindow());
await s.wait(800);
await s.stage(() => window.stage.showCursor());

// ── Enable ───────────────────────────────────────────────────────────────
await s.say("enable");
await s.caption("Settings → <b>AI agents</b> → Enable the MCP endpoint");
await s.click(app.getByRole("button", { name: "AI agents", exact: true }), { ms: 900 });
const toggle = app.getByRole("switch", { name: "Enable the MCP endpoint" }).or(app.getByLabel("Enable the MCP endpoint"));
await s.box(toggle.first());
await s.camera(await s.containerOf(app.getByText(/Before you turn this on/), 560, 14), 0.8);
await s.moveTo(app.getByText(/Before you turn this on/), { ms: 900 });
await s.wait(500);
await s.click(toggle.first(), { ms: 800 });
await app.getByText("MCP endpoint enabled").waitFor({ timeout: 10_000 });
await s.voiceDone(200);
await s.camera(null);

// ── Connect ──────────────────────────────────────────────────────────────
await s.say("connect");
await s.caption("<code>claude mcp add rexenv -- rex mcp</code>");
const cmd = app.getByText("claude mcp add rexenv -- rex mcp", { exact: true });
await s.box(cmd);
await s.camera(await s.containerOf(cmd, 520, 12), 0.75);
await s.click(app.getByTitle("Copy").first(), { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── Read + an agent at work ──────────────────────────────────────────────
await s.say("read");
await s.caption("Agent access: <b>Read</b> — look, and build scratch sites");
const readRadio = app.getByRole("radio", { name: /^Read/ });
await s.box(readRadio);
await s.camera(await s.containerOf(readRadio, 560, 40), 0.8);
await s.moveTo(readRadio, { ms: 900 });
await s.voiceDone(200);

await s.camera(null);
await app.evaluate(() => window.__scene.agentWorks());
await s.say("feed");
await s.caption("Every call listed — and a change to <i>your</i> site, refused", "top");
await s.click(app.getByRole("button", { name: /Recent activity/ }), { ms: 900 });
const denied = app.getByText("denied", { exact: true }).first();
await denied.waitFor({ timeout: 15_000 });
await app.getByText("scratch_create_site", { exact: true }).waitFor();
await s.box(app.getByText("scratch_create_site", { exact: true }));
await s.camera(await s.union([app.getByText("Recent activity"), app.getByText("scratch_create_site", { exact: true })], 14), 0.8);
await s.moveTo(denied, { ms: 800 });
await s.voiceDone(200);
await s.camera(null);

// ── The dial ─────────────────────────────────────────────────────────────
await s.say("dial");
await s.caption("Turn it up: <b>Changes</b> · 7 days");
const changes = app.getByRole("radio", { name: /^Changes/ });
await s.box(changes);
await s.click(changes, { ms: 900 });
await s.click(app.getByRole("radio", { name: "7 days" }), { ms: 800 });
await s.voiceDone(300);

// ── Scratch sites ────────────────────────────────────────────────────────
await s.say("scratch");
await s.caption("<b>Agent scratch</b> — cleaned up after a day unused");
await s.click(app.getByRole("link", { name: /^Sites/ }), { ms: 900 });
const scratch = app.getByText("plugin-test.scratch.rex", { exact: true });
await scratch.waitFor();
await s.spotlight(await s.containerOf(scratch, 700));
await s.moveTo(scratch, { ms: 900 });
await s.voiceDone(200);
await s.spotlight(null);

await s.say("caveat");
await s.caption("Not a sandbox — turn it off when you're done");
await s.voiceDone(900);
await s.caption(null);
await s.stage(() => window.stage.showCursor(false));
await s.title("rexenv", "Your whole local stack — no Docker");
await s.wait(500);
await s.say("outro");
await s.voiceDone(1200);

console.log(`saved ${await s.save("mcp-agent")}`);
