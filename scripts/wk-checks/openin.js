// WebKit check: the "which app opens this" surfaces — the header split button
// and the Browser/editor Quick-links tiles (`/dev/ui-review?view=openin`).
//
// Why WebKit and not Chrome: the chevron half is a SIBLING <button> because a
// nested one is invalid HTML and WKWebView silently drops the inner click —
// exactly the bug class this harness exists for. The menu also portals to
// <body> and fixed-positions from the trigger rect, which is a WebKit metric.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const url = (q) => `${BASE}/dev/ui-review?view=openin${q ?? ""}`;

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  page.on("console", (m) => m.type() === "error" && fails.push(`console.error: ${m.text()}`));

  // ── 1. Three browsers + two editors: icons, labels, chevrons ──────────────
  await page.goto(url(), { waitUntil: "networkidle" });
  await page.waitForTimeout(500);

  // The button must name the browser the click resolves to (the system default
  // here, since no preference is stored) — "Browser" alone was the whole bug.
  if ((await page.getByText("Open in Google Chrome", { exact: false }).count()) === 0)
    fails.push("tile does not name the resolved browser");
  if ((await page.getByText("Open in VS Code", { exact: false }).count()) === 0)
    fails.push("tile does not name the resolved editor");

  // Icons must be REAL decoded images, not just present <img> tags: a broken
  // data URI still renders an element, and naturalWidth is what separates them.
  const icons = await page.evaluate(() =>
    [...document.querySelectorAll("img")].map((i) => ({
      src: i.getAttribute("src")?.slice(0, 30) ?? "",
      w: i.naturalWidth,
      box: i.getBoundingClientRect().width,
    })),
  );
  if (icons.length < 3) fails.push(`expected ≥3 app icons, saw ${icons.length}`);
  for (const i of icons) {
    if (!i.src.startsWith("data:image/png;base64,")) fails.push(`icon src not a PNG data URI: ${i.src}`);
    if (i.w === 0) fails.push("icon did not decode (naturalWidth 0)");
    if (i.box < 12 || i.box > 20) fails.push(`icon box ${i.box}px — outside the 12–20px slot`);
  }

  const chevrons = () => page.getByRole("button", { name: /another browser|another editor/ });
  // 2 header (browser + Magic Login) + 3 tiles (browser, Magic Login, editor).
  if ((await chevrons().count()) !== 5)
    fails.push(`expected 5 chevrons, saw ${await chevrons().count()}`);

  // EVERY chevron must carry a visible seam. Without it the arrow reads as
  // decoration on one wide button and "click there for what?" has no answer —
  // reported on the tiles, whose bordered neighbour made the header look fine.
  // `primary` needs its own divider: that variant draws no border at all.
  const seams = await page.evaluate(() =>
    [...document.querySelectorAll("button")]
      .filter((b) => /another browser|another editor/.test(b.getAttribute("aria-label") ?? ""))
      .map((b) => {
        const cs = getComputedStyle(b);
        return {
          label: b.getAttribute("aria-label"),
          width: parseFloat(cs.borderLeftWidth),
          transparent: cs.borderLeftColor === "rgba(0, 0, 0, 0)",
        };
      }),
  );
  for (const s of seams) {
    if (!(s.width >= 1) || s.transparent)
      fails.push(`chevron "${s.label}" has no separator (border-left ${s.width}px${s.transparent ? ", transparent" : ""})`);
  }

  // The chevron opens the list, marks the default, and the pick is one-time —
  // there is no setting write to observe here, so what the probe CAN prove is
  // that the item exists and the menu closes on select.
  await chevrons().first().click();
  await page.waitForTimeout(300);
  for (const name of ["Safari", "Google Chrome", "Firefox"]) {
    if ((await page.getByRole("button", { name: new RegExp(`^${name}`) }).count()) === 0)
      fails.push(`menu missing ${name}`);
  }
  if ((await page.getByText("default", { exact: true }).count()) === 0)
    fails.push("menu does not mark which browser a plain click uses");
  // Each row carries a SECOND target — the same url in that browser's private
  // window — and only for browsers that can really open one. Safari has no
  // private-window command line, so its row must offer nothing: an icon that
  // quietly opened an ordinary, recorded window is the failure this feature
  // cannot have. (Fixture mirrors the real table: safari no, chrome/firefox yes.)
  const privates = page.getByRole("button", { name: /private .+ window/i });
  if ((await privates.count()) !== 2)
    fails.push(`expected 2 private targets (Chrome, Firefox), saw ${await privates.count()}`);
  if ((await page.getByRole("button", { name: /private Safari window/i }).count()) !== 0)
    fails.push("Safari has no private-window command line yet its row offers one");

  // Sibling, never nested — the whole reason this check runs in WebKit: WKWebView
  // drops the inner click of a nested button, so the private icon would look
  // right and do nothing. Checked over the WHOLE document, not just this menu.
  const nested = await page.evaluate(() => document.querySelectorAll("button button").length);
  if (nested !== 0) fails.push(`${nested} nested <button>s — WKWebView will swallow those clicks`);

  // And the seam, same rule as the chevrons: without a divider the icon reads as
  // decoration on the row rather than its own target.
  const rowSeams = await page.evaluate(() =>
    [...document.querySelectorAll("button")]
      .filter((b) => /private .+ window/i.test(b.getAttribute("aria-label") ?? ""))
      .map((b) => {
        const sep = b.previousElementSibling;
        const cs = sep ? getComputedStyle(sep) : null;
        return {
          label: b.getAttribute("aria-label"),
          w: sep ? sep.getBoundingClientRect().width : 0,
          bg: cs ? cs.backgroundColor : "none",
        };
      }),
  );
  for (const s of rowSeams) {
    if (s.w < 0.5 || s.bg === "rgba(0, 0, 0, 0)")
      fails.push(`private target "${s.label}" has no separator (${s.w}px, ${s.bg})`);
  }

  // The click itself: the icon must ask for a PRIVATE window, and must not also
  // fire the row's ordinary open. Both are one IPC call apart, so the mock
  // records them and the probe reads them back.
  await page.evaluate(() => { window.__rexOpens = []; });
  await page.getByRole("button", { name: /private Google Chrome window/i }).click();
  await page.waitForTimeout(250);
  const privOpens = await page.evaluate(() => window.__rexOpens ?? []);
  if (privOpens.length !== 1)
    fails.push(`private icon fired ${privOpens.length} opens, expected exactly 1`);
  const p = privOpens[0];
  if (p && (p.cmd !== "open_in_browser" || p.args?.private !== true || p.args?.browserId !== "chrome"))
    fails.push(`private icon sent ${JSON.stringify(p)} — expected open_in_browser chrome private:true`);
  if ((await page.getByText("default", { exact: true }).count()) !== 0)
    fails.push("menu stayed open after the private pick");

  // The row beside it stays the ORDINARY open — the two targets must not have
  // collapsed into one behaviour.
  await chevrons().first().click();
  await page.waitForTimeout(300);
  await page.evaluate(() => { window.__rexOpens = []; });
  await page.getByRole("button", { name: /^Firefox/ }).click();
  await page.waitForTimeout(250);
  const rowOpens = await page.evaluate(() => window.__rexOpens ?? []);
  const r = rowOpens[0];
  if (rowOpens.length !== 1 || !r || r.args?.private !== false || r.args?.browserId !== "firefox")
    fails.push(`row pick sent ${JSON.stringify(rowOpens)} — expected one firefox open with private:false`);
  if ((await page.getByText("default", { exact: true }).count()) !== 0)
    fails.push("menu stayed open after a pick");

  await page.screenshot({ path: "shot-openin.png", fullPage: true });
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");

  // ── 2. One browser: the chevron must not exist at all ─────────────────────
  await page.goto(url("&browsers=one"), { waitUntil: "networkidle" });
  await page.waitForTimeout(400);
  const oneBrowserChevrons = await page.getByRole("button", { name: /another browser/ }).count();
  if (oneBrowserChevrons !== 0)
    fails.push(`one browser installed but ${oneBrowserChevrons} browser chevrons rendered`);
  if ((await page.getByText("Open in browser", { exact: false }).count()) === 0)
    fails.push("primary action disappeared with the chevron");

  // ── 3. No icons readable: the glyph, not a broken image ───────────────────
  await page.goto(url("&icons=none"), { waitUntil: "networkidle" });
  await page.waitForTimeout(400);
  const imgs = await page.evaluate(() => document.querySelectorAll("img").length);
  if (imgs !== 0) fails.push(`icons=none still rendered ${imgs} <img> elements`);
  if ((await page.getByText("Open in Google Chrome", { exact: false }).count()) === 0)
    fails.push("label lost when the icon is unreadable — the name is the fallback that matters");
  await page.screenshot({ path: "shot-openin-noicons.png", fullPage: true });

  await browser.close();
  if (fails.length) {
    console.log("OPENIN CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("OPENIN CHECK: ALL PASS");
})();
