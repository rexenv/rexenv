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

  // ── Working tree: Status / Stash / Restore / Reset ────────────────────────
  // The row exists, and the parked work is VISIBLE — stashed files are gone
  // from the tree and the branch reads clean, so the count in the header is
  // the only thing that says "parked, not lost".
  for (const text of ["Working tree", "2 stashed"]) {
    if ((await page.getByText(text, { exact: false }).count()) === 0)
      fails.push(`missing: ${text}`);
  }
  for (const b of ["Status", "Stash", "Restore", "Reset"]) {
    if ((await page.getByRole("button", { name: b, exact: true }).count()) === 0)
      fails.push(`missing working-tree button: ${b}`);
  }

  // Reset destroys work with nothing to recover it from, so it must be GATED:
  // no op may be sent until the confirm is accepted, and the dialog has to say
  // both halves — what cannot come back, and what it does NOT delete.
  await page.evaluate(() => { window.__repoOps = []; });
  await page.getByRole("button", { name: "Reset", exact: true }).click();
  await page.waitForTimeout(300);
  const beforeConfirm = await page.evaluate(() => window.__repoOps ?? []);
  if (beforeConfirm.length !== 0)
    fails.push(`Reset sent ${beforeConfirm.length} op(s) BEFORE the confirm was accepted`);
  const dialog = page.locator('[role="dialog"]');
  if ((await dialog.count()) === 0) {
    // Named, not thrown: the interesting regression is "the confirm went
    // away", and a probe that dies on a missing locator reports a stack
    // trace instead of the sentence that says what broke.
    fails.push("Reset opened NO confirm — the unrecoverable op is one click away");
  } else {
    for (const text of ["CANNOT be recovered", "untracked files", "KEPT", "Stash instead"]) {
      if ((await dialog.getByText(text, { exact: false }).count()) === 0)
        fails.push(`reset confirm does not say: ${text}`);
    }
    // The dialog's own Reset button is the second match — scope to the dialog.
    await dialog.getByRole("button", { name: "Reset", exact: true }).click();
    await page.waitForTimeout(400);
    const afterConfirm = await page.evaluate(() => window.__repoOps ?? []);
    if (afterConfirm.length !== 1 || afterConfirm[0]?.op !== "reset")
      fails.push(`after confirming, Reset sent ${JSON.stringify(afterConfirm)} — expected one op "reset"`);
  }

  // Restore sends the SELECTED entry, not a hardcoded stash@{0}: the list
  // renumbers on every pop, so the ref on the wire is the whole claim.
  await page.getByLabel("Stash entry to restore").click();
  await page.waitForTimeout(300);
  if ((await page.getByText("block editor crash", { exact: false }).count()) === 0)
    fails.push("stash picker does not show the entry MESSAGE (stash@{1} alone says nothing)");
  await page.getByText("block editor crash", { exact: false }).click();
  await page.waitForTimeout(200);
  await page.evaluate(() => { window.__repoOps = []; });
  await page.getByRole("button", { name: "Restore", exact: true }).click();
  await page.waitForTimeout(400);
  const restoreOps = await page.evaluate(() => window.__repoOps ?? []);
  if (restoreOps[0]?.op !== "stash-pop" || restoreOps[0]?.targetRef !== "stash@{1}")
    fails.push(`Restore sent ${JSON.stringify(restoreOps)} — expected stash-pop of stash@{1}`);

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  if (overflow) fails.push("horizontal overflow");
  await page.screenshot({ path: "shot-repopanel.png", fullPage: true });

  // ── A clean tree with nothing stashed: every door is shut, visibly ────────
  await page.goto(`${BASE}/dev/git-panel?panel=repo&clean=1&stashes=none`, {
    waitUntil: "networkidle",
  });
  await page.waitForTimeout(600);
  for (const [name, why] of [
    ["Stash", "a clean tree would stash nothing and report success anyway"],
    ["Reset", "there are no tracked changes to throw away"],
    ["Restore", "there is nothing stashed to restore"],
  ]) {
    if (!(await page.getByRole("button", { name, exact: true }).isDisabled()))
      fails.push(`${name} is enabled on a clean checkout — ${why}`);
  }
  if ((await page.getByText("stashed", { exact: false }).count()) !== 0)
    fails.push("a checkout with no stashes still renders a stash chip");
  if (await page.getByRole("button", { name: "Status", exact: true }).isDisabled())
    fails.push("Status is disabled on a clean tree — it reads, so it always has an answer");
  await page.screenshot({ path: "shot-repopanel-clean.png", fullPage: true });

  await browser.close();
  if (fails.length) {
    console.log("REPOPANEL CHECK FAILURES:");
    fails.forEach((f) => console.log(" - " + f));
    process.exit(1);
  }
  console.log("REPOPANEL CHECK: ALL PASS");
})();
