// WebKit check: the sidebar footer and the Services page never disagree about
// what is running.
//
// The claim (ledger #174): ONE snapshot is the liveness truth, and the views
// render it — they do not each decide. Two views that count independently drift
// the moment one of them learns a new rule (a service that is "running" but not
// yet answering, an adopted process, a debug pool), and the user is left with a
// footer that says 4/7 beside a list showing five green dots, with no way to
// know which is lying.
//
// The mock derives the footer from the SAME service list the page renders
// (`src/lib/mock.ts`), mirroring the backend's single source — so this check is
// about the two views agreeing, not about the fixture.
//
// What it cannot prove: that the snapshot itself is right. Whether a process is
// really alive is `ServiceManager`'s answer and L1's job — a browser only sees
// what it was handed.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1300, height: 1000 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/services`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  const seen = await page.evaluate(() => {
    const text = document.body.innerText;
    const footer = text.match(/(\d+)\s*\/\s*(\d+)/);
    const header = text.match(/(\d+)\s+of\s+(\d+)\s+running/);
    const summary = /Partial/.test(text) ? "partial" : /All\b/.test(text) ? "all" : "stopped";
    return {
      footerRunning: footer ? Number(footer[1]) : null,
      footerTotal: footer ? Number(footer[2]) : null,
      pageRunning: header ? Number(header[1]) : null,
      pageTotal: header ? Number(header[2]) : null,
      summary,
    };
  });

  if (seen.footerRunning === null) fails.push("no running/total in the sidebar footer");
  if (seen.pageRunning === null) fails.push("no 'N of M running' on the Services page");
  if (seen.footerRunning !== seen.pageRunning || seen.footerTotal !== seen.pageTotal) {
    fails.push(
      `the footer says ${seen.footerRunning}/${seen.footerTotal} and the page says ` +
        `${seen.pageRunning}/${seen.pageTotal} — two views counting liveness separately`,
    );
  }
  // The summary word must match the arithmetic, or the badge is a third opinion.
  const expected =
    seen.footerRunning === seen.footerTotal
      ? "all"
      : seen.footerRunning > 0
        ? "partial"
        : "stopped";
  if (seen.summary !== expected)
    fails.push(`the summary says "${seen.summary}" while the count says "${expected}"`);

  // CONTROL: a fixture where everything runs makes agreement trivial — the two
  // views could both be printing the total and this would pass.
  if (!(seen.footerRunning > 0 && seen.footerRunning < seen.footerTotal))
    fails.push(
      `CONTROL FAILED: the fixture is not MIXED (${seen.footerRunning}/${seen.footerTotal}), so ` +
        "agreement here proves nothing about two views that count separately",
    );

  await page.screenshot({ path: "shot-statusagree.png" });
  await browser.close();

  if (fails.length) {
    console.log("STATUS-AGREEMENT CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    `STATUS-AGREEMENT CHECK: ALL PASS (footer and page both ${seen.footerRunning}/${seen.footerTotal}, summary "${seen.summary}", fixture mixed)`,
  );
})();
