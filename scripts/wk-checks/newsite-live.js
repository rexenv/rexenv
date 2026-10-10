// WebKit check: New site → From a live site (docs/PLAN-wp-live-sync.md §2.7, L7).
//   1. the source button exists only for WordPress; Check stays disabled until
//      a `rexsync1:` key is pasted, and SENDS the key (no HTTP auth when none typed);
//   2. the preview names the live site, its WordPress, PHP and table count, and
//      prefills the name (→ domain example.rex) and the PHP version;
//   3. Create reads "Create and pull", is enabled only after the preview, and
//      SENDS the key, the domain and the PHP version;
//   4. a multisite preview says so and keeps Create disabled;
//   5. no page error, in both themes;
//   6. (#855) a live PHP that is not installed here: the line names the PHP the site WILL run and
//      says the live one is not installed — never "will run PHP 8.0".
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  for (const scheme of ["dark", "light"]) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 900 }, colorScheme: scheme });
    page.on("pageerror", (e) => fails.push(`${scheme}: pageerror ${String(e).split("\n")[0]}`));
    await page.goto(`${BASE}/dev/ui-review?view=newsite`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    // Blank PHP is the default type: no live source there.
    await page.getByRole("button", { name: "Continue" }).click();
    await page.waitForTimeout(200);
    if (await page.getByRole("button", { name: "From a live site" }).count()) fails.push(`${scheme} 1: the live source is offered for Blank PHP`);
    await page.getByRole("button", { name: "Back" }).click();
    await page.getByText("WordPress", { exact: true }).click();
    await page.getByRole("button", { name: "Continue" }).click();
    await page.waitForTimeout(200);
    await page.getByRole("button", { name: "From a live site" }).click();
    await page.waitForTimeout(200);
    if (!(await page.getByRole("button", { name: "Download plugin (.zip)" }).count())) fails.push(`${scheme} 1: no Download plugin in the live source`);
    const check = page.getByRole("button", { name: "Check", exact: true });
    if (!(await check.isDisabled())) fails.push(`${scheme} 1: Check enabled with no key`);
    await page.getByLabel("Connection key").fill("not a key");
    if (!(await check.isDisabled())) fails.push(`${scheme} 1: Check enabled on a non-key`);
    const create = page.getByRole("button", { name: "Create and pull" });
    if (!(await create.count())) fails.push(`${scheme} 3: the Create button does not read "Create and pull"`);
    else if (!(await create.isDisabled())) fails.push(`${scheme} 3: Create enabled before the preview`);
    await page.getByLabel("Connection key").fill("rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9");
    await check.click();
    await page.waitForTimeout(400);
    const sent = (await page.evaluate(() => window.__livePreviews ?? []))[0] || {};
    if (!(sent.key === "rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9" && sent.basicAuthUser === null)) fails.push(`${scheme} 1: preview sent ${JSON.stringify(sent)}`);
    const text = await page.evaluate(() => document.body.innerText);
    for (const want of ["https://example.com", "WordPress 6.8", "PHP 8.3.1", "12 tables", "run PHP 8.3"]) if (!text.includes(want)) fails.push(`${scheme} 2: "${want}" not shown`);
    // The domain is an input (its value is not in innerText) beside a `.rex` suffix.
    if (!(await page.evaluate(() => [...document.querySelectorAll("input")].filter((i) => i.value === "example").length >= 2 && document.body.innerText.includes(".rex"))))
      fails.push(`${scheme} 2: the name and domain were not prefilled from the live host`);
    if (await page.getByText("WordPress install", { exact: true }).count()) fails.push(`${scheme} 2: the admin fields are asked for a from-live site`);
    if (await create.isDisabled()) fails.push(`${scheme} 3: Create disabled after the preview`);
    await page.screenshot({ path: `shot-newsite-live-${scheme}.png`, fullPage: true });
    await create.click();
    await page.waitForTimeout(400);
    const made = (await page.evaluate(() => window.__liveCreates ?? []))[0] || {};
    if (!(made.key === "rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9" && made.domain === "example.rex" && made.phpVersion === "8.3" && made.name === "example"))
      fails.push(`${scheme} 3: create sent ${JSON.stringify(made)}`);

    await page.goto(`${BASE}/dev/ui-review?view=newsite&live=multisite`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    await page.getByText("WordPress", { exact: true }).click();
    await page.getByRole("button", { name: "Continue" }).click();
    await page.getByRole("button", { name: "From a live site" }).click();
    await page.getByLabel("Connection key").fill("rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9");
    await page.getByRole("button", { name: "Check", exact: true }).click();
    await page.waitForTimeout(400);
    if (!(await page.evaluate(() => document.body.innerText)).includes("cannot copy a network")) fails.push(`${scheme} 4: the multisite refusal is not shown`);
    if (!(await page.getByRole("button", { name: "Create and pull" }).isDisabled())) fails.push(`${scheme} 4: Create enabled on a network`);
    // 6. the live site's PHP is not installed here.
    await page.goto(`${BASE}/dev/ui-review?view=newsite&live=php80`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    await page.getByText("WordPress", { exact: true }).click();
    await page.getByRole("button", { name: "Continue" }).click();
    await page.getByRole("button", { name: "From a live site" }).click();
    await page.getByLabel("Connection key").fill("rexsync1:eyJ1IjoiaHR0cHM6Ly9leGFtcGxlLmNvbSJ9");
    await page.getByRole("button", { name: "Check", exact: true }).click();
    await page.waitForTimeout(400);
    const line6 = await page.getByTestId("live-preview").innerText().catch(() => "");
    if (line6.includes("will run PHP 8.0") || !line6.includes("PHP 8.0, the live site's, is not installed here"))
      fails.push(`${scheme} 6: the preview says ${JSON.stringify(line6)}`);
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("newsite-live.js: FAIL\n  " + fails.join("\n  "));
    process.exit(1);
  }
  console.log("newsite-live.js: all green");
})();
