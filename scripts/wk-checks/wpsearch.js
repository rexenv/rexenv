// WebKit check: the installed-plugin filter searches what the row SHOWS.
//
// Reported 19 Aug 2026 with a screenshot: typing "loopback" on a site whose
// list plainly reads "rexenv loopback DNS" answered "No plugins match." The
// filter matched the SLUG only (`rexenv-dns`), and every row in that list is
// labelled with its TITLE — so the box searched a string the user could not
// see, which from the outside is indistinguishable from a broken filter.
//
// Held here rather than in a unit test for `wpverdict.js`'s reason: the rule
// lives in the rendered list, and this repo has no JS test runner. The fixture
// rows are the ones that make the two directions separable —
//
//   - "yoast"     appears ONLY in a title ("Yoast SEO" / slug `wordpress-seo`)
//   - "hello"     appears in both title and slug (`hello-dolly` / "Hello Dolly")
//   - "akismet"   the same word in both, the case that passed before the fix
//                 and must keep passing
//   - "zzzz"      matches nothing: the empty state must still be reachable, or
//                 a filter that simply stopped filtering would pass everything
//                 above.
const { webkit } = require("playwright");

const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const URL = `${BASE}/dev/git-panel?panel=wp-add&plugins=list`;

/** The slugs of the rows currently rendered, read from each row's checkbox
 *  label — the one place the slug appears verbatim regardless of the label the
 *  row displays. */
const shownSlugs = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll("input[aria-label^='Select ']")]
      .map((i) => i.getAttribute("aria-label").replace(/^Select /, ""))
      // The header's select-all wears the same label shape ("Select all
      // plugins") and is not a row — counting it made every assertion here
      // off by one, in the direction that looks like a filter bug.
      .filter((n) => n !== "all plugins")
      .sort(),
  );

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1180, height: 900 }, colorScheme: "dark" });
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${String(e).split("\n")[0]}`));

  await page.goto(URL, { waitUntil: "networkidle" });
  await page.waitForTimeout(400);

  const box = page.getByPlaceholder("Search plugins…");
  const type = async (text) => {
    await box.click();
    await box.fill(text);
    await page.waitForTimeout(200);
    return shownSlugs(page);
  };

  // The fixture itself first: every assertion below is about which rows
  // SURVIVE a filter, and all of them pass vacuously on a list that renders
  // nothing.
  const all = await shownSlugs(page);
  for (const want of ["akismet", "hello-dolly", "wordpress-seo"]) {
    if (!all.includes(want)) problems.push(`fixture row "${want}" is not rendered — nothing below is meaningful`);
  }

  // 1. The reported bug: a word that exists ONLY in the displayed title.
  const yoast = await type("yoast");
  if (!yoast.includes("wordpress-seo"))
    problems.push(`"yoast" is the row's own label and matched nothing (shown: ${JSON.stringify(yoast)})`);
  if (yoast.length !== 1) problems.push(`"yoast" matched ${yoast.length} rows, expected 1`);

  // 2. A word in both title and slug — must not be double-counted or dropped.
  const hello = await type("hello");
  if (!hello.includes("hello-dolly") || hello.length !== 1)
    problems.push(`"hello" showed ${JSON.stringify(hello)}, expected exactly ["hello-dolly"]`);

  // 3. The MIRROR of case 1: a word that exists only in the SLUG
  //    (`wordpress-seo`, titled "Yoast SEO"). Added after the first version of
  //    this check let a plant through — swapping `name` for `title` instead of
  //    matching both passed every other assertion here, because in the fixture
  //    every slug word also appears in its title. A check that only proves the
  //    NEW half is how a fix trades one broken search for another.
  const wp = await type("wordpress");
  if (!wp.includes("wordpress-seo") || wp.length !== 1)
    problems.push(`"wordpress" (slug only) showed ${JSON.stringify(wp)}, expected exactly ["wordpress-seo"]`);

  // 4. The case that already worked before the fix.
  const akismet = await type("akismet");
  if (!akismet.includes("akismet") || akismet.length !== 1)
    problems.push(`"akismet" showed ${JSON.stringify(akismet)}, expected exactly ["akismet"]`);

  // 5. The filter must still be able to say no.
  const none = await type("zzzz");
  if (none.length !== 0) problems.push(`a matchless query still showed ${JSON.stringify(none)}`);
  const empty = await page.evaluate(() => document.body.innerText.includes("No plugins match"));
  if (!empty) problems.push("nothing matched and the panel did not say so");

  // 6. Clearing brings everything back — a filter that leaked state would
  //    leave the list short after the empty query above.
  const cleared = await type("");
  if (cleared.length !== all.length)
    problems.push(`clearing the box left ${cleared.length} of ${all.length} rows`);

  await page.screenshot({ path: `${__dirname}/shot-wpsearch.png`, fullPage: true });
  await browser.close();

  if (problems.length) {
    console.log(`✗ wpsearch — ${problems.join("; ")}`);
    process.exit(1);
  }
  console.log("✓ wpsearch — the plugin filter matches the label the row shows, and can still say no");
})();
