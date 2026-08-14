// WebKit check: the Mail screen's unread handling (`/dev/ui-review?view=mail`).
//
// What this exists to catch, in one sentence: the inbox list is polled every
// 5 seconds, so anything that only becomes true "on the next refetch" is a
// feature that works somewhere between instantly and five seconds later — which
// is exactly how the read-state flip was reported ("clicking the subject marks
// it read, clicking the sender takes longer"). It was never about where you
// clicked; it was where the poll happened to be. So the assertions below all
// pin the same thing: the screen must update from what it already knows, at the
// moment it becomes true, with NO list refetch in between.
//
// The mock marks a message read when its DETAIL is fetched — Mailpit's own side
// effect — so a UI that waits for the poll is visibly wrong here rather than
// accidentally right.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const url = (q) => `${BASE}/dev/ui-review?view=mail${q ?? ""}`;

const listCalls = (page) =>
  page.evaluate(() => (window.__mailCalls ?? []).filter((c) => c.cmd === "mailpit_messages").length);

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1000, height: 780 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  page.on("console", (m) => m.type() === "error" && fails.push(`console.error: ${m.text()}`));

  // ── 1. Baseline: six messages, four unread, grouped by site ───────────────
  await page.goto(url(), { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  const unreadRows = () => page.locator('[data-read="0"]');
  const allRows = () => page.locator("[data-read]");
  if ((await page.getByText("6 captured · 4 unread", { exact: false }).count()) === 0)
    fails.push("header does not state captured/unread counts");
  if ((await unreadRows().count()) !== 4)
    fails.push(`expected 4 unread rows across both site groups, saw ${await unreadRows().count()}`);

  // ── 2. The unread filter is a SERVER-side search, not a display trick ─────
  // Hiding read rows from the fetched page would leave the unread ones that
  // didn't make the page invisible — the exact complaint the filter answers.
  await page.getByRole("button", { name: "Show unread only" }).click();
  await page.waitForTimeout(600);
  const calls = await page.evaluate(() => window.__mailCalls ?? []);
  const filtered = calls.filter((c) => c.cmd === "mailpit_messages" && c.unreadOnly === true);
  if (filtered.length === 0)
    fails.push("clicking Unread never asked the backend for unread mail (client-side filtering only)");
  if ((await allRows().count()) !== (await unreadRows().count()))
    fails.push("a READ message is still listed under the Unread filter");
  for (const gone of ["Plugin updated: Akismet", "Weekly digest"]) {
    if ((await page.getByText(gone, { exact: false }).count()) !== 0)
      fails.push(`read message still listed under Unread: ${gone}`);
  }
  // The count on the chip stays MAILBOX-wide — "unread among the unread" would
  // be a number that means nothing.
  if ((await page.getByText("6 captured · 4 unread", { exact: false }).count()) === 0)
    fails.push("the unread count changed meaning while filtering");
  await page.screenshot({ path: "shot-mail-unread.png", fullPage: true });

  // ── 3. Opening a message flips its row NOW, not on the next poll ──────────
  await page.getByRole("button", { name: "Show all mail" }).click();
  await page.waitForTimeout(500);
  const before = await listCalls(page);
  const target = page.locator('[data-read="0"]').first();
  const label = await target.getAttribute("aria-label");
  await target.click();
  await page.waitForTimeout(450); // well inside the 5s poll
  const after = await listCalls(page);
  if (after !== before)
    fails.push(`the list refetched during the click (${before} → ${after}) — the timing claim is untested`);
  const flipped = await page
    .locator(`[aria-label="${(label ?? "").replace("Unread message", "Read message")}"]`)
    .count();
  if (flipped !== 1)
    fails.push("the opened message still reads as unread with no refetch — back to waiting for the poll");
  await page.screenshot({ path: "shot-mail-read.png", fullPage: true });

  // ── 4. …and the message you are READING does not vanish under you ────────
  // Under the unread filter, the row it just left the filter's result set: the
  // next poll would drop it, taking the open preview with it.
  await page.getByRole("button", { name: "Show unread only" }).click();
  await page.waitForTimeout(400);
  const openUnread = page.locator('[data-read="0"]').first();
  const openLabel = (await openUnread.getAttribute("aria-label")) ?? "";
  const subject = openLabel.replace("Unread message: ", "").split(" from ")[0];
  await openUnread.click();
  await page.waitForTimeout(5600); // one full poll — the moment it used to vanish
  if ((await page.getByText(subject, { exact: false }).count()) === 0)
    fails.push(`the message being read (${subject}) disappeared from the list under the unread filter`);
  if ((await page.getByText("Loading…", { exact: false }).count()) !== 0)
    fails.push("the preview reset itself after the poll");

  // ── 5. Mark all read updates the screen from its own patch ───────────────
  // `stale=1` makes the server-side mark a NO-OP: every later poll still says
  // unread. Anything that flips here can only have come from the app's own
  // update, which is what makes the button feel instant.
  await page.goto(url("&stale=1"), { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  await page.getByRole("button", { name: "Mark all read" }).click();
  await page.waitForTimeout(400);
  const marked = await page.evaluate(() =>
    (window.__mailCalls ?? []).some((c) => c.cmd === "mailpit_mark_all_read"),
  );
  if (!marked) fails.push("Mark all read sent no command");
  if ((await unreadRows().count()) !== 0)
    fails.push(`${await unreadRows().count()} rows still read as unread right after Mark all read`);
  if (!(await page.getByRole("button", { name: "Mark all read" }).isDisabled()))
    fails.push("Mark all read stays enabled with nothing unread");

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");

  await browser.close();
  if (fails.length) {
    console.log("MAIL CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("MAIL CHECK: ALL PASS");
})();
