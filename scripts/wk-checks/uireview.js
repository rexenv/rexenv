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
  // #810: a job that settles before the card subscribes must still show its end.
  ["card-instantfail", "view=card&dbjob=instantfail", []],
  // #812: "nothing to import" is an answer — neutral, no Retry.
  ["card-thissite", "view=card&rec=connected&preview=noop&dbjob=thissite", []],
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
  // The Database tab's two stopped states (#752): the panel names the service and
  // offers Start all, and no Adminer frame is mounted. Their control is the four
  // rows above — the same mock answering "running" must still mount the frame.
  ["dbtab-stopped-engine", "view=dbtab&shape=plain&stopped=engine", []],
  ["dbtab-stopped-web", "view=dbtab&shape=plain&stopped=web", []],
  ["sites-scale-menu", "view=sites&rows=28", ["lastMenu"]],
  ["resolver-handback", "view=resolver", []],
  // #808: a DRIFTED borrow (Valet took the file back) must not read "rexenv answers this".
  ["resolver-handback-drifted", "view=resolver&owner=drifted", []],
  ["toasts", "view=toast", []],
  // Every StatusPill state + every StartStopToggle state — the states the
  // other fixtures never render (they hardcode `running`).
  ["pills", "view=pills", []],
  // The AI-agents (MCP) card: the status-line states + the concerning-row
  // (muted-amber) feed treatment + the empty state + off (nothing below the
  // toggle since D16 — except the dial, when a standing level must be lowerable).
  ["agents-working", "view=agents&astate=working", []],
  ["agents-erroring", "view=agents&astate=erroring", []],
  ["agents-idle-empty", "view=agents&astate=idle&feed=empty", []],
  ["agents-off", "view=agents&astate=off", []],
  ["agents-site-section", "view=agents&astate=working&site=1", []],
  ["onboarding-clear", "view=onboarding", []],
  ["onboarding-herd", "view=onboarding&edge=herd", []],
  ["onboarding-anon", "view=onboarding&edge=anon", []],
  ["wppackages-named", "view=wppackages", []],
  ["wppackages-unnamed", "view=wppackages&names=none", []],
  // The Agent access dial (D15): Read by default; `access=full&mode=days` a
  // 7-day Full; `expired=1` the expired notice.
  ["agents-access-read", "view=agents&astate=working", []],
  ["agents-access-full", "view=agents&astate=working&access=full&mode=days", []],
  ["agents-access-expired", "view=agents&astate=working&expired=1", []],
  // Off with a standing Full · Always: the note under the toggle and the dial
  // (to lower it) render; the feed does not.
  ["agents-off-standing", "view=agents&astate=off&access=full&mode=always", []],
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
  // Resolver-drift banner: the probe drives the WHOLE lifecycle (present →
  // dismissed → ours-again heals → re-loss re-shows) by re-navigating itself.
  ["resolver-drift", "view=drift&drift=test", []],
  ["scratch-keep-dialog", "view=keep", []],
  // Settings → PHP versions, at the Settings column width, with a row carrying
  // EVERY chip at once. Shipped broken: five chips inline in a 9rem column made a
  // badge wrap INTERNALLY ("EOL" / "November 2022", each with half the pill's
  // border). Nothing caught it because the row had no scenario — reviewers only
  // ever saw a fresh install, where `serving` and `exists` are absent.
  ["php-versions", "view=phpversions", []],
  // The Adminer version card, in all four states. It carries the one control in
  // the app that installs bytes this build was not shipped with, on the screen
  // whose only other job is opening a database console — so what it SAYS is the
  // whole check, not just whether it fits.
  ["adminer-offer", "view=adminer", []],
  ["adminer-current", "view=adminer&adminer=current", []],
  ["adminer-pending", "view=adminer&adminer=pending", []],
  ["adminer-fresh", "view=adminer&adminer=fresh", []],
  ["adminer-updating", "view=adminer&adminer=updating", []],
  // The app-update card, in every state it can be in. It carries the one
  // control in this app that replaces the app itself, and what it SAYS is most
  // of the check: the three states that must be right — never checked, checked
  // and nothing, and refused — are exactly the ones nobody looks at, so a
  // fixture in one state would prove the least useful one.
  ["appupdate-never-checked", "view=appupdate&state=never-checked", []],
  ["appupdate-none", "view=appupdate&state=none", []],
  ["appupdate-offered", "view=appupdate&state=offered", []],
  ["appupdate-skipped", "view=appupdate&state=skipped", []],
  ["appupdate-refused", "view=appupdate&state=refused", []],
  ["appupdate-installing", "view=appupdate&state=installing", []],
  ["appupdate-auto-off", "view=appupdate&state=auto-off", []],
  ["appupdate-installed-pending", "view=appupdate&state=installed-pending", []],
  ["appupdate-installed-undecided", "view=appupdate&state=installed-undecided", []],
  // The themes grid labels each card with the theme's own name, the way
  // wp-admin does — and keeps the slug, because that is the folder name and
  // what `theme activate` takes. The third fixture row has no title at all.
  ["themes-titles", "view=themes", []],
  // The Tunnels filter. Every other search box in this app hides rows that are
  // still exactly where they were; this one can hide a site the whole internet
  // can reach right now, so three of these four scenarios are about what the
  // page SAYS while a row is hidden, not about whether the filter works.
  ["tunnels-plain", "view=tunnels", []],
  ["tunnels-filtered-hides-shared", "view=tunnels", ["searchDocs"]],
  ["tunnels-by-url", "view=tunnels", ["searchUrl"]],
  ["tunnels-no-match", "view=tunnels", ["searchNoMatch"]],
];

/** Per-scenario layout assertions (beyond the universal overflow probe).
 *  Return a list of problem strings; empty = pass. */
const PROBES = {
  // What the update card SAYS, per state. The layout matters less here than the
  // copy: this is the surface where a reassuring sentence would be a lie, and
  // the honest ones ("checked 2h ago", "couldn't reach") are what a user needs
  // when something is wrong. One probe drives every state by re-navigating,
  // because the states only mean anything against each other.
  "appupdate-never-checked": async (page) => {
    const problems = [];
    const banned = ["up to date", "up-to-date", "update available", "updates available"];
    const states = {
      // `mustNot: " ago"` and not "checked": the honest never-checked sentence
      // is "Not checked YET", which contains the word. What must be absent is a
      // TIMESTAMP — the card claiming a check that never happened.
      "never-checked": { button: false, must: ["Not checked yet"], mustNot: [" ago"] },
      none: { button: false, must: ["is what you are running", "checked "], mustNot: [] },
      offered: {
        button: true,
        // The consent sentence is asserted by its FIXTURE text: the claim is
        // that the card renders what the backend sent, not that it knows the
        // real wording — which lives in Rust and is guarded there.
        must: ["0.6.0", "29.9 MB", "FIXTURE CONSENT", "Skip this version"],
        mustNot: [],
      },
      skipped: { button: false, must: ["was set aside", "Undo"], mustNot: ["Install rexenv"] },
      refused: {
        button: false,
        must: ["not writable", "sudo chown"],
        mustNot: ["Install rexenv"],
      },
      installing: { button: false, must: ["Downloading", "11.8 MB", "29.9 MB"], mustNot: [] },
      // The one state where the switch's own sentence changes. Asserting the
      // OFF copy is what makes the checkbox a control rather than decoration.
      // Installed, quit cancelled. The assertion that matters is the ABSENCE of
      // a button: the bytes are already in /Applications, and a second Install
      // would re-download them.
      "installed-pending": {
        button: false,
        must: ["is installed", "next time rexenv opens", "keep rexenv running"],
        mustNot: ["Skip this version"],
      },
      // Swapped, nothing chosen yet (the restart dialog up, or the gate's confirm
      // unanswered): the header states the fact and the card adds NO sentence
      // about a choice — it said "you chose to keep rexenv running" here (#771).
      "installed-undecided": {
        button: false,
        must: ["is installed", "next time rexenv opens"],
        mustNot: ["keep rexenv running", "Skip this version"],
      },
      "auto-off": {
        button: false,
        must: ["Check for new releases automatically", "contacts nothing on its own"],
        mustNot: ["asks GitHub"],
      },
    };
    for (const [state, want] of Object.entries(states)) {
      await page.goto(`${BASE}/dev/ui-review?view=appupdate&state=${state}`);
      await page.waitForSelector('[data-probe="appupdate-view"]', { timeout: 5000 });
      const card = await page.$('[data-probe="app-update"]');
      if (!card) {
        problems.push(`${state}: the card did not render`);
        continue;
      }
      const text = (await card.innerText()) || "";
      const lower = text.toLowerCase();
      for (const b of banned) {
        if (lower.includes(b)) problems.push(`${state}: the card says "${b}" — it cannot know that`);
      }
      for (const m of want.must) {
        if (!text.includes(m)) problems.push(`${state}: the card never says "${m}"`);
      }
      for (const m of want.mustNot) {
        if (text.includes(m)) problems.push(`${state}: the card says "${m}" and should not`);
      }
      const install = await card.$("text=Install rexenv");
      if (want.button && !install) problems.push(`${state}: no Install button where one belongs`);
      if (!want.button && install) {
        problems.push(`${state}: an Install button that could not work is offered`);
      }
      // The phase attribute is what the card believes it is showing; a state
      // whose copy and phase disagree is the bug this catches.
      const phase = await card.getAttribute("data-phase");
      const expected =
        state === "never-checked" || state === "auto-off"
          ? "none"
          : state === "installed-pending"
            ? "installed"
            : state === "installed-undecided"
              ? "pending"
              : state;
      if (phase !== expected) problems.push(`${state}: data-phase is "${phase}"`);

      // The auto-check switch must AGREE with the value the card is rendering
      // from. A checkbox drawn from a constant looks identical in a screenshot
      // and does nothing, which is the failure a picture cannot show.
      const auto = await card.getAttribute("data-auto-check");
      const box = await card.$("#app-update-auto");
      if (!box) {
        problems.push(`${state}: no automatic-check switch on the card`);
      } else {
        const checked = await box.isChecked();
        if (String(checked) !== auto) {
          problems.push(`${state}: the switch shows ${checked} while the card says ${auto}`);
        }
      }
    }
    return problems;
  },

  // Every chip in a PHP version row must wrap as a UNIT, never internally. A
  // one-line chip is ~18px tall; a badge whose own text broke across two lines
  // roughly doubles that, which is the tell. Same shape as the `pills` height
  // check — a wrapped pill is the defect that looks like a rendering bug.
  "php-versions": async (page) =>
    page.evaluate(() => {
      const problems = [];
      const root = document.querySelector('[data-probe="phpversions"]');
      if (!root) return ["the php-versions view rendered nothing"];
      // DIRECT CHILDREN of the name column, selected STRUCTURALLY. Selecting by
      // `span.whitespace-nowrap` — the class the fix adds — made this guard blind
      // to exactly the regression it exists to catch: removing the class removed
      // the chip from the query, and the plant passed. Same defect as declaring
      // contrast exemptions by token name (ledger #337), committed inside a probe
      // written to catch a layout bug.
      const chips = root.querySelectorAll('[data-probe="php-row-chips"] > span');
      if (chips.length < 8) problems.push(`only ${chips.length} chips rendered — fixture too thin`);
      // Three distinct failures, because the planted regression produced the
      // one a height check cannot see. Measured, not assumed: the pre-fix layout
      // crushed chips ON TOP OF each other and NONE of them grew taller.
      const cols = [...document.querySelectorAll('[data-probe="php-row-chips"]')];
      for (const col of cols) {
        const cr = col.getBoundingClientRect();
        const kids = [...col.children].map((s) => ({
          t: (s.textContent || "?").trim().slice(0, 24),
          r: s.getBoundingClientRect(),
        }));
        for (const k of kids) {
          // (1) A badge whose own text broke across lines — half a pill per line.
          if (k.r.height > 26) problems.push(`chip "${k.t}" is ${k.r.height.toFixed(1)}px tall — its text wrapped`);
          // (2) Escaping the column it lives in.
          if (k.r.right > cr.right + 0.5) problems.push(`chip "${k.t}" overflows its column by ${(k.r.right - cr.right).toFixed(1)}px`);
        }
        // (3b) The column against the CONTROLS beside it. This is where the
        // planted regression actually collided — "8.4.24 exists" landing on top
        // of "Make default" — and a check confined to siblings inside the column
        // could not see it. Measured from the screenshot, not reasoned about.
        for (const sib of [...col.parentElement.children].filter((n) => n !== col)) {
          const sr = sib.getBoundingClientRect();
          for (const k of kids) {
            const sameLine = k.r.top < sr.bottom - 2 && sr.top < k.r.bottom - 2;
            const ox = Math.min(k.r.right, sr.right) - Math.max(k.r.left, sr.left);
            if (sameLine && ox > 1) {
              problems.push(
                `chip "${k.t}" overlaps the control "${(sib.textContent || "?").trim().slice(0, 18)}" by ${ox.toFixed(1)}px`
              );
            }
          }
        }
        // (3) OVERLAP — two chips occupying the same pixels on the same line.
        for (let i = 0; i < kids.length; i++) {
          for (let j = i + 1; j < kids.length; j++) {
            const a = kids[i].r, b = kids[j].r;
            const sameLine = a.top < b.bottom - 2 && b.top < a.bottom - 2;
            const overlapX = Math.min(a.right, b.right) - Math.max(a.left, b.left);
            if (sameLine && overlapX > 1) {
              problems.push(`chips "${kids[i].t}" and "${kids[j].t}" overlap by ${overlapX.toFixed(1)}px`);
            }
          }
        }
      }
      // The fixture must actually exercise the worst case, or this goes vacuous.
      const all = root.innerText;
      for (const want of ["EOL", "exists", "serving", "Default"]) {
        if (!all.includes(want)) problems.push(`fixture is missing a "${want}" chip`);
      }
      // The explanation must precede the chips it explains — the ordering fix for
      // "8.3.33 exists with no button reads as half-built".
      //
      // Compared by DOM POSITION, not by string index. The first version searched
      // innerText for "exists" and found it inside the note's OWN first sentence
      // (“exists” is not an update you can press), so a correctly-ordered page
      // failed. Fourth time in one session that a check matched its own
      // explanation; a structural comparison cannot.
      const note = root.querySelector('[data-probe="php-upstream-note"]');
      const firstChipRow = root.querySelector('[data-probe="php-row-chips"]');
      if (!note) problems.push("the why-no-button note is gone");
      else if (
        firstChipRow &&
        !(note.compareDocumentPosition(firstChipRow) & Node.DOCUMENT_POSITION_FOLLOWING)
      ) {
        problems.push("the note renders AFTER the chips it explains");
      }

      // ── The four update STATES ────────────────────────────────────────────
      //
      // Every row defect in this feature was found by a person looking at a
      // screenshot: chips overlapping, the EOL badge wrapping mid-pill,
      // "8.2.32 exists" printed beside "Update to 8.2.32", and the button still
      // showing after the update had been applied. The layout half is checked
      // above; this is the half that asks whether the row is telling the TRUTH.
      const rows = [...root.querySelectorAll('[data-probe="php-row"]')];
      if (rows.length < 5) problems.push(`only ${rows.length} version rows — fixture too thin`);
      const seen = new Set();
      for (const row of rows) {
        const minor = row.dataset.minor;
        const updatable = row.dataset.updatable;
        const upstream = row.dataset.upstream;
        const text = (row.innerText || "").replace(/\s+/g, " ");
        // STRUCTURAL, not by label. This matched `/^Update to /`, which the
        // button stopped saying long ago — and the mismatch survived because no
        // fixture row set `updatable`, so this branch had never executed. The
        // probe's own "that branch is unchecked" note was printing all along;
        // adding the fixture is what made it fail, which is the point of adding
        // it (10 Sep 2026).
        const button = row.querySelector('[data-probe="php-update"]');
        const chip = [...row.querySelectorAll('[data-probe="php-row-chips"] > span')].find((c) =>
          /exists$/.test((c.textContent || "").trim())
        );

        // A button appears IF AND ONLY IF a verified manifest offers something
        // for a minor the user HAS. `updatable` is set on uninstalled rows too —
        // it means "the catalog carries this", not "you can press something" —
        // and the first draft of this rule called that correct state a defect.
        // Found by planting it, which is the only reason the rule is scoped.
        const installed = row.dataset.installed === "1";
        if (updatable && installed && !button)
          problems.push(`${minor}: offers ${updatable} but has no Update button`);
        if (!updatable && button)
          problems.push(`${minor}: an Update button with nothing offered — "${text.slice(0, 60)}"`);
        if (button && !installed)
          problems.push(`${minor}: an Update button on a version that is not installed`);
        if (button && !button.textContent.includes(updatable))
          problems.push(`${minor}: the button says "${button.textContent.trim()}" but the row offers ${updatable}`);

        // The chip means "there is no button for this version". Beside a button
        // naming the SAME version it reads as two different versions.
        if (upstream && upstream === updatable && chip)
          problems.push(`${minor}: "${chip.textContent.trim()}" rendered next to a button offering the same version`);
        if (upstream && upstream !== updatable && !chip)
          problems.push(`${minor}: php.net lists ${upstream} and the row says nothing about it`);
        if (!upstream && chip) problems.push(`${minor}: an "exists" chip with no upstream version`);

        // A row with nothing to offer must be QUIET. The "serving" chip means
        // the live pool disagrees with what this minor should run; painting a
        // correct pool amber is how a successful update read as a failure.
        if (!updatable && !upstream && / serving /.test(` ${text} `))
          problems.push(`${minor}: nothing pending, yet the row reports "serving" — ${text.slice(0, 70)}`);
        // An update that REMOVES something must SAY so where it is offered, and
        // it must say it visibly: the sentence is in a tooltip, but a tooltip
        // nobody hovers is not a tell. The chip is what makes the offer look
        // different from an ordinary one.
        const cost = row.querySelector('[data-update-cost]');
        if (cost) {
          seen.add("update-with-a-cost");
          if (!button)
            problems.push(`${minor}: a cost chip with no update to pay for it`);
          if (!(cost.getAttribute("title") || "").includes("PostgreSQL"))
            problems.push(`${minor}: the cost chip carries no sentence — the chip alone says what, not why`);
        }
        if (updatable && installed) seen.add(upstream === updatable ? "button-only" : "button-and-chip");
        else if (upstream) seen.add("chip-only");
        else if (installed) seen.add("settled");
        else seen.add("not-installed");
      }
      // The fixture must actually carry every state, or each branch above is a
      // rule nothing exercises. This is the assert that made the mock honest:
      // before it, every row was installed and none was post-update.
      for (const want of [
        "button-only",
        "button-and-chip",
        "chip-only",
        "settled",
        "not-installed",
        "update-with-a-cost",
      ]) {
        if (!seen.has(want)) problems.push(`the fixture has no "${want}" row — that branch is unchecked`);
      }
      // NOT checked here, and said out loud rather than implied: whether the
      // patch shown is the POST-UPDATE one. Nothing in the DOM carries the
      // compiled-in pin — by design, since the row's job is to show what the
      // minor WILL RUN — so "8.1.35 is above the pin" is not a question this
      // layer can ask. That is `after_an_update_the_row_shows_the_new_patch_
      // and_offers_nothing` (L0). What this layer adds is the half L0 cannot
      // see: that such a row renders QUIET, and that the chips fit.
      //
      // The note only where it is true: it explains a chip, so a screen whose
      // only upstream version HAS a button must not print "exists is not a button".
      const anyChipOnly = rows.some(
        (r) => r.dataset.upstream && r.dataset.upstream !== r.dataset.updatable
      );
      if (!anyChipOnly && note) problems.push("the why-no-button note renders on a screen where every upstream version has a button");
      return problems;
    }),
  // The Adminer card tells the truth about three different facts: what is
  // SERVING, what will run, and what is offered. They are separate on purpose —
  // conflating "chosen" with "serving" is how a pending restage renders as done.
  // The RUNNING state is the one that shipped wrong, and it is only reachable by
  // pressing the button — every other scenario renders a screen at rest. So this
  // probe clicks, and measures the control before and after: a label that
  // collapses to "…" is invisible to any assertion about text, and its width
  // collapse is the part the eye actually notices.
  adminerVersion: async (page, name) => {
    let before = null;
    if (name === "adminer-updating") {
      before = await page.evaluate(() => {
        const b = document.querySelector('[data-probe="adminer-version"] button');
        return b ? { w: b.getBoundingClientRect().width, text: b.textContent.trim() } : null;
      });
      await page.click('[data-probe="adminer-version"] button');
      // The mutation never settles in this fixture; wait for the control to
      // actually enter its running state rather than racing the re-render.
      await page
        .waitForFunction(
          () => {
            const b = document.querySelector('[data-probe="adminer-version"] button');
            return !!b && b.disabled;
          },
          { timeout: 4000 }
        )
        .catch(() => {});
    }
    return page.evaluate((before) => {
      const problems = [];
      const card = document.querySelector('[data-probe="adminer-version"]');
      if (!card) return ["the Adminer version card rendered nothing"];
      const text = (card.innerText || "").replace(/\s+/g, " ");
      const staged = card.dataset.staged;
      const effective = card.dataset.effective;
      const updatable = card.dataset.updatable;
      // Matches the resting AND the running label: the control does not stop
      // being the update button while it is doing the update.
      const button = [...card.querySelectorAll("button")].find((b) =>
        /^Updat(e|ing) to /.test((b.textContent || "").trim())
      );

      // A button appears IF AND ONLY IF a verified manifest offers something.
      if (updatable && !button) problems.push(`offers ${updatable} but has no Update button`);
      if (!updatable && button) problems.push(`an Update button with nothing offered — "${text}"`);
      if (button && !button.textContent.includes(updatable))
        problems.push(`the button says "${button.textContent.trim()}" but the row offers ${updatable}`);

      // What is SERVING, said honestly. Nothing staged is its own sentence.
      if (!staged && !/not installed yet/.test(text))
        problems.push(`nothing is staged, yet the row shows a version: "${text}"`);
      if (staged && !text.includes(staged))
        problems.push(`serving ${staged}, and the row does not say so: "${text}"`);

      // The amber line ONLY when the two genuinely disagree. A console already
      // on the chosen version is not a discrepancy, and painting it as one is
      // how a completed update reads as pending.
      const pendingShown = /on next start/.test(text);
      const reallyPending = !!staged && staged !== effective;
      if (reallyPending && !pendingShown)
        problems.push(`serving ${staged} while set to ${effective}, and the row is silent about it`);
      if (!reallyPending && pendingShown)
        problems.push(`nothing pending, yet the row says "on next start": "${text}"`);

      // Adminer has ONE fact, not two. An "exists" chip here would be a
      // falsehood: rexenv downloads Adminer's own release asset.
      if (/exists/.test(text)) problems.push(`an "exists" chip on a row with one fact: "${text}"`);

      // Mid-update, the control still has to SAY something and still has to be
      // the same size. The bug this covers: the label became a bare "…", so the
      // button lost both its sentence and its width and read as an empty box at
      // the one moment it had news.
      if (before) {
        if (!button) {
          problems.push(`the update button vanished mid-update — the row now reads "${text}"`);
        } else {
          const label = button.textContent.trim();
          const now = button.getBoundingClientRect().width;
          if (!button.disabled) problems.push("the update button is still clickable while updating");
          if (!label.includes(updatable))
            problems.push(`updating ${updatable}, and the button does not name it: "${label}"`);
          if (!button.querySelector("svg")) problems.push("the running button has no spinner");
          if (now < before.w * 0.8)
            problems.push(
              `the button collapsed from ${Math.round(before.w)}px ("${before.text}") to ${Math.round(now)}px ("${label}")`
            );
        }
      }
      return problems;
    }, before);
  },
  // Onboarding's :443 notice. The clear case is the load-bearing one: reporting
  // "nothing is answering" as a problem at onboarding — where the stack has not
  // started — is the same fault the import path shipped, in a new place.
  onboardingEdge: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const body = document.body.innerText.replace(/\s+/g, " ");
      // The harness navigates to `/dev/ui-review?<query>` — the params are in
      // the SEARCH, not the hash. Reading the wrong one made this report the
      // rendering cases as failures while the render was correct.
      const expectNotice = new URLSearchParams(location.search).has("edge");
      const has = /answering HTTPS on this Mac/.test(body);
      if (expectNotice && !has) problems.push("the notice did not render for a foreign holder");
      if (!expectNotice && has) {
        problems.push(
          `nothing is on :443 and onboarding reported it anyway: "${body.slice(0, 260)}"`,
        );
      }
      if (has && !/you can finish setting up/.test(body)) {
        problems.push("the notice dropped the clause that says continuing is fine");
      }
      // It must never look like a wall: the step's own Finish control stays.
      if (expectNotice && !/kingdom is ready/i.test(body)) {
        problems.push("the notice replaced the step instead of sitting under it");
      }
      return problems;
    }),
  // The #301 tell, checked where it RENDERS. The L0 guard reads the source and
  // can prove the branch exists; only this can prove what the branch produces —
  // and the failure that matters is silent: an unnameable packages dir rendering
  // "the 0 packages", which invites the reader to conclude something false about
  // their own machine.
  wpPackages: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const card = document.querySelector('[data-probe="wp-cli-packages"]');
      if (!card) return ["no WP-CLI packages card in the DOM"];
      const text = card.innerText.replace(/\s+/g, " ");
      if (!/still work in rexenv's terminal/i.test(text)) {
        problems.push("the relief valve sentence is not rendering — see the must-say list");
      }
      if (/\b0 packages?\b/.test(text)) {
        problems.push(`a count it cannot support reached the card: "${text}"`);
      }
      // An unnamed variant must not render the em-dash list frame with nothing
      // between the dashes.
      if (/—\s*—/.test(text)) problems.push(`an empty package list rendered: "${text}"`);
      return problems;
    }),
  // The §C2 h-full class: the Adminer iframe participates in the region's
  // height chain. A severed percentage chain collapses it to its ~150px
  // intrinsic default (the shipped bug), or to 0. DatabaseTab's min-h-[420px]
  // floor sits on the WRAPPER; the iframe legitimately gets the floor minus
  // AdminerFrame's header row (~52px → ~368px measured healthy), so 300 is
  // the discriminating line: healthy ≥ 360, collapsed ≤ 150.
  // A stopped dependency (#752): the panel, its Start all, and NO frame — and never the
  // "Checking…" interim, which is what an unanswered poll renders forever.
  dbtabStopped: async (page) =>
    page.evaluate(() => {
      const problems = [];
      if (document.querySelector('iframe[title="Adminer"]')) problems.push("an Adminer frame is mounted over a stopped dependency");
      const body = document.body.textContent ?? "";
      const which = new URLSearchParams(location.search).get("stopped");
      const headline = which === "engine" ? "MySQL is stopped" : "rexenv's web services are stopped";
      if (!body.includes(headline)) problems.push(`no "${headline}" panel`);
      if (body.includes("Checking rexenv")) problems.push("still \"Checking rexenv's services…\" — the poll never answered");
      if (![...document.querySelectorAll("button")].some((b) => /start all/i.test(b.textContent ?? "")))
        problems.push("no Start all button in the panel");
      return problems;
    }),
  dbtab: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const frame = document.querySelector('iframe[title="Adminer"]');
      if (!frame) {
        const body = document.body.textContent ?? "";
        return [body.includes("Checking rexenv") ? "no Adminer iframe — the tab is still \"Checking rexenv's services…\" (a poll the harness never answered)" : "no Adminer iframe in the DOM"];
      }
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
  // The dial: three radios, the chosen one checked, the duration row only
  // above Read, the expired notice only when expired.
  agentsAccess: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const text = document.body.textContent || "";
      const p = new URLSearchParams(location.search);
      const radios = [...document.querySelectorAll('[role="radiogroup"][aria-label="Agent access"] [role="radio"]')];
      if (radios.length !== 3) problems.push(`expected 3 level radios, got ${radios.length}`);
      const checked = radios.filter((r) => r.getAttribute("aria-checked") === "true").map((r) => r.textContent || "");
      const want = p.get("access") === "full" ? "Full" : p.get("access") === "changes" ? "Changes" : "Read";
      if (!(checked.length === 1 && checked[0].startsWith(want))) problems.push(`checked level ${JSON.stringify(checked)} ≠ ${want}`);
      const durationShown = text.includes("This session") && text.includes("7 days") && text.includes("Always");
      if (want === "Read" && durationShown) problems.push("a duration row at Read");
      if (want !== "Read" && !durationShown) problems.push("no duration row above Read");
      if (p.get("expired") === "1" && !text.includes("expired")) problems.push("expired notice missing");
      if (p.get("expired") !== "1" && text.includes("setting expired")) problems.push("expired notice shown while not expired");
      if (!text.includes("publish a site to the internet")) problems.push("the dial no longer says Full includes publishing (D17)");
      if (text.includes("Site access")) problems.push("the retired Site access section is back (D17)");
      if (text.includes("without asking")) problems.push("an auto-allow switch survived D15");
      return problems;
    }),
  agents: async (page) => {
    // "Recent activity" is FOLDED by default since fdfc88c (8 Sep) — the rows and
    // the empty state are not in the page until it is opened. Open it first, so
    // the checks below read the feed rather than failing on a fold.
    const fold = page.locator('button[aria-expanded="false"]', { hasText: "Recent activity" });
    if (await fold.count()) {
      await fold.first().click();
      await page.locator('button[aria-expanded="true"]', { hasText: "Recent activity" }).waitFor();
    }
    return page.evaluate(() => {
      const problems = [];
      const text = document.body.textContent || "";
      if (!text.includes("AI agents (MCP)")) problems.push("card title missing");
      if (!text.includes("Before you turn this on")) problems.push("enable-moment copy missing");
      const p = new URLSearchParams(location.search);
      const off = p.get("astate") === "off";
      const amber = document.querySelectorAll('[class*="border-l-status-warning"]').length;
      // The feed exists only with the endpoint ON (D16).
      if (!off) {
        if (p.get("feed") === "empty") {
          if (!text.includes("No agent activity yet")) problems.push("empty state missing");
        } else if (amber < 1) {
          problems.push("no muted-amber concerning row rendered");
        }
      }
      const toggle = document.querySelector('[role="switch"]');
      if (!toggle) problems.push("no toggle rendered");
      else if (toggle.getAttribute("aria-checked") !== (p.get("astate") === "off" ? "false" : "true"))
        problems.push("toggle state does not match astate");
      // D16: with the endpoint OFF nothing renders below the toggle — no dial,
      // no feed; the paragraph and the toggle are all there is. With it ON,
      // the dial (three level radios) is there, and no retired section is —
      // mail, database and (D17) the publish prompt all folded into the dial.
      const switches = [...document.querySelectorAll('[role="switch"]')];
      if (switches.length !== 1) problems.push(`expected exactly one switch (the endpoint), got ${switches.length}`);
      const dialRadios = document.querySelectorAll('[role="radiogroup"][aria-label="Agent access"] [role="radio"]').length;
      if (p.get("astate") === "off") {
        const standing = p.get("access") === "full" || p.get("access") === "changes";
        if (standing) {
          if (!text.includes("applies as soon as you turn this on")) problems.push("the standing-level note is missing while off");
          // fdfc88c: the DIAL no longer renders while off; its way DOWN does, as a
          // button beside the standing sentence — or the level is un-lowerable
          // without turning the endpoint back on.
          if (![...document.querySelectorAll("button")].some((b) => (b.textContent || "").trim() === "Set it back to Read"))
            problems.push("a standing level above Read has no way down while the endpoint is off");
        }
        if (dialRadios !== 0) problems.push("the dial rendered with the endpoint off");
        if (text.includes("Recent activity")) problems.push("the feed rendered with the endpoint off");
      } else {
        if (dialRadios !== 3) problems.push(`expected the Agent access dial's 3 levels, got ${dialRadios}`);
        if (!text.includes("publish a site to the internet")) problems.push("the dial no longer says Full includes publishing");
      }
      if (text.includes("scratch-site mail") || text.includes("Database access") || text.includes("Site access")) problems.push("a retired section is back");
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
      if (!off && p.get("feed") !== "empty") {
        if (!text.includes("scratch_reap"))
          problems.push("the rexenv (reaper) row is missing — actor rows must be listed, not hidden");
        if (!text.includes("rexenv · automatic"))
          problems.push("the rexenv row is not labelled as rexenv's own — untrue by juxtaposition");
        if (!text.includes("(deleted site)"))
          problems.push("a reap's deleted target does not read as '(deleted site)'");
      }
      return problems;
    });
  },

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

  // The "setup incomplete" badge is the claim that a provision job DIED, and
  // its Retry re-runs one. A row whose job is still RUNNING must show neither
  // (30 Sep 2026: for the whole of a first WordPress install the row behind
  // the New Site dialog read "setup incomplete" + Retry, because the backend
  // persists the row at job start with provisioned=false and the row read
  // that flag alone). Both legs are asserted: the dead row DOES carry badge +
  // Retry, so a page that dropped the badge for everyone cannot pass either;
  // and a row whose answer has not arrived claims nothing.
  provisionBadges: async (page) =>
    page.evaluate(() => {
      const problems = [];
      const rowOf = (domain) =>
        [...document.querySelectorAll("[role=button]")].find((el) =>
          (el.textContent ?? "").includes(domain),
        );
      const dead = rowOf("half.test");
      if (!dead) problems.push("the half-provisioned row (half.test) did not render");
      else {
        if (!dead.querySelector('[data-probe="setup-incomplete"]'))
          problems.push("a DEAD job's row shows no `setup incomplete` badge — the running-row check below proves nothing");
        if (!dead.querySelector('[aria-label="Retry setup"]'))
          problems.push("a DEAD job's row offers no Retry");
      }
      const live = rowOf("creating.test");
      if (!live) problems.push("the row with a RUNNING job (creating.test) did not render");
      else {
        if (live.querySelector('[data-probe="setup-incomplete"]'))
          problems.push("a row whose job is RUNNING says `setup incomplete`");
        if (live.querySelector('[aria-label="Retry setup"]'))
          problems.push("a row whose job is RUNNING offers Retry");
        if (!live.querySelector('[data-probe="setup-running"]'))
          problems.push("a row whose job is RUNNING shows no running state");
        else if (!(live.textContent ?? "").includes("Setting up"))
          problems.push("the running state does not read `Setting up`");
      }
      const asking = rowOf("asking.test");
      if (!asking) problems.push("the not-yet-answered row (asking.test) did not render");
      else if (
        asking.querySelector('[data-probe="setup-incomplete"]') ||
        asking.querySelector('[aria-label="Retry setup"]') ||
        asking.querySelector('[data-probe="setup-running"]')
      )
        problems.push("a row whose job state is not yet known already claims a state");
      return problems;
    }),

  // The delete confirm's type-the-domain gate, on EVERY delete variant (the
  // connected one has two destructive buttons, and both must be gated — a gate
  // that covers one button in a dialog is the same false guard this project has
  // paid for before). Runs after the screenshot, so it may type into the page:
  // empty → every destructive button disabled; a near-miss → still disabled;
  // the exact domain (trailing space, since the copy button invites a paste) →
  // all enabled. The copy button must exist, or the gate is retype-from-memory.
  // The resolver-drift banner's four ruled behaviours, driven end to end.
  // Both zero-render legs lean on the landmark: absence of the banner is also
  // what a broken route renders, so "no banner" only counts with the view
  // provably mounted (the probe-needs-a-known-good rule).
  resolverDrift: async (page) => {
    const problems = [];
    const banner = () =>
      page.evaluate(
        () => document.querySelector('[data-probe="resolver-drift-banner"]')?.innerText ?? null,
      );
    const mounted = () =>
      page.evaluate(() => !!document.querySelector('[data-probe="drift-view"]'));
    const goto = async (drift, driftAt = null) => {
      const u = new URL(page.url());
      if (drift === null) u.searchParams.delete("drift");
      else u.searchParams.set("drift", drift);
      if (driftAt === null) u.searchParams.delete("driftAt");
      else u.searchParams.set("driftAt", driftAt);
      await page.goto(u.toString());
      await page.waitForSelector('[data-probe="drift-view"]');
    };

    // Clean slate: a previous run's dismissals must not leak in.
    await page.evaluate(() => localStorage.removeItem("rexenv.resolverDriftDismissedTlds"));
    await goto("test");

    // 1. One drifted TLD: symptom-first headline + the approved sentence.
    let t = await banner();
    if (!t) problems.push("drifted .test rendered no banner");
    else {
      if (!/Your \.test sites stopped resolving/.test(t))
        problems.push(`headline does not name the symptom + TLD: ${JSON.stringify(t.slice(0, 120))}`);
      // The approved sentence gained "Settings or" in af09a3f (5 Sep 2026),
      // when the take-over consent started rendering on Settings too. This
      // probe still read the old copy and went red on the 0.5.0 release gate —
      // the wk tier is not in verify.sh, so a copy change lands unchecked
      // until the next verify-full. Keep this regex WITH the sentence.
      if (!/You can take it back from Settings or Import\./.test(t))
        problems.push("the approved (redlined) sentence is missing");
      if (!/Valet or Herd/.test(t))
        problems.push("the banner stopped naming who took the file");
    }

    // 2. Two TLDs read as a list.
    await goto("test,dev");
    t = await banner();
    if (!t || !/\.test and \.dev sites stopped resolving/.test(t))
      problems.push(`two drifted TLDs did not render as a list: ${JSON.stringify((t ?? "").slice(0, 120))}`);

    // 3. Dismiss hides it — and PERSISTS across a reload with the same drift.
    await page.getByRole("button", { name: "Dismiss" }).click();
    await page.waitForFunction(
      () => !document.querySelector('[data-probe="resolver-drift-banner"]'),
    );
    await goto("test,dev");
    if (await banner()) problems.push("a dismissed drift re-rendered on reload (dismissal did not persist)");
    if (!(await mounted())) problems.push("CONTROL BROKEN: harness view absent — the dismissal legs prove nothing");

    // 4. #807: a re-take the banner never SAW (made on Import, Settings or by an
    //    agent — no zero-drift visit in between) and a second loss → the banner
    //    is back. The 15.8 VM's §F 7c found it hidden by the first dismissal.
    await goto("test", "2026-10-09 06:00:00");
    if (!(await banner())) {
      problems.push(
        "a second loss after an unseen re-take stayed hidden behind the first loss's dismissal (#807)",
      );
      return problems; // nothing to dismiss — the later legs would only time out
    }
    await page.getByRole("button", { name: "Dismiss" }).click();
    await page.waitForFunction(
      () => !document.querySelector('[data-probe="resolver-drift-banner"]'),
    );

    // 5. Zero drift renders NOTHING (the #306 rule) — and prunes the stored
    //    dismissals, because the TLDs read as ours again.
    await goto(null);
    if (await banner()) problems.push("nothing is drifted and the banner rendered anyway (#306's shape)");
    if (!(await mounted())) problems.push("CONTROL BROKEN: harness view absent on the zero-drift leg");

    // 6. The NEXT loss re-shows after a SEEN ours-again visit too.
    await goto("test");
    if (!(await banner()))
      problems.push(
        "a re-drifted TLD stayed hidden behind an old dismissal — the self-heal is not pruning",
      );
    return problems;
  },
  // #808: the hand-back row's sentence follows WHO answers the TLD. Both legs
  // (borrowed control + drifted) are scenarios, so neither passes vacuously.
  handbackCopy: async (page) => {
    const drifted = new URL(page.url()).searchParams.get("owner") === "drifted";
    const t = await page.evaluate(
      () => document.querySelector('[data-probe="handback-copy"]')?.innerText ?? null,
    );
    if (t === null) return ["CONTROL BROKEN: the hand-back row did not render"];
    if (drifted) {
      const p = [];
      if (/rexenv answers this/.test(t)) p.push("a drifted borrow still claims \"rexenv answers this\" (#808)");
      if (!/took this back/.test(t)) p.push(`the drifted row does not say the TLD was taken back: ${JSON.stringify(t)}`);
      return p;
    }
    return /rexenv answers this/.test(t) ? [] : [`the borrowed row lost its sentence: ${JSON.stringify(t)}`];
  },
  thisSite: async (page) => {
    const p = await page.evaluate(() => {
      const box = document.querySelector('[data-probe="db-nothing-to-do"]');
      if (!box) return ["the nothing-to-import answer did not render as its own state (#812)"];
      const out = [];
      if (!/nothing to import/.test(box.textContent || "")) out.push("the neutral box lost the sentence");
      const retry = [...document.querySelectorAll("button")].some((b) => (b.textContent || "").trim() === "Retry");
      if (retry) out.push("a correct refusal still offers Retry (#812)");
      if (box.className.includes("status-error")) out.push("a correct refusal is styled as an error (#812)");
      return out;
    });
    return p;
  },
  instantFail: async (page) => {
    const start = page.getByRole("button", { name: "Import database" });
    if (!(await start.count())) return ["CONTROL BROKEN: no Import database button"];
    await start.click();
    try {
      await page.waitForFunction(
        () => /couldn't find a wp-config\.php or \.env/.test(document.body.innerText),
        null,
        { timeout: 5000 },
      );
    } catch {
      return ["a job that failed before the card subscribed still reads as running (#810)"];
    }
    return /checking the source database · 0%/.test(await page.evaluate(() => document.body.innerText))
      ? ["the refusal shows, but the running line is still there beside it"]
      : [];
  },
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

/** Each theme card shows the theme's NAME, with its slug still on the card —
 *  and falls back to the slug when the header has no name. */
PROBES.themeTitles = async (page) =>
  page.evaluate(() => {
    const problems = [];
    const text = document.body.innerText;
    // The label a person reads.
    for (const title of ["Twenty Twenty-Five", "Twenty Twenty-Four"]) {
      if (!text.includes(title)) problems.push(`the card is not labelled "${title}"`);
    }
    // The slug is NOT dropped: it is the directory name and the argument every
    // theme command takes, so replacing the label must not remove it.
    for (const slug of ["twentytwentyfive", "twentytwentyfour"]) {
      if (!text.includes(slug)) problems.push(`the slug "${slug}" vanished from its card`);
    }
    // No title in the header → the slug is the label, never a blank line.
    if (!text.includes("custom-child")) problems.push("the untitled theme rendered no label at all");
    // Three cards, or the assertions above could be passing on one row.
    // The version line reads "<slug> · v1.5" on a titled card and a bare
    // "v1.0" on the untitled one, so the counter accepts both — a /^v/
    // count sees only the fallback row and calls the fixture thin.
    const cards = text.split("\n").filter((l) => /(^|·\s*)v\d/.test(l.trim())).length;
    if (cards < 3) problems.push(`only ${cards} version lines rendered — fixture too thin`);
    return problems;
  });

/** The Tunnels filter, per scenario. The assertion that matters is the third
 *  one in each list: a hidden LIVE row must be announced. */
PROBES.tunnelsFilter = async (page, name) =>
  page.evaluate((scenario) => {
    const problems = [];
    const cards = [...document.querySelectorAll('[data-probe="tunnel-card"]')];
    const domains = cards.map((c) => c.getAttribute("data-domain"));
    const live = cards.filter((c) => c.getAttribute("data-live") === "1")
      .map((c) => c.getAttribute("data-domain"));
    const note = document.querySelector('[data-probe="tunnels-hidden-shared"]');
    const empty = document.querySelector('[data-probe="tunnels-no-match"]');
    const noteCount = note ? Number(note.getAttribute("data-count")) : 0;

    if (scenario === "tunnels-plain") {
      // The fixture itself, or every assertion below is about an empty page.
      if (domains.length !== 4) problems.push(`unfiltered page shows ${domains.length} cards, expected 4`);
      if (live.length !== 2) problems.push(`unfiltered page shows ${live.length} live cards, expected 2`);
      if (note) problems.push("the hidden-shared warning renders with no filter applied");
      if (empty) problems.push("the no-match block renders with no filter applied");
    }
    if (scenario === "tunnels-filtered-hides-shared") {
      if (!domains.includes("docs.rex")) problems.push("the matching site was filtered out");
      if (live.length !== 0) problems.push(`expected the two shared cards hidden, ${live.length} still shown`);
      if (!note) problems.push("TWO live public URLs were hidden by the filter and the page said nothing");
      if (noteCount !== 2) problems.push(`the warning claims ${noteCount} hidden shared sites, expected 2`);
      if (note && !/still public/i.test(note.textContent || ""))
        problems.push("the warning names a count but not the consequence");
    }
    if (scenario === "tunnels-by-url") {
      // Pasting a link must find its site — the one question only this page answers.
      if (domains.length !== 1 || domains[0] !== "blog.rex")
        problems.push(`pasting a tunnel URL showed ${JSON.stringify(domains)}, expected ["blog.rex"]`);
      if (live.length !== 1) problems.push("the matched card is not rendered as live");
      if (!note || noteCount !== 1)
        problems.push("the OTHER shared site is hidden and unannounced");
    }
    if (scenario === "tunnels-no-match") {
      if (cards.length !== 0) problems.push(`a non-matching filter still rendered ${cards.length} cards`);
      if (!empty) problems.push("no rows and no explanation — the page just goes blank");
      if (!note || noteCount !== 2)
        problems.push("everything is hidden INCLUDING two live URLs, and only the empty state is shown");
    }
    return problems;
  }, name);

/** Every action `runActions` knows. An unknown one is a scenario bug, not a
 *  no-op — see the throw below. */
const KNOWN_ACTIONS = new Set([
  "consent", "apply", "revert", "confirmRevert", "scrollBottom", "lastMenu",
  "searchDocs", "searchUrl", "searchNoMatch",
]);

function probeFor(name) {
  if (name.startsWith("onboarding")) return PROBES.onboardingEdge;
  if (name.startsWith("wppackages")) return PROBES.wpPackages;
  if (name.startsWith("dbtab-stopped")) return PROBES.dbtabStopped;
  if (name.startsWith("dbtab")) return PROBES.dbtab;
  if (name === "pills") return PROBES.pills;
  if (name.startsWith("agents-access")) return PROBES.agentsAccess;
  if (name.startsWith("agents")) return PROBES.agents;
  if (name === "badges") return PROBES.provisionBadges;
  if (name === "scratch-rows") return PROBES.scratchGroup;
  if (name.startsWith("provision")) return PROBES.provisionRow;
  if (name.startsWith("delete")) return PROBES.deleteGate;
  if (name === "resolver-drift") return PROBES.resolverDrift;
  if (name === "card-instantfail") return PROBES.instantFail;
  if (name === "card-thissite") return PROBES.thisSite;
  if (name.startsWith("resolver-handback")) return PROBES.handbackCopy;
  if (name === "php-versions") return PROBES["php-versions"];
  // ONE probe drives every app-update state by re-navigating, so it is attached
  // to the first scenario only — attaching it to all six would run the same
  // sweep six times per width.
  if (name === "appupdate-never-checked") return PROBES["appupdate-never-checked"];
  if (name.startsWith("adminer-")) return PROBES.adminerVersion;
  if (name.startsWith("tunnels-")) return PROBES.tunnelsFilter;
  if (name === "themes-titles") return PROBES.themeTitles;
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
    } else if (a.startsWith("search")) {
      // Typed, not set: the input is controlled, and assigning `.value`
      // would leave React's state on the empty string — a probe that then
      // passed would be reading the unfiltered page.
      const text = { searchDocs: "docs", searchUrl: "tall-moon", searchNoMatch: "zzzz" }[a];
      const box = page.getByPlaceholder(/Filter sites or paste a link/);
      await box.click();
      await box.fill(text);
      await page.waitForTimeout(200);
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
        // The scenario NAME goes with it: some probes assert a different thing
        // per scenario (the tunnels filter), and the others simply ignore it.
        if (probe) problems.push(...(await probe(page, name)));
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
