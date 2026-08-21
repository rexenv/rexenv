// WebKit check: a FrankenPHP site's PHP picker is READ-ONLY, and says why.
//
// The claim (ledger #333, shipped 15 Aug 2026): a FrankenPHP site's
// `php_version` is a promise it cannot keep — the backend embeds its own PHP and
// the per-version pools are not involved — so SiteDetail shows the SERVED
// version, disables the picker, and explains the swap that would give the user
// their choice back. Refusing the pairing was rejected: it is not invalid, it is
// fixed by the backend, and refusing teaches nothing.
//
// It shipped with its L2 gap STATED — "no wk-check asserts the picker is
// disabled" — and that gap sat open in docs/TODO.md until this file. Worth
// noticing why it is cheap now and was not then: the mock already carries
// `network.rex` as a FrankenPHP site precisely so this state renders
// (`src/lib/mock.ts`), which is the fixture half someone did in advance.
//
// What this proves: the DISABLED attribute, the served version in the option,
// and the sentence that names the way out. What it cannot prove: that the
// version shown is the one FrankenPHP really embeds — that comes from the
// `frankenphp_embedded_php` command and is L1's job, not a browser's.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

// Mock ids from src/lib/mock.ts: 3 is the FrankenPHP site, 1 is nginx.
const FRANKENPHP_SITE = "3";
const NGINX_SITE = "1";

const picker = async (page, id) => {
  await page.goto(`${BASE}/sites/${id}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);
  return page.evaluate(() => {
    const label = [...document.querySelectorAll("*")].find(
      (el) => el.children.length === 0 && el.textContent.trim() === "PHP version",
    );
    const card = label?.closest("div")?.parentElement;
    const select = card?.querySelector("select");
    return {
      found: !!select,
      disabled: select?.disabled ?? null,
      optionText: select?.options?.[select.selectedIndex]?.textContent?.trim() ?? "",
      cardText: card?.textContent ?? "",
    };
  });
};

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  // ── The subject: a FrankenPHP site ──────────────────────────────────────
  const fp = await picker(page, FRANKENPHP_SITE);
  if (!fp.found) {
    fails.push("no PHP-version select on the FrankenPHP site — harness or markup changed");
  } else {
    if (fp.disabled !== true) fails.push("the FrankenPHP site's PHP picker is NOT disabled");
    if (!/FrankenPHP's embedded PHP/.test(fp.optionText))
      fails.push(`the option does not name the embedded build: ${JSON.stringify(fp.optionText)}`);
    // The served version, not the stored one. mock's network.rex stores 8.1 and
    // is served by FrankenPHP's own build — showing 8.1 here would be the exact
    // lie the row was filed about.
    if (/^8\.1\b/.test(fp.optionText))
      fails.push("the picker shows the STORED 8.1, which is the promise FrankenPHP cannot keep");
    if (!/Fixed by FrankenPHP/.test(fp.cardText))
      fails.push("the card never says the version is fixed by FrankenPHP");
    if (!/Switch the web server to Nginx or Apache/.test(fp.cardText))
      fails.push("the card does not say how to get the choice back");
  }

  // ── The control, which is what stops this passing vacuously ─────────────
  // A page where every select happens to be disabled would satisfy everything
  // above. An nginx site must still have a WORKING picker.
  const ng = await picker(page, NGINX_SITE);
  if (!ng.found) {
    fails.push("no PHP-version select on the nginx site — control broken, the check proves nothing");
  } else if (ng.disabled !== false) {
    fails.push("CONTROL FAILED: the nginx site's picker is disabled too, so 'disabled' means nothing here");
  }

  await page.screenshot({ path: "shot-phppicker.png" });
  await browser.close();

  if (fails.length) {
    console.log("PHP-PICKER CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("PHP-PICKER CHECK: ALL PASS (disabled + served version + both sentences, nginx control enabled)");
})();
