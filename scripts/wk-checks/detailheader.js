// WebKit check: the Site page header's title row never runs into its actions.
//
// The bug (27 Oct 2026, found on the Dell — WebView2, a ~945 px window): on a site whose
// name has no break opportunity (`smoke1w`), the "Running" pill sat on top of the
// "Stop site" button. The title block is `min-w-0`, so the flex row may squeeze it below
// its content, and nothing in it could give: the name neither wrapped nor truncated, so
// the name + pill ran out of their column into the actions. A name with a break in it
// (`live-sync`, "Acme Store") wrapped onto two lines instead, which hid the bug — and was
// its own small ugliness.
//
// Measured at the 980 px minimum window, for every mock site (WordPress ones carry the
// wider Magic Login button): the pill's right edge, and the name's, are at or left of the
// actions group's left edge; the name is ONE line; and a name cut short says itself in
// `title`. Before the fix: the no-break name put the pill 104 px into the actions and
// the spaced names wrapped — both red.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const SITES = ["1", "2", "3", "4"]; // src/lib/mock.ts

const measure = (page) =>
  page.evaluate(() => {
    const name = document.querySelector("[data-site-title]");
    const pill = name?.parentElement?.querySelector("[data-status-pill]") ?? null;
    const actions = document.querySelector("[data-site-actions]");
    if (!name || !pill || !actions) return { found: false, name: !!name, pill: !!pill, actions: !!actions };
    const n = name.getBoundingClientRect();
    const lh = parseFloat(getComputedStyle(name).lineHeight) || n.height;
    return {
      found: true,
      text: name.textContent.trim(),
      title: name.getAttribute("title"),
      cut: name.scrollWidth > name.clientWidth,
      nameRight: n.right,
      nameLines: Math.round(n.height / lh),
      pillRight: pill.getBoundingClientRect().right,
      actionsLeft: actions.getBoundingClientRect().left,
    };
  });

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  const page = await browser.newPage({ viewport: { width: 980, height: 800 } });
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  // A name long enough to need cutting at 980 — the mock's own names all fit once the
  // row is right, so without one the title/ellipsis half of the claim is never exercised.
  // Two injected names: one with spaces (it used to WRAP), and one with no break at all —
  // the Dell's `smoke1w` shape, the one that pushed the pill into the actions.
  const INJECT = {
    long: "Client portal staging environment for acme",
    nobreak: "clientportalstagingacme",
  };
  for (const id of [...SITES, ...Object.keys(INJECT)]) {
    const siteId = INJECT[id] ? "1" : id;
    await page.goto(`${BASE}/sites/${siteId}`, { waitUntil: "networkidle" });
    await page.waitForTimeout(600);
    if (INJECT[id]) {
      await page.evaluate((t) => {
        const el = document.querySelector("[data-site-title]");
        if (el) { el.textContent = t; el.setAttribute("title", t); }
      }, INJECT[id]);
      await page.waitForTimeout(100);
    }
    const m = await measure(page);
    if (!m.found) {
      fails.push(`site ${id}: header markers missing (${JSON.stringify(m)}) — markup changed`);
      continue;
    }
    if (m.pillRight > m.actionsLeft + 0.5)
      fails.push(`site ${id} (${m.text}): the status pill ends at ${m.pillRight.toFixed(1)}, past the actions' left edge ${m.actionsLeft.toFixed(1)}`);
    if (m.nameRight > m.actionsLeft + 0.5)
      fails.push(`site ${id} (${m.text}): the name runs under the actions`);
    if (m.nameLines > 1) fails.push(`site ${id} (${m.text}): the name wrapped onto ${m.nameLines} lines`);
    if (m.cut && m.title !== m.text) fails.push(`site ${id}: the name is cut short and its title does not say it`);
    if (INJECT[id] && !m.cut) fails.push(`the ${id} name was not cut — the ellipsis half proved nothing`);
    await page.screenshot({ path: `shot-detailheader-${id}.png` });
  }
  await browser.close();

  if (fails.length) {
    console.log("DETAIL-HEADER CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("DETAIL-HEADER CHECK: ALL PASS (4 sites + two long names at 980: pill and name clear of the actions, one line, cut names titled)");
})();
