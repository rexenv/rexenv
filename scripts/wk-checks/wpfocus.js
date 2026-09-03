// WebKit check: the WordPress panel re-reads its list when the user comes back,
// and does NOT drag the expensive checks along.
//
// The bug (QA, 10 Aug 2026, ledger #255): plugins and themes change in wp-admin
// with no event to tell us, and these queries were cached 30s with
// focus-refetch OFF — so rexenv showed the opposite of reality until the user
// left the tab and returned. The git half of that fix has had a probe since it
// shipped (`focusrefresh.js`); the WordPress half never did, and the ledger row
// said so.
//
// The pair matters as much as the refetch: a panel that re-reads EVERYTHING on
// focus turns every alt-tab into a burst of wp-cli invocations, each of which
// boots PHP. The list is cheap and must be live; the update check keeps its
// five-minute window and must not ride along.
//
// What this cannot prove: that the native window's focus event fires — no
// browser has one. That half is L3 (SMOKE-TEST), and `focusrefresh.js` says so
// for the same reason.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);

  const calls = async (cmd) => page.evaluate((c) => (window.__ipcCalls ?? {})[c] ?? 0, cmd);

  const before = await calls("wp_plugins");
  if (before === 0) {
    fails.push("the panel never read the plugin list at all — harness broken, nothing below means anything");
  }
  // The update pass is the SAME command with `updates: true`, so the harness
  // tallies it under its own key — counting `wp_plugins` alone cannot tell the
  // live list from the costly check, and this assertion would pass on a number
  // that never moves.
  const updatesBefore = await calls("wp_plugins:updates");

  // The user comes back to rexenv.
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await page.waitForTimeout(800);

  const after = await calls("wp_plugins");
  if (after <= before)
    fails.push(
      `the plugin list was not re-read on focus (${before} → ${after}) — a plugin activated in ` +
        "wp-admin keeps showing its old state, which is the bug this fix exists for",
    );

  // The costly pass must NOT ride along. Held back by the FLAG
  // (`refetchOnWindowFocus: false` on WP_QUERY), not by its stale window: this
  // probe fires focus ~2s after load, inside any stale window, so it could
  // not tell the two apart — the sentence has to name the mechanism it tests.
  const updatesAfter = await calls("wp_plugins:updates");
  if (updatesAfter > updatesBefore)
    fails.push(
      `focus fired the update check (${updatesBefore} → ${updatesAfter}) — it is opted out ` +
        "of focus refetch on purpose (refetchOnWindowFocus: false), and every call boots PHP",
    );

  await page.screenshot({ path: "shot-wpfocus.png" });
  await browser.close();
  if (fails.length) {
    console.log("WP-FOCUS CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(`WP-FOCUS CHECK: ALL PASS (list re-read ${before} → ${after}; update check stayed put)`);
})();
