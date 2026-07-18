// WebKit render check for GitAddPanel via the dev harness route (mocked IPC).
// Drives the harness page ONLY — never the real app.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  const shot = (n) => page.screenshot({ path: `shot-${n}.png`, fullPage: true });

  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") fails.push(`console.error: ${m.text()}`);
  });

  await page.goto(`${BASE}/dev/git-panel`, { waitUntil: "networkidle" });
  await page.waitForTimeout(600);

  // 1. Initial: url input + Fetch + shorthand disclosure + tool chips.
  const input = page.getByPlaceholder(/github\.com\/owner\/repo/);
  if (!(await input.isVisible())) fails.push("url input not visible");
  for (const text of ["Fetch", "owner/repo", "means github.com", "Re-detect tools"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing initial text: ${text}`);
  }
  if ((await page.getByText("node v22.23.1", { exact: false }).count()) === 0)
    fails.push("node tool chip missing");
  await shot(1);

  // 2. Probe: paste → Fetch → ref select + folder + Add appear.
  await input.fill("https://github.com/acme/my-plugin");
  await page.getByRole("button", { name: "Fetch" }).click();
  await page.waitForTimeout(400);
  const sel = page.getByLabel("Branch or tag");
  if (!(await sel.isVisible())) fails.push("ref select missing after probe");
  const opts = await sel.locator("option").allTextContents();
  if (!opts.some((o) => o.includes("main (default)"))) fails.push("default branch not marked");
  if (!opts.some((o) => o.includes("v1.2.0"))) fails.push("tags missing from select");
  const folder = page.getByLabel("Folder name");
  if ((await folder.inputValue()) !== "my-plugin") fails.push("folder not prefilled");
  if (!(await page.getByRole("button", { name: "Add plugin" }).isVisible()))
    fails.push("Add button missing");
  await shot(2);

  // 3. Add: job card — steps in mixed states, mapped error box with $ line,
  //    node warning, disclosure line, explicit step buttons.
  await page.getByRole("button", { name: "Add plugin" }).click();
  await page.waitForTimeout(500);
  for (const text of [
    "Clone repository",
    "Detect dependencies",
    "composer install",
    "pnpm install",
    "pnpm run build",
    "xcode-select --install",
    "This repo wants Node 18",
    "repo's own scripts",
    "Hide log",
  ]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing job text: ${text}`);
  }
  await shot(3);

  // 4. Log pane: open by default after Add, toggles closed and back.
  if ((await page.getByText("(no output yet)").count()) === 0)
    fails.push("log pane not open after Add");
  await page.getByText("Hide log").click();
  await page.waitForTimeout(200);
  if ((await page.getByText("(no output yet)").count()) !== 0)
    fails.push("log pane did not close");
  await page.getByText("Show log").click();
  await page.waitForTimeout(200);
  if ((await page.getByText("(no output yet)").count()) === 0)
    fails.push("log pane did not reopen");
  await shot(4);

  // 5. Layout sanity: no horizontal overflow anywhere in the panel.
  const overflow = await page.evaluate(() => {
    const el = document.documentElement;
    return el.scrollWidth > el.clientWidth + 1;
  });
  if (overflow) fails.push("horizontal overflow (layout breaks)");

  await browser.close();
  if (fails.length) {
    console.log("WEBKIT CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("WEBKIT CHECK: ALL PASS");
})();
