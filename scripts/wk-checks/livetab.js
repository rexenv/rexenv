// WebKit check: the Live tab (docs/PLAN-wp-live-sync.md §2.9).
//   1. the connect form keeps Connect disabled until a `rexsync1:` key is pasted,
//      and SENDS the key (and no HTTP auth when none was typed);
//   2. paired: the site URL and key id show, Pull asks before it runs, and the
//      settled pull's numbers and the backup database are shown;
//   3. a failed pull shows its reason;
//   4. no horizontal overflow, no page error, in both themes.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  for (const scheme of ["dark", "light"]) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, colorScheme: scheme });
    page.on("pageerror", (e) => fails.push(`${scheme}: pageerror ${String(e).split("\n")[0]}`));
    await page.goto(`${BASE}/dev/ui-review?view=live`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    const connect = page.getByRole("button", { name: "Connect", exact: true });
    if (!(await connect.isDisabled())) fails.push(`${scheme} 1: Connect enabled with no key`);
    await page.getByLabel("Connection key").fill("not a key");
    if (!(await connect.isDisabled())) fails.push(`${scheme} 1: Connect enabled on a non-key`);
    await page.getByLabel("Connection key").fill("rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9");
    if (await connect.isDisabled()) fails.push(`${scheme} 1: Connect disabled on a key`);
    await connect.click();
    await page.waitForTimeout(400);
    const sent = await page.evaluate(() => window.__livePairs ?? []);
    const a = sent[0] || {};
    if (!(a.key === "rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9" && a.basicAuthUser === null)) fails.push(`${scheme} 1: pair sent ${JSON.stringify(sent)}`);
    if ((await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth))) fails.push(`${scheme}: overflow (connect)`);

    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    const text = await page.evaluate(() => document.body.innerText);
    for (const want of ["https://example.com", "k_0123abcd", "12 tables", "457 files", "wp_example_rex_prepull"])
      if (!text.includes(want)) fails.push(`${scheme} 2: "${want}" not shown`);
    await page.getByRole("button", { name: "Pull from live" }).click();
    await page.waitForTimeout(200);
    const dlg = await page.getByRole("dialog").innerText().catch(() => "");
    if (!dlg.includes("replace this site")) fails.push(`${scheme} 2: Pull did not ask first (${dlg.slice(0, 80)})`);
    await page.screenshot({ path: `shot-livetab-${scheme}.png`, fullPage: true });
    if ((await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth))) fails.push(`${scheme}: overflow (paired)`);

    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&pull=failed`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    if (!(await page.evaluate(() => document.body.innerText)).includes("firewall")) fails.push(`${scheme} 3: the failed pull's reason is not shown`);
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("livetab.js: FAIL\n  " + fails.join("\n  "));
    process.exit(1);
  }
  console.log("livetab.js: all green");
})();
