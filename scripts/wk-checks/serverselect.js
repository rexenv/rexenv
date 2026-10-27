// WebKit check: the Site page's Web server select stays inside its card.
//
// The bug (issue #8, 27 Oct 2026, reported on 0.9.2 / macOS): a native <select>
// is as wide as its LONGEST option, and the list carried
// "OpenLiteSpeed (.htaccess + LSCache)". On an Nginx site the select ran past the
// right edge of the Web server card, under the SSL certificate card, with its
// chevron hidden there. The 16 Oct fix (03891c6f) gave it `min-w-0 max-w-full`
// and the card `flex-wrap` — WKWebView does not shrink a native select below its
// longest option, so that fix held only in Chrome. The fix now is structural: the
// options are the short names, and the .htaccess sentence is a note under the
// select, which wraps.
//
// Measured, not eyeballed, twice: (1) the select's right edge is at or inside its
// card's right edge, at the 980px minimum window and at a wide one; (2) the width
// the select ASKS for (a clone outside the flex row, no max-width) fits the card. (2) is the
// one that matters: Playwright's WebKit and this Mac's WKWebView both shrink
// the select (`min-w-0 max-w-full` in a flex row), so (1) passed on the broken build — only the natural width shows
// the dependence on an engine shrinking a select, which the reporter's did not.
// The mock's
// `offeredWebServers` offers all four servers, as macOS does, so the
// OpenLiteSpeed option is in the list — without it this check passes vacuously.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const NGINX_SITE = "1"; // src/lib/mock.ts: acme.rex, nginx

const measure = (page) =>
  page.evaluate(() => {
    const label = [...document.querySelectorAll("*")].find(
      (el) => el.children.length === 0 && el.textContent.trim() === "Web server",
    );
    const card = label?.parentElement;
    const select = card?.querySelector("select");
    if (!select) return { found: false };
    const c = card.getBoundingClientRect();
    const s = select.getBoundingClientRect();
    const cs = getComputedStyle(card);
    const contentW = c.width - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight)
      - parseFloat(cs.borderLeftWidth) - parseFloat(cs.borderRightWidth);
    // The width the select ASKS for: a clone, same classes, outside any flex row,
    // in a max-content box — no max-width, no flex shrink to rescue it.
    const box = document.createElement("div");
    box.style.cssText = "position:absolute;left:0;top:0;width:max-content;visibility:hidden";
    const clone = select.cloneNode(true);
    clone.style.maxWidth = "none";
    box.appendChild(clone);
    document.body.appendChild(box);
    const naturalW = clone.getBoundingClientRect().width;
    box.remove();
    return {
      found: true,
      cardRight: c.right,
      selectRight: s.right,
      selectLeft: s.left,
      cardLeft: c.left,
      contentW,
      naturalW,
      options: [...select.options].map((o) => o.textContent.trim()),
    };
  });

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  for (const width of [980, 1440]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    page.on("pageerror", (e) => fails.push(`pageerror @${width}: ${e.message}`));
    await page.goto(`${BASE}/sites/${NGINX_SITE}`, { waitUntil: "networkidle" });
    await page.waitForTimeout(600);
    const m = await measure(page);
    if (!m.found) {
      fails.push(`@${width}: no Web server select — harness or markup changed`);
    } else {
      if (!m.options.some((o) => /OpenLiteSpeed/.test(o)))
        fails.push(`@${width}: OpenLiteSpeed is not offered, so the widest option is absent and this proves nothing: ${JSON.stringify(m.options)}`);
      // 0.5px slack for subpixel rounding.
      if (m.selectRight > m.cardRight + 0.5)
        fails.push(`@${width}: the select's right edge (${m.selectRight.toFixed(1)}) is past its card's (${m.cardRight.toFixed(1)})`);
      if (m.naturalW > m.contentW + 0.5)
        fails.push(`@${width}: unclamped, the select is ${m.naturalW.toFixed(1)}px — wider than the card's ${m.contentW.toFixed(1)}px; it fits only where the engine shrinks a select (issue #8)`);
      if (m.selectLeft < m.cardLeft - 0.5)
        fails.push(`@${width}: the select starts left of its card`);
    }
    await page.screenshot({ path: `shot-serverselect-${width}.png` });
    await page.close();
  }
  await browser.close();

  if (fails.length) {
    console.log("SERVER-SELECT CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("SERVER-SELECT CHECK: ALL PASS (select inside its card at 980 and 1440, natural width fits unclamped, OpenLiteSpeed offered)");
})();
