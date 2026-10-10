// WebKit check: a plugin's WORKTREE sites (docs/PLAN-git-worktrees.md, W8).
//
// Under a git plugin's repo panel: the list of worktree sites (domain, git's
// live branch, the uncommitted count), the New-worktree dialog, and Remove.
// What the dialog SENDS and what Remove does after a refusal are the
// assertions — both are recorded by the harness:
//   1. the section and the child row render, with branch + "2 uncommitted";
//   2. the dialog: Create stays disabled until a branch is chosen; a NEW branch
//      previews its domain; Create sends {assetKind, assetDir, branch, base};
//   3. a refused (dirty) remove does NOT silently force: a second dialog quotes
//      the backend's file list, and only accepting it sends force=true;
//   4. no horizontal overflow, no page error, in both themes;
//   5. (#848) worktrees made elsewhere: a servable one offers Serve; one Serve would
//      refuse (inside the sites folder) shows the reason INSTEAD of a Serve button;
//   6. (#849) a worktree whose folder is gone: "Clean up missing (1)" prunes once;
//   7. a worktree child's header line, in a column as narrow as the header leaves it, wraps
//      between its pieces and never inside one ("feature-" / "x" on the Dell and the VM).
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const URL = `${BASE}/dev/git-panel?panel=wp-add&plugins=list&git=1&wt=1&dirty=1`;

(async () => {
  const browser = await webkit.launch();
  const fails = [];
  for (const scheme of ["dark", "light"]) {
    const page = await browser.newPage({ viewport: { width: 1180, height: 820 }, colorScheme: scheme });
    // Create hands off to the Sites page, which this harness does not mock (its
    // database poll gets the harness's generic answer and throws). Errors are
    // counted until that hand-off and after the page is reloaded for leg 3.
    let counting = true;
    page.on("pageerror", (e) => counting && fails.push(`${scheme}: pageerror ${String(e).split("\n")[0]}`));
    await page.goto(URL, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    await page.getByRole("button", { name: "git", exact: true }).first().click();
    await page.waitForTimeout(500);

    // 1. the list
    const body = await page.evaluate(() => document.body.innerText);
    for (const want of ["Worktree sites", "feature-x.dev.rex", "feature/x", "2 uncommitted"])
      if (!body.includes(want)) fails.push(`${scheme} 1: "${want}" not shown`);

    // 2. the dialog
    await page.getByRole("button", { name: "New worktree…" }).click();
    await page.waitForTimeout(300);
    const dialog = page.getByRole("dialog");
    const create = dialog.getByRole("button", { name: /Create worktree site/ });
    if (!(await create.isDisabled())) fails.push(`${scheme} 2: Create enabled before a branch was chosen`);
    await dialog.getByText("A new branch").click();
    await dialog.getByLabel("New branch name").fill("feature/New-Thing");
    await page.waitForTimeout(500);
    const dtext = await dialog.innerText();
    if (!dtext.includes("https://feature-new-thing.dev.rex")) fails.push(`${scheme} 2: no domain preview (${dtext.slice(0, 200)})`);
    if (await create.isDisabled()) fails.push(`${scheme} 2: Create still disabled with a previewed branch`);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) fails.push(`${scheme}: horizontal overflow`);
    await page.screenshot({ path: `shot-worktree-${scheme}.png`, fullPage: true });
    counting = false;
    await create.click();
    await page.waitForTimeout(500);
    if (!page.url().endsWith("/sites")) fails.push(`${scheme} 2: Create did not hand off to the Sites page (${page.url()})`);
    const sent = await page.evaluate(() => window.__worktreeCreates ?? []);
    const r = sent[0] || {};
    if (!(r.assetKind === "plugin" && r.assetDir === "akismet" && r.branch === "feature/New-Thing" && r.base === "feat/x"))
      fails.push(`${scheme} 2: create sent ${JSON.stringify(sent)}`);

    // 3. remove: refused, then forced only on a second yes
    await page.goto(URL, { waitUntil: "networkidle" });
    counting = true;
    await page.waitForTimeout(400);
    await page.getByRole("button", { name: "git", exact: true }).first().click();
    await page.waitForTimeout(400);
    await page.getByTitle(/Remove feature-x\.dev\.rex/).click();
    await page.waitForTimeout(200);
    await page.getByRole("dialog").getByRole("button", { name: "Remove", exact: true }).click();
    await page.waitForTimeout(400);
    const second = await page.getByRole("dialog").innerText().catch(() => "");
    if (!second.includes("notes.php")) fails.push(`${scheme} 3: the forced-remove dialog does not quote the files (${second.slice(0, 120)})`);
    let calls = await page.evaluate(() => window.__worktreeRemoves ?? []);
    if (JSON.stringify(calls) !== "[false]") fails.push(`${scheme} 3: before the second yes, calls were ${JSON.stringify(calls)}`);
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Remove anyway" })
      .click({ timeout: 3000 })
      .catch(() => fails.push(`${scheme} 3: no "Remove anyway" question after a refused remove`));
    await page.waitForTimeout(400);
    calls = await page.evaluate(() => window.__worktreeRemoves ?? []);
    if (JSON.stringify(calls) !== "[false,true]") fails.push(`${scheme} 3: remove calls ${JSON.stringify(calls)}`);
    // 5. the "made elsewhere" list never offers what Serve would refuse.
    await page.goto(`${BASE}/dev/git-panel?panel=wt-site`, { waitUntil: "networkidle" });
    await page.waitForTimeout(500);
    const serves = await page.getByRole("button", { name: "Serve" }).count();
    const why = await page.evaluate(() => document.body.innerText);
    // The missing one's Serve is disabled, not absent — one ENABLED Serve, for the servable one.
    const enabledServes = await page.locator('button:has-text("Serve"):not([disabled])').count();
    if (!(serves === 2 && enabledServes === 1 && why.includes("git worktree move") && why.includes("/Users/somebody/elsewhere-try"))) fails.push(`${scheme} 5: Serve buttons=${serves} (enabled ${enabledServes}), reason shown=${why.includes("git worktree move")}`);
    // 6. (#849) a vanished folder: "Clean up missing (1)" asks git to prune, once.
    await page.getByRole("button", { name: "Clean up missing (1)" }).click();
    await page.waitForTimeout(300);
    const prunes = await page.evaluate(() => window.__prunes ?? 0);
    if (prunes !== 1) fails.push(`${scheme} 6: Clean up missing → ${prunes} prune calls`);
    // 7. the header line never breaks inside a piece.
    await page.goto(`${BASE}/dev/git-panel?panel=wt-of`, { waitUntil: "networkidle" });
    await page.waitForTimeout(400);
    const broken = await page.evaluate(() => {
      const line = document.querySelector('[data-testid="wt-of"] > div');
      if (!line) return ["(no line rendered)"];
      const out = [];
      for (const el of line.children) {
        const text = el.textContent.trim();
        if (!text) continue;
        const r = document.createRange();
        r.selectNodeContents(el);
        const tops = new Set([...r.getClientRects()].map((q) => Math.round(q.top)));
        if (tops.size > 1) out.push(text);
      }
      return out;
    });
    if (broken.length) fails.push(`${scheme} 7: broken inside a piece: ${JSON.stringify(broken)}`);
    await page.close();
  }
  await browser.close();
  if (fails.length) {
    console.log("worktree.js: FAIL\n  " + fails.join("\n  "));
    process.exit(1);
  }
  console.log("worktree.js: all green");
})();
