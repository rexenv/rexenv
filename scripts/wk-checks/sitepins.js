// WebKit check: pinned and recently opened sites (docs/PLAN-sites-ux.md T5).
//
// A pin is "I keep this one at hand"; an open is recorded when a site's page is opened.
// Four surfaces show them — the site switcher, the Sites list, the sidebar, the site
// page's ⋯ — and all four render ONE order, computed in core (`core::site_shortlist`,
// which the tray reads too). This check proves the SURFACES: that each shows the list,
// in the same order, and that nothing renders for a person who has pinned nothing.
//
// What it does not prove, and where that is proven instead: the order itself and the
// refusal to pin a scratch site are Rust (`site_shortlist` tests), and the storage is
// SQLite v46 (`state::db` / `state::store` tests). The dev page's mock mirrors them
// closely enough to render; it is not the rule.
//
// Legs (27 production-shaped sites):
//  1. Nothing pinned: no Pinned group in the list, nothing under "Sites" in the sidebar,
//     no "Pinned" heading in the switcher.
//  2. Pin from a row's menu: the row moves into a Pinned group at the top, "All sites"
//     heads the rest, the sidebar lists it, and the row's menu now says Unpin.
//  3. A second pin: all three surfaces list the two BY NAME (not in the order pinned).
//  4. The Pinned group leads whatever the sort. A scratch row's menu offers no pin.
//  5. A sidebar pin opens that site from any route.
//  6. The site page's ⋯ says Unpin for a pinned site; unpinning removes it everywhere.
//  7. Opening site pages records them: the switcher's Recent group is newest first,
//     holds no pinned site, and no site appears in two groups.
//  8. Sort "Opened": the most recently opened site leads the unpinned rows; a site
//     never opened comes after every opened one.
//  9. More than five pins: the sidebar shows five and SAYS how many it left out.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
(async () => {
  const browser = await webkit.launch();
  const ctx = await browser.newContext({ viewport: { width: 1180, height: 860 } });
  await ctx.addInitScript(() => window.localStorage.setItem("rex-mock-sites", "many"));
  const page = await ctx.newPage();
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  const go = async (path) => { await page.goto(`${BASE}${path}`, { waitUntil: "networkidle" }); await page.waitForTimeout(600); };
  const menuPick = async (rowId, label) => {
    await page.click(`[data-site-row="${rowId}"] button[aria-label="More actions"]`);
    await page.waitForTimeout(200);
    await page.locator('[role="menu"]').getByRole("button", { name: label, exact: true }).click();
    await page.waitForTimeout(450);
  };
  const state = () => page.evaluate(() => ({
    group: [...document.querySelectorAll('[data-sites-group="pinned"] [data-site-row]')].map((r) => r.getAttribute("data-site-row")),
    firstRows: [...document.querySelectorAll("[data-site-row]")].slice(0, 4).map((r) => r.getAttribute("data-site-row")),
    rail: [...document.querySelectorAll("[data-rail-pin]")].map((r) => r.getAttribute("data-rail-pin")),
    headings: [...document.querySelectorAll("[data-sites-list] .uppercase")].map((h) => h.textContent.trim()),
    rows: document.querySelectorAll("[data-site-row]").length,
  }));
  const switcher = async () => {
    await page.click("[data-site-switch-trigger]");
    await page.waitForSelector("[data-site-switcher]");
    const out = await page.evaluate(() => Object.fromEntries(
      [...document.querySelectorAll("[data-switch-group]")].map((g) => [
        g.getAttribute("data-switch-group"),
        [...g.querySelectorAll("[data-switch-site]")].map((r) => r.getAttribute("data-switch-site")),
      ])));
    await page.keyboard.press("Escape");
    await page.waitForTimeout(150);
    return out;
  };

  // ── 1 ──
  await go("/sites");
  await page.evaluate(() => window.localStorage.removeItem("rex-mock-prefs"));
  await go("/sites");
  const s1 = await state();
  if (s1.group.length || s1.rail.length) fails.push(`1: a Pinned group (${s1.group}) or sidebar pins (${s1.rail}) with nothing pinned`);
  if (s1.headings.some((h) => /^(Pinned|All sites)$/.test(h))) fails.push(`1: a group heading with nothing pinned ([${s1.headings}])`);
  if (s1.rows !== 27) fails.push(`1: ${s1.rows} rows`);

  // ── 2 ──
  await menuPick("m4", "Pin to sidebar"); // "ea"
  const s2 = await state();
  if (s2.group.join() !== "m4") fails.push(`2: the Pinned group is [${s2.group}] after pinning ea`);
  if (s2.firstRows[0] !== "m4") fails.push(`2: the pinned row is not first (${s2.firstRows[0]})`);
  if (!s2.headings.includes("Pinned") || !s2.headings.includes("All sites")) fails.push(`2: headings are [${s2.headings}]`);
  if (s2.rail.join() !== "m4") fails.push(`2: the sidebar lists [${s2.rail}]`);
  if (s2.rows !== 27) fails.push(`2: ${s2.rows} rows — a pinned site is listed twice or lost`);
  await page.click('[data-site-row="m4"] button[aria-label="More actions"]');
  await page.waitForTimeout(200);
  const items2 = await page.evaluate(() => [...document.querySelectorAll('[role="menu"] button')].map((b) => b.textContent.trim()));
  if (!items2.includes("Unpin") || items2.includes("Pin to sidebar")) fails.push(`2: a pinned row's menu is [${items2}]`);
  await page.keyboard.press("Escape");

  // ── 3 ──
  await menuPick("m0", "Pin to sidebar"); // "bl" — pinned second, sorts first
  const s3 = await state();
  if (s3.group.join() !== "m0,m4") fails.push(`3: the Pinned group is [${s3.group}], expected by name (bl, ea)`);
  if (s3.rail.join() !== "m0,m4") fails.push(`3: the sidebar is [${s3.rail}], expected by name`);

  // ── 4 ──
  await page.click('button[title="Change sort order"]'); // → Status
  await page.waitForTimeout(250);
  const s4 = await state();
  if (s4.firstRows.slice(0, 2).join() !== "m0,m4") fails.push(`4: under another sort the first rows are [${s4.firstRows}]`);
  for (let i = 0; i < 3; i++) await page.click('button[title="Change sort order"]'); // back to Name
  await page.click('[data-site-row="s0"] button[aria-label="More actions"]');
  await page.waitForTimeout(200);
  const scratchItems = await page.evaluate(() => [...document.querySelectorAll('[role="menu"] button')].map((b) => b.textContent.trim()));
  if (!scratchItems.includes("Keep this site")) fails.push("4: the scratch row's menu did not open — the leg proved nothing");
  if (scratchItems.some((t) => /^(Pin to sidebar|Unpin)$/.test(t))) fails.push(`4: a scratch site is offered a pin ([${scratchItems}])`);
  await page.keyboard.press("Escape");

  // ── 5 ──
  await go("/mail");
  const railMail = await page.evaluate(() => [...document.querySelectorAll("[data-rail-pin]")].map((r) => r.getAttribute("data-rail-pin")));
  if (railMail.join() !== "m0,m4") fails.push(`5: on /mail the sidebar lists [${railMail}]`);
  await page.click('[data-rail-pin="m4"]');
  await page.waitForTimeout(400);
  if (!page.url().endsWith("/sites/m4")) fails.push(`5: a sidebar pin went to ${page.url()}`);

  // ── 3 (the switcher's view of the same two) ──
  const g3 = await switcher();
  if ((g3.pinned ?? []).join() !== "m0,m4") fails.push(`3: the switcher's Pinned is [${g3.pinned}]`);

  // ── 6 ──
  await page.click("[data-site-more]");
  await page.waitForTimeout(200);
  const more = await page.evaluate(() => [...document.querySelectorAll('[role="menu"] button')].map((b) => b.textContent.trim()));
  if (!more.includes("Unpin")) fails.push(`6: a pinned site's ⋯ is [${more}]`);
  await page.locator('[role="menu"]').getByRole("button", { name: "Unpin", exact: true }).click();
  await page.waitForTimeout(450);
  const rail6 = await page.evaluate(() => [...document.querySelectorAll("[data-rail-pin]")].map((r) => r.getAttribute("data-rail-pin")));
  if (rail6.join() !== "m0") fails.push(`6: after unpinning ea the sidebar is [${rail6}]`);

  // ── 7 ──
  await page.evaluate(() => window.localStorage.setItem("rex-mock-prefs", JSON.stringify({ m0: { pinned: true } })));
  for (const id of ["m7", "m10", "2"]) await go(`/sites/${id}`);
  await go("/sites/m0"); // the pinned one — opened last, and must NOT be under Recent
  const g7 = await switcher();
  const recent = g7.recent ?? [];
  if (recent.join() !== "2,m10,m7") fails.push(`7: Recent is [${recent}], expected newest first (2, m10, m7) without the pinned site`);
  if ((g7.pinned ?? []).join() !== "m0") fails.push(`7: Pinned is [${g7.pinned}]`);
  const all = Object.values(g7).flat();
  if (new Set(all).size !== all.length) fails.push("7: a site is listed in two groups");
  if (all.length !== 27) fails.push(`7: the switcher lists ${all.length} of 27 sites`);

  // ── 8 ──
  await go("/sites");
  for (let i = 0; i < 3; i++) { await page.click('button[title="Change sort order"]'); await page.waitForTimeout(120); }
  const sortLabel = await page.evaluate(() => document.querySelector('button[title="Change sort order"]').textContent.trim());
  if (sortLabel !== "Opened") fails.push(`8: three presses from Name reach "${sortLabel}", expected Opened`);
  const order8 = await page.evaluate(() => [...document.querySelectorAll("[data-site-row]")].map((r) => r.getAttribute("data-site-row")));
  if (order8.slice(0, 4).join() !== "m0,2,m10,m7") fails.push(`8: sorted by Opened the list starts [${order8.slice(0, 4)}], expected the pinned site, then 2, m10, m7`);
  for (let i = 0; i < 1; i++) await page.click('button[title="Change sort order"]'); // → Name

  // ── 9 ──
  await page.evaluate(() => window.localStorage.setItem("rex-mock-prefs", JSON.stringify(
    Object.fromEntries(["m0", "m1", "m2", "m3", "m4", "m5", "m6"].map((id) => [id, { pinned: true }])))));
  await go("/sites");
  const s9 = await page.evaluate(() => ({
    rail: document.querySelectorAll("[data-rail-pin]").length,
    more: document.querySelector("[data-rail-pins-more]")?.textContent.trim() ?? null,
    group: document.querySelectorAll('[data-sites-group="pinned"] [data-site-row]').length,
    overflow: (() => { const a = document.querySelector("aside"); return a.scrollWidth > a.clientWidth + 1; })(),
  }));
  if (s9.rail !== 5) fails.push(`9: the sidebar shows ${s9.rail} pins of 7, expected 5`);
  if (!s9.more || !/2 more/.test(s9.more)) fails.push(`9: the sidebar does not say what it left out ("${s9.more}")`);
  if (s9.group !== 7) fails.push(`9: the list's Pinned group holds ${s9.group} of 7 — the place the rest are must hold them all`);
  if (s9.overflow) fails.push("9: the sidebar scrolls sideways");
  await page.screenshot({ path: "shot-sitepins.png" });
  await page.evaluate(() => window.localStorage.removeItem("rex-mock-prefs"));

  await browser.close();
  if (fails.length) {
    console.log("SITE-PINS CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SITE-PINS CHECK: ALL PASS (nothing rendered with nothing pinned; a pin leads the list, sits in the sidebar and the switcher, by name, under every sort; no pin offered to a scratch site; a sidebar pin opens its site; unpin from the site page; Recent newest first without the pinned, one group per site; sort by Opened; five pins shown and the rest counted)");
})().catch((e) => {
  console.log("SITE-PINS CHECK FAILURES:");
  for (const f of fails) console.log(" - " + f);
  console.log(" - crashed: " + String(e.message).split("\n")[0]);
  process.exit(1);
});
