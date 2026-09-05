// WebKit check: the "AI agents (MCP)" Logs tab, and its "Only this site" toggle
// narrows ONE machine-wide file to the lines naming THIS site — by id, in both
// shapes the writer uses — and says what it hid.
//
// The claim (ledger #516, 5 Sep 2026): `mcp.log` is common, not per-site — the
// feed table is one table, and `list_sites` names no site at all — so the
// per-site view is a FILTER over the file, client-side. What only a browser
// can show: that the toggle changes what is on screen and nothing else (the
// unfiltered count survives in the chip), that `(1)` does not match `(12)`
// (site 1's view must not pick up the reaped site `12`), that a bare-id line
// (`· site 12`, a deleted site) is matched by ITS site and by no other, and
// that the tab exists at all — a category added in Rust with no tab in
// `CATEGORY_TABS` is a compile error, but a tab that renders with no way to
// reach it is not.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// Mock ids from src/lib/ipc/index.ts MOCK_MCP_LOG: site 1 has three lines
// (two ok, one WARN), site 2 has two, site 12 one bare-id line, one site-less.
const SITE = "1";
const TOTAL = 7;
const MINE = 3;

const pane = async (page) =>
  page.evaluate(() => {
    const lines = [...document.querySelectorAll("div")]
      .filter((d) => d.children.length === 0 && /\[mcp\]/.test(d.textContent || ""))
      .map((d) => d.textContent.trim());
    const toggle = [...document.querySelectorAll("button")].find((b) =>
      /Only this site/.test(b.textContent || ""),
    );
    const empty = [...document.querySelectorAll("div")].find(
      (d) => d.children.length > 0 && /No agent activity names/.test(d.textContent || ""),
    );
    return {
      lines,
      toggle: toggle ? { pressed: toggle.getAttribute("aria-pressed"), text: toggle.textContent.trim() } : null,
      emptyShown: !!empty,
    };
  });

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/sites/${SITE}`, { waitUntil: "networkidle" });
  await page.click('button:has-text("Logs"), [role="tab"]:has-text("Logs")');
  await page.waitForTimeout(500);

  const tab = await page.$('button:has-text("AI agents (MCP)")');
  if (!tab) {
    fails.push("no 'AI agents (MCP)' tab — the agents category has no tab, or the mock offers no mcp.log");
  } else {
    await tab.click();
    await page.waitForTimeout(600);

    const all = await pane(page);
    if (all.lines.length !== TOTAL) fails.push(`unfiltered: expected ${TOTAL} lines, saw ${all.lines.length}`);
    if (!all.toggle) fails.push("no 'Only this site' toggle on the agents tab");
    else if (all.toggle.pressed !== "false") fails.push(`the toggle starts pressed (${all.toggle.pressed}) — the file is common, the default view must say so`);
    // The other tabs must NOT grow the toggle: it is a statement about one file.
    await page.click('button:has-text("Server (nginx/PHP)")');
    await page.waitForTimeout(300);
    if ((await pane(page)).toggle) fails.push("the 'Only this site' toggle leaked onto the Server tab");
    await page.click('button:has-text("AI agents (MCP)")');
    await page.waitForTimeout(300);

    await page.click('button:has-text("Only this site")');
    await page.waitForTimeout(400);
    const mine = await pane(page);
    if (mine.toggle?.pressed !== "true") fails.push("clicking the toggle did not press it");
    if (mine.lines.length !== MINE) fails.push(`filtered: expected ${MINE} lines for site ${SITE}, saw ${mine.lines.length}: ${JSON.stringify(mine.lines)}`);
    if (!mine.lines.every((l) => /\(1\)/.test(l))) fails.push(`a filtered line does not name site ${SITE}: ${JSON.stringify(mine.lines)}`);
    if (mine.lines.some((l) => /site 12\b|\(2\)/.test(l))) fails.push("the filter matched another site — `(1)` is matching inside `(12)` or `(2)`");
    if (mine.lines.some((l) => /list_sites/.test(l))) fails.push("a site-less line survived the filter");
    if (!mine.lines.some((l) => /\[WARN\]/.test(l))) fails.push("the site's WARN line was dropped — the filter is reading outcome, not site");
    if (!(mine.toggle?.text || "").includes(`${MINE}/${TOTAL}`)) fails.push(`the chip does not say ${MINE}/${TOTAL}: "${mine.toggle?.text}"`);
    // Keep filters survive a tick of the 1s poll (the filter is applied to
    // the query's data, not to a copy taken once).
    await page.waitForTimeout(1300);
    const still = await pane(page);
    if (still.lines.length !== MINE) fails.push(`the filter did not survive a poll: ${still.lines.length} lines`);

    await page.click('button:has-text("Only this site")');
    await page.waitForTimeout(400);
    const back = await pane(page);
    if (back.lines.length !== TOTAL) fails.push(`toggling off did not restore all ${TOTAL} lines (${back.lines.length})`);
  }

  // A site with NO lines: the empty state must say the other lines exist and
  // how to see them — "no output" would read as "agents never touched anything".
  await page.goto(`${BASE}/sites/3`, { waitUntil: "networkidle" });
  await page.click('button:has-text("Logs"), [role="tab"]:has-text("Logs")');
  await page.waitForTimeout(500);
  const tab3 = await page.$('button:has-text("AI agents (MCP)")');
  if (tab3) {
    await tab3.click();
    await page.waitForTimeout(400);
    await page.click('button:has-text("Only this site")');
    await page.waitForTimeout(400);
    const none = await pane(page);
    if (none.lines.length !== 0) fails.push(`site 3 has agent lines in the mock? ${none.lines.length}`);
    if (!none.emptyShown) fails.push("the filtered-empty state does not tell the user the other lines exist");
  } else {
    fails.push("site 3: no agents tab");
  }

  await page.screenshot({ path: "shot-agentlog.png" });
  await browser.close();

  if (fails.length) {
    console.log("AGENT LOG CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(`AGENT LOG CHECK: ALL PASS (tab renders; Only this site narrows ${TOTAL} → ${MINE} by id, both shapes, no cross-match; chip and empty state say what was hidden)`);
})();
