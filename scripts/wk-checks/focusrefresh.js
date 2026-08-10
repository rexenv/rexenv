// WebKit check: a panel showing state that lives OUTSIDE rexenv re-reads it
// when the user comes back.
//
// The bug (QA, 10 Aug 2026): activate a plugin in wp-admin, or `git checkout`
// in a terminal, and rexenv kept showing the old answer until you left the tab
// and returned. `refetchOnWindowFocus` was off on exactly these queries, and
// TanStack's default focus source (DOM visibility/focus) is unreliable inside
// wry anyway — so `lib/window-focus.ts` makes the NATIVE window's focus the
// source, with the DOM events kept as the fallback this check drives.
//
// What this proves: focus → a fresh read (the wiring). What it CANNOT prove:
// that the native Tauri `onFocusChanged` event fires — no browser has one. That
// half is L3 (SMOKE-TEST: change a plugin in the browser, switch back, watch
// the row flip without touching anything).
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  await page.goto(`${BASE}/dev/git-panel?panel=repo`, { waitUntil: "networkidle" });
  await page.waitForTimeout(800);

  const calls = async (cmd) =>
    page.evaluate((c) => (window.__ipcCalls ?? {})[c] ?? 0, cmd);

  const before = await calls("repo_asset_status");
  if (before === 0) fails.push("the panel never read git status at all — harness broken");

  // The user comes back to rexenv. (In the app this arrives as the native
  // window's focus event; in a browser it is the DOM one — same subscriber.)
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await page.waitForTimeout(600);

  const afterStatus = await calls("repo_asset_status");
  const afterBranches = await calls("repo_branches");
  if (afterStatus <= before)
    fails.push(`git status was not re-read on focus (${before} → ${afterStatus})`);
  if (afterBranches < 2)
    fails.push(`branches were not re-read on focus (${afterBranches} call(s) total)`);

  // The network read (ls-remote for PR refs) must NOT ride along: it is lazy by
  // design, and firing it on every alt-tab is how a quiet panel becomes chatty.
  if ((await calls("repo_pull_refs")) > 0)
    fails.push("focus fired the network PR-ref read, which is meant to stay lazy");

  await browser.close();
  if (fails.length) {
    console.log("FOCUS-REFRESH CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("FOCUS-REFRESH CHECK: ALL PASS");
})();
