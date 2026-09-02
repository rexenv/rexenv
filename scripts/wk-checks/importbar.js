// WebKit check: the import batch bar reports work that HAPPENED, and freezes
// rather than rolling back when a site fails.
//
// The claim (ledger #243): the bar is driven by settled rows plus the running
// one — never a clock — and its detail line is the CHILD job's label, not a
// sentence the batch card invented. The arithmetic half has been L0 since it
// shipped; the row's own verdict said the SOURCING half was L2 and unwritten,
// because "the label is really the child's" and "the bar freezes on failure"
// are claims about the running screen.
//
// A percentage that rolls back is the specific dishonesty this is about: the
// user watches a long import, sees 55%, then sees 40% and cannot tell whether
// work was undone or the number was never real. Freezing says the truth — this
// is where the work stopped.
//
// The harness (`?panel=import-bar`) steps a scripted batch, failure included,
// through the REAL card. What this cannot prove: that a real import emits those
// events in that order — that is `valet_import_run`'s job and PUBLISH-TESTING
// §G's pass.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1000, height: 700 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=import-bar`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);

  const read = () =>
    page.evaluate(() => {
      const host = document.querySelector('[data-probe="import-bar-host"]');
      const fill = host?.querySelector("div[style*='width']");
      return {
        width: fill ? parseFloat(fill.style.width) : null,
        text: host?.textContent ?? "",
      };
    });

  const seen = [];
  for (let i = 0; i < 4; i++) {
    seen.push(await read());
    if (i < 3) {
      await page.click('[data-probe="import-step"]');
      await page.waitForTimeout(250);
    }
  }

  if (seen.some((s) => s.width === null)) fails.push("no progress fill rendered — harness broken");

  // 1. The detail line is the CHILD's label, verbatim, at every step.
  const labels = ["installing WordPress", "issuing the certificate", "linking the folder", "database import failed"];
  labels.forEach((label, i) => {
    if (!seen[i]?.text.includes(label))
      fails.push(`step ${i + 1} does not show the child job's label ("${label}") — the card is narrating its own sentence`);
  });

  // 2. The bar never goes BACKWARDS, and the failing step freezes it.
  for (let i = 1; i < seen.length; i++) {
    if (seen[i].width < seen[i - 1].width - 0.01)
      fails.push(
        `the bar rolled back at step ${i + 1} (${seen[i - 1].width}% → ${seen[i].width}%) — the user cannot tell whether work was undone or the number was never real`,
      );
  }
  if (seen[3].width !== seen[2].width)
    fails.push(
      `the failing step moved the bar (${seen[2].width}% → ${seen[3].width}%) — a failure is where the work STOPPED, and the bar says so by staying there`,
    );

  // CONTROL: the bar must actually MOVE somewhere in the sequence, or "never
  // rolled back" is satisfied by a bar that never changes at all.
  if (!(seen[1].width > seen[0].width))
    fails.push("CONTROL FAILED: the bar never advanced, so 'it never rolled back' proves nothing");

  await page.screenshot({ path: "shot-importbar.png" });
  await browser.close();
  if (fails.length) {
    console.log("IMPORT-BAR CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    `IMPORT-BAR CHECK: ALL PASS (labels are the child's; ${seen.map((s) => s.width).join("% → ")}% — advanced, never rolled back, frozen on failure)`,
  );
})();
