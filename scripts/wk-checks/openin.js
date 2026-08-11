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
  if ((await chevrons().count()) !== 3)
    fails.push(`expected 3 chevrons (header + 2 tiles), saw ${await chevrons().count()}`);

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
  await page.getByRole("button", { name: /^Firefox/ }).click();
  await page.waitForTimeout(250);
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
