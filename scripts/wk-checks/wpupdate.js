// WebKit check: the two halves of the plugin-update claims that no layer could
// reach until the harness could deliver EVENTS (ledger #249 wiring, #250 timing).
//
// Both were open for weeks as "needs a real site". They do not: what they need
// is a listener that can fire and a query that resolves late, and the harness
// can now do each. `DevGitPanel` mocks events (`shouldMockEvents`), holds the
// update run open, and can arm a late, stale plugin-list response.
//
//   A) WIRING (#249) — the bar is fed by WP-CLI's own steps. An emitted
//      `wp-update://plugins/dev` payload must move the rendered progressbar:
//      0 → 25 → 60, with the phase word and the "n of m · slug" counter
//      following it. Before this the tracker was L0-proven against wp-cli's
//      strings and NOTHING proved the emit reached the panel — the two ends of
//      a wire, each tested, with the wire itself untested.
//
//   B) TIMING (#250) — a late in-flight check landing AFTER a finished update
//      must not restore the badge. `verdict` cannot help here: the claim is
//      `available` with an EMPTY target, which cannot be ordered and so is let
//      through by design.
//
//      MEASURED while writing this check, because the first version passed a
//      plant and therefore proved nothing. The row said `cancelQueries` was
//      "the only thing standing in front of it". It is not — `settleAfterUpdate`
//      is redundant by two, and each half is sufficient on its own:
//
//        cancel + invalidate (shipped)  badge stays gone
//        invalidate only                badge stays gone  (react-query drops a
//                                       resolution superseded by a newer fetch)
//        cancel only                    badge stays gone
//        NEITHER                        badge RETURNS — "update", no arrow
//
//      So this check holds the composite claim, which is the user-visible one,
//      and its plant is removing BOTH. Deleting either alone is not a
//      regression this layer can see, and saying so is the point: an
//      attribution the test cannot make is an attribution the row must not
//      carry.
//
// Canaries throughout: an assertion that a badge is ABSENT also passes on a
// panel that rendered nothing, so every negative is preceded by a positive.
const { webkit } = require("playwright");

const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const SITE_EVENT = "wp-update://plugins/dev";

/** Fire a progress event the way the Rust side does. */
const emit = (page, payload) =>
  page.evaluate(
    ([event, p]) => window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event, payload: p }),
    [SITE_EVENT, payload],
  );

/** Every rendered progressbar's aria-valuenow, in DOM order. */
const bars = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('[role="progressbar"]')].map((b) =>
      Number(b.getAttribute("aria-valuenow")),
    ),
  );

/** The run bar's text (count · current · phase), or null when it isn't there. */
const runBarText = (page) =>
  page.evaluate(() => {
    const bar = document.querySelector('[role="progressbar"]');
    if (!bar) return null;
    const row = bar.closest("div.flex.items-center");
    return row ? row.innerText.replace(/\s+/g, " ").trim() : null;
  });

/** What the row for `slug` claims: the amber badge and the `→ target` arrow. */
const rowOffer = (page, slug) =>
  page.evaluate((name) => {
    const rows = [...document.querySelectorAll("div")].filter(
      (d) => d.querySelector(`input[aria-label="Select ${name}"]`) !== null,
    );
    if (!rows.length) return { found: false };
    const row = rows[rows.length - 1];
    const text = row.innerText.replace(/\s+/g, " ");
    const badge = [...row.querySelectorAll("span")].some(
      (s) => s.textContent.trim().toLowerCase() === "update",
    );
    return { found: true, badge, arrow: /→/.test(text), text };
  }, slug);

(async () => {
  const browser = await webkit.launch();
  const problems = [];

  // ── A) the wiring half ────────────────────────────────────────────────────
  {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on("pageerror", (e) => problems.push(`page error (wiring): ${e.message}`));
    await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list&wpupdate=hold`, {
      waitUntil: "networkidle",
    });
    await page.waitForTimeout(700);

    // Two rows, so the run bar renders (it is gated on updating.length > 1).
    for (const slug of ["wordpress-seo", "numeric-order"]) {
      const box = page.locator(`input[aria-label="Select ${slug}"]`);
      if ((await box.count()) === 0) {
        problems.push(`FIXTURE: no row for ${slug} — nothing below proves anything`);
      } else {
        await box.check();
      }
    }
    await page.getByRole("button", { name: "Update", exact: true }).first().click();
    await page.waitForTimeout(300);

    const start = await bars(page);
    if (!start.length) {
      problems.push(
        "no progressbar rendered after starting an update — the panel shows a dead button " +
          "for the whole run, which is the state #249 exists to end",
      );
    } else if (start.some((v) => v !== 0)) {
      problems.push(`the bar starts at ${start.join("/")}, not 0 — it is inventing progress`);
    }
    const startText = await runBarText(page);
    if (startText && !/0 of 2/.test(startText)) {
      problems.push(`run bar does not say "0 of 2" before any event: "${startText}"`);
    }

    // WP-CLI's first phase for the first plugin.
    await emit(page, {
      total: 2,
      done: 0,
      current: "wordpress-seo",
      phase: "Downloading",
      fraction: 0.25,
      line: "Downloading update from https://downloads.wordpress.org/plugin/wordpress-seo.22.4.zip...",
    });
    await page.waitForTimeout(250);
    const mid = await bars(page);
    const midText = await runBarText(page);
    if (!mid.includes(25)) {
      problems.push(
        `an emitted 0.25 did not reach the bar (bars: ${mid.join("/") || "none"}). The tracker is ` +
          `L0-proven and the emit is not — this is the wire between them (#249)`,
      );
    }
    if (midText && !/Downloading/.test(midText)) {
      problems.push(`the phase word did not follow the event: "${midText}"`);
    }
    if (midText && !/wordpress-seo/.test(midText)) {
      problems.push(`the run bar does not name the plugin wp-cli is sitting on: "${midText}"`);
    }

    // Second item, and the counter has to move with it.
    await emit(page, {
      total: 2,
      done: 1,
      current: "numeric-order",
      phase: "Unpacking",
      fraction: 0.6,
      line: "Unpacking the update...",
    });
    await page.waitForTimeout(250);
    const late = await bars(page);
    const lateText = await runBarText(page);
    if (!late.includes(60)) {
      problems.push(`a second event (0.6) did not advance the bar (bars: ${late.join("/")})`);
    }
    if (lateText && !/1 of 2/.test(lateText)) {
      problems.push(`the done-count did not advance with the stream: "${lateText}"`);
    }

    // The run ends: the bar must GO, not sit at its last value.
    await page.evaluate(() => window.__rexDevFinishUpdate());
    await page.waitForTimeout(600);
    if ((await bars(page)).length) {
      problems.push("the progressbar outlived the finished run — it must clear with the mutation");
    }
    await page.screenshot({ path: `${__dirname}/shot-wpupdate-stream.png`, fullPage: true });
    await page.close();
  }

  // ── B) the timing half ────────────────────────────────────────────────────
  {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on("pageerror", (e) => problems.push(`page error (timing): ${e.message}`));
    await page.goto(`${BASE}/dev/git-panel?panel=wp-add&plugins=list&wpupdate=late`, {
      waitUntil: "networkidle",
    });
    await page.waitForTimeout(700);

    // Canary: the row must be offering an update BEFORE we run one, or the
    // "no badge" assertion at the end is about an empty list.
    const before = await rowOffer(page, "late-check");
    if (!before.found) {
      problems.push("FIXTURE: no late-check row — the timing assertion would prove nothing");
    } else if (!(before.badge && before.arrow)) {
      problems.push(
        `late-check does not offer 4.1 over 4.0 before the run, so its absence afterwards ` +
          `says nothing. row: "${before.text}"`,
      );
    }

    // Arm the late pair, then put them in flight with a refresh: this is the
    // pre-update check that used to land after the update and undo it.
    await page.evaluate(() => window.__rexDevArmLateList());
    await page.getByRole("button", { name: "Refresh the plugin list" }).click();
    await page.waitForTimeout(150); // in flight, not yet resolved

    // Start and finish the update while they are still out there.
    await page.locator('button[title="Update to 4.1"]').first().click();
    await page.waitForTimeout(150);
    await page.evaluate(() => window.__rexDevFinishUpdate());

    // Long enough for the armed responses (1.2s) to land. Nothing else can
    // write the list afterwards — the harness freezes it — so what renders now
    // is either the settled truth or the late claim.
    await page.waitForTimeout(2500);
    const after = await rowOffer(page, "late-check");
    if (after.found && (after.badge || after.arrow)) {
      problems.push(
        `late-check claims an update again AFTER its update finished — a late in-flight check ` +
          `("available", empty target) was allowed to write the cache. That claim cannot be ` +
          `ordered, so \`verdict\` lets it through by design; settleAfterUpdate's cancel AND ` +
          `its invalidate have BOTH stopped covering it (#250). row: "${after.text}"`,
      );
    }
    // The anti-vacuity guard for the assertion above. It must NOT be "the row
    // now reads v4.1": `settleAfterUpdate` only clears the CLAIM (update:none,
    // target:"") and the version comes from the next fetch, which this scenario
    // deliberately freezes — so demanding v4.1 fails on correct behaviour, as
    // the first run of this check showed. What proves the panel is still
    // rendering claims at all is a neighbouring row that should still offer one.
    const neighbour = await rowOffer(page, "wordpress-seo");
    if (!neighbour.found || !(neighbour.badge && neighbour.arrow)) {
      problems.push(
        `wordpress-seo stopped offering 22.4 as well, so "late-check shows no badge" may just ` +
          `mean the list stopped claiming anything. row: "${neighbour.text ?? "not found"}"`,
      );
    }
    await page.screenshot({ path: `${__dirname}/shot-wpupdate-late.png`, fullPage: true });
    await page.close();
  }

  await browser.close();
  if (problems.length) {
    console.error("✗ wpupdate");
    problems.forEach((p) => console.error(`  ${p}`));
    process.exit(1);
  }
  console.log("✓ wpupdate — the bar follows wp-cli's stream, and a late check can't undo a finish");
})();
