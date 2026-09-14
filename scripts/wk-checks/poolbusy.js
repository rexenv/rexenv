// WebKit check: the Services row says when a PHP pool's every worker is busy (ledger #608,
// plan §3 D1(b)).
//
// The claim: a running shared pool whose backend status carries `busyNote` shows that sentence
// as the row's sub-line, in the warning colour, inside its own row — and a pool without it shows
// nothing. The mock's `?busy=8.3` is the fixture (`src/lib/mock.ts`, `mockServicesView`); absent,
// the page must render no note at all, so the probe asserts both ways.
//
// What it cannot prove: WHEN the backend sends the note. That is `core::pool_busy`'s L0 and
// `pool_busy_check`'s L1 on real pools — a browser only renders what it was handed.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const NOTE = "all 10 workers busy — requests are queuing";

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  const ok = (label, pass, detail = "") => {
    console.log(`${pass ? "✓" : "✗"} ${label}${pass ? "" : ` — ${detail}`}`);
    if (!pass) fails.push(label);
  };

  for (const width of [1300, 980]) {
    const page = await browser.newPage({ viewport: { width, height: 1000 } });
    page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

    await page.goto(`${BASE}/services`, { waitUntil: "networkidle" });
    await page.waitForTimeout(500);
    const plain = await page.evaluate((note) => document.body.innerText.includes(note), NOTE);
    ok(`@${width} no ?busy: no pool shows the note`, !plain, "the note rendered with no busy pool");

    await page.goto(`${BASE}/services?busy=8.3`, { waitUntil: "networkidle" });
    await page.waitForTimeout(500);
    const seen = await page.evaluate((note) => {
      const el = [...document.querySelectorAll("div")].find(
        (d) => d.children.length === 0 && d.textContent.trim() === note,
      );
      if (!el) return null;
      // The row is the nearest ancestor that carries the row's border.
      let row = el;
      while (row && !(row.className || "").toString().includes("border-b")) row = row.parentElement;
      const probe = document.createElement("span");
      // The text shade Tunnels' warnings use (`text-status-warning-bright`). The first version of
      // the row named `text-rex-warning`, a class Tailwind never generates — this line caught it.
      probe.style.color = "var(--rex-warning-bright)";
      document.body.appendChild(probe);
      const warning = getComputedStyle(probe).color;
      probe.remove();
      const r = el.getBoundingClientRect();
      const rr = row ? row.getBoundingClientRect() : null;
      return {
        color: getComputedStyle(el).color,
        warning,
        rowText: row ? row.innerText : "",
        inside: rr ? r.left >= rr.left - 1 && r.right <= rr.right + 1 && r.top >= rr.top - 1 && r.bottom <= rr.bottom + 1 : false,
        overflow: el.scrollWidth > el.clientWidth + 1,
        count: [...document.querySelectorAll("div")].filter((d) => d.children.length === 0 && d.textContent.trim() === note).length,
      };
    }, NOTE);
    ok(`@${width} ?busy=8.3: the note renders`, seen !== null, "not found");
    if (seen) {
      ok(`@${width} it is in the PHP-FPM 8.3 row`, seen.rowText.includes("PHP-FPM 8.3"), seen.rowText.slice(0, 120));
      ok(`@${width} only that pool carries it`, seen.count === 1, `${seen.count} notes`);
      ok(`@${width} in the warning colour`, seen.color === seen.warning, `${seen.color} vs ${seen.warning}`);
      ok(`@${width} inside its row's box`, seen.inside, "escapes the row");
      // A truncated line is acceptable at a narrow window (the title carries the rest); report it.
      console.log(`  · @${width} truncated: ${seen.overflow}`);
    }
    await page.close();
  }

  await browser.close();
  if (fails.length) {
    console.log(`poolbusy: FAIL (${fails.length})`);
    process.exit(1);
  }
  console.log("poolbusy: PASS");
})();
