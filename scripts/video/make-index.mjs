// Writes out/index.html: every recorded video in videos.json order, with its
// length, what it shows, and links to its preview page, video, voice and SRT.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const dir = import.meta.dirname;
const out = path.join(dir, "out");
const videos = JSON.parse(readFileSync(path.join(dir, "videos.json"), "utf8"));
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const tier = (t) => (typeof t === "number" ? `Tier ${t}` : t);
const mmss = (t) => `${Math.floor(t / 60)}:${String(Math.round(t % 60)).padStart(2, "0")}`;

const rows = videos.map((v, i) => {
  const cues = path.join(out, `${v.name}.cues.json`);
  if (!existsSync(cues)) return `<tr class="missing"><td>${i + 1}</td><td>${tier(v.tier)}</td><td><b>${esc(v.title)}</b><div>${esc(v.about)}</div></td><td>—</td><td>not recorded yet</td></tr>`;
  const { videoSeconds } = JSON.parse(readFileSync(cues, "utf8"));
  const links = [
    `<a href="${v.name}.preview.html">▶ preview (with voice)</a>`,
    ...(existsSync(path.join(out, `${v.name}.mp4`)) ? [`<a href="${v.name}.mp4"><b>mp4</b></a>`] : []),
    `<a href="${v.name}.webm">video</a>`,
    `<a href="${v.name}.voice.m4a">voice</a>`,
    `<a href="${v.name}.srt">srt</a>`,
  ].join(" · ");
  return `<tr><td>${i + 1}</td><td>${tier(v.tier)}</td><td><b>${esc(v.title)}</b><div>${esc(v.about)}</div></td><td>${mmss(videoSeconds)}</td><td>${links}</td></tr>`;
});

writeFileSync(
  path.join(out, "index.html"),
  `<!doctype html><meta charset="utf-8"><title>rexenv tutorial videos</title>
<style>
body{font:15px/1.5 -apple-system,sans-serif;background:#111;color:#e8e8ee;margin:32px}
h1{font-weight:600}table{border-collapse:collapse;width:100%}td,th{padding:10px 12px;border-bottom:1px solid #2a2a33;vertical-align:top;text-align:left}
td div{color:#9a9aa8;font-size:13px}a{color:#9d8cff}tr.missing{opacity:.45}
</style>
<h1>rexenv tutorial videos</h1>
<p>Open a <b>preview</b> in Chrome to watch with the voiceover. Each video is 1920×1080 WebM (the vertical intro is 1080×1920); the voice track is a separate file of the same length.</p>
<table><tr><th>#</th><th>Tier</th><th>Video</th><th>Length</th><th>Files</th></tr>${rows.join("")}</table>`,
);
console.log(`index: ${rows.length} videos`);
