// WebKit check: the engine we ship on ENFORCES the frame-ancestors we emit.
//
// The claim (ledger #39): the Adminer console is embeddable only by the rexenv
// app webview. Adminer's own blanket `X-Frame-Options: deny` is replaced with a
// `frame-ancestors` list scoped to the app origins, so a local process squatting
// :1420 cannot frame a shipped Adminer and drive a PASSWORDLESS database browser
// through the user's own session (B14).
//
// The string was asserted in Rust; that the BROWSER acts on it was not, and the
// browser is the whole mechanism — a header nothing enforces is a comment. So
// this serves a fixture with the exact production header value through
// Playwright's own routing and asks WebKit (the same engine the packaged app
// embeds) to frame it from a foreign origin.
//
// What it cannot prove: that the header reaches the wire in a real install —
// that is `adminer_serve_check` (L1) and the packaged pass. This is about the
// engine's behaviour, which no Rust test can reach.
const { webkit } = require("playwright");

// The value `core::adminer.rs`'s `csp()` writes in a RELEASE build (the Vite dev
// origin is appended only in debug, which is what makes a shipped console
// unframeable by a :1420 squatter).
const PROD_FRAME_ANCESTORS = "tauri://localhost https://tauri.localhost";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
  const fails = [];

  // A "foreign" page (the squatter) that frames the console.
  await page.route("**/squatter", (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html",
      body: `<html><body><iframe id="f" src="https://console.rexenv.test/adminer"></iframe></body></html>`,
    }),
  );
  // The console, with production's header.
  await page.route("**/adminer", (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html",
      headers: { "content-security-policy": `frame-ancestors ${PROD_FRAME_ANCESTORS}` },
      body: "<html><body>ADMINER CONSOLE BODY</body></html>",
    }),
  );

  await page.goto("https://squatter.rexenv.test/squatter", { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(600);

  // Read the frame through PLAYWRIGHT's frame API, not `contentDocument`: the
  // frame is cross-origin by construction, so the DOM property is null whether
  // CSP blocked it or not — the first version of this check "passed" with the
  // header deleted, which is a probe measuring the same-origin policy and
  // calling it CSP.
  const frameBody = async (needle) => {
    const f = page.frames().find((fr) => fr.url().includes(needle));
    if (!f) return null;
    try {
      return await f.evaluate(() => document.body.innerText);
    } catch {
      return null;
    }
  };

  if ((await frameBody("/adminer"))?.includes("ADMINER CONSOLE BODY"))
    fails.push(
      "WebKit FRAMED the console for a foreign origin — the header we emit is not enforced by " +
        "the engine we ship on, and a local squatter could drive a passwordless database browser",
    );

  // CONTROL: the SAME cross-origin shape without the header must frame. Without
  // it, "the console did not load" proves nothing — every cross-origin frame
  // looks the same from the parent.
  await page.route("**/open", (route) =>
    route.fulfill({ status: 200, contentType: "text/html", body: "<html><body>OPEN BODY</body></html>" }),
  );
  await page.route("**/squatter2", (route) =>
    route.fulfill({
      status: 200,
      contentType: "text/html",
      body: `<html><body><iframe id="f" src="https://console.rexenv.test/open"></iframe></body></html>`,
    }),
  );
  await page.goto("https://squatter.rexenv.test/squatter2", { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(600);
  if (!(await frameBody("/open"))?.includes("OPEN BODY"))
    fails.push(
      "CONTROL FAILED: a cross-origin frame with NO CSP did not load either, so 'the console " +
        "did not load' proves nothing about frame-ancestors",
    );

  await browser.close();
  if (fails.length) {
    console.log("FRAME-ANCESTORS CHECK FAILURES:");
    for (const f of fails) console.log(" - " + f);
    process.exit(1);
  }
  console.log("FRAME-ANCESTORS CHECK: ALL PASS (WebKit refuses the foreign frame; an unguarded one loads)");
})();
