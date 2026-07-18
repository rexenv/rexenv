// Phase A WebKit check: RepoPanel renders the full status vocabulary.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=repo`, { waitUntil: "networkidle" });
  await page.waitForTimeout(800);
  for (const text of [
    "⎇ feat/x",
    "3 changed · 2 untracked",
    "↑2 ↓1 vs origin/feat/x",
    "cloned",
    "git@github.com:acme/my-plugin.git",
    "added @ develop",
    "Show last job log",
    "Refresh",
  ]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing: ${text}`);
  }
  await page.getByText("Show last job log").click();
  await page.waitForTimeout(300);
  if ((await page.getByText("Generating autoload files").count()) === 0)
    fails.push("log lines did not render");
  // Phase B: ops row present; Pull starts an op job whose card renders with
  // the deps-changed offer + disclosure (mocked repo_git_op).
  for (const b of ["Fetch", "Pull", "Push", "Checkout"]) {
    if ((await page.getByRole("button", { name: b }).count()) === 0)
      fails.push(`missing op button: ${b}`);
  }
  if ((await page.getByLabel("Checkout target").count()) === 0)
    fails.push("checkout select missing");
  await page.getByRole("button", { name: "Pull", exact: true }).click();
  await page.waitForTimeout(500);
  for (const text of [
    "git pull --ff-only",
    "Dependencies changed with this pull",
    "repo's own scripts",
    "composer install",
    "pnpm install",
  ]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing after Pull: ${text}`);
  }
  await page.screenshot({ path: "shot-repopanel-ops.png", fullPage: true });

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");
  await page.screenshot({ path: "shot-repopanel.png", fullPage: true });
  await browser.close();
  if (fails.length) {
    console.log("REPOPANEL CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("REPOPANEL CHECK: ALL PASS");
})();
