// WebKit check: a terminal session outlives the tab that shows it (ledger #424).
//
// Reported 27 Aug 2026: leave the Terminal tab, come back, and the shell had restarted —
// scrollback gone, a new prompt. The fix keeps each site's xterm and PTY in a module-level
// map (`live`) and MOVES the DOM node into a hidden parking bay while the tab is away, so a
// remount re-attaches the same node instead of rebuilding it. No unit layer can see that
// (no DOM, no PTY); this mounts the real SiteTerminal over DevGitPanel's fake PTY.
//
// The fake PTY counts opens and closes. A KEPT session is: the text written before the tab
// went away is still on screen after it comes back, `terminal_open` ran ONCE, and nothing
// closed. Canary: the text is asserted on screen BEFORE the tab leaves, or "still there"
// would also describe a terminal that never rendered.
const { webkit } = require("playwright");
const BASE = process.env.WK_BASE_URL ?? "http://localhost:5199";
const MARK = "rexenv-424-scrollback-marker";

const screenText = (page) =>
  page.evaluate(() => [...document.querySelectorAll(".xterm-rows")].map((r) => r.innerText).join("\n"));
const counts = (page) =>
  page.evaluate(() => ({ opens: window.__termOpens ?? 0, closes: window.__termCloses ?? 0 }));

(async () => {
  const browser = await webkit.launch();
  const page = await browser.newPage({ viewport: { width: 1100, height: 700 } });
  const problems = [];
  page.on("pageerror", (e) => problems.push(`page error: ${e.message}`));
  await page.goto(`${BASE}/dev/git-panel?panel=terminal`, { waitUntil: "networkidle" });
  await page.waitForTimeout(1200);

  const first = await counts(page);
  if (first.opens !== 1) problems.push(`terminal_open ran ${first.opens}× on mount (want 1)`);
  await page.evaluate(
    ([m]) =>
      window.__TAURI_INTERNALS__.invoke("plugin:event|emit", {
        event: "terminal://output/pty-1",
        payload: Array.from(new TextEncoder().encode(`${m}\r\n$ `)),
      }),
    [MARK],
  );
  await page.waitForTimeout(500);
  if (!(await screenText(page)).includes(MARK)) problems.push("CANARY: the output never reached the screen — nothing below proves anything");

  await page.getByRole("button", { name: "Hide terminal tab" }).click();
  await page.waitForTimeout(500);
  if (!(await page.locator('[data-probe="terminal-away"]').count())) problems.push("the tab did not leave");
  await page.getByRole("button", { name: "Show terminal tab" }).click();
  await page.waitForTimeout(800);

  const back = await screenText(page);
  if (!back.includes(MARK)) problems.push("the scrollback is gone after coming back — the terminal was rebuilt, not re-attached");
  const after = await counts(page);
  if (after.opens !== 1) problems.push(`terminal_open ran ${after.opens}× across leave + return (want 1: the PTY must be kept)`);
  if (after.closes !== 0) problems.push(`terminal_close ran ${after.closes}× — leaving the tab must not end the shell`);
  const visible = await page.evaluate(() => {
    const t = document.querySelector(".xterm");
    return !!t && t.getBoundingClientRect().height > 50 && !t.closest('[aria-hidden="true"]');
  });
  if (!visible) problems.push("the re-attached terminal is not visible in the tab");

  await page.screenshot({ path: `${__dirname}/shot-termkeep.png` });
  await browser.close();
  if (problems.length) {
    console.error("termkeep: FAIL\n  - " + problems.join("\n  - "));
    process.exit(1);
  }
  console.log("termkeep: the terminal survives leaving its tab — same PTY, same scrollback, nothing closed");
})();
