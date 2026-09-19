// WebKit check: a link in an HTML mail opens in the browser — and nothing else does.
//
// Why this exists: the preview's iframe shipped with `sandbox=""`, which reads as
// "maximum isolation" and also switches off top-level navigation and popups. Every
// link in every HTML mail did NOTHING when clicked, for as long as the screen has
// existed, and no check noticed because the mail fixture had no anchor in it
// (ledger #697). So this clicks a REAL anchor inside the REAL frame, in WebKit —
// the engine the app ships — and asserts what the click did.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const url = `${BASE}/dev/ui-review?view=mail`;

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  const NOISE = /Blocked script execution|TLS error|Failed to load resource|Refused to (execute|load)/i;
  page.on("console", (m) => m.type() === "error" && !NOISE.test(m.text()) && fails.push(`console.error: ${m.text()}`));

  await page.goto(url, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  // Open the first message that has an HTML part.
  await page.locator("[data-read]").first().click();
  await page.waitForTimeout(400);
  // The preview opens on Text; the HTML tab is where the frame lives.
  await page.getByRole("button", { name: "HTML", exact: true }).click();
  await page.waitForTimeout(600);

  const frame = page.frameLocator('iframe[title="HTML preview"]');
  // Click by COORDINATE, computed in the parent: Playwright cannot evaluate inside
  // the frame when it is sandboxed, so a locator click there proves nothing about
  // what a user's click does.
  const pointAt = (selector) =>
    page.evaluate((sel) => {
      const f = document.querySelector('iframe[title="HTML preview"]');
      const fb = f.getBoundingClientRect();
      const el = f.contentDocument?.querySelector(sel);
      if (!el) return null;
      const b = el.getBoundingClientRect();
      return { x: fb.left + b.left + b.width / 2, y: fb.top + b.top + b.height / 2 };
    }, selector);
  const link = frame.locator('a[href^="https://"]');
  if ((await link.count()) === 0) {
    fails.push("the HTML preview shows no link — the fixture cannot prove anything");
  } else {
    const at = await pointAt('a[href^="https://"]');
    if (!at) fails.push("the link has no box in the parent — the frame is not readable");
    else await page.mouse.click(at.x, at.y);
    await page.waitForTimeout(500);
    const opens = await page.evaluate(() => window.__rexOpens ?? []);
    const opened = opens.filter((o) => o.cmd === "open_external");
    if (opened.length !== 1) {
      fails.push(`clicking an http link called open_external ${opened.length} times, expected 1`);
    } else if (opened[0].args?.target !== "https://example.test/reset?key=abc") {
      fails.push(`open_external got ${JSON.stringify(opened[0].args)} — not the link's own href`);
    }
    // The frame must still be showing the email, not the link's page.
    if ((await frame.locator('a[href^="https://"]').count()) === 0)
      fails.push("the preview navigated away — the frame followed the link instead of the browser");
  }

  // A non-web scheme is named, never handed to the OS opener.
  const mailto = frame.locator('a[href^="mailto:"]');
  if ((await mailto.count()) === 1) {
    const at = await pointAt('a[href^="mailto:"]');
    if (at) await page.mouse.click(at.x, at.y);
    await page.waitForTimeout(500);
    const opens = await page.evaluate(() => window.__rexOpens ?? []);
    if (opens.filter((o) => o.cmd === "open_external").length !== 1)
      fails.push("a mailto: link reached open_external — the OS opener must never get one from an email");
    if ((await page.getByText("isn't a web address", { exact: false }).count()) === 0)
      fails.push("a mailto: link did nothing and said nothing");
  }

  // ── The email's script never runs, whatever the sweep missed ─────────────
  const ran = await page.evaluate(() => {
    const f = document.querySelector('iframe[title="HTML preview"]');
    const d = f?.contentDocument;
    return {
      csp: !!d?.querySelector('meta[http-equiv="Content-Security-Policy"]'),
      scripts: d ? d.querySelectorAll("script").length : -1,
      canary: d ? d.title : "",
    };
  });
  if (!ran.csp) fails.push("the preview document carries no Content-Security-Policy");
  if (ran.scripts !== 0) fails.push(`the preview document still has ${ran.scripts} <script> element(s)`);
  if (ran.canary === "PWNED")
    fails.push("the email's own script RAN — it set the preview document's title");
  const onerror = await page.evaluate(() => {
    const d = document.querySelector('iframe[title="HTML preview"]')?.contentDocument;
    return d ? d.querySelectorAll("[onerror], [onload], [onclick]").length : -1;
  });
  if (onerror !== 0) fails.push(`${onerror} event-handler attribute(s) survived the sweep`);

  await page.screenshot({ path: "shot-maillink.png", fullPage: true });
  await browser.close();
  if (fails.length) {
    console.error("maillink: FAIL");
    for (const f of fails) console.error("  ✗ " + f);
    process.exit(1);
  }
  console.log("maillink: an HTML mail's links open in the browser, and only web links do");
})();
