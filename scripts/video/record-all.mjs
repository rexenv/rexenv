// Records every video in videos.json, one after another, then rebuilds the
// index. Re-run after a UI change and every tutorial follows it.
//   node record-all.mjs            all of them
//   node record-all.mjs mail share just these
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";

const dir = import.meta.dirname;
const only = process.argv.slice(2);
const videos = JSON.parse(readFileSync(path.join(dir, "videos.json"), "utf8")).filter((v) => !only.length || only.includes(v.name));
const failed = [];
for (const v of videos) {
  const t = Date.now();
  process.stdout.write(`${v.name.padEnd(12)} … `);
  // `script`/`args`: one recorder, several cuts (intro + intro portrait).
  const r = spawnSync("node", [`record-${v.script ?? v.name}.mjs`, ...(v.args ?? [])], { cwd: dir, encoding: "utf8" });
  if (r.status === 0) console.log(`ok (${Math.round((Date.now() - t) / 1000)} s)`);
  else {
    failed.push(v.name);
    console.log(`FAILED\n${(r.stderr || r.stdout).split("\n").filter((l) => !/^\s+at /.test(l)).slice(-12).join("\n")}`);
  }
}
spawnSync("node", ["make-index.mjs"], { cwd: dir, stdio: "inherit" });
if (failed.length) {
  console.log(`failed: ${failed.join(", ")}`);
  process.exit(1);
}
