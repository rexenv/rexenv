// WebKit check: the contrast the user actually SEES, in both themes.
//
// The gap this closes (docs/TODO.md, promoted out of the WCAG row): the 16 Aug
// token sweep moved 135 consumers to `text-muted`, deleted two tokens and
// darkened one accent — ~40 files — and the only thing that had confirmed the
// RESULT was an eye. L0 (`core::copy_scan::every_text_on_surface_pairing_meets_wcag_aa`)
// is strong but reads tokens.css and Tailwind class names; it computes what a
// PAIRING would be, not what a pixel is.
//
// What only a browser can answer:
//   - composition — the effective background is whatever ancestor actually
//     paints, which class-pair analysis cannot know;
//   - alpha — a token used at 60% opacity is a different colour on screen;
//   - raw CSS — `globals.css` styles `::placeholder` outside Tailwind entirely,
//     and that was the WORST instance the last sweep found;
//   - the SIZE threshold — AA is 4.5:1 for normal text and 3:1 only for large
//     (24px, or 18.66px bold), and size is a rendered fact.
//
// Deliberately NOT a redesign gate: it reports pairs, it does not police
// aesthetics. Icon-only elements are skipped STRUCTURALLY (no text node of their
// own), which is the lesson from the last sweep — an exemption keyed on a token
// NAME went stale the moment the token moved onto a <span>.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

const ROUTES = ["/sites", "/sites/3", "/services", "/databases", "/mail", "/settings"];
const THEMES = ["light", "dark"];

// ── The debt this check found on its first run, recorded so the gate can go
// ── live while the fix is scoped. Same ratchet the 16 Aug token sweep used
// ── (39 pairs recorded, then deleted along with the debt).
//
// Every one of these is a pair L0 CANNOT see, which is the argument for this
// file: `every_text_on_surface_pairing_meets_wcag_aa` computes text tokens
// against SURFACE tokens, and none of these six is text-on-surface.
//
// The list may only SHRINK. A pair that now passes is a FAILURE here — it means
// the debt was repaid and the entry outlived it, which is how an allow-list
// quietly becomes permanent.
const KNOWN = [
  // EMPTY, and that is the point: all six pairs recorded on 21 Aug were repaid on
  // 24 Aug, and this list is what made the repayment visible. A recorded pair that
  // starts passing FAILS this check — so each fix arrived with the ratchet
  // demanding its entry be deleted, one line at a time, instead of the list
  // quietly becoming permanent.
  //
  // What they were, so the next reader knows what this cost:
  //   white on --rex-brand 4.35 and brand-as-text 4.12 (dark) — the brand hex is
  //     now the light theme's, so both themes share one, and action labels moved
  //     to --rex-brand-light;
  //   --rex-placeholder on the Sites column headers 2.11 / 2.61 — a token
  //     documented for decorative unbuilt-screen text, doing duty as real UI
  //     labels; the headers use --rex-text-muted now and the token keeps its job;
  //   --rex-accent-blue 4.37 and --rex-accent-red 4.48 on the letter tiles — both
  //     had been darkened once already, against surface-3, and measured here on a
  //     lighter background nobody had computed.
];
const key = (t, fg, bg) => `${t}|${fg}|${bg}`;

const AUDIT = `(() => {
  const parse = (c) => {
    const m = c.match(/rgba?\\(([^)]+)\\)/);
    if (!m) return null;
    const p = m[1].split(",").map((v) => parseFloat(v));
    return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
  };
  const over = (fg, bg) => ({
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  });
  const lum = (c) => {
    const f = (v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
  };
  const ratio = (a, b) => {
    const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
    return (x + 0.05) / (y + 0.05);
  };
  // The background a pixel really has: the nearest ancestor that paints.
  const paintedBg = (el) => {
    let node = el;
    let acc = null;
    while (node && node !== document.documentElement.parentElement) {
      const c = parse(getComputedStyle(node).backgroundColor);
      if (c && c.a > 0) {
        acc = acc ? over(acc, c) : c;
        if (acc.a >= 1 || c.a >= 1) return acc.a >= 1 ? acc : over(acc, { r: 255, g: 255, b: 255, a: 1 });
      }
      node = node.parentElement;
    }
    return acc ?? { r: 255, g: 255, b: 255, a: 1 };
  };

  const out = [];
  for (const el of document.querySelectorAll("body *")) {
    // OWN text only — an element whose text lives in a child is not the thing
    // being painted, and counting it double-reports every wrapper.
    const own = [...el.childNodes]
      .filter((n) => n.nodeType === 3)
      .map((n) => n.textContent.trim())
      .join("")
      .trim();
    if (!own) continue;
    if (el.closest("[aria-hidden='true']")) continue;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none" || parseFloat(st.opacity) === 0) continue;
    const box = el.getBoundingClientRect();
    if (box.width < 2 || box.height < 2) continue;

    const fg0 = parse(st.color);
    if (!fg0) continue;
    const bg = paintedBg(el);
    const fg = fg0.a < 1 ? over(fg0, bg) : fg0;
    const size = parseFloat(st.fontSize);
    const weight = parseInt(st.fontWeight, 10) || 400;
    const large = size >= 24 || (size >= 18.66 && weight >= 700);
    const need = large ? 3.0 : 4.5;
    const r = ratio(fg, bg);
    if (r + 0.005 < need) {
      out.push({
        text: own.slice(0, 40),
        tag: el.tagName.toLowerCase(),
        cls: (el.className && el.className.baseVal !== undefined ? el.className.baseVal : el.className || "")
          .toString()
          .slice(0, 70),
        color: st.color,
        bg: \`rgb(\${Math.round(bg.r)}, \${Math.round(bg.g)}, \${Math.round(bg.b)})\`,
        size,
        ratio: Math.round(r * 100) / 100,
        need,
      });
    }
  }
  return out;
})()`;

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1300, height: 950 } });
  const fails = [];
  const errors = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));

  let sampled = 0;
  const seen = new Set();
  for (const theme of THEMES) {
    for (const route of ROUTES) {
      await page.goto(`${BASE}${route}`, { waitUntil: "networkidle" });
      await page.evaluate((t) => {
        document.documentElement.dataset.theme = t;
        localStorage.setItem("rex-theme", t);
      }, theme);
      await page.waitForTimeout(500);

      const found = await page.evaluate(AUDIT);
      const counted = await page.evaluate(
        `[...document.querySelectorAll("body *")].filter((el) =>
           [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())).length`,
      );
      sampled += counted;
      for (const f of found) {
        const k = key(theme, f.color, f.bg);
        if (KNOWN.some((d) => key(d.theme, d.fg, d.bg) === k)) {
          seen.add(k);
          continue;
        }
        fails.push(
          `${theme} ${route}  ${f.ratio}:1 (needs ${f.need}) — ${f.color} on ${f.bg} ` +
            `at ${f.size}px  <${f.tag}> ${JSON.stringify(f.text)}  [${f.cls}]`,
        );
      }
    }
  }

  // A sweep that sampled nothing would report zero failures and look perfect.
  // The number is printed on every run for the same reason the ledger's tally is.
  if (sampled < 200) {
    fails.push(
      `SAMPLE TOO SMALL: only ${sampled} text elements across ${ROUTES.length} routes × ` +
        `${THEMES.length} themes — the harness is not rendering, so "no failures" means nothing`,
    );
  }

  // A recorded pair that no longer appears has been REPAID, and the entry must
  // go with it. Forcing that here is what stops the list becoming permanent —
  // the 16 Aug sweep needed exactly this failure to make its repayment visible.
  for (const d of KNOWN) {
    if (!seen.has(key(d.theme, d.fg, d.bg))) {
      fails.push(
        `RECORDED PAIR NOW PASSES (or no longer renders): ${d.theme} ${d.fg} on ${d.bg} ` +
          `(was ${d.ratio}:1). Delete its KNOWN entry in this file — the debt is paid.`,
      );
    }
  }

  await browser.close();
  if (errors.length) {
    console.log("CONTRAST CHECK — PAGE ERRORS (a crashed route audits nothing):");
    for (const e of [...new Set(errors)]) console.log(" - " + e);
  }
  if (fails.length || errors.length) {
    console.log(`CONTRAST CHECK FAILURES (${fails.length} pairs, ${sampled} text elements sampled):`);
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log(
    `CONTRAST CHECK: ALL PASS (${sampled} rendered text elements, both themes, AA) ` +
      `— ${KNOWN.length} pairs recorded as known debt, see KNOWN in this file`,
  );
})();
