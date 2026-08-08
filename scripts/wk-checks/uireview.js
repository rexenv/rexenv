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
  // The AI-agents (MCP) card: the status-line states + the concerning-row
  // (muted-amber) feed treatment + the empty state + off (feed still shown).
  ["agents-working", "view=agents&astate=working", []],
  ["agents-erroring", "view=agents&astate=erroring", []],
  ["agents-idle-empty", "view=agents&astate=idle&feed=empty", []],
  ["agents-off", "view=agents&astate=off", []],
  ["agents-site-section", "view=agents&astate=working&site=1", []],
  // The MAIL sub-toggle (M2b) — three paragraphs above a toggle, so the width
  // it has to survive is the narrow one. Both states, since "off by default"
  // is the shipped one and the one a first-time reader meets.
  ["agents-mail-off", "view=agents&astate=working", []],
  ["agents-mail-on", "view=agents&astate=working&mail=1", []],
  // The Agent-scratch group: client badge + TTL + last-synced, a moved source,
  // an expired site, an expired one the reaper could not remove — and the two
  // rows that must render as ORDINARY sites (a Kept one, and a user's own site
  // hand-named `*.scratch.*`), which is what the probe below checks.
  // The provision card at the New Site dialog's own width, with the longest
  // real phase label. This row shipped broken: the phase label was `flex-none`,
  // so a long backend label ("installing Laravel (composer create-project)")
  // pushed the domain clean out of the row. Probe below, plus the universal
  // overflow assertion.
  ["provision-long-label", "view=provision", []],
  ["scratch-rows", "view=scratch", []],
  ["scratch-keep-dialog", "view=keep", []],
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
  // The provision card's header row: everything must stay INSIDE the card, and
  // "creating <domain>" must still be readable. The shipped bug had the domain
  // at zero width while the phase label ran past the card's right edge, so both
  // halves are asserted — a truncated label with no domain left is not a pass.
  provisionRow: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const cards = document.querySelectorAll('[data-probe="provision-card"]');
      if (!cards.length) return ["no provision card in the DOM"];
      for (const card of cards) {
        const cr = card.getBoundingClientRect();
        const domain = card.querySelector('[data-probe="provision-domain"]');
        const label = card.querySelector('[data-probe="provision-phase"]');
        if (!domain) { problems.push("no domain span"); continue; }
        const dr = domain.getBoundingClientRect();
        if (dr.width < 60) problems.push(`domain squeezed to ${dr.width.toFixed(1)}px`);
        if (dr.height > 24) problems.push(`domain wrapped (${dr.height.toFixed(1)}px tall)`);
        for (const [what, el] of [["domain", domain], ["phase label", label]]) {
          if (!el) continue;
          const r = el.getBoundingClientRect();
          if (r.right > cr.right + 0.5) problems.push(`${what} overflows the card by ${(r.right - cr.right).toFixed(1)}px`);
          if (r.height > 24) problems.push(`${what} wrapped (${r.height.toFixed(1)}px tall)`);
        }
      }
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
  // The AI-agents card: the residual copy renders verbatim above the toggle,
  // the concerning rows get the muted-amber accent (or the empty state shows),
  // and the toggle reflects enabled/off.
  agents: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const text = document.body.textContent || "";
      if (!text.includes("AI agents (MCP)")) problems.push("card title missing");
      if (!text.includes("Before you turn this on")) problems.push("enable-moment copy missing");
      const p = new URLSearchParams(location.search);
      const amber = document.querySelectorAll('[class*="border-l-status-warning"]').length;
      if (p.get("feed") === "empty") {
        if (!text.includes("No agent activity yet")) problems.push("empty state missing");
      } else if (amber < 1) {
        problems.push("no muted-amber concerning row rendered");
      }
      const toggle = document.querySelector('[role="switch"]');
      if (!toggle) problems.push("no toggle rendered");
      else if (toggle.getAttribute("aria-checked") !== (p.get("astate") === "off" ? "false" : "true"))
        problems.push("toggle state does not match astate");
      // The feed must show resolved DOMAINS, never a raw UUID site handle (the
      // fix: target_site is a uuid, target_label is the domain shown). This now
      // also covers the case that CANNOT resolve — a reap names a site it just
      // deleted — which must read as "(deleted site)", not as a UUID.
      if (/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-/.test(text))
        problems.push("raw UUID site handle rendered — target_site not resolved to a domain");
      // A rexenv row (the scratch reaper) must SAY it is rexenv's. Unlabelled,
      // it reads as an agent action under a heading about agents — and putting
      // "rexenv" in the client-name slot alone would read as an agent that calls
      // itself rexenv. It must also be LISTED, never filtered out.
      if (p.get("feed") !== "empty") {
        if (!text.includes("scratch_reap"))
          problems.push("the rexenv (reaper) row is missing — actor rows must be listed, not hidden");
        if (!text.includes("rexenv · automatic"))
          problems.push("the rexenv row is not labelled as rexenv's own — untrue by juxtaposition");
        if (!text.includes("(deleted site)"))
          problems.push("a reap's deleted target does not read as '(deleted site)'");
      }
      return problems;
    }),

  // The Agent-scratch group's ONE rule, checked on rendered text: what is
  // agent-flavoured is decided by the RECORDED origin, never by the domain.
  // Both negative rows end in `.scratch.rex` and are `origin: "user"` — a Kept
  // site and a site the user hand-named — so a predicate that reached for the
  // suffix (the obvious shortcut) puts someone's real site in the disposable
  // section and fails here.
  scratchGroup: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const rowOf = (domain) =>
        [...document.querySelectorAll("[role=button]")].find((el) =>
          (el.textContent ?? "").includes(domain),
        );
      const agentish = (el) => {
        const t = el?.textContent ?? "";
        return /left|expired|Claude Code|Cursor|synced|source moved/.test(t);
      };
      for (const own of ["kept.scratch.rex", "mine.scratch.rex"]) {
        const row = rowOf(own);
        if (!row) {
          problems.push(`${own} did not render at all`);
          continue;
        }
        if (agentish(row))
          problems.push(
            `${own} is the USER'S site (origin=user) but renders agent state — the group is reading the domain, not the recorded origin`,
          );
      }
      // ...and non-vacuously: the real scratch rows DO carry that state, so a
      // page that simply rendered no badges anywhere would not pass.
      const scratch = rowOf("plugin-test.scratch.rex");
      if (!scratch) problems.push("the scratch row did not render");
      else if (!agentish(scratch))
        problems.push("the scratch row shows no client/TTL/sync state — the check above proves nothing");
      const body = document.body.textContent ?? "";
      if (!body.includes("Agent scratch")) problems.push("no Agent-scratch group heading");
      if (!body.includes("source moved"))
        problems.push("a moved package source does not read as its own state");
      if (!body.includes("expired — couldn't remove"))
        problems.push("an expired site the reaper failed on does not say so");
      return problems;
    }),

  // The delete confirm's type-the-domain gate, on EVERY delete variant (the
  // connected one has two destructive buttons, and both must be gated — a gate
  // that covers one button in a dialog is the same false guard this project has
  // paid for before). Runs after the screenshot, so it may type into the page:
  // empty → every destructive button disabled; a near-miss → still disabled;
  // the exact domain (trailing space, since the copy button invites a paste) →
  // all enabled. The copy button must exist, or the gate is retype-from-memory.
  deleteGate: async (page) => {
    const problems = await page.evaluate(() => {
      const out = [];
      if (!document.querySelector('[data-probe="confirm-copy"]'))
        out.push("no copy button beside the domain");
      const input = document.querySelector('[data-probe="confirm-input"]');
      if (!input) out.push("no type-to-confirm input");
      return out;
    });
    if (problems.length) return problems;
    const destructive = () =>
      page.evaluate(() =>
        [...document.querySelectorAll("button")]
          .filter((b) => /^(Delete site|Delete without reverting|Revert, then delete)$/.test((b.textContent || "").trim()))
          .map((b) => ({ label: b.textContent.trim(), disabled: b.disabled })),
      );
    const before = await destructive();
    if (!before.length) return ["no destructive button in the delete confirm"];
    for (const b of before) if (!b.disabled) problems.push(`"${b.label}" is live with an EMPTY confirm box`);
    const phrase = await page.getAttribute('[data-probe="confirm-input"]', "placeholder");
    await page.fill('[data-probe="confirm-input"]', phrase.slice(0, -1));
    for (const b of await destructive())
      if (!b.disabled) problems.push(`"${b.label}" is live on a near-miss ("${phrase.slice(0, -1)}")`);
    await page.fill('[data-probe="confirm-input"]', `${phrase} `);
    for (const b of await destructive())
      if (b.disabled) problems.push(`"${b.label}" stays dead after the exact domain was entered`);
    return problems;
  },
};

/** Every action `runActions` knows. An unknown one is a scenario bug, not a
 *  no-op — see the throw below. */
const KNOWN_ACTIONS = new Set(["consent", "apply", "revert", "confirmRevert", "scrollBottom", "lastMenu"]);

function probeFor(name) {
  if (name.startsWith("dbtab")) return PROBES.dbtab;
  if (name === "pills") return PROBES.pills;
  if (name.startsWith("agents")) return PROBES.agents;
  if (name === "scratch-rows") return PROBES.scratchGroup;
  if (name.startsWith("provision")) return PROBES.provisionRow;
  if (name.startsWith("delete")) return PROBES.deleteGate;
  if (name.startsWith("agents-mail")) return PROBES.agents;
  return null;
}

async function runActions(page, actions) {
  for (const a of actions) {
    if (!KNOWN_ACTIONS.has(a)) {
      // A typo'd or invented action used to be ignored in silence, so a
      // scenario could declare setup that never ran and still report green —
      // found by planting (a probe name was passed here, where it did nothing
      // and said nothing). Fail loudly instead.
      throw new Error(
        `unknown action "${a}" — actions are ${[...KNOWN_ACTIONS].join(", ")}; ` +
          `per-scenario ASSERTIONS go in PROBES + probeFor(), not in this list`,
      );
    }
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
