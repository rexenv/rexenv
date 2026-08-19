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
//
// It also covers the OTHER thing this card has to do with a failure: when
// wp-cli refused because the destination folder already exists, the card names
// that folder and offers wp-admin's "Replace current with uploaded" — and the
// check reads what the button SENDS, since `--force` is the whole difference
// and nothing on screen shows it.
//
// Both panels are driven: the plugins one through the git-panel harness, the
// themes one through `/dev/ui-review?view=themes`. They share a hook and a
// component, and that is precisely why the second is checked — a shared
// implementation still needs both call sites to pass the props.
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

  // 5. The wall a re-uploaded zip hits, and the way out.
  //    wp-cli refuses to unpack over a folder that exists and reports it as an
  //    ordinary failure whose summary says only "No plugins installed." —
  //    which is what the report showed. wp-admin answers the same wall with
  //    "Replace current with uploaded"; the card has to name the folder and
  //    offer that, and the offer has to actually send `--force`, which is
  //    invisible on screen: a button that looked right and re-sent the same
  //    refused command would pass every rendering assertion.
  {
    const page = await open("blocked");
    const text = await page.evaluate(() => document.body.innerText);
    if (!text.includes("betterlinks-pro is already installed")) {
      problems.push("blocked: the card does not name the folder that is in the way");
    }
    // The TOAST is half of this: reporting "Install … failed: Error: No plugins
    // installed." is wp-cli's sentence and a lie about what happened — nothing
    // was installed and nothing was harmed, and the card is offering a way
    // forward while the toast calls it a failure.
    const toastText = await page.evaluate(() => {
      const el = [...document.querySelectorAll("div")].filter((d) =>
        /already installed|failed/i.test(d.textContent || ""),
      );
      return el.map((d) => d.textContent.trim()).join(" | ");
    });
    if (/install .*failed/i.test(toastText)) {
      problems.push(`blocked: the toast still calls it a failure — ${toastText.slice(0, 120)}`);
    }
    if (!/already installed/i.test(toastText)) {
      problems.push("blocked: the toast does not say the plugin is already installed");
    }

    const replace = page.getByRole("button", { name: "Replace with the uploaded zip" });
    if ((await replace.count()) === 0) {
      problems.push("blocked: no Replace control — the only way forward is re-uploading and failing again");
    } else {
      await replace.click();
      await page.waitForTimeout(300);
      const sent = await page.evaluate(() => window.__wpInstalls ?? []);
      const last = sent[sent.length - 1];
      if (!last) {
        problems.push("blocked: Replace started no install at all");
      } else {
        if (last.force !== true) problems.push(`blocked: Replace sent force=${JSON.stringify(last.force)} — it re-runs the command wp-cli already refused`);
        if (last.source !== "zip") problems.push(`blocked: Replace changed the source to ${JSON.stringify(last.source)}`);
        if (!Array.isArray(last.slugs) || !last.slugs[0]?.endsWith(".zip"))
          problems.push(`blocked: Replace did not re-send the zip (${JSON.stringify(last.slugs)})`);
      }
    }
    await page.close();
  }

  // 6. THE THEMES PANEL. Same hook, same component — which is an argument, not
  //    evidence. The rules are re-asserted where a job's `kind` really is
  //    "theme", because "it is the same code" is exactly what someone says
  //    right before one of the two call sites is missing a prop.
  {
    const themeUrl = (q) => `${BASE}/dev/ui-review?view=themes&install=${q}`;
    const themeCards = (page) =>
      page.evaluate(() => document.body.innerText.split("theme install").length - 1);

    const ok = await browser.newPage({ viewport: { width: 1180, height: 900 }, colorScheme: "dark" });
    ok.on("pageerror", (e) => problems.push(`themes ok: pageerror ${String(e).split("\n")[0]}`));
    await ok.goto(themeUrl("ok"), { waitUntil: "networkidle" });
    await ok.waitForTimeout(300);
    if ((await themeCards(ok)) === 0) problems.push("themes ok: the card never rendered");
    await ok.waitForTimeout(PAST_LINGER);
    if ((await themeCards(ok)) !== 0)
      problems.push("themes ok: the card is still there after the linger — the themes panel did not get the rule");
    await ok.close();

    const bad = await browser.newPage({ viewport: { width: 1180, height: 900 }, colorScheme: "dark" });
    bad.on("pageerror", (e) => problems.push(`themes partial: pageerror ${String(e).split("\n")[0]}`));
    await bad.goto(themeUrl("partial"), { waitUntil: "networkidle" });
    await bad.waitForTimeout(PAST_LINGER);
    if ((await themeCards(bad)) === 0) {
      problems.push("themes partial: the card cleared itself — the failure's log is gone");
    } else if ((await dismissBtn(bad).count()) === 0) {
      problems.push("themes partial: no dismiss control — the card cannot be put away");
    } else {
      await dismissBtn(bad).click();
      await bad.waitForTimeout(200);
      if ((await themeCards(bad)) !== 0) problems.push("themes partial: dismiss did not clear the card");
    }
    await bad.close();
  }

  await browser.close();
  if (problems.length) {
    console.log(`✗ wpinstallcard — ${problems.join("; ")}`);
    process.exit(1);
  }
  console.log("✓ wpinstallcard — success clears itself, failures wait to be dismissed, and a blocked zip offers a real --force replace");
})();
