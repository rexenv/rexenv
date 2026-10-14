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
//   7. a settled push shows its backup id, and Roll back asks before it runs;
//   8. Download plugin saves the zip once and the toast names it;
//   9. a log taller than its box opens scrolled to its END — the verdict lines are in view;
//  10. Check status (14 Oct 2026): one call, and the verdict card reads up to date / behind
//      (with Pull) / ahead (with Push…) / diverged / never synced, "Show what moved" lists the
//      tables with a live and a here column;
//  11. the Pull menu: "Pull everything again" asks and SENDS full=true; the plain button sends
//      full=false; a settled incremental pull says what was kept and removed;
//  12. the picker: an UNCHANGED table starts unticked and badged "unchanged", a changed one ticked
//      and badged "changed here";
//  13. the history card lists past syncs with their outcome; the header says when the last sync was.
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
    // 8. Download plugin: one click saves the zip and says where (plan §11 Q1).
    await page.goto(`${BASE}/dev/ui-review?view=live`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: "Download plugin (.zip)" }).click();
    await page.waitForTimeout(400);
    const zips = await page.evaluate(() => window.__pluginZips ?? 0);
    const toastText = await page.evaluate(() => document.body.innerText);
    if (!(zips === 1 && toastText.includes("rexenv-sync.zip") && toastText.includes("Upload Plugin"))) fails.push(`${scheme} 8: Download plugin (calls=${zips}, toast shown=${toastText.includes("rexenv-sync.zip")})`);

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
    // 9. the log opens at its end.
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&longlog=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    await page.getByRole("button", { name: "Show log" }).click();
    await page.waitForTimeout(300);
    const atEnd = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="live-job-log"]');
      return el ? { tall: el.scrollHeight > el.clientHeight + 20, end: el.scrollHeight - el.scrollTop - el.clientHeight < 8 } : null;
    });
    if (!atEnd || !atEnd.tall || !atEnd.end) fails.push(`${scheme} 9: the log did not open at its end ${JSON.stringify(atEnd)}`);

    // 10. Check status, every verdict.
    for (const [verdict, want, button] of [
      ["up_to_date", "Up to date", null],
      ["behind", "Live has changes you do not have here — 1 table, 1 file. Pull", "Pull"],
      ["ahead", "This site is ahead of live — 1 table, 2 files changed here. Push", "Push…"],
      ["diverged", "Both sides changed — live: 1 table, 2 files; here: 1 table, 2 files", "Pull"],
      ["never_synced", "Never synced — pull once first", "Pull"],
    ]) {
      await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&status=${verdict}`, { waitUntil: "networkidle" });
      await page.waitForTimeout(300);
      await page.getByRole("button", { name: "Check status" }).click();
      await page.waitForTimeout(400);
      const calls = await page.evaluate(() => (window.__liveStatusCalls ?? []).length);
      const card = page.getByTestId("live-status");
      const vtext = (await card.innerText().catch(() => "")).replace(/\s+/g, " ");
      if (!(calls === 1 && vtext.includes(want))) fails.push(`${scheme} 10 ${verdict}: calls=${calls} card="${vtext.slice(0, 120)}"`);
      if (button && !(await card.getByRole("button", { name: button, exact: true }).count())) fails.push(`${scheme} 10 ${verdict}: no ${button} button on the card`);
      if (!button && (await card.getByRole("button").count())) fails.push(`${scheme} 10 ${verdict}: a button on an up-to-date card`);
      if (verdict === "diverged") {
        await card.getByRole("button", { name: "Show what moved" }).click();
        await page.waitForTimeout(200);
        const d = (await page.getByTestId("live-status-detail").innerText()).replace(/\s+/g, " ");
        for (const w of ["wp_posts", "changed", "themes/shop/style.css", "uploads/2025/old.jpg", "changed on both sides"]) if (!d.includes(w)) fails.push(`${scheme} 10 diverged: detail lacks "${w}"`);
        await page.screenshot({ path: `shot-livetab-status-${scheme}.png`, fullPage: true });
      }
      if (verdict === "ahead" && !vtext.includes("1 live-owned table changed here")) fails.push(`${scheme} 10 ahead: the live-owned line is missing`);
    }
    if ((await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth))) fails.push(`${scheme}: overflow (status)`);

    // 11. The Pull menu.
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: "More ways to pull" }).click();
    await page.waitForTimeout(150);
    await page.getByRole("menuitem", { name: /Pull everything again/ }).click();
    await page.waitForTimeout(200);
    const fdlg = await page.getByRole("dialog").innerText().catch(() => "");
    if (!fdlg.includes("Every table and every wp-content file")) fails.push(`${scheme} 11: "everything again" did not ask (${fdlg.slice(0, 80)})`);
    await page.getByRole("dialog").getByRole("button", { name: "Pull", exact: true }).click();
    await page.waitForTimeout(300);
    const pulls = await page.evaluate(() => window.__livePulls ?? []);
    if (!(pulls[0] && pulls[0].full === true)) fails.push(`${scheme} 11: everything-again sent ${JSON.stringify(pulls)}`);
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: "Pull from live" }).click();
    await page.waitForTimeout(200);
    const idlg = await page.getByRole("dialog").innerText().catch(() => "");
    if (!idlg.includes("Only the tables and files that changed on the live site")) fails.push(`${scheme} 11: the plain pull's confirm (${idlg.slice(0, 80)})`);
    await page.getByRole("dialog").getByRole("button", { name: "Pull", exact: true }).click();
    await page.waitForTimeout(300);
    const pulls2 = await page.evaluate(() => window.__livePulls ?? []);
    if (!(pulls2[0] && pulls2[0].full === false)) fails.push(`${scheme} 11: the plain pull sent ${JSON.stringify(pulls2)}`);
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&pull=incremental`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    const itext = (await page.evaluate(() => document.body.innerText)).replace(/\s+/g, " ");
    for (const w of ["Pulled 3 tables", "9 tables and 455 files unchanged on live were kept", "1 file gone from live removed here", "uploads/2026/notes.txt"]) if (!itext.includes(w)) fails.push(`${scheme} 11: incremental card lacks "${w}"`);

    // 12. The picker's default ticks follow what changed here.
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: "Push to live…" }).click();
    await page.waitForTimeout(400);
    if (await page.getByLabel("wp_terms", { exact: true }).isChecked()) fails.push(`${scheme} 12: an unchanged table started ticked`);
    if (!(await page.getByLabel("wp_posts", { exact: true }).isChecked())) fails.push(`${scheme} 12: a changed table started unticked`);
    const ptxt = (await page.getByTestId("push-picker").innerText()).replace(/\s+/g, " ");
    if (!(ptxt.includes("unchanged") && ptxt.includes("changed here") && ptxt.includes("1 unchanged table start unticked"))) fails.push(`${scheme} 12: picker badges (${ptxt.slice(0, 200)})`);
    // 12b. an old base (every local column "unknown"): no table is listed as moved, and the
    //      card says why what changed here cannot be told.
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&status=old_base`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: "Check status" }).click();
    await page.waitForTimeout(400);
    const otext = (await page.getByTestId("live-status").innerText()).replace(/\s+/g, " ");
    if (!(otext.includes("Up to date") && otext.includes("cannot be told yet") && !otext.includes("Show what moved"))) fails.push(`${scheme} 12b: old-base card (${otext.slice(0, 160)})`);

    // 13. History + the last-sync line.
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    const lastLine = await page.getByTestId("live-last-sync").innerText();
    if (!/Last sync: pulled \d+ days? ago/.test(lastLine)) fails.push(`${scheme} 13: last-sync line "${lastLine}"`);
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1&history=none`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    const baseLine = await page.getByTestId("live-last-sync").innerText();
    if (!/Last sync: pulled \d+ days? ago/.test(baseLine)) fails.push(`${scheme} 13: without history the line must read the base ("${baseLine}")`);
    if (await page.getByTestId("live-history").count()) fails.push(`${scheme} 13: a history card with no entries`);
    await page.goto(`${BASE}/dev/ui-review?view=live&paired=1`, { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    await page.getByRole("button", { name: /History/ }).click();
    await page.waitForTimeout(200);
    const htext = (await page.getByTestId("live-history").innerText()).replace(/\s+/g, " ");
    for (const w of ["3 syncs", "Pulled 12 tables (1560 rows), 457 files", "Pushed (conflicts)", "wp_posts", "Pushed 2 tables, 1 file", "rxbak_20"]) if (!htext.includes(w)) fails.push(`${scheme} 13: history lacks "${w}" (${htext.slice(0, 200)})`);
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("livetab.js: FAIL\n  " + fails.join("\n  "));
    process.exit(1);
  }
  console.log("livetab.js: all green");
})();
