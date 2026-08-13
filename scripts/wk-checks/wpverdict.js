// WebKit check: a row may claim an update ONLY when the offered version is
// newer than the one on disk (ledger #250).
//
// `verdict` + `isNewerVersion` are pure functions in WordPressManager.tsx, and
// this repo has no JS test runner to hold a pure function. Rather than add one
// for two assertions, the rule is held where it MATTERS — in the rendered list,
// through the real component, over rows the dev panel supplies.
//
// Both directions, because each fails differently:
//
//   1. `stale-claim` is on 3.4.1 and claims an update TO 3.4.1. wp-cli reports
//      this whenever its source is stale (an in-flight pre-update check, a
//      premium plugin's own updater caching for hours), and it is what made a
//      finished update's badge reappear. The row must offer NOTHING.
//   2. `numeric-order` is on 1.1.3.8 and is offered 1.1.11 — a REAL update that
//      a string compare gets backwards ("1.1.11" < "1.1.3.8" as text). The row
//      must offer the update. This is the direction a naive "just compare the
//      strings" fix breaks, and the one a test that only checked case 1 would
//      let through.
//
// A check that asserted only "no badge" would also pass on a panel that
// rendered no rows at all, so the fixture rows are proven present first.
const { webkit } = require("playwright");

const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

/** Does the row for `slug` offer an update? Read from what RENDERS: the amber
 *  `update` badge and the Update button are both gated on the same `updatable`,
 *  so either one appearing is the claim being made to the user. */
const rowOffer = (page, slug) =>
  page.evaluate((name) => {
    const rows = [...document.querySelectorAll("div")].filter(
      (d) => d.querySelector(`input[aria-label="Select ${name}"]`) !== null,
    );
    if (!rows.length) return { found: false };
    // The innermost matching container is the row itself.
    const row = rows[rows.length - 1];
    const text = row.innerText.replace(/\s+/g, " ");
    const badge = [...row.querySelectorAll("span")].some(
      (s) => s.textContent.trim().toLowerCase() === "update",
    );
    // The Update control is ICON-ONLY (ArrowUpCircle) — its text is empty and
    // the version it would install lives in the title. Matching on innerText
    // finds nothing and reads as "no offer" for every row, which is how this
    // probe first reported the working rows as broken.
    const button = [...row.querySelectorAll("button")].some((b) =>
      (b.getAttribute("title") ?? "").toLowerCase().startsWith("update"),
    );
    // The claim in words: `v<have> → <target>` renders only when updatable.
    const arrow = /→/.test(text);
    return { found: true, badge, button, arrow, text };
  }, slug);

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const problems = [];
  page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));

  await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list`, {
    waitUntil: "networkidle",
  });
  await page.waitForTimeout(700);

  // The canary: if the fixture rows are not on screen, every assertion below
  // passes for the wrong reason — "no update badge" is also what an empty list
  // looks like.
  const stale = await rowOffer(page, "stale-claim");
  const numeric = await rowOffer(page, "numeric-order");
  const control = await rowOffer(page, "wordpress-seo");
  for (const [slug, r] of [
    ["stale-claim", stale],
    ["numeric-order", numeric],
    ["wordpress-seo", control],
  ]) {
    if (!r.found) problems.push(`FIXTURE: no row rendered for ${slug} — nothing here proves anything`);
  }

  if (stale.found && (stale.badge || stale.button || stale.arrow)) {
    problems.push(
      `stale-claim offers an update to the version it is ALREADY ON (3.4.1 → 3.4.1). ` +
        `That is the claim wp-cli makes from a stale source, and rendering it is how a ` +
        `finished update's badge comes back. row: "${stale.text}"`,
    );
  }
  if (numeric.found && !(numeric.badge && numeric.button && numeric.arrow)) {
    problems.push(
      `numeric-order does NOT offer 1.1.11 over 1.1.3.8, which is a real update — a string ` +
        `compare says 1.1.11 is older and it is not. badge=${numeric.badge} ` +
        `button=${numeric.button} arrow=${numeric.arrow} row: "${numeric.text}"`,
    );
  }
  // The ordinary case, so the two above are not passing because nothing ever
  // offers anything.
  if (control.found && !(control.badge && control.button && control.arrow)) {
    problems.push(
      `wordpress-seo does not offer 22.4 over 22.1 — the plain case is broken, so the two ` +
        `version-ordering assertions above prove nothing. row: "${control.text}"`,
    );
  }

  await browser.close();
  if (problems.length) {
    console.error("✗ wpverdict");
    problems.forEach((p) => console.error(`  ${p}`));
    process.exit(1);
  }
  console.log("✓ wpverdict — an update is offered only when the offer is newer, both directions");
})();
