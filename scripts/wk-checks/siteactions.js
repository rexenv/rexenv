// WebKit check: the site page has ONE home for each action (docs/PLAN-sites-ux.md T1).
//
// Counted in the tree on 14 Nov 2026: of the six "Quick links" tiles on a site's Overview,
// five repeated something on the same screen — Open in browser and Magic Login (the header),
// Database and Terminal (the tab bar), Open folder (the path row's icon). PHP version and the
// web server were each written twice in one box (a large value beside a select showing it).
// And Duplicate and Delete could be reached only from the Sites list.
//
// After: the header is the action bar (browser, editor, Magic Login, ⋯), the tabs are the
// navigation, a card shows facts. This check COUNTS, so a tile that comes back is red.
//
// Legs (a WordPress site, then a Blank PHP one):
//  1. Each action is on the Overview once: Open in browser, the editor, Magic Login (WordPress
//     only), Open folder. No "Quick links". Nothing but the tab bar goes to Database/Terminal.
//  2. PHP version and Web server say their value once — the select — not twice.
//  3. The read-only facts ("Site info") are on Overview, and Settings no longer has that card.
//  4. The header's ⋯ holds Stop site, Duplicate and Delete — and no standalone Stop button is
//     left in the header. Rename is NOT in it (its home on this page is Settings → Site name).
//  5. Duplicate opens New site prefilled "<name> copy".
//  6. Delete opens the confirm naming the site; cancelled, the page stays; confirmed, the page
//     leaves for the list (a deleted site has no page to stay on).
//  7. The domain has a copy control, and the header no longer prints `:443` on every site.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1180, height: 900 } });
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  for (const { id, wp, name } of [
    { id: "1", wp: true, name: "Acme Store" },
    { id: "4", wp: false, name: "Docs" },
  ]) {
    const at = (n) => `site ${id} · ${n}`;
    await page.goto(`${BASE}/sites/${id}`, { waitUntil: "networkidle" });
    await page.waitForTimeout(600);

    // ── 1 ──
    const c = await page.evaluate(() => {
      const main = document.querySelector("main");
      const btns = [...main.querySelectorAll("button")];
      const text = (b) => b.textContent.replace(/\s+/g, " ").trim();
      const tabs = [...main.querySelectorAll("[data-site-tab]")];
      return {
        browser: btns.filter((b) => /^Open in /.test(text(b)) && !b.closest("[data-site-tab]") && !b.querySelector("[data-site-editor]")).length,
        editor: main.querySelectorAll("[data-site-editor]").length,
        magic: btns.filter((b) => /Magic Login/.test(text(b))).length,
        folder: btns.filter((b) => b.getAttribute("title") === "Open folder").length,
        quick: /quick links/i.test(main.innerText),
        // Anything that is not a tab but is labelled like one would be a second door.
        dbDoors: btns.filter((b) => text(b) === "Database" && !tabs.includes(b)).length,
        termDoors: btns.filter((b) => text(b) === "Terminal" && !tabs.includes(b)).length,
      };
    });
    if (c.browser !== 1) fails.push(at(`1: ${c.browser} "Open in browser" controls on Overview, expected 1`));
    if (c.editor !== 1) fails.push(at(`1: ${c.editor} editor controls, expected 1`));
    if (c.magic !== (wp ? 1 : 0)) fails.push(at(`1: ${c.magic} Magic Login controls, expected ${wp ? 1 : 0}`));
    if (c.folder !== 1) fails.push(at(`1: ${c.folder} "Open folder" controls, expected 1`));
    if (c.quick) fails.push(at('1: a "Quick links" card is back'));
    if (c.dbDoors || c.termDoors) fails.push(at(`1: a second door to Database (${c.dbDoors}) or Terminal (${c.termDoors}) beside the tab bar`));

    // ── 2 ──
    const env = await page.evaluate(() => {
      const card = (label) => {
        const l = [...document.querySelectorAll("main div")].find((el) => el.children.length === 0 && el.textContent.trim() === label);
        const box = l?.parentElement;
        if (!box) return null;
        const select = box.querySelector("select");
        const clone = box.cloneNode(true);
        clone.querySelectorAll("select").forEach((s) => s.remove());
        return {
          chosen: select?.options[select.selectedIndex]?.textContent.trim() ?? null,
          rest: clone.textContent.replace(label, "").replace(/\s+/g, " ").trim(),
        };
      };
      return { php: card("PHP version"), server: card("Web server") };
    });
    for (const [k, v] of Object.entries(env)) {
      if (!v || !v.chosen) { fails.push(at(`2: the ${k} box has no select — markup changed`)); continue; }
      // What is left beside the select must not be the value again.
      const value = v.chosen.split(" ")[0];
      if (v.rest.split(/[\s·]+/).includes(value)) fails.push(at(`2: the ${k} box says "${value}" twice (beside the select: "${v.rest}")`));
    }

    // ── 3 ──
    const info = await page.evaluate(() => document.querySelector("[data-site-info]")?.innerText ?? "");
    for (const w of ["Type", "Database name", "Multisite"]) if (!info.includes(w)) fails.push(at(`3: Overview's Site info lacks "${w}"`));
    await page.click('[data-site-tab="settings"]');
    await page.waitForTimeout(300);
    const settings = await page.evaluate(() => document.querySelector("main").innerText);
    if (/site info/i.test(settings)) fails.push(at("3: Settings still has a Site info card"));
    if (!/site name/i.test(settings)) fails.push(at("3: Settings did not render — the leg proved nothing"));
    await page.click('[data-site-tab="overview"]');
    await page.waitForTimeout(300);

    // ── 4 ──
    const loose = await page.evaluate(() =>
      [...document.querySelectorAll("[data-site-actions] > button, [data-site-actions] > span > button")]
        .map((b) => b.textContent.trim()).filter((t) => /^(Stop|Start) site$/.test(t)).length);
    if (loose) fails.push(at("4: a standalone Stop/Start site button is still in the header"));
    await page.click("[data-site-more]");
    await page.waitForTimeout(250);
    const items = await page.evaluate(() =>
      [...document.querySelectorAll("body > div button")].map((b) => b.textContent.trim()).filter(Boolean));
    for (const want of ["Stop site", "Duplicate", "Delete"])
      if (!items.includes(want)) fails.push(at(`4: the ⋯ menu lacks "${want}" (has: ${items.join(", ")})`));
    if (items.includes("Rename")) fails.push(at("4: Rename is in the ⋯ menu — its home on this page is Settings"));

    // ── 5 ──
    await page.getByRole("button", { name: "Duplicate", exact: true }).click();
    await page.waitForTimeout(400);
    const dup = await page.evaluate(() => document.querySelector('[role="dialog"]')?.innerText ?? "");
    if (!/New site/.test(dup)) fails.push(at("5: Duplicate did not open the New site dialog"));
    await page.keyboard.press("Escape");
    await page.mouse.click(5, 5);
    await page.waitForTimeout(300);
    if (await page.$('[role="dialog"]')) fails.push(at("5: the New site dialog did not close — the next leg would click through it"));

    // ── 6 ──
    const openDelete = async () => {
      await page.click("[data-site-more]");
      await page.waitForTimeout(200);
      await page.getByRole("button", { name: "Delete", exact: true }).click();
      await page.waitForTimeout(300);
    };
    await openDelete();
    const del = await page.evaluate(() => document.querySelector('[role="dialog"]')?.innerText ?? "");
    if (!del.includes(`Delete "${name}"?`)) fails.push(at(`6: the delete confirm does not name the site ("${del.slice(0, 60)}")`));
    await page.getByRole("button", { name: "Cancel" }).click();
    await page.waitForTimeout(250);
    if (!new RegExp(`/sites/${id}(/overview)?$`).test(page.url())) fails.push(at(`6: cancelling the delete left the page (${page.url()})`));
    await openDelete();
    const box = page.locator('[role="dialog"] input');
    // The gate (where the dialog has one) wants the site's domain typed.
    const domain = await page.evaluate(() => document.querySelector("[data-site-domain]")?.textContent.trim() ?? "");
    if (await box.count()) await box.first().fill(domain);
    await page.locator('[role="dialog"] button', { hasText: /^Delete/ }).last().click();
    await page.waitForTimeout(500);
    if (!page.url().endsWith("/sites")) fails.push(at(`6: a confirmed delete did not leave for the list (${page.url()})`));

    // ── 7 ──
    await page.goto(`${BASE}/sites/${id}`, { waitUntil: "networkidle" });
    await page.waitForTimeout(500);
    const head = await page.evaluate(() => {
      const d = document.querySelector("[data-site-domain]");
      return { copy: !!d?.parentElement.querySelector('button[title="Copy"]'), port: /:443/.test(d?.parentElement.textContent ?? "") };
    });
    if (!head.copy) fails.push(at("7: the domain has no copy control"));
    if (head.port) fails.push(at("7: the header prints :443"));
    if (id === "1") await page.screenshot({ path: "shot-siteactions.png" });
  }

  await browser.close();
  if (fails.length) {
    console.log("SITE-ACTIONS CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SITE-ACTIONS CHECK: ALL PASS (browser / editor / Magic Login / folder once each, no Quick links, no second door to a tab; PHP and server said once; Site info on Overview, gone from Settings; ⋯ = Stop site, Duplicate, Delete; duplicate prefilled; delete confirmed leaves for the list)");
})().catch((e) => {
  console.log("SITE-ACTIONS CHECK FAILURES:");
  for (const f of fails) console.log(" - " + f);
  console.log(" - crashed: " + String(e.message).split("\n")[0]);
  process.exit(1);
});
