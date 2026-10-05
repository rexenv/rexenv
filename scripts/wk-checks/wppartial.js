// WebKit check: a PARTIAL plugin update re-reads the list (ledger #250, the half added
// 3 Sep 2026 and never rendered until 5 Oct).
//
// wp-cli exits non-zero when ANY item fails, so "Only updated 1 of 2 plugins" lands in the
// mutation's onError — and settleAfterUpdate, which runs only on success, never touched
// the cache. The item that DID update kept its old version and its "update" badge until a
// manual Refresh. `onSettled` now invalidates the list: a re-read, never an optimistic
// write. DevGitPanel's `?update=partial` rejects the run with wp-cli's own sentence and,
// from then on, lists wordpress-seo at 22.4 with nothing to offer — what the disk says.
//
// Canaries: the row is asserted to OFFER 22.4 before the run (a missing badge afterwards
// would otherwise also describe an empty list), and a neighbour that still has an update
// (numeric-order) must still offer it afterwards (the re-read must not blank the panel).
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const rowOffer = (page, slug) =>
  page.evaluate((name) => {
    const rows = [...document.querySelectorAll("div")].filter(
      (d) => d.querySelector(`input[aria-label="Select ${name}"]`) !== null,
    );
    if (!rows.length) return { found: false };
    const row = rows[rows.length - 1];
    const text = row.innerText.replace(/\s+/g, " ");
    const badge = [...row.querySelectorAll("span")].some((s) => s.textContent.trim().toLowerCase() === "update");
    return { found: true, badge, arrow: /→/.test(text), text };
  }, slug);

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const problems = [];
  page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list&update=partial`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  const before = await rowOffer(page, "wordpress-seo");
  if (!before.found) problems.push("FIXTURE: no wordpress-seo row — nothing below proves anything");
  else if (!(before.badge && before.arrow)) problems.push(`wordpress-seo does not offer 22.4 before the run: "${before.text}"`);

  for (const slug of ["wordpress-seo", "numeric-order"]) {
    const box = page.locator(`input[aria-label="Select ${slug}"]`);
    if (await box.count()) await box.check();
  }
  await page.getByRole("button", { name: "Update", exact: true }).first().click();
  await page.waitForTimeout(1500);

  const toastText = await page.evaluate(() => document.body.innerText);
  if (!/Only updated 1 of 2/.test(toastText)) problems.push("the partial failure was not surfaced in wp-cli's own words");

  const after = await rowOffer(page, "wordpress-seo");
  if (!after.found) problems.push("wordpress-seo vanished after the partial run");
  else if (after.badge || after.arrow) {
    problems.push(
      `wordpress-seo still offers an update after wp-cli updated it (partial run, onError path): "${after.text}" — ` +
        `the list was not re-read (onSettled's invalidate)`,
    );
  } else if (!/22\.4/.test(after.text)) problems.push(`wordpress-seo does not show the version it is now on (22.4): "${after.text}"`);

  const neighbour = await rowOffer(page, "numeric-order");
  if (!(neighbour.found && neighbour.badge)) problems.push(`numeric-order lost its offer — the re-read blanked more than it should: "${neighbour.text}"`);

  await page.screenshot({ path: `${__dirname}/shot-wppartial.png`, fullPage: true });
  await browser.close();
  if (problems.length) {
    console.error("wppartial: FAIL\n  - " + problems.join("\n  - "));
    process.exit(1);
  }
  console.log("wppartial: a partial update re-reads the list — the updated row stops offering, the other still does");
})();
