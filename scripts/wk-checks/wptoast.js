// WebKit check: plugin-list actions report what they did.
//
// The gap (QA, 11 Aug 2026): Activate / Deactivate / Delete flipped a row and
// said nothing. On a long list — the one in the report had 26 rows — the row you
// acted on is often off screen by the time the call returns, so the only
// feedback was a toggle you could no longer see.
//
// Asserted against `role="status"` (the toaster), so the row's own "Active"
// label cannot satisfy it:
//   1. deactivating an active plugin toasts, naming the plugin and the verb;
//   2. it toasts exactly ONCE per click;
//   3. nothing is announced before anything is clicked.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const toasts = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[role="status"] > div')].map((n) => n.innerText.trim()),
  );

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  if ((await toasts(page)).length !== 0) fails.push("a toast appeared before any click");

  const deactivate = page.getByLabel("Deactivate akismet");
  if ((await deactivate.count()) === 0) {
    fails.push("the harness rendered no akismet row — mock or fixture drift");
  } else {
    await deactivate.click();
    await page.waitForTimeout(700);
    const said = await toasts(page);
    const mine = said.filter((t) => t.includes("akismet"));
    if (mine.length === 0) fails.push(`deactivate said nothing (saw: ${JSON.stringify(said)})`);
    if (mine.length > 1) fails.push(`deactivate announced ${mine.length}×`);
    if (mine[0] && !/deactivated/i.test(mine[0]))
      fails.push(`the toast does not name the action: ${mine[0]}`);
  }

  // The other direction, on the row that starts inactive.
  const activate = page.getByLabel("Activate hello-dolly");
  if ((await activate.count()) === 0) {
    fails.push("no hello-dolly row to activate");
  } else {
    await activate.click();
    await page.waitForTimeout(700);
    const mine = (await toasts(page)).filter((t) => t.includes("hello-dolly"));
    if (mine.length !== 1) fails.push(`activate announced ${mine.length}× (want 1)`);
    if (mine[0] && !/activated/i.test(mine[0]))
      fails.push(`the toast does not name the action: ${mine[0]}`);
  }

  await page.screenshot({ path: "shot-wptoast.png", fullPage: true });
  await browser.close();

  if (fails.length) {
    console.log("WP-TOAST CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("WP-TOAST CHECK: ALL PASS");
})();
