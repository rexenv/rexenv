// WebKit check: the two git chips on a plugin row RENDER, in both themes.
//
// They had no fixture anywhere — `repo_assets` and `repo_unmanaged` both
// answered `[]` in every harness — so the chips a git-managed plugin shows
// ("git", and the dashed "git?" for a directory that merely looks like a
// checkout) were never rendered by any check. That is how their colours were
// changed during the 19 Aug 2026 token sweep (#373) with nothing to look at.
//
// What this asserts is narrow and specific to the failure a token swap can
// cause: Tailwind emits NO rule for a key that is not in the theme, so a class
// naming a token that exists in `tokens.css` but was never mapped in
// `tailwind.config.js` leaves the element with no colour at all — transparent
// background, inherited text — which looks deliberate on screen and passes
// every other check here. `bg-rex-accent-blue-bg` and
// `border-rex-accent-blue-border` are exactly that case: both were added to the
// config in that commit.
//
// So it reads COMPUTED styles: the chip must have a non-transparent background
// (or border, for the dashed one) and a text colour that is not simply the
// inherited body colour. Both themes, because a token resolving in one and not
// the other is the shape of the bug that started this.
const { webkit } = require("playwright");

const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const URL = `${BASE}/dev/git-panel?panel=wp-add&plugins=list&git=1`;

const alpha = (rgb) => {
  const m = /rgba?\(([^)]+)\)/.exec(rgb || "");
  if (!m) return 0;
  const parts = m[1].split(",").map((n) => parseFloat(n));
  return parts.length > 3 ? parts[3] : 1;
};

(async () => {
  const browser = await webkit.launch();
  const problems = [];

  for (const scheme of ["dark", "light"]) {
    const page = await browser.newPage({ viewport: { width: 1180, height: 760 }, colorScheme: scheme });
    page.on("pageerror", (e) => problems.push(`${scheme}: pageerror ${String(e).split("\n")[0]}`));
    await page.goto(URL, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);

    const chips = await page.evaluate(() =>
      [...document.querySelectorAll("button")]
        .filter((b) => ["git", "git?"].includes((b.textContent || "").trim()))
        .map((b) => {
          const cs = getComputedStyle(b);
          return {
            label: b.textContent.trim(),
            color: cs.color,
            background: cs.backgroundColor,
            border: cs.borderTopColor,
            borderStyle: cs.borderTopStyle,
            body: getComputedStyle(document.body).color,
          };
        }),
    );

    for (const want of ["git", "git?"]) {
      const chip = chips.find((c) => c.label === want);
      if (!chip) {
        problems.push(`${scheme}: the "${want}" chip did not render — the fixture or the row is broken`);
        continue;
      }
      if (chip.color === chip.body) {
        problems.push(`${scheme}: "${want}" text is the inherited body colour (${chip.color}) — its class emitted no rule`);
      }
      // The solid chip is a FILL; the dashed one is a border. Each must have
      // the one it is made of, or the token behind it produced nothing.
      if (want === "git" && alpha(chip.background) === 0) {
        problems.push(`${scheme}: "git" has no background (${chip.background}) — bg-rex-accent-blue-bg emitted nothing`);
      }
      if (want === "git?") {
        if (alpha(chip.border) === 0) {
          problems.push(`${scheme}: "git?" has no border colour (${chip.border}) — border-rex-accent-blue-border emitted nothing`);
        }
        if (chip.borderStyle !== "dashed") {
          problems.push(`${scheme}: "git?" is ${chip.borderStyle}, not dashed — it reads as a confirmed checkout`);
        }
      }
    }

    await page.screenshot({ path: `${__dirname}/shot-wpgitchip-${scheme}.png` });
    await page.close();
  }

  await browser.close();
  if (problems.length) {
    console.log(`✗ wpgitchip — ${problems.join("; ")}`);
    process.exit(1);
  }
  console.log("✓ wpgitchip — both git chips resolve a real colour in dark and light");
})();
