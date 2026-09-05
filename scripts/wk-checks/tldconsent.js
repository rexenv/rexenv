// WebKit check: the resolver-takeover consent renders WHERE the refusal lands.
//
// The report (5 Sep 2026): a user who had moved from Valet to rexenv changed a
// site's domain from .rex to .test and got a toast — "/etc/resolver/test is
// managed by another tool … rexenv can take that TLD over" — with nothing on
// that page, or any page, that did. The takeover card lived on Import only, and
// only for TLDs the scan found in Valet's OWN sites, which after leaving Valet
// is none. Settings' Repair for a foreign TLD refused by design: a button that
// could only fail.
//
// The claim: type a domain on a TLD another tool owns, and the consent card
// (their file beside ours, unticked checkbox, "Take over .test") appears under
// the input with the Change button DISABLED; take it over and the card goes
// and the button wakes. Same card under the default-TLD setting.
//
// Fixture: `?foreign=test` (src/lib/mock.ts) — absent, `.test` is nobody's and
// the card must NOT render, which is the control that proves the card is
// driven by ownership and not by the TLD's spelling. The mock's takeover flips
// the TLD to `borrowed`, so "the card went away" is a re-read, not a hide.
//
// What this cannot prove: that `take_over_resolver` writes the backup and the
// row before the privileged write. That is core's, held by L0 + the live leg.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// Mock ids from src/lib/mock.ts: site 1 is acme.rex (WordPress).
const SITE = "1";

const dialog = async (page) =>
  page.evaluate(() => {
    const heading = [...document.querySelectorAll("*")].find(
      (el) => el.children.length === 0 && el.textContent.trim() === "Change domain?",
    );
    const box = heading?.parentElement;
    const buttons = [...(box?.querySelectorAll("button") ?? [])];
    const change = buttons.find((b) => b.textContent.trim() === "Change domain");
    return {
      found: !!box,
      text: box?.textContent ?? "",
      changeDisabled: change ? change.disabled : null,
      takeOver: buttons.find((b) => /^Take over \./.test(b.textContent.trim()))?.textContent.trim() ?? null,
    };
  });

const managed = (t) => /is managed by Valet or Herd/.test(t);

async function openDialog(page, url) {
  await page.goto(url, { waitUntil: "networkidle" });
  await page.click('button:has-text("Change domain…")');
  await page.waitForTimeout(400);
}

async function type(page, domain) {
  await page.fill('input[placeholder="myshop.rex"]', domain);
  await page.waitForTimeout(700);
  return dialog(page);
}

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  // ── 1. Valet owns .test: the card, and a Change button that waits for it.
  await openDialog(page, `${BASE}/sites/${SITE}/settings?foreign=test`);
  let d = await type(page, "x.test");
  if (!d.found) fails.push("no Change domain dialog — harness or markup changed");
  if (!managed(d.text)) fails.push("typed x.test with Valet owning .test: no consent card under the input");
  if (d.takeOver !== "Take over .test") fails.push(`no "Take over .test" button in the dialog (got ${d.takeOver})`);
  if (d.changeDisabled !== true)
    fails.push("Change domain is ENABLED while another tool owns the TLD — a click can only end in the refusal toast");
  if (!/nameserver 127\.0\.0\.1/.test(d.text)) fails.push("their file is not shown beside ours");
  if (!/\.rex.*always works/.test(d.text)) fails.push("the way out (a different ending) is not named");
  await page.screenshot({ path: "shot-tldconsent.png" });

  // ── 2. CONTROL: a TLD nobody owns clears the card and wakes the button — the
  //       gate is ownership, not "you typed a new TLD".
  d = await type(page, "x.rex");
  if (managed(d.text)) fails.push("x.rex still shows the consent card — the card is not keyed by the typed TLD");
  if (d.changeDisabled !== false) fails.push("x.rex: Change domain disabled with nothing to consent to");

  // ── 3. Consent → takeover → the card goes on the RE-READ and the button wakes.
  d = await type(page, "x.test");
  if (!managed(d.text)) fails.push("x.test again: the card did not come back");
  // The checkbox is unticked by design; the button must be dead until it is.
  const beforeTick = await page.evaluate(() => {
    const b = [...document.querySelectorAll("button")].find((x) => /^Take over \./.test(x.textContent.trim()));
    return b ? b.disabled : null;
  });
  if (beforeTick !== true) fails.push("Take over is enabled before the checkbox is ticked");
  await page.click('label:has-text("Let rexenv answer") input[type="checkbox"]');
  await page.click('button:has-text("Take over .test")');
  await page.waitForTimeout(900);
  d = await dialog(page);
  if (managed(d.text)) fails.push("after take-over the consent card is still there — the status was not re-read");
  if (d.changeDisabled !== false) fails.push("after take-over Change domain is still disabled");

  // ── 4. CONTROL: no fixture → .test is nobody's → no card, no gate. Fresh page
  //       so the mock's taken-over set from step 3 cannot be what clears it.
  const page2 = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  page2.on("pageerror", (e) => fails.push(`pageerror(2): ${e.message}`));
  await openDialog(page2, `${BASE}/sites/${SITE}/settings`);
  await page2.fill('input[placeholder="myshop.rex"]', "x.test");
  await page2.waitForTimeout(700);
  d = await dialog(page2);
  if (managed(d.text)) fails.push("CONTROL FAILED: the card renders for .test with no foreign file — it is always-on");
  if (d.changeDisabled !== false) fails.push("CONTROL FAILED: Change domain disabled for a TLD nobody owns");

  // ── 5. The default-TLD setting: the FIRST site created under a foreign TLD
  //       is what would hit the refusal, so the card sits under that input too.
  await page2.goto(`${BASE}/settings?section=dns&foreign=test`, { waitUntil: "networkidle" });
  await page2.fill('input[aria-label="Default TLD for new sites"]', "test");
  await page2.waitForTimeout(700);
  const settings = await page2.evaluate(() => ({
    text: document.body.textContent ?? "",
    takeOver: [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Take over .test"),
  }));
  if (!managed(settings.text)) fails.push("Settings: typing a foreign default TLD shows no consent card");
  if (!settings.takeOver) fails.push("Settings: no Take over button under the default-TLD input");
  await page2.fill('input[aria-label="Default TLD for new sites"]', "dev");
  await page2.waitForTimeout(700);
  const cleared = await page2.evaluate(() => document.body.textContent ?? "");
  if (managed(cleared)) fails.push("Settings: the card stays for .dev, which nobody owns");
  await page2.screenshot({ path: "shot-tldconsent-settings.png" });

  await browser.close();

  if (fails.length) {
    console.log("TLD CONSENT CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    "TLD CONSENT CHECK: ALL PASS (foreign TLD → card + disabled button; unowned TLD → neither; take-over clears on re-read; Settings too)",
  );
})();
