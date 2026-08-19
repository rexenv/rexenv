// WebKit check: the install card's LIFECYCLE — success clears itself, every
// other outcome waits to be dismissed.
//
// Asked for on 19 Aug 2026, and the asymmetry is the whole feature: after a
// successful install the card repeats what the toast already said and what the
// list below it now shows, so it clears after three seconds. After a FAILURE
// the card holds the only copy of the reason — so it stays, and gains an × so
// the user can put it away when they are done reading.
//
// Two traps this exists to catch, both of which make an auto-hide worse than
// no auto-hide:
//   - hiding a card the user is READING. Opening the log holds the timer; the
//     check opens it inside the window and expects the card to still be there
//     well after the timer would have fired.
//   - offering a dismiss on a RUNNING job, which would hide work still
//     happening — the one thing this card exists to prevent.
const { webkit } = require("playwright");

const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const url = (q) => `${BASE}/dev/git-panel?panel=wp-add&install=${q}`;
/** The card's own linger is 3s; wait past it with margin, never up to it. */
const PAST_LINGER = 4_000;

const cardCount = (page) =>
  page.evaluate(() => document.body.innerText.split("plugin install").length - 1);
const dismissBtn = (page) => page.getByRole("button", { name: "Dismiss install result" });

(async () => {
  const browser = await webkit.launch();
  const problems = [];
  const open = async (q) => {
    const page = await browser.newPage({ viewport: { width: 1180, height: 800 }, colorScheme: "dark" });
    page.on("pageerror", (e) => problems.push(`${q}: pageerror ${String(e).split("\n")[0]}`));
    await page.goto(url(q), { waitUntil: "networkidle" });
    await page.waitForTimeout(300);
    return page;
  };

  // 1. Success clears itself.
  {
    const page = await open("ok");
    if ((await cardCount(page)) === 0) problems.push("ok: the card never rendered — nothing below is meaningful");
    await page.waitForTimeout(PAST_LINGER);
    if ((await cardCount(page)) !== 0) problems.push("ok: the card is still there after the linger — success does not clear itself");
    await page.close();
  }

  // 2. Success + the log open: the timer is HELD.
  {
    const page = await open("ok");
    await page.getByRole("button", { name: "Show log" }).click();
    await page.waitForTimeout(PAST_LINGER);
    if ((await cardCount(page)) === 0) {
      problems.push("ok+log: the card vanished while its log was open — the reader lost what they clicked to see");
    } else {
      // ...and it can still be put away by hand.
      if ((await dismissBtn(page).count()) === 0) {
        problems.push("ok+log: held open with no dismiss — the card is now permanent");
      } else {
        await dismissBtn(page).click();
        await page.waitForTimeout(200);
        if ((await cardCount(page)) !== 0) problems.push("ok+log: dismiss did not clear the card");
      }
    }
    await page.close();
  }

  // 3. Every non-ok outcome STAYS, and can be dismissed.
  for (const q of ["partial", "cancelled"]) {
    const page = await open(q);
    if ((await cardCount(page)) === 0) {
      problems.push(`${q}: the card never rendered`);
      await page.close();
      continue;
    }
    await page.waitForTimeout(PAST_LINGER);
    if ((await cardCount(page)) === 0) {
      problems.push(`${q}: the card cleared itself — a failure's log is the point, and it is gone`);
      await page.close();
      continue;
    }
    if ((await dismissBtn(page).count()) === 0) {
      problems.push(`${q}: no dismiss control — the card cannot be put away`);
    } else {
      await dismissBtn(page).click();
      await page.waitForTimeout(200);
      if ((await cardCount(page)) !== 0) problems.push(`${q}: dismiss did not clear the card`);
    }
    await page.close();
  }

  // 4. A RUNNING job offers Cancel, never Dismiss.
  {
    const page = await open("running");
    if ((await dismissBtn(page).count()) !== 0) {
      problems.push("running: a dismiss control is offered — it would hide work that is still happening");
    }
    if ((await page.getByRole("button", { name: "Cancel" }).count()) === 0) {
      problems.push("running: no Cancel — the escape from wp-cli's silent stretches is gone");
    }
    await page.waitForTimeout(PAST_LINGER);
    if ((await cardCount(page)) === 0) problems.push("running: the card cleared itself mid-install");
    await page.screenshot({ path: `${__dirname}/shot-wpinstallcard.png` });
    await page.close();
  }

  await browser.close();
  if (problems.length) {
    console.log(`✗ wpinstallcard — ${problems.join("; ")}`);
    process.exit(1);
  }
  console.log("✓ wpinstallcard — success clears itself, every other outcome waits to be dismissed");
})();
