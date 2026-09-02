// WebKit check: the Domains card renders what the BACKEND returned, and the
// primary is never removable.
//
// The claim (ledger #450/#451, shipped 2 Sep 2026): a site can answer on extra
// hostnames, and the screen "never renders a name the server did not confirm" —
// every mutation's reply IS the new list. That sentence is only true if the card
// re-renders from the reply rather than from local state, which is exactly the
// difference a browser can see and a unit test cannot.
//
// The mock is MUTABLE on purpose (`src/lib/mock.ts`): a fixture that answered
// the same list forever would let a card that ignores the reply pass this.
//
// What this cannot prove: that the added name is SERVED (config rebuilt, cert
// re-issued). That is the backend's job, held by L0 in core and by the live leg
// in the ledger row — a mock reply proves the screen, never the server.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// Mock ids from src/lib/mock.ts: site 1 ships with one extra domain.
const SITE = "1";

const card = async (page) =>
  page.evaluate(() => {
    const label = [...document.querySelectorAll("*")].find(
      (el) => el.children.length === 0 && el.textContent.trim() === "Domains",
    );
    const box = label?.closest("div")?.parentElement;
    return {
      found: !!box,
      text: box?.textContent ?? "",
      removeLabels: [...(box?.querySelectorAll("button") ?? [])]
        .map((b) => b.getAttribute("aria-label") ?? "")
        .filter((l) => l.startsWith("Remove ")),
    };
  });

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/sites/${SITE}`, { waitUntil: "networkidle" });
  // The card lives on the site's Settings tab, which is where every other
  // per-site setting is — a domain list on the overview would be the fourth
  // place a user has to look for "how is this site configured".
  await page.click('button:has-text("Settings"), [role="tab"]:has-text("Settings")');
  await page.waitForTimeout(600);

  const first = await card(page);
  if (!first.found) {
    fails.push("no Domains card on the site page — harness or markup changed");
  } else {
    if (!/acme\.rex/.test(first.text)) fails.push("the primary domain is not listed");
    if (!/primary/.test(first.text)) fails.push("the primary is not MARKED as the primary");
    if (!/shop\.acme\.rex/.test(first.text)) fails.push("the existing extra domain is not listed");
    // The primary must have no Remove: its files, database and cert folder are
    // named for it, and removing it here would be a different operation
    // (Change domain) wearing this one's button.
    if (first.removeLabels.includes("Remove acme.rex"))
      fails.push("the PRIMARY domain has a Remove button — that is Change domain's job");
    if (!first.removeLabels.includes("Remove shop.acme.rex"))
      fails.push("the extra domain has no Remove button");
  }

  // Add: the row must appear because the REPLY contained it.
  await page.fill('input[placeholder="another.rex"]', "www.acme.rex");
  await page.click('button:has-text("Add domain")');
  await page.waitForTimeout(500);
  const added = await card(page);
  if (!/www\.acme\.rex/.test(added.text))
    fails.push("the added domain never appeared — the card is not rendering the reply");

  // Remove: and disappear again for the same reason.
  await page.click('button[aria-label="Remove www.acme.rex"]');
  await page.waitForTimeout(500);
  const removed = await card(page);
  if (/www\.acme\.rex/.test(removed.text))
    fails.push("the removed domain is still on screen — the card kept local state");
  if (!/shop\.acme\.rex/.test(removed.text))
    fails.push("CONTROL FAILED: the untouched extra domain vanished too, so 'gone' means nothing here");

  await page.screenshot({ path: "shot-domains.png" });
  await browser.close();

  if (fails.length) {
    console.log("DOMAINS CARD CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("DOMAINS CARD CHECK: ALL PASS (primary marked + not removable, add and remove both render the reply)");
})();
