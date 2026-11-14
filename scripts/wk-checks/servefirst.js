// WebKit check: "Start & …" — the click that starts rexenv's services, then does what it said
// (ledger #969; owner, 14 Nov 2026).
//
// Two reports in one: (1) with every service off, a row that read "Stopped" still offered
// "Stop site"; (2) people press Magic Login without pressing Start all first, and got an
// error toast and a browser on "can't connect". Before this the browser button was DISABLED
// with "Start all first" — honest, and a dead end.
//
// The dev page's stack is `localStorage["rex-mock-stack"]`: `down` (nothing serves; the mock
// Start all moves it to `up`, as the real one does) or `fail` (Start all throws a port
// conflict). `window.open` is recorded, so "what was opened, and when" is read, not assumed.
//
// Legs:
//  1. Services off, Sites list: every row reads "Services off" (never a bare "Stopped"), and
//     its menu's first item is "Start services" — NOT "Stop site". A site the user stopped
//     reads "Stopped by you" and offers "Start site and services".
//  2. A row's browser icon is LIVE with the services off, and says it starts them. One click:
//     Start all runs once, nothing opens until the site serves, then the site's URL opens.
//  3. Site page, services off: the buttons read "Start & open" and "Start & Magic Login" —
//     the start is SAID before the click — with no other-browser chevron. One click on
//     Magic Login: Start all once, then the sign-in URL opens, and the buttons read
//     "Open in browser" / "Magic Login" again.
//  4. A start that FAILS opens nothing and shows Start all's own error.
//  5. A site the user stopped is never started by this: Magic Login is off there and says
//     why; its browser button opens rexenv's stopped page as before, with no start.
//  6. Services on: nothing changed — "Stop site", plain labels, no start on a click.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
(async () => {
  const browser = await webkit.launch();
  const mk = async (stack) => {
    const ctx = await browser.newContext({ viewport: { width: 1180, height: 860 } });
    await ctx.addInitScript((st) => {
      window.localStorage.setItem("rex-mock-sites", "many");
      if (st) window.localStorage.setItem("rex-mock-stack", st);
      window.__opened = [];
      window.open = (u) => { window.__opened.push({ url: String(u), starts: window.__rexStarts ?? 0 }); return null; };
    }, stack);
    const page = await ctx.newPage();
    page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
    return { ctx, page };
  };
  const go = async (page, path) => { await page.goto(`${BASE}${path}`, { waitUntil: "networkidle" }); await page.waitForTimeout(700); };
  const state = (page) => page.evaluate(() => ({ starts: window.__rexStarts ?? 0, opened: window.__opened }));
  const menuOf = async (page, id) => {
    await page.click(`[data-site-row="${id}"] button[aria-label="More actions"]`);
    await page.waitForTimeout(250);
    const items = await page.evaluate(() => [...document.querySelectorAll('[role="menu"] button')].map((b) => b.textContent.trim()));
    await page.keyboard.press("Escape");
    await page.waitForTimeout(150);
    return items;
  };

  // ── 1 + 2 ───────────────────────────────────────────────────────────────────────
  {
    const { ctx, page } = await mk("down");
    await go(page, "/sites");
    const words = await page.evaluate(() => [...document.querySelectorAll('[data-probe="status-words"]')].map((w) => w.textContent.trim()));
    if (words.length !== 27) fails.push(`1: ${words.length} of 27 rows say their status with the services off`);
    if (words.some((w) => w === "Stopped")) fails.push('1: a row reads a bare "Stopped" with the services off');
    if (words.filter((w) => w === "Services off").length !== 26) fails.push(`1: "Services off" on ${words.filter((w) => w === "Services off").length} rows, expected 26`);
    if (words.filter((w) => w === "Stopped by you").length !== 1) fails.push('1: the user-stopped row lost "Stopped by you"');
    const m1 = await menuOf(page, "m1");
    if (m1[0] !== "Start services") fails.push(`1: with the services off the menu's first item is "${m1[0]}", expected "Start services"`);
    if (m1.includes("Stop site")) fails.push('1: "Stop site" is offered under a row that reads Services off');
    const m9 = await menuOf(page, "m9");
    if (!m9.includes("Start site and services")) fails.push(`1: the user-stopped site's menu is [${m9.slice(0, 3)}], expected "Start site and services"`);

    const b = await page.evaluate(() => {
      const el = document.querySelector('[data-site-row="m1"] button[aria-label="Open in browser"]');
      return { disabled: el.disabled, says: el.hasAttribute("data-starts-services"), title: el.parentElement.getAttribute("title") };
    });
    if (b.disabled) fails.push("2: the row's browser icon is disabled with the services off — the dead end this replaces");
    if (!b.says || !/Start rexenv's services/.test(b.title ?? "")) fails.push(`2: the icon does not say it starts the services ("${b.title}")`);
    await page.click('[data-site-row="m1"] button[aria-label="Open in browser"]');
    await page.waitForTimeout(120);
    const mid = await state(page);
    if (mid.opened.length) fails.push("2: the site was opened BEFORE the services had started");
    await page.waitForTimeout(2600);
    const end = await state(page);
    if (end.starts !== 1) fails.push(`2: Start all ran ${end.starts} times for one click`);
    if (end.opened.length !== 1 || !/bs\.test$/.test(end.opened[0]?.url ?? "")) fails.push(`2: opened ${JSON.stringify(end.opened)} — expected the site's URL, once`);
    if (end.opened[0] && end.opened[0].starts !== 1) fails.push("2: the open did not come after the start");
    const after = await page.evaluate(() => [...document.querySelectorAll('[data-probe="status-words"]')].map((w) => w.textContent.trim()));
    // One row may: the fixture's site 3 is a site whose OWN upstream is down with the stack up
    // — for it "Start services" is still the right offer (Start all starts what is missing).
    if (after.filter((w) => w === "Services off").length > 1) fails.push(`2: ${after.filter((w) => w === "Services off").length} rows still read Services off after the start`);
    if (!page.url().endsWith("/sites")) fails.push("2: the click also opened the site's page");
    await ctx.close();
  }

  // ── 3 ───────────────────────────────────────────────────────────────────────────
  {
    const { ctx, page } = await mk("down");
    await go(page, "/sites/1");
    const h = await page.evaluate(() => ({
      labels: [...document.querySelectorAll("[data-site-actions] [data-starts-services]")].map((x) => x.textContent.trim()),
      pill: document.querySelector("[data-status-pill]")?.textContent.trim(),
      chevrons: [...document.querySelectorAll("[data-site-actions] button")].filter((x) => /another browser/.test(x.getAttribute("aria-label") ?? "")).length,
    }));
    if (h.labels.join("|") !== "Start & open|Start & Magic Login") fails.push(`3: the header reads [${h.labels}] with the services off`);
    if (h.pill !== "Services off") fails.push(`3: the header pill reads "${h.pill}"`);
    if (h.chevrons) fails.push(`3: ${h.chevrons} other-browser chevrons offered for a URL nothing serves`);
    await page.click("[data-site-more]");
    await page.waitForTimeout(250);
    const more = await page.evaluate(() => [...document.querySelectorAll('[role="menu"] button')].map((x) => x.textContent.trim()));
    if (more[0] !== "Start services") fails.push(`3: the ⋯ menu's first item is "${more[0]}"`);
    await page.keyboard.press("Escape");
    await page.getByText("Start & Magic Login").click();
    await page.waitForTimeout(150);
    if ((await state(page)).opened.length) fails.push("3: the sign-in opened before the services had started");
    await page.waitForTimeout(2800);
    const s = await state(page);
    if (s.starts !== 1) fails.push(`3: Start all ran ${s.starts} times for one Magic Login click`);
    if (s.opened.length !== 1 || !/acme\.rex\/wp-admin/.test(s.opened[0]?.url ?? "")) fails.push(`3: opened ${JSON.stringify(s.opened)} — expected the site's sign-in, once`);
    const back = await page.evaluate(() => document.querySelector("[data-site-actions]").innerText.replace(/\s+/g, " "));
    if (!/Open in browser/.test(back) || !/Magic Login/.test(back) || /Start &/.test(back)) fails.push(`3: after the start the header reads "${back}"`);
    await page.screenshot({ path: "shot-servefirst.png" });
    await ctx.close();
  }

  // ── 4 ───────────────────────────────────────────────────────────────────────────
  {
    const { ctx, page } = await mk("fail");
    await go(page, "/sites/1");
    await page.getByText("Start & Magic Login").click();
    await page.waitForTimeout(1500);
    const s = await state(page);
    const body = await page.evaluate(() => document.body.innerText);
    if (s.opened.length) fails.push(`4: a FAILED start still opened ${JSON.stringify(s.opened)}`);
    if (!/Port 443 is in use/.test(body)) fails.push("4: the failed start does not show Start all's own error");
    if (!/Start & Magic Login/.test(body)) fails.push("4: after a failed start the button no longer offers the start");
    await ctx.close();
  }

  // ── 5 ───────────────────────────────────────────────────────────────────────────
  for (const stack of ["up", "down"]) {
    const { ctx, page } = await mk(stack);
    await go(page, "/sites/m9"); // stopped by the user
    const u = await page.evaluate(() => {
      const ml = [...document.querySelectorAll("[data-site-actions] button")].find((b) => /Magic Login/.test(b.textContent));
      return { disabled: ml?.disabled, title: ml?.getAttribute("title") ?? "", text: ml?.textContent.trim(), pill: document.querySelector("[data-status-pill]")?.textContent.trim() };
    });
    if (!u.disabled) fails.push(`5 (${stack}): Magic Login is live on a site the user stopped`);
    if (!/You stopped this site/.test(u.title)) fails.push(`5 (${stack}): it does not say why ("${u.title}")`);
    if (/Start &/.test(u.text ?? "")) fails.push(`5 (${stack}): it offers to start a site the user stopped`);
    if (u.pill !== "Stopped by you") fails.push(`5 (${stack}): the pill reads "${u.pill}"`);
    await page.getByText("Open in browser").first().click();
    await page.waitForTimeout(700);
    const s = await state(page);
    if (s.starts !== 0) fails.push(`5 (${stack}): opening a user-stopped site's page started the services`);
    if (s.opened.length !== 1) fails.push(`5 (${stack}): the stopped page was not opened (${JSON.stringify(s.opened)})`);
    await ctx.close();
  }

  // ── 6 ───────────────────────────────────────────────────────────────────────────
  {
    const { ctx, page } = await mk("up");
    await go(page, "/sites");
    const m = await menuOf(page, "m1");
    if (m[0] !== "Stop site") fails.push(`6: with the services on the first item is "${m[0]}"`);
    const m9 = await menuOf(page, "m9");
    if (m9[0] !== "Start site") fails.push(`6: a user-stopped site with the services on offers "${m9[0]}", expected "Start site"`);
    await page.click('[data-site-row="m1"] button[aria-label="Open in browser"]');
    await page.waitForTimeout(500);
    const s = await state(page);
    if (s.starts !== 0) fails.push("6: a click on a serving site's icon started the services");
    if (s.opened.length !== 1) fails.push("6: a serving site's icon did not open it");
    const says = await page.evaluate(() => [...document.querySelectorAll("[data-starts-services]")].map((x) => x.closest("[data-site-row]")?.getAttribute("data-site-row")));
    if (says.some((id) => id !== "3")) fails.push(`6: a control on a SERVING site says it starts the services ([${says}])`);
    await ctx.close();
  }

  await browser.close();
  if (fails.length) {
    console.log("SERVE-FIRST CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log('SERVE-FIRST CHECK: ALL PASS (services off: rows say "Services off" and offer Start services, never Stop site; one click starts once, opens only after the site serves; the site page says "Start & open" / "Start & Magic Login"; a failed start opens nothing and shows its error; a user-stopped site is never started; services on: unchanged)');
})().catch((e) => {
  console.log("SERVE-FIRST CHECK FAILURES:");
  for (const f of fails) console.log(" - " + f);
  console.log(" - crashed: " + String(e.message).split("\n")[0]);
  process.exit(1);
});
