// Run every WebKit check in sequence; exit non-zero if any fails.
// Prereqs + usage: see README.md in this directory.
const { spawnSync } = require("node:child_process");

const CHECKS = [
  "check.js",
  "rehydrate.js",
  "repopanel.js",
  "watchpanel.js",
  "linkpanel.js",
  "focusrefresh.js",
  "repotoast.js",
  "wptoast.js",
  "wpverdict.js",
  "wpupdate.js",
  "cronargs.js",
  "wpsearch.js",
  "wpgitchip.js",
  "wpinstallcard.js",
  "zipinstall.js",
  "openin.js",
  "phppicker.js",
  "domains.js",
  "tldconsent.js",
  "tunnelhealth.js",
  "statusagree.js",
  "wpfocus.js",
  "frameancestors.js",
  "importbar.js",
  "sharedstopped.js",
  "contrast.js",
  "mail.js",
  "agentlog.js",
  // The UI-review sweep asserts now (overflow fatal, pageerror listeners,
  // dbtab height probe, pill metrics) — it belongs in the bar.
  "uireview.js",
];

let failed = 0;
for (const script of CHECKS) {
  console.log(`\n── ${script} ──`);
  const r = spawnSync("node", [script], { cwd: __dirname, stdio: "inherit" });
  if (r.status !== 0) failed++;
}
console.log(failed === 0 ? "\nwk-checks: ALL PASS" : `\nwk-checks: ${failed} FAILED`);
process.exit(failed === 0 ? 0 : 1);
