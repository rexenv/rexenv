// WebKit screenshot sweep for the UI review (docs/archive/UI-REVIEW.md §C).
// Drives `#/dev/ui-review` (DevUiReview.tsx, mocked IPC — zero backend, zero
// contact with the real app). Captures every Stage 2/3 surface — including
// the three that had never rendered anywhere — at a narrow and a wide width.
// Screenshots land in shots-uireview/ (gitignored, like all shot-*.png).
const { webkit } = require("playwright");
const fs = require("fs");
const path = require("path");

const BASE = process.env.WK_BASE_URL || "http://localhost:5199";
const OUT = path.join(__dirname, "shots-uireview");
const WIDTHS = [
  ["narrow", 900],
  ["wide", 1440],
];

/** [name, query, actions] — actions run before the shot. */
const SCENARIOS = [
  ["card-consent-root", "view=card&rec=imported&preview=ready&root=1", []],
  [
    "card-consent-cache-mariadb-backup",
    "view=card&rec=imported&preview=ready&root=1&cache=1&engine=mariadb&backup=1",
    [],
  ],
  ["card-refused-mariadb", "view=card&rec=imported&preview=refused&engine=mariadb", []],
  ["card-noop-verify", "view=card&rec=imported&preview=noop", []],
  [
    "card-apply-fileChanged",
    "view=card&rec=imported&preview=ready&apply=fileChanged",
    ["consent", "apply"],
  ],
  [
    "card-apply-engineStopped",
    "view=card&rec=imported&preview=ready&apply=engineStopped",
    ["consent", "apply"],
  ],
  [
    "card-apply-verifyFailed",
    "view=card&rec=imported&preview=ready&apply=verifyFailed",
    ["consent", "apply"],
  ],
  ["card-connected-http-cache", "view=card&rec=connectedHttp&preview=noop&cache=1", []],
  // The file edited AFTER verification: connected (a proven past fact) with
  // a live diff — the panel must say the file no longer points at the copy.
  ["card-connected-drift", "view=card&rec=connected&preview=ready", []],
  ["card-revert-confirm", "view=card&rec=connected&preview=noop", ["revert"]],
  [
    "card-revert-refusedEdited",
    "view=card&rec=connected&preview=noop&revert=refusedEdited",
    ["revert", "confirmRevert"],
  ],
  [
    "card-revert-backupMissing",
    "view=card&rec=connected&preview=noop&revert=backupMissing",
    ["revert", "confirmRevert"],
  ],
  ["delete-connected", "view=delete&kind=connected", []],
  ["delete-connected-long", "view=delete&kind=connected&long=1", []],
  ["delete-preexisting-db", "view=delete&kind=preexisting", []],
  ["delete-wp-plain", "view=delete&kind=wp", []],
  ["delete-imported-laravel", "view=delete&kind=imported", []],
  ["delete-linked-nodb", "view=delete&kind=linked", []],
  ["badges", "view=badges", []],
  // §C2: the Database tab's three shapes (real DatabaseTab inside a replica
  // of SiteDetail's region chain) + the row menu on the LAST row at scale.
  ["dbtab-plain", "view=dbtab&shape=plain", []],
  ["dbtab-imported-nodb", "view=dbtab&shape=imported", []],
  ["dbtab-imported-consent", "view=dbtab&shape=imported&rec=imported&preview=ready&root=1&cache=1", ["scrollBottom"]],
  ["dbtab-imported-connected", "view=dbtab&shape=imported&rec=connectedHttp&preview=noop&cache=1", []],
  ["sites-scale-menu", "view=sites&rows=28", ["lastMenu"]],
  ["resolver-handback", "view=resolver", []],
  ["toasts", "view=toast", []],
  // Every StatusPill state + every StartStopToggle state — the states the
  // other fixtures never render (they hardcode `running`).
  ["pills", "view=pills", []],
];

/** Per-scenario layout assertions (beyond the universal overflow probe).
 *  Return a list of problem strings; empty = pass. */
const PROBES = {
  // The §C2 h-full class: the Adminer iframe participates in the region's
  // height chain. A severed percentage chain collapses it to its ~150px
  // intrinsic default (the shipped bug), or to 0. DatabaseTab's min-h-[420px]
  // floor sits on the WRAPPER; the iframe legitimately gets the floor minus
  // AdminerFrame's header row (~52px → ~368px measured healthy), so 300 is
  // the discriminating line: healthy ≥ 360, collapsed ≤ 150.
  dbtab: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const frame = document.querySelector('iframe[title="Adminer"]');
      if (!frame) return ["no Adminer iframe in the DOM"];
      const h = frame.getBoundingClientRect().height;
      if (h < 300) problems.push(`iframe height ${Math.round(h)}px — percentage chain collapsed`);
      return problems;
    }),
  // The WKWebView metrics fix, committed as a check: every pill at least the
  // 92px floor, one line tall, label inside the pill (the bug rendered
  // "Running" as two overlapping words in an exact-fit 86px pill).
  pills: async (page) =>
    page.evaluate(() => {
      const problems = [];
      for (const pill of document.querySelectorAll('[data-probe="pills"] > span')) {
        const r = pill.getBoundingClientRect();
        const label = pill.querySelector("span.whitespace-nowrap");
        const lr = label ? label.getBoundingClientRect() : null;
        const text = label ? label.textContent : "?";
        if (r.width < 92) problems.push(`pill "${text}" width ${r.width.toFixed(1)}px < 92`);
        if (r.height > 34) problems.push(`pill "${text}" height ${r.height.toFixed(1)}px — wrapped?`);
        if (lr && lr.right > r.right + 0.5)
          problems.push(`pill "${text}" label overflows its pill`);
      }
      const toggles = document.querySelectorAll('[data-probe="toggles"] [role="switch"]');
      if (toggles.length !== 4) problems.push(`${toggles.length}/4 toggles rendered`);
      for (const t of toggles) {
        if (t.getAttribute("aria-label")?.includes("locked") && !t.disabled)
          problems.push("the locked toggle is not disabled");
      }
      return problems;
    }),
};

function probeFor(name) {
  if (name.startsWith("dbtab")) return PROBES.dbtab;
  if (name === "pills") return PROBES.pills;
  return null;
}

async function runActions(page, actions) {
  for (const a of actions) {
    if (a === "consent") {
      await page.locator('input[type="checkbox"]').check();
    } else if (a === "apply") {
      await page.getByRole("button", { name: /Apply and verify|Verify connection/ }).click();
      await page.waitForTimeout(400);
    } else if (a === "revert") {
      await page.getByRole("button", { name: "Revert", exact: true }).click();
      await page.waitForTimeout(200);
    } else if (a === "confirmRevert") {
      // The ConfirmDialog's confirm button (also labeled "Revert") — last one.
      await page.getByRole("button", { name: "Revert", exact: true }).last().click();
      await page.waitForTimeout(400);
    } else if (a === "scrollBottom") {
      // Scroll the inner region (an overflow-auto container) to its end —
      // proves the frame is REACHABLE below tall cards (the before-state was
      // overflow-hidden: same layout, no way to get there).
      await page.evaluate(() => {
        document
          .querySelectorAll(".overflow-auto")
          .forEach((el) => (el.scrollTop = el.scrollHeight));
      });
      await page.waitForTimeout(150);
    } else if (a === "lastMenu") {
      // Scroll to the bottom, open the LAST row's actions menu — the clipped
      // case. The shot must show the menu fully inside the viewport.
      const last = page.getByRole("button", { name: "More actions" }).last();
      await last.scrollIntoViewIfNeeded();
      await last.click();
      await page.waitForTimeout(250);
    }
  }
}

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  // ONLY=regex narrows the sweep to matching scenario names (per-fix re-runs).
  const only = process.env.ONLY ? new RegExp(process.env.ONLY) : null;
  const picked = only ? SCENARIOS.filter(([n]) => only.test(n)) : SCENARIOS;
  const browser = await webkit.launch();
  let failures = 0;
  for (const [wName, width] of WIDTHS) {
    // His mode — and the packaged default. The theme resolves "system" via
    // prefers-color-scheme, which Playwright defaults to LIGHT.
    const page = await browser.newPage({
      viewport: { width, height: 940 },
      colorScheme: "dark",
    });
    // A page that throws, or logs an error, is a failed scenario — this sweep
    // used to be unable to fail on anything but a selector timeout.
    let pageProblems = [];
    page.on("pageerror", (e) => pageProblems.push(`pageerror: ${String(e).split("\n")[0]}`));
    page.on("console", (m) => {
      if (m.type() === "error") pageProblems.push(`console.error: ${m.text().split("\n")[0]}`);
    });
    for (const [name, query, actions] of picked) {
      pageProblems = [];
      try {
        await page.goto(`${BASE}/dev/ui-review?${query}`);
        await page.waitForSelector("h1");
        await page.waitForTimeout(300);
        await runActions(page, actions);
        // The menu scenario shoots the VIEWPORT: fullPage stitching scrolls,
        // which both closes the menu and misplaces fixed-position elements.
        await page.screenshot({
          path: path.join(OUT, `shot-${name}-${wName}.png`),
          fullPage: name !== "sites-scale-menu",
        });
        const problems = [...pageProblems];
        // Horizontal overflow is a FAILURE, not a warning nobody reads.
        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        if (overflow > 1) problems.push(`horizontal overflow ${overflow}px`);
        const probe = probeFor(name);
        if (probe) problems.push(...(await probe(page)));
        if (problems.length) {
          failures++;
          console.log(`✗ ${name} @${wName} — ${problems.join("; ")}`);
        } else {
          console.log(`✓ ${name} @${wName}`);
        }
      } catch (e) {
        failures++;
        console.log(`✗ ${name} @${wName} — ${String(e).split("\n")[0]}`);
      }
    }
    await page.close();
  }
  await browser.close();
  console.log(failures ? `\n${failures} scenario(s) failed` : "\nall scenarios captured");
  process.exit(failures ? 1 : 0);
})();
