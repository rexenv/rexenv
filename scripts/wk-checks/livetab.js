// WebKit check: the Live tab (docs/PLAN-wp-live-sync.md §2.9).
//   1. the connect form keeps Connect disabled until a `rexsync1:` key is pasted,
//      and SENDS the key (and no HTTP auth when none was typed);
//   2. paired: the site URL and key id show, Pull asks before it runs, and the
//      settled pull's numbers and the backup database are shown;
//   3. a failed pull shows its reason;
//   4. no horizontal overflow, no page error, in both themes;
//   5. Push: the picker starts with the live-owned tables UNTICKED, Push stays
//      disabled until the live host is typed exactly, and what is SENT is the
//      explicit table list (ticked ones only) + files + the typed host;
//   6. a push stopped by conflicts lists them and offers "Push anyway", which
//      asks first and sends them as overrides;
//   7. a settled push shows its backup id, and Roll back asks before it runs.
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

    // 5. the push picker
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    await page.getByRole("button", { name: "Push to live…" }).click();
    await page.waitForTimeout(400);
    const picker = page.getByTestId("push-picker");
    if (!(await picker.count())) fails.push(`${scheme} 5: no picker after Push`);
    for (const [name, want] of [["wp_posts", true], ["wp_options", true], ["wp_users", false], ["wp_comments", false]])
      if ((await page.getByLabel(name, { exact: true }).isChecked()) !== want) fails.push(`${scheme} 5: ${name} ticked=${!want} by default`);
    const pushBtn = picker.getByRole("button", { name: "Push", exact: true });
    if (!(await pushBtn.isDisabled())) fails.push(`${scheme} 5: Push enabled before the host was typed`);
    await page.getByLabel("Live host to confirm").fill("example.co");
    if (!(await pushBtn.isDisabled())) fails.push(`${scheme} 5: Push enabled on a near-miss host`);
    await page.getByLabel("Live host to confirm").fill("example.com");
    if (await pushBtn.isDisabled()) fails.push(`${scheme} 5: Push disabled on the right host`);
    await page.getByLabel("wp_postmeta", { exact: true }).uncheck();
    await pushBtn.click();
    await page.waitForTimeout(300);
    const pushes = await page.evaluate(() => window.__livePushes ?? []);
    const sentPush = pushes[0] || {};
    const wantTables = ["wp_options", "wp_posts"];
    if (!(JSON.stringify((sentPush.tables || []).slice().sort()) === JSON.stringify(wantTables) && sentPush.files === true && sentPush.confirmHost === "example.com" && Array.isArray(sentPush.overrideItems) && sentPush.overrideItems.length === 0))
      fails.push(`${scheme} 5: push sent ${JSON.stringify(pushes)}`);
    if (await picker.count()) fails.push(`${scheme} 5: the picker stayed open after Push`);
    if ((await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth))) fails.push(`${scheme}: overflow (picker)`);

    // 6. conflicts
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&push=conflicts`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    const ctext = await page.evaluate(() => document.body.innerText);
    for (const want of ["Nothing was sent", "wp_posts", "themes/shop/style.css"]) if (!ctext.includes(want)) fails.push(`${scheme} 6: "${want}" not shown`);
    await page.screenshot({ path: `shot-livetab-conflicts-${scheme}.png`, fullPage: true });
    await page.getByRole("button", { name: "Push anyway, overwriting these" }).click();
    await page.waitForTimeout(200);
    const cdlg = await page.getByRole("dialog").innerText().catch(() => "");
    if (!cdlg.includes("themes/shop/style.css")) fails.push(`${scheme} 6: Push anyway did not ask first (${cdlg.slice(0, 80)})`);
    await page.getByRole("dialog").getByRole("button", { name: "Push anyway" }).click();
    await page.waitForTimeout(300);
    const over = (await page.evaluate(() => window.__livePushes ?? []))[0] || {};
    if (!(JSON.stringify(over.overrideItems) === JSON.stringify(["wp_posts", "themes/shop/style.css"]) && JSON.stringify(over.tables) === JSON.stringify(["wp_options", "wp_posts"]) && over.files === true && over.confirmHost === "example.com"))
      fails.push(`${scheme} 6: override sent ${JSON.stringify(over)}`);

    // 7. a settled push + Roll back
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&push=ok`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    const ptext = await page.evaluate(() => document.body.innerText);
    for (const want of ["Pushed 10 tables and 3 files", "rxbak_20261010_1a2b3c"]) if (!ptext.includes(want)) fails.push(`${scheme} 7: "${want}" not shown`);
    await page.getByRole("button", { name: "Roll back" }).click();
    await page.waitForTimeout(200);
    const rdlg = await page.getByRole("dialog").innerText().catch(() => "");
    if (!rdlg.includes("rxbak_20261010_1a2b3c")) fails.push(`${scheme} 7: Roll back did not ask first (${rdlg.slice(0, 80)})`);
    if ((await page.evaluate(() => (window.__liveRollbacks ?? []).length)) !== 0) fails.push(`${scheme} 7: rollback ran before the answer`);
    await page.getByRole("dialog").getByRole("button", { name: "Roll back" }).click();
    await page.waitForTimeout(300);
    const rb = await page.evaluate(() => window.__liveRollbacks ?? []);
    if (!(rb[0] && rb[0].backupId === "rxbak_20261010_1a2b3c")) fails.push(`${scheme} 7: rollback sent ${JSON.stringify(rb)}`);
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("livetab.js: FAIL\n  " + fails.join("\n  "));
    process.exit(1);
  }
  console.log("livetab.js: all green");
})();
