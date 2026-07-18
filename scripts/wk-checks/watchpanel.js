// Phase C WebKit check: scripts row + watch running/exited states.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));

  // Scripts row (no watch running): watchy → Watch:, others → Run:, disclosure present.
  await page.goto(`${BASE}/dev/git-panel?panel=repo`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  for (const text of ["Watch: start", "Run: build", "Run: lint", "repo's own scripts"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing scripts row item: ${text}`);
  }

  // Running watcher: dot + Stop + output seeding from the ring.
  await page.goto(`${BASE}/dev/git-panel?panel=repo&watch=1`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  for (const text of ["watching — start", "Stop", "Show output"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing running-watch item: ${text}`);
  }
  if (!(await page.getByRole("button", { name: "Watch: start" }).isDisabled()))
    fails.push("Watch button not disabled while running");
  await page.getByText("Show output").click();
  await page.waitForTimeout(300);
  if ((await page.getByText("compiled successfully").count()) === 0)
    fails.push("ring-seeded watch output missing");
  await page.screenshot({ path: "shot-watch-running.png", fullPage: true });

  // Exited watcher: code + Restart (never auto).
  await page.goto(`${BASE}/dev/git-panel?panel=repo&watch=exited`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  for (const text of ["watcher exited (code 1)", "Restart"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing exited-watch item: ${text}`);
  }
  await page.screenshot({ path: "shot-watch-exited.png", fullPage: true });

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");
  await browser.close();
  if (fails.length) {
    console.log("WATCH CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("WATCH CHECK: ALL PASS");
})();
