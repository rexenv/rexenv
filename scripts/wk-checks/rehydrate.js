// Rehydration check: on load with a live backend job, the panel must show it
// with ZERO clicks — steps, seeded log, Cancel — and no fresh-start form state.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?rehydrate=1`, { waitUntil: "networkidle" });
  await page.waitForTimeout(900);

  // Job card adopted without any interaction:
  for (const text of [
    "my-plugin · https://github.com/acme/my-plugin @ main", // header line
    "Clone repository",
    "pnpm install",           // running step
    "Cancel",                 // running → cancel offered
    "Generating autoload files", // seeded from tail_log
    "Progress: resolved 212",
  ]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing after rehydrate: ${text}`);
  }
  // The running spinner exists (one Loader2 in the steps list).
  if ((await page.locator(".animate-rex-spin").count()) === 0)
    fails.push("no running spinner after rehydrate");
  await page.screenshot({ path: "shot-rehydrate.png", fullPage: true });

  // Non-rehydrate mode still starts blank (no job card).
  await page.goto(`${BASE}/dev/git-panel`, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);
  if ((await page.getByText("Clone repository").count()) !== 0)
    fails.push("blank mode wrongly shows a job");

  await browser.close();
  if (fails.length) {
    console.log("REHYDRATE CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("REHYDRATE CHECK: ALL PASS");
})();
