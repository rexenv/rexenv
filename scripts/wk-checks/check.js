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
  // The ref control is the SEARCHABLE picker, not a native select: the first
  // fetch answers with every branch the remote advertises (92 in this fixture,
  // hundreds on a real project), and a <select> makes finding one a scroll.
  const sel = page.getByLabel("Branch or tag");
  if (!(await sel.isVisible())) fails.push("ref picker missing after probe");
  const isNativeSelect = (await sel.locator("option").count()) !== 0;
  if (isNativeSelect) {
    // Named and SKIP the rest: a probe that then waits 30s for a filter box
    // that cannot exist reports a stack trace where a sentence belongs.
    fails.push("the ref control is still a native <select> — no search at the ~100-branch scale");
  } else {
  await sel.click();
  await page.waitForTimeout(300);
  const listed = page.locator('[cmdk-item]');
  if ((await listed.count()) < 90)
    fails.push(`picker listed ${await listed.count()} refs, expected the full remote list`);
  for (const text of ["default", "v1.2.0", "Tags"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`ref picker missing: ${text}`);
  }
  // Filtering is the whole point — and it must reach a branch that is nowhere
  // near the top of the list.
  await page.getByPlaceholder(/Filter branches/).fill("release");
  await page.waitForTimeout(250);
  const narrowed = await listed.count();
  if (narrowed !== 1)
    fails.push(`filtering "release" left ${narrowed} items, expected exactly release/2026-08`);
  await page.getByText("release/2026-08", { exact: false }).click();
  await page.waitForTimeout(200);
  if ((await sel.textContent())?.includes("release/2026-08") !== true)
    fails.push("picking a filtered branch did not set the ref");
  // Back to the default for the Add step below.
  await sel.click();
  await page.waitForTimeout(250);
  await page.getByPlaceholder(/Filter branches/).fill("main");
  await page.waitForTimeout(250);
  await page.getByText("main", { exact: true }).first().click();
  await page.waitForTimeout(200);
  }
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
