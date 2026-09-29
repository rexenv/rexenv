// Shared recorder helpers: launch a recording WebKit on the stage, drive the
// cursor, click, type, and save the video. Every scene script builds on these.
import { chromium, webkit } from "playwright";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { synthesize, writeNarration } from "./voice.mjs";

export const BASE = process.env.VIDEO_BASE_URL ?? "http://localhost:5199";
const OUT = path.join(import.meta.dirname, "out");

/** narration: { key: { text, say? } } — optional; see voice.mjs. */
export async function openStage({ narration } = {}) {
  mkdirSync(OUT, { recursive: true });
  // Synthesised BEFORE recording starts: the scene paces itself on each line's
  // real length, so it must be known up front.
  const lines = narration ? synthesize(narration, path.join(OUT, "voice-cache")) : {};
  const cues = [];
  let voiceEnd = 0;
  // Chromium by default: headless WebKit on macOS cannot be larger than the
  // screen, so on a laptop display (1512×982) every frame came out at 0.9 scale
  // inside a grey border — the same script had been fine with a big external
  // monitor attached. Chromium renders offscreen at any size. VIDEO_ENGINE=webkit
  // records in the macOS app's own engine, on a display at least 1920 wide.
  const engine = process.env.VIDEO_ENGINE === "webkit" ? webkit : chromium;
  const browser = await engine.launch();
  const context = await browser.newContext({
    viewport: { width: 1920, height: 1080 },
    colorScheme: "dark",
    recordVideo: { dir: OUT, size: { width: 1920, height: 1080 } },
  });
  const page = await context.newPage();
  const t0 = Date.now();
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error" || m.text().includes("[demo-backend] unanswered")) problems.push(m.text());
  });
  await page.goto(`${BASE}/scripts/video/stage.html`);
  await page.evaluate(() => window.stage.ready());
  const app = await waitForApp(page);
  // The recorder's clock and the video's are NOT the same clock: WebKit's
  // frames reach the video ~0.5s after they happen, a constant (measured with
  // flashes at 2/6/12/20s: 0.48–0.53). Uncorrected, every line of narration
  // led its picture. So the video is synced by a flash — one magenta frame
  // whose wall time we know and whose video time `save` finds — and everything
  // before `startedAt` (this flash included) is load time the video trims off.
  await page.evaluate(() => document.getElementById("sync").classList.add("on"));
  const flashAt = (Date.now() - t0) / 1000;
  await page.waitForTimeout(240);
  await page.evaluate(() => document.getElementById("sync").classList.remove("on"));
  await page.waitForTimeout(200);
  const startedAt = (Date.now() - t0) / 1000;

  const box = async (loc) => {
    await loc.waitFor({ state: "visible" });
    // Scroll the element's own scroller only — never scrollIntoView, which
    // also scrolls the stage around the iframe.
    await loc.evaluate((el) => {
      let p = el.parentElement;
      while (p && !(p.scrollHeight > p.clientHeight && /(auto|scroll)/.test(getComputedStyle(p).overflowY))) p = p.parentElement;
      if (!p) return;
      const r = el.getBoundingClientRect();
      const pr = p.getBoundingClientRect();
      if (r.top < pr.top) p.scrollBy({ top: r.top - pr.top - 12, behavior: "smooth" });
      else if (r.bottom > pr.bottom) p.scrollBy({ top: r.bottom - pr.bottom + 12, behavior: "smooth" });
    });
    await page.waitForTimeout(250);
    return loc.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    });
  };

  const s = {
    page,
    app,
    wait: (ms) => page.waitForTimeout(ms),
    stage: (fn, arg) => page.evaluate(fn, arg),
    caption: (html, at) => page.evaluate(([h, a]) => window.stage.caption(h, a), [html, at]),
    title: (h, p) => page.evaluate(([a, b]) => window.stage.title(a, b), [h, p]),
    spotlight: (rect) => page.evaluate((r) => window.stage.spotlight(r), rect),
    box,

    /** Move the camera and wait for it to SETTLE: the real mouse is aimed from
     *  the drawn cursor's on-screen position, which is wrong mid-transition. */
    async camera(rect, fill) {
      await page.evaluate(([r, f]) => window.stage.camera(r, f), [rect, fill]);
      await page.waitForTimeout(950);
    },

    /** The app-px rect covering every locator, padded. */
    async union(locs, pad = 16) {
      const bs = [];
      for (const l of locs) bs.push(await box(l));
      const x = Math.min(...bs.map((b) => b.x)) - pad;
      const y = Math.min(...bs.map((b) => b.y)) - pad;
      const r = Math.max(...bs.map((b) => b.x + b.w)) + pad;
      const btm = Math.max(...bs.map((b) => b.y + b.h)) + pad;
      return { x, y, w: r - x, h: btm - y };
    },

    /** Glide to an element (fx/fy = where inside it, 0..1), then put the real
     *  mouse under the drawn cursor so hover states follow it. */
    async moveTo(loc, { ms = 750, fx = 0.5, fy = 0.5 } = {}) {
      const b = await box(loc);
      await page.evaluate(([x, y, d]) => window.stage.moveTo(x, y, d), [b.x + b.w * fx, b.y + b.h * fy, ms]);
      const t = await page.evaluate(() => window.stage.tip());
      await page.mouse.move(t.x, t.y);
    },

    async click(loc, opts) {
      await s.moveTo(loc, opts);
      await page.waitForTimeout(140);
      const t = await page.evaluate(() => window.stage.tip());
      await Promise.all([page.evaluate(() => window.stage.press()), page.mouse.click(t.x, t.y)]);
      await page.waitForTimeout(220);
    },

    /** Start a narration line — after the previous one has finished, so lines
     *  never overlap. Returns at once; the scene keeps acting while it plays. */
    async say(key, gap = 250) {
      const line = lines[key];
      if (!line) throw new Error(`no narration line "${key}"`);
      await s.voiceDone(gap);
      cues.push({ ...line, key, at: (Date.now() - t0) / 1000 });
      voiceEnd = Date.now() + line.dur * 1000;
    },

    /** Wait for the current line to finish (+ a breath). */
    async voiceDone(gap = 250) {
      const left = voiceEnd + gap - Date.now();
      if (voiceEnd && left > 0) await page.waitForTimeout(left);
    },

    /** Human-paced typing: a steady rhythm with a little jitter. */
    async type(text, perChar = 75) {
      for (const ch of text) {
        await page.keyboard.type(ch);
        await page.waitForTimeout(perChar * (0.6 + Math.random() * 0.8));
      }
    },

    /** Close the recording and write out/<name>.webm, trimmed to start where
     *  the stage was ready (the page load before it is blank frames). */
    async save(name) {
      const video = page.video();
      await context.close();
      await browser.close();
      const raw = path.join(OUT, `${name}.raw.webm`);
      renameSync(await video.path(), raw);
      if (problems.length) console.warn(`\n${problems.length} problem(s) while recording:\n  ${problems.join("\n  ")}`);
      const dest = path.join(OUT, `${name}.webm`);
      const ff = playwrightFfmpeg();
      if (!ff) {
        console.warn("no ffmpeg in the Playwright cache — kept the untrimmed recording");
        return raw;
      }
      // Where the recorder's `startedAt` falls on the VIDEO's clock: shifted by
      // however late the flash showed up in the frames.
      const lag = findFlash(ff, raw, flashAt + 4) - flashAt;
      const trimAt = startedAt + lag;
      console.log(`sync: flash at ${flashAt.toFixed(2)}s wall, video lags by ${lag.toFixed(2)}s`);
      // Re-encoded (VP8 is the only encoder Playwright's ffmpeg has), at a
      // quality high enough that the second pass adds no visible loss.
      execFileSync(ff, ["-hide_banner", "-loglevel", "error", "-y", "-ss", trimAt.toFixed(3), "-i", raw,
        "-c:v", "libvpx", "-crf", "4", "-b:v", "8M", "-deadline", "good", "-cpu-used", "4",
        "-threads", "8", dest]);
      rmSync(raw);
      if (cues.length) {
        const info = spawnSync(ff, ["-hide_banner", "-i", dest], { encoding: "utf8" }).stderr;
        const [, h, m, sec] = info.match(/Duration: (\d+):(\d+):([\d.]+)/);
        const videoSeconds = +h * 3600 + +m * 60 + +sec;
        const shifted = cues.map((c) => ({ ...c, at: c.at - startedAt }));
        // Kept so the audio can be rebuilt without re-recording:
        //   node -e 'import("./voice.mjs").then(v=>v.rebuild("<name>"))'
        writeFileSync(path.join(OUT, `${name}.cues.json`), JSON.stringify({ videoSeconds, cues: shifted }, null, 2));
        writeNarration(shifted, { dir: OUT, name, videoSeconds });
      }
      return dest;
    },
  };
  return s;
}

/** The video time (s) of the first magenta frame in the first `until`
 *  seconds. Playwright's ffmpeg can write PNG but not read it, so frames go
 *  out as tiny PNGs and macOS `sips` turns them into BMPs we can read. */
function findFlash(ff, video, until) {
  const dir = mkdtempSync(path.join(tmpdir(), "rexenv-sync-"));
  try {
    const fps = +spawnSync(ff, ["-hide_banner", "-i", video], { encoding: "utf8" }).stderr.match(/(\d+(?:\.\d+)?) fps/)[1];
    execFileSync(ff, ["-hide_banner", "-loglevel", "error", "-t", String(until), "-i", video,
      "-vf", "crop=200:200:860:440,scale=4:4", path.join(dir, "f%05d.png")]);
    const pngs = readdirSync(dir).filter((f) => f.endsWith(".png")).sort();
    execFileSync("sips", ["-s", "format", "bmp", ...pngs.map((f) => path.join(dir, f)), "--out", dir], { stdio: "ignore" });
    for (const [i, f] of pngs.entries()) {
      const b = readFileSync(path.join(dir, f.replace(/\.png$/, ".bmp")));
      const start = b.readUInt32LE(10);
      const step = b.readUInt16LE(28) / 8; // bytes per pixel; the 4th (alpha) is skipped
      let bl = 0, g = 0, r = 0, n = 0;
      for (let p = start; p + 2 < b.length; p += step) {
        bl += b[p];
        g += b[p + 1];
        r += b[p + 2];
        n++;
      }
      if (r / n > 180 && bl / n > 180 && g / n < 80) return i / fps;
    }
    throw new Error("sync flash not found in the recording");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function playwrightFfmpeg() {
  const cache = path.join(homedir(), "Library", "Caches", "ms-playwright");
  if (!existsSync(cache)) return null;
  for (const d of readdirSync(cache).filter((n) => n.startsWith("ffmpeg-")).sort().reverse()) {
    const bin = path.join(cache, d, "ffmpeg-mac");
    if (existsSync(bin)) return bin;
  }
  return null;
}

async function waitForApp(page) {
  for (let i = 0; i < 100; i++) {
    const f = page.frames().find((fr) => fr !== page.mainFrame() && !fr.url().includes("demo.html"));
    if (f) {
      await f.locator("text=Sites").first().waitFor();
      return f;
    }
    await page.waitForTimeout(100);
  }
  throw new Error("the app never booted inside the stage — is vite running at " + BASE + "?");
}
