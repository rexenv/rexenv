// WebKit check: the Sites list — a quiet row with three actions, one menu two ways, and a
// view that survives a visit to a site (docs/PLAN-sites-ux.md T4).
//
// What the tree said on 14 Nov 2026: the row's quick actions were `opacity-0` until hover
// and gone from layout below `lg`, so a narrow window offered a site nothing but `⋮`;
// Magic Login was not on the row; every row carried a lock and a "Running" pill (50 marks
// on 25 sites that said nothing); the filter, sort and search reset on every return from a
// site; and the header button "All sites ▾" was the bulk start/stop menu.
//
// Run against the production-shaped list (`rex-mock-sites=many`: 27 sites — a no-break
// name, a very long one, one stopped by the user, one the stack is not serving, one
// insecure, external folders, two scratch) at 940 / 1100 / 1440. The tidy four-site
// fixture is the one that let the header overflow ship (16 Oct 2026).
//
// Legs:
//  1. Per width: no row reaches past the list, the page does not scroll sideways, and the
//     three actions are VISIBLE without a hover — in layout, opaque — on every row.
//  2. A normal row is quiet: no status words, no lock. Exactly the exception rows speak —
//     "Stopped by you", "Stopped" (stack), the one insecure site's open lock.
//  3. Magic Login is on WordPress rows only (the slot held on the others, so the columns
//     line up); OFF on a site the user stopped. Where nothing serves the site, both actions
//     are live and SAY they start rexenv's services first (`servefirst.js`); the browser
//     action is plain and live on a site the user stopped (rexenv's own stopped page).
//  4. Pressing a row action — by pointer or by Enter — does not also open the site.
//  5. `⋮` and a right-click open the SAME menu; the right-click opens it at the pointer.
//     The menu reaches the site page's tabs (Database → that site's Database tab).
//  6. The header: the "All" tabs carry no number; the bulk menu is `⋯` and no control
//     reads "All sites"; with one site type the type filter is not drawn at all.
//  7. The view survives a visit: status filter, search, sort and scroll are as they were.
//     "Show all sites" clears the narrowing.
//  8. Keys: `/` focuses the filter, ↓ ↑ move between rows, Enter opens the focused one.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
(async () => {
  const browser = await webkit.launch();
  const mk = async (mode, width = 1180) => {
    const ctx = await browser.newContext({ viewport: { width, height: 800 } });
    await ctx.addInitScript((m) => window.localStorage.setItem("rex-mock-sites", m), mode);
    const page = await ctx.newPage();
    page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
    await page.goto(`${BASE}/sites`, { waitUntil: "networkidle" });
    await page.waitForTimeout(700);
    return { ctx, page };
  };

  // ── 1 ────────────────────────────────────────────────────────────────────────────
  for (const width of [940, 1100, 1440]) {
    const { ctx, page } = await mk("many", width);
    const m = await page.evaluate(() => {
      const list = document.querySelector("[data-sites-list]").getBoundingClientRect();
      const out = { rows: 0, over: [], hidden: [], sideways: document.documentElement.scrollWidth > innerWidth + 1 };
      for (const row of document.querySelectorAll("[data-site-row]")) {
        out.rows++;
        const id = row.getAttribute("data-site-row");
        for (const el of row.querySelectorAll("*")) {
          const r = el.getBoundingClientRect();
          if (r.width && r.right > list.right + 0.5) { out.over.push(`${id}: <${el.tagName.toLowerCase()}> ends ${Math.round(r.right - list.right)}px past the list`); break; }
        }
        const acts = row.querySelector("[data-site-row-actions]");
        const btn = acts?.querySelector('button[aria-label="Open in browser"]');
        const b = btn?.getBoundingClientRect();
        let opacity = 1;
        for (let el = btn; el && el !== row; el = el.parentElement) opacity *= parseFloat(getComputedStyle(el).opacity);
        // A DISABLED button is dimmed by the button's own rule; that is a state, not hiding.
        if (!btn || !b.width || (opacity < 0.99 && !btn.disabled)) out.hidden.push(`${id} (opacity ${opacity})`);
      }
      return out;
    });
    if (m.rows !== 27) fails.push(`1 @${width}: ${m.rows} rows for 27 sites`);
    if (m.sideways) fails.push(`1 @${width}: the page scrolls sideways`);
    for (const o of m.over.slice(0, 4)) fails.push(`1 @${width}: ${o}`);
    if (m.hidden.length) fails.push(`1 @${width}: actions not visible without a hover on ${m.hidden.slice(0, 4).join(", ")}`);
    await page.screenshot({ path: `shot-sitesrow-${width}.png` });
    await ctx.close();
  }

  const { ctx, page } = await mk("many");

  // ── 2 ────────────────────────────────────────────────────────────────────────────
  const q = await page.evaluate(() => {
    const row = (id) => document.querySelector(`[data-site-row="${id}"]`);
    const words = [...document.querySelectorAll('[data-probe="status-words"]')].map((w) => `${w.closest("[data-site-row]").getAttribute("data-site-row")}:${w.textContent.trim()}`);
    return {
      words: words.sort(),
      locks: [...document.querySelectorAll('[data-probe="insecure"]')].map((l) => l.closest("[data-site-row]").getAttribute("data-site-row")),
      normalText: row("m0").innerText.replace(/\s+/g, " ").trim(),
      dots: { m0: row("m0").querySelector("[data-site-dot]").getAttribute("data-site-dot"), m9: row("m9").querySelector("[data-site-dot]").getAttribute("data-site-dot"), s3: row("3").querySelector("[data-site-dot]").getAttribute("data-site-dot") },
      pills: document.querySelectorAll("[data-site-row] .rounded-full.border").length,
    };
  });
  if (q.words.join("|") !== "3:Services off|m9:Stopped by you") fails.push(`2: the rows that speak are [${q.words}] — expected the one nothing serves ("Services off") and the one stopped by the user, only`);
  if (q.locks.join() !== "m13") fails.push(`2: locks on [${q.locks}] — expected the one insecure site (m13) only`);
  if (/Running|Stopped/.test(q.normalText)) fails.push(`2: a normal row still says its status in words ("${q.normalText}")`);
  if (q.dots.m0 !== "running" || q.dots.m9 !== "stopped" || q.dots.s3 !== "stopped") fails.push(`2: dots ${JSON.stringify(q.dots)}`);

  // ── 3 ────────────────────────────────────────────────────────────────────────────
  const a = await page.evaluate(() => {
    const of = (id) => {
      const row = document.querySelector(`[data-site-row="${id}"]`);
      const b = row.querySelector('button[aria-label="Open in browser"]');
      const ml = row.querySelector('button[aria-label="Magic Login"]');
      return { browser: !!b && !b.disabled, magic: ml ? !ml.disabled : null, x: b.getBoundingClientRect().left,
        browserStarts: b.hasAttribute("data-starts-services"), magicStarts: !!ml && ml.hasAttribute("data-starts-services") };
    };
    return { wp: of("m0"), laravel: of("2"), php: of("4"), byUser: of("m9"), stack: of("3") };
  });
  if (a.wp.magic !== true) fails.push("3: a serving WordPress row has no live Magic Login");
  if (a.laravel.magic !== null || a.php.magic !== null) fails.push("3: Magic Login on a row that is not WordPress");
  if (Math.abs(a.wp.x - a.laravel.x) > 0.5 || Math.abs(a.wp.x - a.php.x) > 0.5) fails.push(`3: the action columns do not line up across site types (${a.wp.x} / ${a.laravel.x} / ${a.php.x})`);
  if (a.byUser.magic !== false) fails.push("3: Magic Login is live on a site the user stopped");
  if (!a.byUser.browser) fails.push("3: Open in browser is disabled on a site the user stopped — its URL shows rexenv's stopped page");
  // A site nothing serves (and the user did not stop): both actions are LIVE and SAY they start
  // rexenv's services first — since 14 Nov 2026 (`servefirst.js` proves what the click does).
  // Before that they were disabled with "Start all first": never a plain link to a dead URL.
  if (!a.stack.browser || !a.stack.browserStarts) fails.push("3: on a site nothing serves the browser action is dead, or does not say it starts the services");
  if (a.stack.magic !== true || !a.stack.magicStarts) fails.push("3: on a site nothing serves Magic Login is dead, or does not say it starts the services");
  if (a.wp.browserStarts || a.wp.magicStarts || a.byUser.browserStarts) fails.push("3: an action says it starts the services on a serving or user-stopped site");

  // ── 4 ────────────────────────────────────────────────────────────────────────────
  await page.click('[data-site-row="m0"] button[aria-label="Open in browser"]');
  await page.click('[data-site-row="m0"] button[aria-label="Magic Login"]');
  await page.waitForTimeout(300);
  if (!page.url().endsWith("/sites")) fails.push(`4: a row action also opened the site (${page.url()})`);
  // The same by keyboard: Enter on one of the row's BUTTONS is that button's key. The row
  // listens for Enter too (it opens the site), and a key bubbling up from a button inside
  // it must not count as the row's.
  await page.focus('[data-site-row="m0"] button[aria-label="Open in browser"]');
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  if (!page.url().endsWith("/sites")) { fails.push(`4: Enter on a row's button also opened the site (${page.url()})`); await page.goBack(); await page.waitForTimeout(400); }

  // ── 5 ────────────────────────────────────────────────────────────────────────────
  const menuItems = () => page.evaluate(() => {
    const m = document.querySelector('[role="menu"]');
    return m ? { items: [...m.querySelectorAll("button")].map((b) => b.textContent.trim()), left: m.getBoundingClientRect().left, top: m.getBoundingClientRect().top } : null;
  });
  await page.click('[data-site-row="m1"] button[aria-label="More actions"]');
  await page.waitForTimeout(250);
  const byDots = await menuItems();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
  const box = await page.locator('[data-site-row="m1"]').boundingBox();
  const px = box.x + 120, py = box.y + box.height / 2;
  await page.mouse.click(px, py, { button: "right" });
  await page.waitForTimeout(250);
  const byRight = await menuItems();
  // Found on the real app, not here: WKWebView selects the word under a right-click, so the
  // menu opened beside a highlighted "pg.rex". Playwright's WebKit does not select — all this
  // leg can hold is the rule that prevents it.
  const selectable = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector('[data-site-row="m1"]'));
    return cs.webkitUserSelect || cs.userSelect;
  });
  if (selectable !== "none") fails.push(`5: the row's text is selectable (user-select: ${selectable}) — a right-click in WKWebView highlights the word under it`);
  if (!byDots) fails.push("5: ⋮ opened no menu");
  if (!byRight) fails.push("5: a right-click opened no menu");
  if (byDots && byRight) {
    if (byDots.items.join("|") !== byRight.items.join("|")) fails.push(`5: ⋮ and right-click list different things ([${byDots.items}] vs [${byRight.items}])`);
    for (const want of ["Stop site", "WordPress", "Database", "Terminal", "Logs", "Open folder", "Rename", "Duplicate", "Copy domain", "Delete"])
      if (!byRight.items.includes(want)) fails.push(`5: the menu lacks "${want}"`);
    if (Math.abs(byRight.left - px) > 12 || byRight.top < py - 2 || byRight.top > py + 14) fails.push(`5: the right-click menu is not at the pointer (${Math.round(byRight.left)},${Math.round(byRight.top)} for ${Math.round(px)},${Math.round(py)})`);
    if (!page.url().endsWith("/sites")) fails.push("5: the right-click also opened the site");
  }
  await page.getByRole("button", { name: "Database", exact: true }).click();
  await page.waitForTimeout(400);
  if (!page.url().endsWith("/sites/m1/database")) fails.push(`5: the menu's Database went to ${page.url()}`);
  await page.goBack();
  await page.waitForTimeout(500);

  // ── 6 ────────────────────────────────────────────────────────────────────────────
  const h = await page.evaluate(() => ({
    all: [...document.querySelectorAll('[data-filter-tab="all"]')].map((t) => t.textContent.trim()),
    running: document.querySelector('[data-filter-tab="running"]')?.textContent.trim(),
    types: [...document.querySelectorAll("[data-filter-tab]")].map((t) => t.getAttribute("data-filter-tab")),
    allSitesBtn: [...document.querySelectorAll("header button")].some((b) => /All sites/.test(b.textContent)),
    bulk: !!document.querySelector("[data-sites-bulk]"),
  }));
  if (h.all.some((t) => /\d/.test(t))) fails.push(`6: an "All" tab still carries a number ([${h.all}])`);
  if (!/25/.test(h.running ?? "")) fails.push(`6: the Running tab lost its count ("${h.running}") — that number is what clicking it shows`);
  if (h.allSitesBtn) fails.push('6: a header control still reads "All sites"');
  if (!h.bulk) fails.push("6: no bulk start/stop control in the header");
  if (!["wordpress", "laravel", "php"].every((t) => h.types.includes(t))) fails.push(`6: the type filter does not offer the three types this list has ([${h.types}])`);
  await page.click("[data-sites-bulk]");
  await page.waitForTimeout(200);
  const bulk = (await menuItems())?.items ?? [];
  if (!bulk.some((t) => /^Start all sites/.test(t)) || !bulk.some((t) => /^Stop all sites/.test(t))) fails.push(`6: the bulk menu is [${bulk}]`);
  await page.keyboard.press("Escape");
  {
    const one = await mk("wponly");
    const types = await one.page.evaluate(() => [...document.querySelectorAll("[data-filter-tab]")].map((t) => t.getAttribute("data-filter-tab")));
    const rows = await one.page.evaluate(() => document.querySelectorAll("[data-site-row]").length);
    if (rows < 10) fails.push(`6: the one-type fixture rendered ${rows} rows — the leg proved nothing`);
    if (types.some((t) => ["wordpress", "laravel", "php"].includes(t))) fails.push(`6: a type filter is drawn for a list with one type ([${types}])`);
    await one.ctx.close();
  }

  // ── 7 ────────────────────────────────────────────────────────────────────────────
  // Scroll, on the unfiltered list.
  await page.evaluate(() => { document.querySelector("[data-sites-list]").scrollTop = 420; });
  await page.waitForTimeout(150);
  const before = await page.evaluate(() => document.querySelector("[data-sites-list]").scrollTop);
  if (before < 300) fails.push(`7: the list did not scroll (${before}) — the leg proved nothing`);
  await page.click('[data-site-row="m12"]');
  await page.waitForTimeout(400);
  if (!page.url().endsWith("/sites/m12")) fails.push(`7: the row did not open its site (${page.url()})`);
  await page.click("text=Sites >> nth=1").catch(() => {});
  if (!page.url().endsWith("/sites")) { await page.goBack(); }
  await page.waitForTimeout(500);
  const after = await page.evaluate(() => document.querySelector("[data-sites-list]").scrollTop);
  if (Math.abs(after - before) > 2) fails.push(`7: the scroll position was not restored (${before} → ${after})`);
  // Filter + search + sort.
  await page.click('[data-filter-tab="running"]');
  await page.fill("[data-topbar-search]", "ea");
  await page.click('button[title="Change sort order"]');
  await page.waitForTimeout(200);
  const view = () => page.evaluate(() => ({
    filter: [...document.querySelectorAll("[data-filter-tab]")].filter((t) => t.className.includes("bg-brand-tint-bg")).map((t) => t.getAttribute("data-filter-tab")).join(),
    query: document.querySelector("[data-topbar-search]").value,
    sort: document.querySelector('button[title="Change sort order"]').textContent.trim(),
    rows: [...document.querySelectorAll("[data-site-row]")].map((r) => r.getAttribute("data-site-row")).join(),
  }));
  const v1 = await view();
  if (!v1.rows || v1.rows.split(",").length >= 27) fails.push(`7: the view did not narrow ([${v1.rows}]) — the leg proved nothing`);
  await page.click("[data-site-row] >> nth=0");
  await page.waitForTimeout(400);
  await page.goBack();
  await page.waitForTimeout(500);
  const v2 = await view();
  if (JSON.stringify(v1) !== JSON.stringify(v2)) fails.push(`7: the view changed across a site visit (${JSON.stringify(v1)} → ${JSON.stringify(v2)})`);
  await page.fill("[data-topbar-search]", "zzz-nothing");
  await page.waitForTimeout(200);
  await page.click("text=Show all sites");
  await page.waitForTimeout(250);
  const v3 = await view();
  if (v3.query !== "" || v3.filter !== "all,all" || v3.rows.split(",").length !== 27) fails.push(`7: "Show all sites" left ${JSON.stringify({ q: v3.query, f: v3.filter, n: v3.rows.split(",").length })}`);

  // ── 8 ────────────────────────────────────────────────────────────────────────────
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press("/");
  await page.waitForTimeout(150);
  if (!(await page.evaluate(() => document.activeElement?.hasAttribute("data-topbar-search")))) fails.push("8: `/` did not focus the filter");
  const ids = await page.evaluate(() => [...document.querySelectorAll("[data-site-row]")].map((r) => r.getAttribute("data-site-row")));
  await page.focus(`[data-site-row="${ids[0]}"]`);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  const focused = await page.evaluate(() => document.activeElement?.getAttribute("data-site-row"));
  if (focused !== ids[1]) fails.push(`8: ↓ ↓ ↑ from the first row is on ${focused}, expected ${ids[1]}`);
  // The ring is the only thing that says which row Enter will open, so it must be SEEN: it
  // was `ring-white/15`, invisible on the light theme (found on the real app, not here).
  const ring = await page.evaluate(() => {
    // Tailwind's ring is one layer of a multi-layer box-shadow (the first is a white
    // zero-width offset): the ring is visible if ANY layer is a non-white, opaque-enough colour.
    const shadow = getComputedStyle(document.activeElement).boxShadow || "";
    const layers = [...shadow.matchAll(/rgba?\(([^)]+)\)/g)].map((m) => m[1].split(",").map((x) => parseFloat(x)));
    return { shadow, seen: layers.some((p) => !(p[0] > 240 && p[1] > 240 && p[2] > 240) && (p.length > 3 ? p[3] : 1) >= 0.5) };
  });
  if (!ring.seen) fails.push(`8: the keyboard-focused row's ring is not visible on a light surface (box-shadow: ${ring.shadow})`);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  if (!page.url().endsWith(`/sites/${ids[1]}`)) fails.push(`8: Enter on the focused row went to ${page.url()}`);

  await ctx.close();
  await browser.close();
  if (fails.length) {
    console.log("SITES-ROW CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SITES-ROW CHECK: ALL PASS (27 rows at 940/1100/1440: nothing past the list, three actions visible with no hover; a normal row is quiet, only the stopped and the insecure speak; Magic Login on WordPress rows, columns aligned, dead links not offered; ⋮ = right-click, at the pointer; All tabs carry no number, bulk is ⋯, no type filter for one type; filter/search/sort/scroll survive a site visit; / ↓ ↑ Enter)");
})().catch((e) => {
  console.log("SITES-ROW CHECK FAILURES:");
  for (const f of fails) console.log(" - " + f);
  console.log(" - crashed: " + String(e.message).split("\n")[0]);
  process.exit(1);
});
