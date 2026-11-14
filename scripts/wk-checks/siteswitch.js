// WebKit check: the site switcher — `Sites / [W] name ▾` on a site page.
//
// The ask (owner, 14 Nov 2026): going from one site to another was back → find the row
// among 25 → click, and the tab was lost on the way. The breadcrumb's chevron now lists
// every site, searchable, and choosing one KEEPS THE TAB.
//
// "Keeps the tab" has a trap, and it is why the rule is one function (`resolveSiteTab`)
// that the page itself calls: WordPress and Live exist only on a WordPress site, so the
// kept tab can be one the target does not have. Before the rule, `/sites/<a Laravel
// site>/wordpress` rendered the WordPress manager over a site with no WordPress in it.
//
// Run against the production-shaped list (`rex-mock-sites=many`: 27 sites, two of them
// scratch, one name with no break, one very long) — a four-site fixture cannot fail the
// "every site is reachable" half, because four rows never scroll.
//
// Legs:
//  1. The popover opens focused on its search box, lists EVERY site (none capped away),
//     checks the current one, and puts scratch sites under their own heading.
//  2. A tab every site has is kept: Database on a WordPress site → Database on a Laravel one.
//  3. A tab the target lacks is Overview, not a blank or a wrong panel: WordPress →
//     a Laravel site; and the same answer for a TYPED url, which no switcher produced.
//  4. Search matches name, domain and an EXTRA domain (the Sites filter's three); a
//     matchless query says so; the last row of the full list can be reached by keyboard
//     and is scrolled into view.
//  5. Esc closes and hands the focus back to the trigger; an outside click closes.
//  6. The list never leaves the window at 940 px, and the trigger truncates a long name.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const fails = [];
(async () => {
  const browser = await webkit.launch();
  const ctx = await browser.newContext({ viewport: { width: 1180, height: 800 } });
  await ctx.addInitScript(() => window.localStorage.setItem("rex-mock-sites", "many"));
  const page = await ctx.newPage();
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  const open = async () => {
    await page.click("[data-site-switch-trigger]");
    await page.waitForSelector("[data-site-switcher]");
  };
  const activeTab = () =>
    page.evaluate(() => {
      const b = [...document.querySelectorAll("[data-site-tab]")].find((x) => x.hasAttribute("data-active"));
      return b ? b.getAttribute("data-site-tab") : null;
    });
  const title = () => page.evaluate(() => document.querySelector("[data-site-title]")?.textContent.trim());

  // ── 1 ────────────────────────────────────────────────────────────────────────────
  await page.goto(`${BASE}/sites/1/database`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  await open();
  const s1 = await page.evaluate(() => ({
    focused: document.activeElement?.getAttribute("aria-label"),
    rows: document.querySelectorAll("[data-switch-site]").length,
    current: document.querySelector("[data-switch-current]")?.closest("[data-switch-site]")?.getAttribute("data-switch-site"),
    heading: [...document.querySelectorAll("[data-site-switcher] div")].some((d) => d.textContent.trim() === "Scratch"),
    scratchLast: [...document.querySelectorAll("[data-switch-site]")].slice(-2).map((r) => r.getAttribute("data-switch-site")).sort().join(","),
  }));
  if (s1.focused !== "Switch site") fails.push(`1: the search box is not focused on open (focus: ${s1.focused})`);
  if (s1.rows !== 27) fails.push(`1: ${s1.rows} rows for 27 sites — the list is capped or filtered`);
  if (s1.current !== "1") fails.push(`1: the current site is not the checked one (${s1.current})`);
  if (!s1.heading) fails.push("1: scratch sites have no heading of their own");
  if (s1.scratchLast !== "s0,s1") fails.push(`1: the scratch sites are not the last group (${s1.scratchLast})`);
  await page.screenshot({ path: "shot-siteswitch-open.png" });

  // ── 2 ────────────────────────────────────────────────────────────────────────────
  await page.click('[data-switch-site="2"]'); // Portfolio — Laravel
  await page.waitForTimeout(400);
  if (!page.url().endsWith("/sites/2/database")) fails.push(`2: Database was not kept (${page.url()})`);
  if ((await activeTab()) !== "database") fails.push(`2: the Database tab is not the active one (${await activeTab()})`);
  if ((await title()) !== "Portfolio") fails.push(`2: landed on "${await title()}", not Portfolio`);

  // ── 3 ────────────────────────────────────────────────────────────────────────────
  await page.goto(`${BASE}/sites/1/wordpress`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  if ((await activeTab()) !== "wordpress") fails.push(`3: a WordPress site's WordPress tab is not active (${await activeTab()})`);
  await open();
  await page.click('[data-switch-site="2"]');
  await page.waitForTimeout(500);
  const t3 = await activeTab();
  const has3 = await page.evaluate(() => !!document.querySelector('[data-site-tab="wordpress"]'));
  if (has3) fails.push("3: a Laravel site offers a WordPress tab");
  if (t3 !== "overview") fails.push(`3: WordPress → a Laravel site shows "${t3}", not Overview`);
  const env3 = await page.evaluate(() => document.body.innerText.includes("ENVIRONMENT") || document.body.innerText.includes("Environment"));
  if (!env3) fails.push("3: Overview's own content is not what rendered");
  // The typed URL — no switcher involved. Same answer.
  await page.goto(`${BASE}/sites/4/live`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  if ((await activeTab()) !== "overview") fails.push(`3: a typed /sites/<Blank PHP>/live shows "${await activeTab()}", not Overview`);
  await page.goto(`${BASE}/sites/4/repository`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  if ((await activeTab()) !== "overview") fails.push(`3: a typed /repository on a site with no checkout shows "${await activeTab()}"`);

  // ── 4 ────────────────────────────────────────────────────────────────────────────
  await page.goto(`${BASE}/sites/1`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  await open();
  const rowsFor = async (q) => {
    await page.fill('[data-site-switcher] input', q);
    await page.waitForTimeout(120);
    return page.evaluate(() => [...document.querySelectorAll("[data-switch-site]")].map((r) => r.getAttribute("data-switch-site")));
  };
  const byName = await rowsFor("hisab");
  if (byName.length !== 1) fails.push(`4: "hisab" (a name) matched ${byName.length} rows`);
  const byDomain = await rowsFor("portfolio.rex");
  if (byDomain.join() !== "2") fails.push(`4: "portfolio.rex" (a domain) matched [${byDomain}]`);
  const extra = await page.evaluate(async () => {
    // What the page's own mock says site 1 also answers on — read, not typed here.
    const m = await import("/src/lib/mock.ts");
    return Object.entries(m.mockAllSiteDomains())[0] ?? null;
  });
  if (!extra) fails.push("4: the fixture has no extra domain — the third search leg proved nothing");
  else {
    const byExtra = await rowsFor(extra[1][0]);
    if (!byExtra.includes(extra[0])) fails.push(`4: the extra domain "${extra[1][0]}" did not find site ${extra[0]} ([${byExtra}])`);
  }
  await rowsFor("zzz-no-such-site");
  const empty = await page.evaluate(() => document.querySelector("[data-site-switcher]").innerText);
  if (!empty.includes("No sites match")) fails.push(`4: a matchless query does not say so ("${empty.trim()}")`);
  await rowsFor("");
  // ArrowUp from the first row wraps to the LAST: it must be on screen inside the list.
  await page.keyboard.press("ArrowUp");
  await page.waitForTimeout(200);
  const last = await page.evaluate(() => {
    const cur = document.querySelector("[data-switch-site][data-cursor]");
    const list = document.getElementById("site-switcher-list");
    if (!cur || !list) return null;
    const c = cur.getBoundingClientRect(), l = list.getBoundingClientRect();
    const all = [...document.querySelectorAll("[data-switch-site]")];
    return { isLast: all[all.length - 1] === cur, inView: c.top >= l.top - 1 && c.bottom <= l.bottom + 1, scrolls: list.scrollHeight > list.clientHeight };
  });
  if (!last) fails.push("4: no cursor row after ArrowUp");
  else {
    if (!last.scrolls) fails.push("4: 27 rows do not scroll — the fixture proved nothing about reaching the last one");
    if (!last.isLast) fails.push("4: ArrowUp from the top did not land on the last row");
    if (!last.inView) fails.push("4: the cursor row is not scrolled into view");
  }
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  if (!/\/sites\/s[01]$/.test(page.url())) fails.push(`4: Enter on the last row went to ${page.url()}`);

  // ── 5 ────────────────────────────────────────────────────────────────────────────
  await open();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
  const s5 = await page.evaluate(() => ({
    open: !!document.querySelector("[data-site-switcher]"),
    focus: document.activeElement?.hasAttribute("data-site-switch-trigger"),
  }));
  if (s5.open) fails.push("5: Esc did not close the switcher");
  if (!s5.focus) fails.push("5: Esc did not hand the focus back to the trigger");
  await open();
  await page.mouse.click(900, 600);
  await page.waitForTimeout(150);
  if (await page.$("[data-site-switcher]")) fails.push("5: an outside click did not close the switcher");

  // ── 6 ────────────────────────────────────────────────────────────────────────────
  await page.setViewportSize({ width: 940, height: 600 });
  await page.goto(`${BASE}/sites/m20`, { waitUntil: "networkidle" }); // the very long name
  await page.waitForTimeout(500);
  await open();
  const s6 = await page.evaluate(() => {
    const p = document.querySelector("[data-site-switcher]").parentElement.getBoundingClientRect();
    const t = document.querySelector("[data-site-switch-trigger]").getBoundingClientRect();
    return { right: p.right, bottom: p.bottom, w: innerWidth, h: innerHeight, trigger: t.width, overflow: document.documentElement.scrollWidth > innerWidth };
  });
  if (s6.right > s6.w || s6.bottom > s6.h) fails.push(`6: the popover leaves the window (${Math.round(s6.right)}×${Math.round(s6.bottom)} in ${s6.w}×${s6.h})`);
  if (s6.trigger > 300) fails.push(`6: the trigger did not truncate a long name (${Math.round(s6.trigger)} px)`);
  if (s6.overflow) fails.push("6: the page scrolls sideways");
  await page.screenshot({ path: "shot-siteswitch-940.png" });

  await browser.close();
  if (fails.length) {
    console.log("SITE-SWITCH CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SITE-SWITCH CHECK: ALL PASS (27 sites listed, current checked, scratch grouped; tab kept; a tab the target lacks → Overview, typed URLs too; search by name/domain/extra domain; keyboard reaches the last row; Esc returns focus; fits at 940)");
})();
