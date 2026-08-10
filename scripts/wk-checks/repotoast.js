// WebKit check: every repo-panel action REPORTS when it finishes.
//
// The gap (QA, 10 Aug 2026): Fetch / Pull / Push / Build zip / Check deps /
// Run: <script> all start a job whose only completion signal was a step glyph
// inside the card. Scroll the panel away — or look at the site in a browser,
// which is what you clicked Pull to do — and a finished job is indistinguishable
// from one that never ran.
//
// Asserted here (the toaster is `role="status"`, so the assertions can't be
// satisfied by the same text inside the job card):
//   1. a settled op toasts once, naming the action AND the asset;
//   2. it toasts ONCE — the panel re-reads the job snapshot right after
//      attaching listeners, so the naive version announced everything twice;
//   3. steps that have not run (composer/pnpm still pending) stay silent;
//   4. a failed op toasts the FIRST line of git's error and stops there.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const toasts = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[role="status"] > div')].map((n) => n.innerText.trim()),
  );

(async () => {
  const browser = await webkit.launch();
  const fails = [];

  // --- 1) the happy path -------------------------------------------------
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=repo`, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);

  if ((await toasts(page)).length !== 0) fails.push("a toast appeared before any click");

  await page.getByRole("button", { name: "Pull", exact: true }).click();
  await page.waitForTimeout(900); // let the post-attach snapshot re-read land too

  const after = await toasts(page);
  const pulls = after.filter((t) => t.includes("Pull"));
  if (pulls.length === 0) fails.push(`no toast for a finished Pull (saw: ${JSON.stringify(after)})`);
  if (pulls.length > 1) fails.push(`Pull announced ${pulls.length}× — the snapshot re-read duplicates it`);
  if (pulls[0] && !pulls[0].includes("my-plugin"))
    fails.push(`the toast does not name the asset: ${pulls[0]}`);
  if (pulls[0] && !/finished/i.test(pulls[0]))
    fails.push(`the toast does not say it finished: ${pulls[0]}`);
  // The dependency steps are PENDING in this fixture — announcing them would be
  // a lie about work that has not happened.
  if (after.some((t) => t.includes("composer install") || t.includes("pnpm install")))
    fails.push("a pending dependency step was announced as finished");

  // --- 2) the failure path ----------------------------------------------
  const fail = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  fail.on("pageerror", (e) => fails.push(`pageerror(fail): ${e.message}`));
  await fail.goto(`${BASE}/dev/git-panel?panel=repo&op=fail`, { waitUntil: "networkidle" });
  await fail.waitForTimeout(600);
  await fail.getByRole("button", { name: "Pull", exact: true }).click();
  await fail.waitForTimeout(900);

  const failed = (await toasts(fail)).filter((t) => t.includes("Pull"));
  if (failed.length !== 1) fails.push(`failed Pull announced ${failed.length}× (want 1)`);
  const text = failed[0] ?? "";
  if (!/failed/i.test(text)) fails.push(`the failure toast does not say failed: ${text}`);
  if (!text.includes("Not possible to fast-forward"))
    fails.push(`the failure toast drops git's reason: ${text}`);
  if (text.includes("hint: rebase"))
    fails.push(`the failure toast spills past the first line: ${text}`);
  // "skipped" steps never ran — the failure that caused them was already said.
  if ((await toasts(fail)).some((t) => t.includes("composer install")))
    fails.push("a skipped step was announced");

  await page.screenshot({ path: "shot-repotoast.png", fullPage: true });
  await browser.close();

  if (fails.length) {
    console.log("REPO-TOAST CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("REPO-TOAST CHECK: ALL PASS");
})();
