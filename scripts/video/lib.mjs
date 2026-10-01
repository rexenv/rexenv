// Shared recorder helpers: launch a recording WebKit on the stage, drive the
// cursor, click, type, and save the video. Every scene script builds on these.
import { chromium, webkit } from "playwright";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { setRate, synthesize, writeNarration } from "./voice.mjs";

export const BASE = process.env.VIDEO_BASE_URL ?? "http://localhost:5199";
const OUT = path.join(import.meta.dirname, "out");

/** narration: { key: { text, say? } } — optional; see voice.mjs.
 *  scene: scenes/<scene>.ts for this video's fixtures; path: the screen the app
 *  opens on; ready: a selector inside the app that means "booted". */
export async function openStage({
  narration,
  scene,
  path: appPath = "/sites",
  ready = "text=Sites",
  timezoneId,
  // The intro's knobs: a 1080×1920 stage, jobs replayed faster, a faster
  // voice, a shorter breath between narration lines, and a shorter pause
  // after a scene switch.
  orient = "landscape",
  speed = 1,
  rate = "+0%",
  gap: lineGap = 250,
  switchSettle = 350,
} = {}) {
  const W = orient === "portrait" ? 1080 : 1920;
  const H = orient === "portrait" ? 1920 : 1080;
  setRate(rate);
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
    viewport: { width: W, height: H },
    colorScheme: "dark",
    // Dates and times render with the browser's locale; pin it so a video
    // reads the same on every machine that records it.
    locale: "en-US",
    // Recorded at night, a mail list reads "03:47 AM"; a scene can move the clock's zone.
    ...(timezoneId ? { timezoneId } : {}),
    recordVideo: { dir: OUT, size: { width: W, height: H } },
  });
  // "Copy" buttons use navigator.clipboard, which headless Chromium refuses
  // without the grant — the button would never say "Copied".
  if (engine === chromium) await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const page = await context.newPage();
  const t0 = Date.now();
  const problems = [];
  page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error" || m.text().includes("[demo-backend] unanswered")) problems.push(m.text());
  });
  const extra = { ...(orient === "portrait" ? { orient } : {}), ...(speed !== 1 ? { speed: String(speed) } : {}) };
  const q = new URLSearchParams({ path: appPath, ...(scene ? { scene } : {}), ...extra });
  await page.goto(`${BASE}/scripts/video/stage.html?${q}`);
  await page.evaluate(() => window.stage.ready());
  const app = await waitForApp(page, ready);
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
    try {
      await loc.waitFor({ state: "visible" });
    } catch (e) {
      // What the stage showed when a step's target never appeared.
      await page.screenshot({ path: path.join(OUT, "_fail.png") }).catch(() => {});
      throw e;
    }
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
    // A smooth scroll can outlast any fixed wait (a long page took >250 ms,
    // and the click landed below the window): read until the rect holds still.
    const rect = () =>
      loc.evaluate((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, w: r.width, h: r.height };
      });
    let prev = null;
    for (let i = 0; i < 20; i++) {
      await page.waitForTimeout(i === 0 ? 150 : 60);
      const r = await rect();
      if (prev && r.x === prev.x && r.y === prev.y) return r;
      prev = r;
    }
    return prev;
  };

  const s = {
    page,
    app,
    wait: (ms) => page.waitForTimeout(ms),
    stage: (fn, arg) => page.evaluate(fn, arg),
    caption: (html, at) => page.evaluate(([h, a]) => window.stage.caption(h, a), [html, at]),
    title: (h, p, fast = false, code = "") => page.evaluate(([a, b, c, d]) => window.stage.title(a, b, c, d), [h, p, fast, code]),
    spotlight: (rect) => page.evaluate((r) => window.stage.spotlight(r), rect),
    box,

    /** Swap the app for another scene mid-recording — one video, one voice
     *  track. The window fades out while the iframe reloads; the app frame
     *  object survives the navigation, so `s.app` stays valid. */
    async switchScene(nextScene, nextPath = "/sites", nextReady = "text=Sites", params = {}) {
      await page.evaluate(() => window.stage.showWindow(false));
      await page.evaluate(() => window.stage.camera(null));
      await page.waitForTimeout(250);
      const q = new URLSearchParams({ path: nextPath, ...(nextScene ? { scene: nextScene } : {}), ...extra, ...params });
      await page.evaluate((search) => window.stage.loadScene(search), q.toString());
      await app.locator(nextReady).first().waitFor({ timeout: 15_000 });
      await page.evaluate(() => window.stage.showWindow(true));
      await page.waitForTimeout(switchSettle);
    },
    layout: (mode) => page.evaluate((m) => window.stage.layout(m), mode),
    headline: (h, sub = "") => page.evaluate(([a, b]) => window.stage.headline(a, b), [h, sub]),

    /** The stage's terminal window (stage.ts): show/hide/clear/print/type/idle. */
    term: {
      show: (o = {}) => page.evaluate((x) => window.stage.terminal.show(x), o),
      hide: () => page.evaluate(() => window.stage.terminal.hide()),
      clear: () => page.evaluate(() => window.stage.terminal.clear()),
      print: (lines, gap = 0) => page.evaluate(([l, g]) => window.stage.terminal.print(l, g), [lines, gap]),
      type: (cmd, o = {}) => page.evaluate(([c, x]) => window.stage.terminal.type(c, x), [cmd, o]),
      idle: (prompt) => page.evaluate((p) => window.stage.terminal.idle(p), prompt),
      bar: (ms) => page.evaluate((m) => window.stage.terminal.bar(m), ms),
    },

    /** Move the camera and wait for it to SETTLE: the real mouse is aimed from
     *  the drawn cursor's on-screen position, which is wrong mid-transition. */
    async camera(rect, fill) {
      await page.evaluate(([r, f]) => window.stage.camera(r, f), [rect, fill]);
      await page.waitForTimeout(950);
    },

    /** The rect of the first ancestor of `loc` at least `minWidth` app-px wide
     *  — "the card this button sits in", without knowing the card's markup. */
    async containerOf(loc, minWidth = 400, pad = 0) {
      await box(loc);
      return loc.evaluate((el, [mw, p]) => {
        let e = el;
        while (e.parentElement && e.getBoundingClientRect().width < mw) e = e.parentElement;
        const r = e.getBoundingClientRect();
        return { x: r.x - p, y: r.y - p, w: r.width + 2 * p, h: r.height + 2 * p };
      }, [minWidth, pad]);
    },

    /** The app-px rect covering every locator, padded. */
    async union(locs, pad = 16) {
      for (const l of locs) await box(l);
      // Scrolling a later one into view moves the earlier ones: read every
      // rect after the last scroll (the PHP 7.4–8.5 zoom landed off-centre).
      const bs = [];
      for (const l of locs)
        bs.push(await l.evaluate((el) => {
          const r = el.getBoundingClientRect();
          return { x: r.x, y: r.y, w: r.width, h: r.height };
        }));
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
      // `pre`/`post`: the beat before and after the press — a tutorial's pace
      // by default; a montage passes shorter ones.
      await page.waitForTimeout(opts?.pre ?? 140);
      let t = await page.evaluate(() => window.stage.tip());
      // A camera zoomed on a card that has since moved can leave the target
      // off-frame, and a click outside the viewport lands nowhere (it silently
      // failed to stop a tunnel once). Pull the camera back and aim again.
      if (t.x < 8 || t.y < 8 || t.x > W - 8 || t.y > H - 8) {
        await s.camera(null);
        await s.moveTo(loc, { ...opts, ms: 400 });
        t = await page.evaluate(() => window.stage.tip());
      }
      await Promise.all([page.evaluate(() => window.stage.press()), page.mouse.click(t.x, t.y)]);
      await page.waitForTimeout(opts?.post ?? 220);
    },

    /** Start a narration line — after the previous one has finished, so lines
     *  never overlap. Returns at once; the scene keeps acting while it plays. */
    async say(key, gap = lineGap) {
      const line = lines[key];
      if (!line) throw new Error(`no narration line "${key}"`);
      await s.voiceDone(gap);
      cues.push({ ...line, key, at: (Date.now() - t0) / 1000 });
      voiceEnd = Date.now() + line.dur * 1000;
    },

    /** A narration line's length in seconds (known before recording starts). */
    dur: (key) => lines[key].dur,

    /** Wait for the current line to finish (+ a breath). */
    async voiceDone(gap = lineGap) {
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
      const unknown = await app.evaluate(() => window.__demoUnknown).catch(() => null);
      if (unknown) problems.push(`unanswered IPC: ${JSON.stringify(unknown)}`);
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

async function waitForApp(page, ready) {
  for (let i = 0; i < 100; i++) {
    const f = page.frames().find((fr) => fr !== page.mainFrame() && !fr.url().includes("demo.html"));
    if (f) {
      await f.locator(ready).first().waitFor();
      return f;
    }
    await page.waitForTimeout(100);
  }
  throw new Error("the app never booted inside the stage — is vite running at " + BASE + "?");
}
