// WebKit check: plugin-list actions report what they did — and never more.
//
// The gap (QA, 11 Aug 2026): Activate / Deactivate / Delete flipped a row and
// said nothing. On a long list — the one in the report had 26 rows — the row you
// acted on is often off screen by the time the call returns, so the only
// feedback was a toggle you could no longer see.
//
// Everything is asserted against `role="status"` (the toaster), so a row's own
// "Active" label or the install card's copy cannot satisfy it:
//   1. Activate / Deactivate — once each, verb + plugin name;
//   2. Delete — the gated one: nothing on open, the toast only after confirming;
//   3. Update — a count on success; wp-cli's own message on failure, never a
//      cheerful count (`?update=fail` makes the command reject);
//   4. Install — announced from the SETTLED job, including `partial` as its own
//      outcome, and learned by ADOPTION (the job settled while the panel was
//      unmounted — the event went to nobody).
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const toasts = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[role="status"] > div')].map((n) => n.innerText.trim()),
  );

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  const open = async (query) => {
    const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
    page.on("pageerror", (e) => fails.push(`pageerror(${query}): ${e.message}`));
    await page.goto(`${BASE}/dev/git-panel?panel=wp-add&${query}`, { waitUntil: "networkidle" });
    await page.waitForTimeout(700);
    return page;
  };
  /** Toasts mentioning `needle`, after giving the click time to settle. */
  const about = async (page, needle) => {
    await page.waitForTimeout(700);
    return (await toasts(page)).filter((t) => t.includes(needle));
  };

  // --- 1) the toggles ----------------------------------------------------
  const list = await open("plugins=list");
  if ((await toasts(list)).length !== 0) fails.push("a toast appeared before any click");

  // Control metrics: the Add bar's "Activate" box was the browser's own ~12px
  // default next to 18px list checkboxes — small enough that QA read it as a
  // different kind of control. Every checkbox on this panel is one control.
  const boxes = await list.evaluate(() =>
    [...document.querySelectorAll('input[type="checkbox"]')].map((n) => {
      const r = n.getBoundingClientRect();
      return { w: Math.round(r.width), h: Math.round(r.height) };
    }),
  );
  const odd = boxes.filter((b) => b.w !== boxes[0].w || b.h !== boxes[0].h);
  if (boxes.length < 2) fails.push("the panel rendered fewer checkboxes than expected");
  if (odd.length) fails.push(`checkbox sizes disagree: ${JSON.stringify(boxes)}`);

  await list.getByLabel("Deactivate akismet").click();
  let said = await about(list, "akismet");
  if (said.length !== 1) fails.push(`deactivate announced ${said.length}× (want 1)`);
  if (said[0] && !/deactivated/i.test(said[0]))
    fails.push(`the toast does not name the action: ${said[0]}`);

  await list.getByLabel("Activate hello-dolly").click();
  said = await about(list, "hello-dolly");
  if (said.length !== 1) fails.push(`activate announced ${said.length}× (want 1)`);
  if (said[0] && !/activated/i.test(said[0]))
    fails.push(`the toast does not name the action: ${said[0]}`);

  // --- 2) delete: gated, so the toast must wait for the confirmation -----
  const del = await open("plugins=list");
  await del.getByTitle("Delete", { exact: true }).first().click();
  await del.waitForTimeout(400);
  if ((await toasts(del)).length !== 0)
    fails.push("opening the delete dialog already claimed a deletion");
  const confirm = del.getByRole("button", { name: "Delete", exact: true }).last();
  if ((await confirm.count()) === 0) {
    fails.push("no confirm button in the delete dialog");
  } else {
    await confirm.click();
    const gone = await about(del, "akismet");
    if (gone.length !== 1) fails.push(`delete announced ${gone.length}× (want 1)`);
    if (gone[0] && !/deleted/i.test(gone[0])) fails.push(`delete toast wrong: ${gone[0]}`);
  }

  // --- 3) update, both directions ---------------------------------------
  const upd = await open("plugins=list");
  const updBtn = upd.getByTitle("Update to 22.4");
  if ((await updBtn.count()) === 0) {
    fails.push("no update button on the row that offers one — verdict or fixture drift");
  } else {
    await updBtn.click();
    const done = await about(upd, "wordpress-seo");
    if (done.length !== 1) fails.push(`update announced ${done.length}× (want 1)`);
    if (done[0] && !/updated/i.test(done[0])) fails.push(`update toast wrong: ${done[0]}`);
  }

  const updFail = await open("plugins=list&update=fail");
  await updFail.getByTitle("Update to 22.4").click();
  await updFail.waitForTimeout(900);
  const afterFail = await toasts(updFail);
  if (afterFail.some((t) => /^Updated /i.test(t)))
    fails.push(`a failed update still claimed success: ${JSON.stringify(afterFail)}`);
  if (!afterFail.some((t) => t.includes("Only updated 0 of 1")))
    fails.push(`the failure toast drops wp-cli's own message: ${JSON.stringify(afterFail)}`);

  // --- 4) install outcomes, learned by adoption -------------------------
  const ok = await open("install=ok");
  const okSaid = (await toasts(ok)).filter((t) => /install/i.test(t));
  if (okSaid.length !== 1) fails.push(`settled install announced ${okSaid.length}× (want 1)`);
  if (okSaid[0] && !/^Installed /.test(okSaid[0])) fails.push(`install toast wrong: ${okSaid[0]}`);

  const partial = await open("install=partial");
  const partSaid = (await toasts(partial)).filter((t) => /install/i.test(t));
  if (partSaid.length !== 1) fails.push(`partial install announced ${partSaid.length}× (want 1)`);
  // The half that matters: "partial" may not round to either success or failure.
  if (partSaid[0] && !/some of/i.test(partSaid[0]))
    fails.push(`a partial install was rounded off: ${partSaid[0]}`);
  if (partSaid[0] && !partSaid[0].includes("Only installed 1 of 2"))
    fails.push(`the partial toast drops wp-cli's own summary: ${partSaid[0]}`);

  const running = await open("install=running");
  if ((await toasts(running)).length !== 0)
    fails.push("a still-RUNNING install was announced as an outcome");

  await list.screenshot({ path: "shot-wptoast.png", fullPage: true });
  await browser.close();

  if (fails.length) {
    console.log("WP-TOAST CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("WP-TOAST CHECK: ALL PASS");
})();
