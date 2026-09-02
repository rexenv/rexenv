// WebKit check: "Live" is earned by a REACHABLE probe, never painted on
// anything that happens to be running.
//
// The claim (ledger #32): a tunnel whose child has died is settled before the
// status snapshot, so it never renders as a live share — and the visible half of
// that promise is the badge. A card that showed "Live" for every running tunnel
// would satisfy the backend guard and still tell the user a dead link works,
// which is the single most expensive lie this screen can tell: they hand that
// URL to somebody else.
//
// The mock ships one tunnel per health (`src/lib/ipc/index.ts`), so the three
// states render side by side and the two non-reachable ones are the control —
// without them, "the reachable card says Live" is satisfied by a screen that
// says Live everywhere.
//
// What this cannot prove: that a dead child is actually settled before the
// snapshot. That is `a_dead_tunnel_is_settled_before_the_status_snapshot` (L0,
// a source-order guard) — a browser reading a mock never sees a process die.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// `includes("Live")`, not a \b word-boundary regex: the card's text runs the
// domain straight into the badge ("acme.rexLive"), so a boundary match reports a
// missing badge that is right there — the check would have failed for a reason
// unrelated to its claim.
const hasLive = (text) => text.includes("Live");

const cards = async (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[data-probe="tunnel-card"]')].map((el) => ({
      domain: el.getAttribute("data-domain"),
      live: el.getAttribute("data-live"),
      text: el.textContent ?? "",
    })),
  );

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/tunnels`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  const found = await cards(page);
  if (found.length < 4) {
    fails.push(`expected four cards (one per health plus an idle control), found ${found.length}`);
  }
  const by = (d) => found.find((c) => c.domain === d);

  const reachable = by("acme.rex");
  if (!reachable) fails.push("no card for the reachable tunnel");
  else if (!hasLive(reachable.text))
    fails.push("a REACHABLE tunnel does not say Live — the badge stopped reading health");

  const unverified = by("portfolio.rex");
  if (!unverified) fails.push("no card for the unverified tunnel");
  else {
    if (hasLive(unverified.text))
      fails.push("an UNVERIFIED tunnel says Live — the user hands out a link nothing confirmed");
    if (!/Unverified/.test(unverified.text))
      fails.push("the unverified tunnel does not say what it is");
    if (unverified.live !== "1")
      fails.push("CONTROL FAILED: the unverified tunnel is not marked running, so 'not Live' proves nothing");
  }

  // The IDLE control: a site with no tunnel at all must not be Live either —
  // without it, "Live appears on the reachable card" is a claim about a screen
  // where every card happened to be a running share.
  const idle = by("docs.rex");
  if (!idle) fails.push("no card for the un-shared site — the idle control is missing");
  else {
    if (idle.live !== "0") fails.push("the un-shared site is marked live");
    if (hasLive(idle.text)) fails.push("a site with NO tunnel says Live");
  }

  const broken = by("network.rex");
  if (!broken) fails.push("no card for the broken tunnel");
  else {
    if (hasLive(broken.text))
      fails.push("a BROKEN tunnel says Live — the loudest possible lie on this screen");
    if (!/Broken/.test(broken.text)) fails.push("the broken tunnel does not say it is broken");
    if (broken.live !== "1")
      fails.push("CONTROL FAILED: the broken tunnel is not marked running, so 'not Live' proves nothing");
  }

  await page.screenshot({ path: "shot-tunnelhealth.png" });
  await browser.close();

  if (fails.length) {
    console.log("TUNNEL-HEALTH CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("TUNNEL-HEALTH CHECK: ALL PASS (Live only when reachable; unverified and broken say so while running)");
})();
