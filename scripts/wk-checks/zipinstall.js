// WebKit check: the "Upload zip" install source (wp-admin's Upload Plugin).
//
// Two things this proves that a lib test cannot, both about the card that a
// zip job renders:
//   1. the items are shown by FILE NAME. A zip job's `slugs` are absolute
//      paths, and a `/Users/…/Downloads/…` in the card's one truncating line
//      pushes everything a person recognises off the end of it;
//   2. the ATTEMPT CURSOR is absent. wp-cli prints no per-item header on the
//      zip path (asserted live in `wp_install_stream_check` job 4), so
//      `itemCursor` never advances — a rendered "installing item 1 of 2"
//      would be a number that stopped being true at the first item.
// Plus the source tab itself: the button, and the disclosure that says where
// the file is read from, must be there before anything runs.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=wp-add&install=zip`, {
    waitUntil: "networkidle",
  });
  await page.waitForTimeout(900);

  // --- the card (adopted zip job) ----------------------------------------
  const card = await page.evaluate(() => document.body.innerText);
  if (!card.includes("advanced-custom-fields-pro-6.3.11.zip"))
    fails.push("card does not name the archive file");
  if (card.includes("/Users/dev/Downloads/"))
    fails.push("card shows the full path — the file name is the recognisable part");
  if (!card.includes("plugin install (zip)"))
    fails.push("card does not say the items came from a zip");
  if (/installing item \d+ of \d+/i.test(card))
    fails.push("card renders an attempt cursor that a zip job can never advance");

  // --- the source tab ----------------------------------------------------
  await page.getByRole("button", { name: "Upload zip" }).click();
  await page.waitForTimeout(400);
  for (const text of ["Choose .zip", "nothing is uploaded anywhere"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing on the zip tab: ${text}`);
  }
  // Nothing picked yet ⇒ Install cannot be clicked (an empty batch is a
  // backend error the user should never be able to reach).
  const installBtn = page.getByRole("button", { name: /^Install/ });
  if (!(await installBtn.isDisabled())) fails.push("Install enabled with no file picked");

  await page.screenshot({ path: "shot-zipinstall.png", fullPage: true });
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");

  await browser.close();
  if (fails.length) {
    console.log("ZIP INSTALL CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("ZIP INSTALL CHECK: ALL PASS");
})();
