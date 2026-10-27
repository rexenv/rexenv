// WebKit check: the Site page header keeps the name readable and never overlaps its actions.
//
// The bug (27 Oct 2026, found on the Dell — WebView2, a ~945 px window): on a site whose
// name has no break opportunity (`smoke1w`), the "Running" pill sat on top of the
// "Stop site" button. The title block is `min-w-0`, so the flex row may squeeze it below
// its content, and nothing in it could give: the name neither wrapped nor truncated, so
// the name + pill ran out of their column into the actions. A name with a break in it
// (`live-sync`, "Acme Store") wrapped onto two lines instead, which hid the bug.
//
// The first fix (d141ec78) truncated the name, and this check passed — at 980 in WebKit.
// On the Dell at ~945 px the same build cut `smoke1w` to "S": the actions left the name
// ~15 px. No overlap, nothing readable. So the check now also says the name is READABLE:
// a short name is never cut, at the 980 px minimum and at the Dell's ~940. When the row
// cannot hold the title and the actions, the actions go to the line below.
//
// Per width, for every mock site plus injected names: the name, the pill and the actions
// never intersect; the name is ONE line; a short name (≤ 16 characters — `smoke1w`,
// "Blog Network") is whole; a cut name says itself in `title`; and the very long injected
// names really are cut (else the ellipsis half proved nothing). Before the wrap fix: at
// 940, "Acme Store", "Blog Network" and `smoke1w` were cut — red.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const SITES = ["1", "2", "3", "4"]; // src/lib/mock.ts — 1 and 3 are WordPress (Magic Login)
const SHORT = 16;
const INJECT = {
  // The Dell's own name: no break, short — must never be cut.
  smoke1w: "smoke1w",
  // Too long for any row, one spaced (it used to WRAP) and one with no break.
  long: "Client portal staging environment for the acme corporation website rebuild",
  nobreak: "clientportalstagingenvironmentfortheacmecorporationwebsiterebuild",
};

const measure = (page) =>
  page.evaluate(() => {
    const name = document.querySelector("[data-site-title]");
    const pill = name?.parentElement?.querySelector("[data-status-pill]") ?? null;
    const actions = document.querySelector("[data-site-actions]");
    if (!name || !pill || !actions) return { found: false, name: !!name, pill: !!pill, actions: !!actions };
    const box = (el) => { const r = el.getBoundingClientRect(); return { l: r.left, r: r.right, t: r.top, b: r.bottom }; };
    const n = name.getBoundingClientRect();
    const lh = parseFloat(getComputedStyle(name).lineHeight) || n.height;
    return {
      found: true,
      text: name.textContent.trim(),
      title: name.getAttribute("title"),
      cut: name.scrollWidth > name.clientWidth,
      nameLines: Math.round(n.height / lh),
      name: box(name),
      pill: box(pill),
      actions: box(actions),
    };
  });

const hit = (a, b) => a.l < b.r - 0.5 && b.l < a.r - 0.5 && a.t < b.b - 0.5 && b.t < a.b - 0.5;

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  for (const width of [940, 980, 1180]) {
    const page = await browser.newPage({ viewport: { width, height: 800 } });
    page.on("pageerror", (e) => fails.push(`pageerror @${width}: ${e.message}`));
    for (const id of [...SITES, ...Object.keys(INJECT)]) {
      // Injected names go on a WordPress site: the widest actions.
      const siteId = INJECT[id] ? "1" : id;
      await page.goto(`${BASE}/sites/${siteId}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(600);
      if (INJECT[id]) {
        await page.evaluate((t) => {
          const el = document.querySelector("[data-site-title]");
          if (el) { el.textContent = t; el.setAttribute("title", t); }
        }, INJECT[id]);
        await page.waitForTimeout(150);
      }
      const m = await measure(page);
      const at = `@${width} site ${id} (${m.text ?? "?"})`;
      if (!m.found) {
        fails.push(`${at}: header markers missing (${JSON.stringify(m)}) — markup changed`);
        continue;
      }
      if (hit(m.pill, m.actions)) fails.push(`${at}: the status pill overlaps the actions`);
      if (hit(m.name, m.actions)) fails.push(`${at}: the name overlaps the actions`);
      if (m.nameLines > 1) fails.push(`${at}: the name wrapped onto ${m.nameLines} lines`);
      if (m.cut && m.title !== m.text) fails.push(`${at}: the name is cut and its title does not say it`);
      if (m.cut && m.text.length <= SHORT) fails.push(`${at}: a ${m.text.length}-character name is cut — unreadable`);
      if ((id === "long" || id === "nobreak") && !m.cut) fails.push(`${at}: the very long name was not cut — the ellipsis half proved nothing`);
      await page.screenshot({ path: `shot-detailheader-${width}-${id}.png` });
    }
    await page.close();
  }
  await browser.close();

  if (fails.length) {
    console.log("DETAIL-HEADER CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("DETAIL-HEADER CHECK: ALL PASS (4 sites + 3 injected names at 940/980/1180: no overlap, one line, short names whole, cut names titled)");
})();
