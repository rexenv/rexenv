// WebKit check: the app-update notice in the app's own chrome
// (docs/archive/PLAN-update-notice.md S1/S2/S4, ledger #884 / #885).
//
// The only always-visible notice used to be a bare mono `0.6.0` beside Settings,
// read as the RUNNING version by the people it was meant to alert (owner,
// 15 Oct 2026), and it lagged a whole session because a background check told
// the window nothing. So this asserts, on the REAL Sidebar and TopBar:
//   1. the TopBar pill's `data-phase` per mocked state, with a verb and the
//      version where there is an offer, and nothing at all with no key pinned;
//   2. the sidebar never renders a bare version, and its pill exists only with
//      a notice and links to `?section=about` (Settings itself keeps going to
//      General);
//   3. no <a> inside an <a> (WKWebView makes the inner one dead);
//   4. the pill shows the CARD's check in flight, without running one;
//   5. S4: an `app-update` event flips both pills within one tick, with no
//      refetch loop of their own;
//   6. "Check for Updates…" lands on About with `check=1`: the CARD runs
//      exactly one check and drops the flag; no flag, no check.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const view = (state) => `${BASE}/dev/ui-review?view=updatenotice&state=${state}`;

// [mocked state, TopBar pill phase or null (no pill), rail pill expected]
const STATES = [
  ["none", "idle", false],
  ["skipped", "skipped", false],
  ["offered", "offered", true],
  ["refused", "refused", true],
  ["installed-pending", "pending", true],
  ["dark", null, false],
];

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  // 980 is the app window's minimum width; the fixture pins its frame there.
  for (const width of [980, 1440]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    page.on("pageerror", (e) => fails.push(`${width}: pageerror: ${e.message}`));
    page.on("console", (m) => m.type() === "error" && fails.push(`${width}: console.error: ${m.text()}`));
    const bar = () => page.locator('[data-probe="update-pill"][data-variant="bar"]');
    const rail = () => page.locator('[data-probe="update-pill"][data-variant="rail"]');

    for (const [state, phase, railWanted] of STATES) {
      const tag = `${width}/${state}`;
      await page.goto(view(state), { waitUntil: "networkidle" });
      // A landmark that renders regardless, so "no pill" is told apart from a
      // broken route.
      await page.waitForSelector('[data-probe="updatenotice-view"]', { timeout: 5000 });
      await page.waitForTimeout(400);

      if (phase === null) {
        if ((await bar().count()) !== 0) fails.push(`${tag}: a pill rendered with no key pinned`);
      } else {
        const got = await bar().first().getAttribute("data-phase").catch(() => null);
        if (got !== phase) fails.push(`${tag}: TopBar pill data-phase=${got}, expected ${phase}`);
      }
      if (phase === "offered" || phase === "refused") {
        const text = await bar().first().innerText();
        if (!/Update to/.test(text) || !text.includes("0.6.0"))
          fails.push(`${tag}: the pill must carry a verb AND the version, got "${text}"`);
        // The pill stays INSIDE the bar with its version whole at the window's
        // minimum width: the Sites header (search + three buttons + pill) only
        // fits there because the search shrinks — with a fixed 190 px search the
        // pill was pushed past the window's edge on the macOS VM (16 Oct 2026).
        const fit = await bar().first().evaluate((el) => {
          const bar = el.closest("header").getBoundingClientRect();
          const me = el.getBoundingClientRect();
          const ver = el.querySelector(".font-mono");
          const v = ver.getBoundingClientRect();
          return {
            inside: me.right <= bar.right + 0.5 && me.left >= bar.left - 0.5,
            versionWhole: v.width > 0 && ver.scrollWidth <= ver.clientWidth + 0.5 && v.right <= me.right + 0.5,
            right: Math.round(me.right), barRight: Math.round(bar.right),
          };
        });
        if (!fit.inside) fails.push(`${tag}: the pill overflows the bar (right ${fit.right} > ${fit.barRight})`);
        if (!fit.versionWhole) fails.push(`${tag}: the version is clipped inside the pill`);
        // The label truncates, never the version.
        const clipped = await bar()
          .first()
          .locator(".font-mono")
          .evaluate((el) => el.scrollWidth > el.clientWidth);
        if (clipped) fails.push(`${tag}: the version is clipped`);
      }
      if (phase === "pending") {
        const text = await bar().first().innerText();
        if (!/Restart to finish/.test(text)) fails.push(`${tag}: pending pill says "${text}"`);
      }
      if (phase === "idle" || phase === "skipped") {
        const title = await bar().first().getAttribute("title");
        if (title !== "Check for Updates…") fails.push(`${tag}: idle control titled "${title}"`);
      }

      const railCount = await rail().count();
      if (railWanted !== (railCount > 0)) fails.push(`${tag}: rail pill present=${railCount > 0}, expected ${railWanted}`);
      if (railCount > 0) {
        const href = await rail().first().getAttribute("href");
        if (!href || !href.endsWith("/settings?section=about"))
          fails.push(`${tag}: the rail pill goes to "${href}", not About`);
      }
      // The Settings nav itself keeps going to General.
      const settingsHref = await page.locator('aside a[href="/settings"]').count();
      if (settingsHref !== 1) fails.push(`${tag}: the Settings nav link is not plain /settings`);
      // No bare version anywhere in the sidebar — the misread this replaced.
      const aside = await page.locator("aside").innerText();
      if (/\b\d+\.\d+\.\d+\b/.test(aside)) fails.push(`${tag}: the sidebar shows a bare version: ${aside.match(/\b\d+\.\d+\.\d+\b/)[0]}`);
      const nested = await page.evaluate(() => document.querySelectorAll("a a").length);
      if (nested) fails.push(`${tag}: ${nested} <a> nested inside an <a>`);
    }

    // 4. The card's check in flight → the pill's spinner. The pill reads the
    // mutation; the CARD's button is what starts it (app_update_check never
    // settles in this fixture).
    await page.goto(view("checking"), { waitUntil: "networkidle" });
    await page.waitForSelector('[data-probe="updatenotice-view"]', { timeout: 5000 });
    await page.getByRole("button", { name: "Check now" }).click();
    await page.waitForTimeout(300);
    const during = await bar().first().getAttribute("data-phase").catch(() => null);
    if (during !== "checking") fails.push(`${width}/checking: pill data-phase=${during} while the card checks`);
    if (!(await bar().first().isDisabled().catch(() => false)))
      fails.push(`${width}/checking: the pill is clickable while a check runs`);
    const checks = await page.evaluate(() => (window.__rexUpdate ?? []).filter((c) => c.cmd === "app_update_check").length);
    if (checks !== 1) fails.push(`${width}/checking: ${checks} checks ran, expected exactly the card's one`);

    // 5. S4 — the backend's push. Start with nothing offered, let the "poller"
    // find a release, emit `app-update`, and expect both pills within a tick.
    await page.goto(view("none"), { waitUntil: "networkidle" });
    await page.waitForSelector('[data-probe="updatenotice-view"]', { timeout: 5000 });
    await page.waitForTimeout(300);
    const reads0 = await page.evaluate(() => window.__rexStateReads ?? 0);
    await page.waitForTimeout(1200);
    const readsIdle = await page.evaluate(() => window.__rexStateReads ?? 0);
    if (readsIdle !== reads0) fails.push(`${width}/event: the pills re-read the state ${readsIdle - reads0}× with no event — a poll`);
    await page.evaluate(() => {
      window.__rexOfferNow = true;
      return window.__rexEmit("app-update");
    });
    await page.waitForTimeout(500);
    const after = await bar().first().getAttribute("data-phase").catch(() => null);
    if (after !== "offered") fails.push(`${width}/event: after app-update the pill is ${after}, expected offered`);
    if ((await rail().count()) === 0) fails.push(`${width}/event: the rail pill did not appear after app-update`);

    // 5b. A SKIPPED version in production's shape — the backend withdraws the
    // offer (`offered: null`, `noOfferReason: "… was skipped"`, `skipped` set).
    // The card must show the "set aside … Undo" row from THAT shape: it used to
    // test `skipped === offered.version`, which the fixture satisfied and
    // production never did, so the row was unreachable and a skip could not be
    // undone from the GUI (macOS VM, 16 Oct 2026).
    await page.goto(`${BASE}/dev/ui-review?view=appupdate&state=skipped`, { waitUntil: "networkidle" });
    await page.waitForSelector('[data-probe="app-update"]', { timeout: 5000 });
    await page.waitForTimeout(300);
    const cardPhase = await page.locator('[data-probe="app-update"]').getAttribute("data-phase");
    if (cardPhase !== "skipped") fails.push(`${width}/card-skipped: the card's data-phase is ${cardPhase}, expected skipped (offered is null in production)`);
    if ((await page.getByRole("button", { name: "Undo" }).count()) !== 1)
      fails.push(`${width}/card-skipped: no Undo button — the skip cannot be undone from the GUI`);

    // 6. "Check for Updates…" arrives at About as `?check=1`, and the CARD runs
    // its own check exactly once — then drops the flag, so a reload does not
    // check again. Without the flag, opening About checks nothing.
    for (const [q, want] of [["&check=1", 1], ["", 0]]) {
      await page.goto(`${BASE}/dev/ui-review?view=appupdate&state=none${q}`, { waitUntil: "networkidle" });
      await page.waitForSelector('[data-probe="appupdate-view"]', { timeout: 5000 });
      await page.waitForTimeout(600);
      const n = await page.evaluate(() => (window.__rexUpdate ?? []).filter((c) => c.cmd === "app_update_check").length);
      if (n !== want) fails.push(`${width}/arrival${q || " (no flag)"}: ${n} checks ran, expected ${want}`);
      if (new URL(page.url()).searchParams.has("check")) fails.push(`${width}/arrival${q}: the check flag stayed in the URL`);
    }

    await page.screenshot({ path: `shot-appupdate-${width}.png`, fullPage: true });
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.error("appupdate: FAIL");
    for (const f of fails) console.error("  ✗ " + f);
    process.exit(1);
  }
  console.log("appupdate: the pills say what the cache says, link to About, never check, and hear the push");
})();
