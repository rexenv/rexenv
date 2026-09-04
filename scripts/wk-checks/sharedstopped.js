// WebKit check: a share of a STOPPED site says so ON THE CARD, and the sentence
// is the backend's.
//
// The ruling (owner, 4 Sep 2026, ledger #510) is that sharing a stopped site is
// ALLOWED — it is the sharer's call — but never silent: a link handed to
// somebody else that shows them a stop page, while the sharer believes their
// site is up, is the shape of a wasted afternoon. L0 proves the sentence is
// recomputed on every report rather than captured at start. What L0 cannot see
// is whether it reaches the screen, and that half was booked to SMOKE.
//
// It does not need to be. The card renders `tunnel.warning` straight through,
// so a fixture carrying the backend's exact sentence turns "a human will look
// one day" into a check that runs in the bar.
//
// The assertion that matters is the LAST one: the rendered text must equal the
// text the backend sent, character for character. A UI that composes its own
// version of this sentence would pass a "warning is visible" check forever
// while drifting away from what the backend actually decided.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// The sentence `core::tunnels::stopped_share_warning` builds. Repeated here on
// purpose: this file is the third copy, and three copies that must agree is
// exactly what makes the check able to fail when one of them moves.
const EXPECTED =
  'network.rex is stopped in rexenv, so this link shows the "site stopped" page to ' +
  "anyone who opens it. Start the site to serve it.";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/tunnels`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  const cards = await page.evaluate(() =>
    [...document.querySelectorAll('[data-probe="tunnel-card"]')].map((el) => ({
      domain: el.getAttribute("data-domain"),
      text: (el.textContent ?? "").replace(/\s+/g, " ").trim(),
    })),
  );
  if (cards.length < 4) {
    fails.push(
      `only ${cards.length} tunnel cards rendered — the fixture is not reaching the page, so ` +
        "nothing below means anything",
    );
  }

  const warned = cards.find((c) => c.domain === "network.rex");
  if (!warned) {
    fails.push("no card for the shared-but-stopped site — the fixture row is not rendering");
  } else if (!warned.text.includes(EXPECTED)) {
    fails.push(
      "the shared-but-stopped card does not carry the backend's warning. Rendered text was:\n" +
        `    ${warned.text}\n  expected to contain:\n    ${EXPECTED}`,
    );
  }

  // The control, COUNTED rather than read. Checking a healthy card for the
  // warning TEXT looked like a control and was not: making the strip
  // unconditional renders an EMPTY amber box on every card — `{tunnel.warning}`
  // of undefined prints nothing — so a text check passed the plant it existed
  // to catch. One strip must exist per tunnel that has a warning, and the
  // fixture has exactly one.
  const strips = await page.evaluate(
    () => document.querySelectorAll('[data-probe="share-warning"]').length,
  );
  if (strips !== 1) {
    fails.push(
      `${strips} share-warning strips rendered, expected exactly 1 — the strip is not ` +
        "conditional on the warning, so it is decoration rather than a report",
    );
  }
  const quiet = cards.find((c) => c.domain === "acme.rex");
  if (!quiet) fails.push("no card for the reachable tunnel — the control is missing");
  else if (quiet.text.includes("is stopped in rexenv"))
    fails.push("a REACHABLE share shows the stopped-site warning — the strip is not conditional");

  await browser.close();
  if (fails.length) {
    console.log("SHARED-STOPPED CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    "SHARED-STOPPED CHECK: ALL PASS (the card carries the backend's sentence verbatim; a " +
      "healthy share stays quiet)",
  );
})();
