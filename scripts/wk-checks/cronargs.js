// WebKit check: the cron list shows the ARGUMENTS that tell two events apart.
//
// The gap this closes (ledger #384, filed 23 Aug 2026 with the column itself):
// `cron_event_list` asked WP-CLI for hook, next_run and recurrence and stopped,
// so the args were never fetched, never in the DTO, never rendered. That is a
// defect rather than a missing nicety because **WP-CLI addresses cron events by
// HOOK — there is no per-instance id** — so a hook scheduled more than once
// showed as N identical rows whose Run buttons all did the same thing.
//
// Not exotic: Action Scheduler ships with WooCommerce and schedules
// `action_scheduler_run_queue` more than once with different runners;
// `publish_future_post` carries a post id per scheduled post.
//
// The column shipped with its own L2 gap stated — "nothing asserts the column
// RENDERS" — because the cron tab had no dev-route arm. This is that arm's
// check.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const URL = `${BASE}/dev/git-panel?panel=cron`;

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${String(e).split("\n")[0]}`));

  await page.goto(URL, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);

  // Read the rendered table: hook + whatever sits in the Arguments column.
  const rows = await page.evaluate(() => {
    const out = [];
    for (const el of document.querySelectorAll("div")) {
      const spans = [...el.children].filter((c) => c.tagName === "SPAN");
      if (spans.length < 3) continue;
      const hook = spans[0].textContent.trim();
      if (!hook || hook === "Hook") continue;
      out.push({ hook, args: spans[1].textContent.trim() });
    }
    return out;
  });

  if (rows.length < 5) {
    fails.push(`only ${rows.length} cron rows rendered — the harness is not showing the panel`);
  }

  // The header must exist, or the column is being read from the wrong span.
  const header = await page.evaluate(() =>
    [...document.querySelectorAll("span")].some((s) => s.textContent.trim() === "Arguments"),
  );
  if (!header) fails.push("no Arguments column header");

  // THE case: one hook, twice, told apart ONLY by its args.
  const dupes = rows.filter((r) => r.hook === "action_scheduler_run_queue");
  if (dupes.length !== 2) {
    fails.push(`expected the duplicated hook twice, saw ${dupes.length}`);
  } else if (dupes[0].args === dupes[1].args) {
    fails.push(
      `the two action_scheduler_run_queue rows render IDENTICALLY (${JSON.stringify(dupes[0].args)}) ` +
        `— which is the defect this column exists to end`,
    );
  } else {
    const seen = dupes.map((d) => d.args).sort();
    if (seen[0] !== '["Async Request"]' || seen[1] !== '["WP Cron"]') {
      fails.push(`args rendered but not faithfully: ${JSON.stringify(seen)}`);
    }
  }

  // A non-string arg must render too — the value is JSON, not a label.
  const future = rows.find((r) => r.hook === "publish_future_post");
  if (!future) fails.push("publish_future_post row missing");
  else if (future.args !== "[1284]") fails.push(`bare id not rendered: ${JSON.stringify(future.args)}`);

  // …and an event with NO args must render EMPTY, not "[]" — a column of
  // brackets on the majority of rows trains the eye to skip the one that
  // matters. This is the half that keeps the column readable.
  const plain = rows.filter((r) => r.hook.startsWith("wp_"));
  if (plain.length !== 2) fails.push(`expected 2 argument-less rows, saw ${plain.length}`);
  for (const p of plain) {
    if (p.args !== "") fails.push(`${p.hook} should show nothing, showed ${JSON.stringify(p.args)}`);
  }

  await browser.close();
  if (fails.length) {
    console.log("CRON-ARGS CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    `CRON-ARGS CHECK: ALL PASS (${rows.length} rows; the duplicated hook is distinguishable, ` +
      `a bare id renders, argument-less rows stay blank)`,
  );
})();
