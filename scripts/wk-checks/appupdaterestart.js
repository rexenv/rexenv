// WebKit check: the self-update tells the user before it closes the app.
//
// It used to quit the moment the swap landed — the window vanished under the
// user's hands with no warning, which is how the owner met it (20 Sep 2026).
// The apply and the restart are two commands now, and the ONLY thing that may
// fire the second is the OK button of the dialog between them. So this asserts
// the ORDER, which is the whole claim: install → dialog → (nothing yet) → OK →
// restart.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const url = `${BASE}/dev/ui-review?view=appupdate&state=offered`;

const calls = (page) => page.evaluate(() => window.__rexUpdate ?? []);

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1000, height: 820 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  page.on("console", (m) => m.type() === "error" && fails.push(`console.error: ${m.text()}`));

  await page.goto(url, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);

  const install = page.getByRole("button", { name: /^Install rexenv/ });
  if ((await install.count()) === 0) {
    fails.push("no Install button in the offered state — the fixture proves nothing");
  } else {
    await install.first().click();
    await page.waitForTimeout(700);

    // 1. The dialog, and what it must actually say.
    const dialog = page.getByText("is installed", { exact: false });
    if ((await dialog.count()) === 0) fails.push("no dialog after the install — the app would just vanish");
    // The dialog must render the sentence the BACKEND sent, not one of its own:
    // that sentence describes what a restart does to the stack, so it lives in
    // Rust (`core::app_update::restart_sentence`) and the card is a renderer.
    // The fixture's marker text is what proves it came down the wire.
    const body = await page.locator("body").innerText();
    if (!body.includes("FIXTURE restart notice from the backend"))
      fails.push("the dialog did not render the notice the apply returned — the card has its own copy");

    // 2. NOTHING has restarted yet. This is the bug the dialog replaced.
    const before = await calls(page);
    if (before.some((c) => c.cmd === "app_update_restart"))
      fails.push("the app restarted WITHOUT waiting for the click — the dialog is decoration");
    if (!before.some((c) => c.cmd === "app_update_apply"))
      fails.push("the install never reached the backend");

    // 3. One button, and it is the one that restarts.
    if ((await page.getByRole("button", { name: "Cancel" }).count()) !== 0)
      fails.push("the acknowledgement dialog offers a Cancel — there is no second answer to give");
    await page.getByRole("button", { name: "OK", exact: true }).click();
    await page.waitForTimeout(600);
    const after = await calls(page);
    const restarts = after.filter((c) => c.cmd === "app_update_restart").length;
    if (restarts !== 1) fails.push(`OK fired ${restarts} restarts, expected exactly 1`);
  }

  await page.screenshot({ path: "shot-appupdate-restart.png", fullPage: true });
  await browser.close();
  if (fails.length) {
    console.error("appupdaterestart: FAIL");
    for (const f of fails) console.error("  ✗ " + f);
    process.exit(1);
  }
  console.log("appupdaterestart: the update asks before it closes the app, and only OK closes it");
})();
