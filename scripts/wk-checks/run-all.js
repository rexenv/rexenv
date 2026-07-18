// Run every WebKit check in sequence; exit non-zero if any fails.
// Prereqs + usage: see README.md in this directory.
const { spawnSync } = require("node:child_process");

const CHECKS = ["check.js", "rehydrate.js", "repopanel.js", "watchpanel.js", "linkpanel.js"];

let failed = 0;
for (const script of CHECKS) {
  console.log(`\n── ${script} ──`);
  const r = spawnSync("node", [script], { cwd: __dirname, stdio: "inherit" });
  if (r.status !== 0) failed++;
}
console.log(failed === 0 ? "\nwk-checks: ALL PASS" : `\nwk-checks: ${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
