// WebKit check: Mod+K — the site switcher from every route (docs/PLAN-sites-ux.md T3).
//
// The same list the site page's breadcrumb opens (`siteswitch.js` proves the list itself);
// this proves the DOOR: which key opens it, where it is deliberately not taken, and that
// the hint names the key the listener answers.
//
// The modifier is a per-OS value (`PlatformWords.modKey` / `modKeyIsCommand`), so every
// leg that presses a key runs twice: with the dev page's macOS words (⌘K) and with
// `rex-mock-os=windows` (Ctrl+K). One modifier is answered per OS, never both — in a macOS
// text field Ctrl+K deletes to the end of the line, and a listener that took either would
// eat it. A check that pressed only ⌘K would prove the branch the dev Mac already is.
//
// Legs, per OS:
//  1. On a non-site route (Mail) the OS's chord opens the switcher; the OTHER OS's chord
//     does not, and neither does the chord with Shift.
//  2. Picking a site from there opens its Overview.
//  3. On a site's Database tab the chord opens it and Enter on a filtered row keeps the tab.
//  4. The chord toggles: pressed again, it closes. Esc closes. A click on the backdrop closes.
//  5. NOT taken while a dialog is open (the New site dialog on Sites).
//  6. NOT taken inside a terminal: the key belongs to the shell. (A built `.xterm`, not a live
//     one — the dev page has no PTY; see the leg.)
//  7. The hints say this OS's modifier — the footer's "in browser" key and the breadcrumb's
//     title — and never the other OS's.
//  8. With the breadcrumb's popover already open the chord opens no second list.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
const OSES = [
  { os: "macos", mod: "Meta", other: "Control", label: "⌘", foreign: "Ctrl" },
  { os: "windows", mod: "Control", other: "Meta", label: "Ctrl+", foreign: "⌘" },
];

(async () => {
  const browser = await webkit.launch();
  for (const o of OSES) {
    const ctx = await browser.newContext({ viewport: { width: 1180, height: 800 } });
    await ctx.addInitScript((os) => {
      window.localStorage.setItem("rex-mock-sites", "many");
      if (os === "windows") window.localStorage.setItem("rex-mock-os", "windows");
    }, o.os);
    const page = await ctx.newPage();
    page.on("pageerror", (e) => fails.push(`[${o.os}] pageerror: ${e.message}`));
    const at = (n) => `[${o.os}] ${n}`;
    const isOpen = () => page.evaluate(() => !!document.querySelector("[data-site-switcher-dialog]"));
    const lists = () => page.evaluate(() => document.querySelectorAll("[data-site-switcher]").length);
    const chord = async (mod, extra = "") => {
      await page.keyboard.press(`${mod}+${extra}k`);
      await page.waitForTimeout(150);
    };
    const go = async (path) => {
      await page.goto(`${BASE}${path}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(500);
    };

    // ── 1 ──
    await go("/mail");
    await chord(o.other);
    if (await isOpen()) fails.push(at(`1: ${o.other}+K opened the switcher — that is the other OS's chord`));
    await chord(o.mod, "Shift+");
    if (await isOpen()) fails.push(at(`1: ${o.mod}+Shift+K opened the switcher`));
    await chord(o.mod);
    if (!(await isOpen())) fails.push(at(`1: ${o.mod}+K did not open the switcher on /mail`));
    const focused = await page.evaluate(() => document.activeElement?.getAttribute("aria-label"));
    if (focused !== "Switch site") fails.push(at(`1: the search box is not focused (${focused})`));
    await page.screenshot({ path: `shot-sitekey-${o.os}.png` });

    // ── 7 (footer, while it is open) ──
    const keys = await page.evaluate(() => document.querySelector("[data-switch-keys]")?.innerText ?? "");
    if (!keys.includes(`${o.label}↵`)) fails.push(at(`7: the footer does not name ${o.label}↵ ("${keys.replace(/\s+/g, " ")}")`));
    if (keys.includes(o.foreign)) fails.push(at(`7: the footer names the other OS's key ("${keys.replace(/\s+/g, " ")}")`));

    // ── 2 ──
    await page.click('[data-switch-site="2"]');
    await page.waitForTimeout(400);
    if (!page.url().endsWith("/sites/2")) fails.push(at(`2: from /mail the pick went to ${page.url()}, not the site's Overview`));
    if (await isOpen()) fails.push(at("2: the switcher stayed open after a pick"));

    // ── 7 (the breadcrumb's title) ──
    const title = await page.evaluate(() => document.querySelector("[data-site-switch-trigger]")?.getAttribute("title") ?? "");
    if (!title.includes(`${o.label}K`)) fails.push(at(`7: the breadcrumb's title does not name ${o.label}K ("${title}")`));

    // ── 3 ──
    await go("/sites/1/database");
    await chord(o.mod);
    await page.keyboard.type("portfolio");
    await page.waitForTimeout(150);
    await page.keyboard.press("Enter");
    await page.waitForTimeout(400);
    if (!page.url().endsWith("/sites/2/database")) fails.push(at(`3: the tab was not kept through ${o.mod}+K (${page.url()})`));

    // ── 4 ──
    await chord(o.mod);
    if (!(await isOpen())) fails.push(at("4: the chord did not open on a site page"));
    await chord(o.mod);
    if (await isOpen()) fails.push(at("4: the chord pressed again did not close it"));
    await chord(o.mod);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(150);
    if (await isOpen()) fails.push(at("4: Esc did not close it"));
    await chord(o.mod);
    await page.mouse.click(40, 760);
    await page.waitForTimeout(150);
    if (await isOpen()) fails.push(at("4: a click on the backdrop did not close it"));

    // ── 8 ──
    await page.click("[data-site-switch-trigger]");
    await page.waitForSelector("[data-site-switcher]");
    await chord(o.mod);
    if ((await lists()) !== 1) fails.push(at(`8: ${await lists()} switcher lists on screen with the breadcrumb's already open`));
    await page.keyboard.press("Escape");

    // ── 5 ──
    await go("/sites");
    await page.click('button:has-text("New site")');
    await page.waitForSelector('[role="dialog"]');
    await chord(o.mod);
    if (await isOpen()) fails.push(at("5: the chord opened the switcher over an open dialog"));
    const stillThere = await page.evaluate(() => !!document.querySelector('[role="dialog"]'));
    if (!stillThere) fails.push(at("5: the dialog under test is gone — the leg proved nothing"));

    // ── 6 ──
    await go("/sites/1/terminal");
    // The dev page has no PTY, so the Terminal tab renders a placeholder and no xterm. The
    // leg builds what xterm.js builds — its `.xterm` root around the hidden textarea that
    // takes the keys — and focuses that. It certifies the RULE (a key pressed inside `.xterm`
    // is the shell's); that the real terminal's focus lands inside `.xterm` is xterm.js's own
    // markup, seen in `termkeep.js`'s harness, and the SMOKE row presses it for real.
    const term = await page.evaluate(() => {
      const root = document.createElement("div");
      root.className = "xterm";
      const ta = document.createElement("textarea");
      ta.className = "xterm-helper-textarea";
      root.appendChild(ta);
      document.querySelector("main").appendChild(root);
      ta.focus();
      return !!document.activeElement?.closest(".xterm");
    });
    if (!term) fails.push(at("6: no focusable terminal on the Terminal tab — the leg proved nothing"));
    else {
      await chord(o.mod);
      if (await isOpen()) fails.push(at("6: the chord was taken from the terminal"));
      // And outside it, on the same page, it still works — else leg 6 passes on a dead listener.
      await page.evaluate(() => document.activeElement?.blur());
      await page.click("[data-site-title]");
      await chord(o.mod);
      if (!(await isOpen())) fails.push(at("6: outside the terminal the chord no longer opens — the listener is dead, not careful"));
    }
    await ctx.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("SITE-KEY CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SITE-KEY CHECK: ALL PASS (⌘K with macOS words, Ctrl+K with Windows words, never the other's; Overview from a non-site route, the tab kept on a site; toggles, Esc, backdrop; not over a dialog, not from a terminal, not a second list; the hints name this OS's key)");
})().catch((e) => {
  console.log("SITE-KEY CHECK FAILURES:");
  for (const f of fails) console.log(" - " + f);
  console.log(" - crashed: " + String(e.message).split("\n")[0]);
  process.exit(1);
});
