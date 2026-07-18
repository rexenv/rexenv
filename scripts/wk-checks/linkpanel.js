// Phase D WebKit check: LinkFolderPanel flow with mocked picker + repo_link.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });
  const fails = [];
  page.on("pageerror", (e) => fails.push(`pageerror: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=link`, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);
  for (const text of ["Choose folder…", "keep your own git workflow", "removes only the link"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing initial: ${text}`);
  }
  await page.getByRole("button", { name: "Choose folder…" }).click();
  await page.waitForTimeout(400);
  if ((await page.getByText("/Users/dev/checkouts/my-plugin").count()) === 0)
    fails.push("picked path not shown");
  const nameField = page.getByLabel("Folder name");
  if ((await nameField.inputValue()) !== "my-plugin") fails.push("name not prefilled");
  await page.getByRole("button", { name: "Link plugin" }).click();
  await page.waitForTimeout(400);
  for (const text of ["✓ linked as my-plugin", "git checkout — repo panel available"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing result: ${text}`);
  }
  await page.screenshot({ path: "shot-linkpanel.png", fullPage: true });
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");
  await browser.close();
  if (fails.length) {
    console.log("LINK CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("LINK CHECK: ALL PASS");
})();
