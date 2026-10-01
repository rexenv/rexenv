/** The stage's controls, called by the recorder through `window.stage`.
 *  Coordinates are APP pixels (the iframe's own CSS px, what the recorder
 *  reads off `getBoundingClientRect` inside the app); the stage scales them. */
import "@fontsource/inter/500.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/space-grotesk/700.css";
import "@fontsource/jetbrains-mono/500.css";

// `?orient=portrait` makes a 1080×1920 stage (the vertical cut).
const PORTRAIT = new URLSearchParams(location.search).get("orient") === "portrait";
const STAGE = PORTRAIT ? { w: 1080, h: 1920 } : { w: 1920, h: 1080 };
document.documentElement.style.width = document.body.style.width = `${STAGE.w}px`;
document.documentElement.style.height = document.body.style.height = `${STAGE.h}px`;
document.body.classList.toggle("portrait", PORTRAIT);
const APP = { w: 1180, h: 760 };
// The window's size on the stage: big enough to read the UI at 1080p, with a
// margin of background so it reads as a window rather than a screen grab.
// `layout()` changes it: "split" leaves the left of the frame to a headline.
let SCALE = PORTRAIT ? 0.87 : 1.3;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const cam = $("cam");
let unzoomTimer = 0;
const win = $("win");
const cursor = $("cursor");
const caption = $("caption");

// The app inside the window: demo.html with this page's own ?scene=&path=.
($("app") as HTMLIFrameElement).src = `/scripts/video/demo.html${location.search}`;

let winLeft = 0;
let winTop = 0;
function layout(mode: "center" | "split") {
  if (PORTRAIT) {
    SCALE = 0.87;
    winLeft = (STAGE.w - APP.w * SCALE) / 2;
    // Split: the headline block (~300 px) above the window, the pair centred
    // in the frame — a window pinned high left the bottom third empty.
    winTop = mode === "split" ? 815 : (STAGE.h - APP.h * SCALE) / 2;
  } else if (mode === "split") {
    SCALE = 1.1;
    winLeft = STAGE.w - APP.w * SCALE - 56;
    winTop = (STAGE.h - APP.h * SCALE) / 2;
  } else {
    SCALE = 1.3;
    winLeft = (STAGE.w - APP.w * SCALE) / 2;
    winTop = (STAGE.h - APP.h * SCALE) / 2 - 6;
  }
  win.style.left = `${winLeft}px`;
  win.style.top = `${winTop}px`;
  win.style.transform = `scale(${SCALE})`;
}
layout("center");

let cx = APP.w * 0.62;
let cy = APP.h * 0.72;
const place = () => (cursor.style.transform = `translate(${cx - 2}px, ${cy - 2}px)`);
place();

type Rect = { x: number; y: number; w: number; h: number };
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

const stage = {
  ready: () => new Promise<void>((r) => (document.fonts ? document.fonts.ready.then(() => r()) : r())),

  showWindow(on = true) {
    win.classList.toggle("shown", on);
  },

  showCursor(on = true) {
    cursor.style.opacity = on ? "1" : "0";
  },

  /** Glide the cursor to (x, y) in app px along a slight arc, like a hand. */
  moveTo(x: number, y: number, ms = 700) {
    const [x0, y0] = [cx, cy];
    const dist = Math.hypot(x - x0, y - y0);
    const bow = Math.min(60, dist * 0.12);
    const t0 = performance.now();
    return new Promise<void>((done) => {
      const tick = (now: number) => {
        const t = Math.min(1, (now - t0) / ms);
        const e = ease(t);
        const arc = Math.sin(Math.PI * e) * bow;
        cx = x0 + (x - x0) * e;
        cy = y0 + (y - y0) * e - arc;
        place();
        if (t < 1) requestAnimationFrame(tick);
        else done();
      };
      requestAnimationFrame(tick);
    });
  },

  /** Where the cursor's tip is on the page — the recorder clicks HERE, so the
   *  real click lands exactly under the drawn cursor, camera zoom included. */
  tip() {
    const r = cursor.getBoundingClientRect();
    const k = r.width / 22;
    return { x: r.left + 2 * k, y: r.top + 2 * k };
  },

  async press() {
    cursor.classList.add("down");
    const ring = document.createElement("div");
    ring.className = "ripple";
    ring.style.left = `${cx}px`;
    ring.style.top = `${cy}px`;
    win.appendChild(ring);
    setTimeout(() => ring.remove(), 600);
    await sleep(110);
    cursor.classList.remove("down");
  },

  /** `top` keeps the caption off whatever the bottom of the frame is showing. */
  caption(html: string | null, at: "bottom" | "top" = "bottom") {
    if (html) {
      caption.innerHTML = html;
      caption.classList.toggle("top", at === "top");
      caption.classList.add("shown");
    } else {
      caption.classList.remove("shown");
    }
  },

  /** Pulsing rings around app-px rects (one or several); null hides them. */
  spotlight(rects: Rect | Rect[] | null, pad = 6) {
    win.querySelectorAll(".spot").forEach((e) => e.remove());
    if (!rects) return;
    for (const r of Array.isArray(rects) ? rects : [rects]) {
      const spot = document.createElement("div");
      spot.className = "spot";
      Object.assign(spot.style, {
        left: `${r.x - pad}px`,
        top: `${r.y - pad}px`,
        width: `${r.w + 2 * pad}px`,
        height: `${r.h + 2 * pad}px`,
      });
      win.appendChild(spot);
      requestAnimationFrame(() => spot.classList.add("shown"));
    }
  },


  /** Zoom the camera so the app-px rect fills ~`fill` of the stage; null resets. */
  camera(rect: { x: number; y: number; w: number; h: number } | null, fill = 0.72) {
    clearTimeout(unzoomTimer);
    if (!rect) {
      cam.style.transform = "none";
      // The headline returns once the window has nearly shrunk back (the
      // transition is 900 ms); at once, it faded in over the zoomed content.
      unzoomTimer = window.setTimeout(() => document.body.classList.remove("zoomed"), 700);
      return;
    }
    const z = Math.min(2.2, Math.max(1, Math.min((STAGE.w * fill) / (rect.w * SCALE), (STAGE.h * fill) / (rect.h * SCALE))));
    // A zoomed window grows over the headline beside it: step the headline aside.
    document.body.classList.toggle("zoomed", z > 1);
    // A target as wide as the window gets no zoom; in portrait, panning to it
    // anyway slid the window up over the headline.
    if (PORTRAIT && z <= 1) {
      cam.style.transform = "none";
      return;
    }
    const c = { x: winLeft + (rect.x + rect.w / 2) * SCALE, y: winTop + (rect.y + rect.h / 2) * SCALE };
    // Landscape: keep the zoomed stage covering the frame — never pan past an
    // edge. Portrait: the window is a band across the middle of a tall frame,
    // so that clamp pinned zooms to the top and left the bottom half empty;
    // centre the target instead (the backdrop is the body's, it never pans).
    const tx = PORTRAIT ? STAGE.w / 2 - z * c.x : Math.min(0, Math.max(STAGE.w - STAGE.w * z, STAGE.w / 2 - z * c.x));
    const ty = PORTRAIT ? STAGE.h / 2 - z * c.y : Math.min(0, Math.max(STAGE.h - STAGE.h * z, STAGE.h / 2 - z * c.y));
    cam.style.transform = `translate(${tx}px, ${ty}px) scale(${z})`;
  },

  layout,

  /** Swap the app for another scene (`scene=…&path=…`); resolves once loaded. */
  loadScene(search: string) {
    const f = $("app") as HTMLIFrameElement;
    return new Promise<void>((done) => {
      f.addEventListener("load", () => done(), { once: true });
      f.src = `/scripts/video/demo.html?${search}`;
    });
  },

  /** The big headline beside (landscape) or above (portrait) the window. */
  headline(h: string | null, sub = "") {
    const el = $("headline");
    if (!h) {
      el.classList.remove("shown");
      return;
    }
    el.classList.remove("shown");
    requestAnimationFrame(() => {
      ($("hl-h") as HTMLElement).innerHTML = h;
      ($("hl-s") as HTMLElement).innerHTML = sub;
      requestAnimationFrame(() => el.classList.add("shown"));
    });
  },

  title(h: string | null, p = "", fast = false, code = "") {
    const t = $("title");
    t.classList.toggle("fast", fast);
    $("title-c").innerHTML = code;
    if (h) {
      $("title-h").textContent = h;
      $("title-p").textContent = p;
      t.classList.add("shown");
    } else {
      t.classList.remove("shown");
    }
  },
};

/** The terminal window. Lines are HTML (callers pass trusted, fixed text);
 *  `type` animates one command on the current prompt. */
const termBody = $("term-body");
const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const terminal = {
  show(opts: { title?: string; x?: number; y?: number; w?: number; h?: number; font?: number } = {}) {
    const t = $("term");
    if (opts.title) $("term-title").textContent = opts.title;
    if (opts.font !== undefined) t.style.fontSize = `${opts.font}px`;
    if (opts.x !== undefined) t.style.left = `${opts.x}px`;
    if (opts.y !== undefined) t.style.top = `${opts.y}px`;
    if (opts.w !== undefined) t.style.width = `${opts.w}px`;
    if (opts.h !== undefined) t.style.height = `${opts.h}px`;
    t.classList.add("shown");
  },
  hide() {
    $("term").classList.remove("shown");
  },
  clear() {
    termBody.innerHTML = "";
  },
  /** Append lines of HTML, `gap` ms apart. */
  async print(lines: string[], gap = 0) {
    termBody.querySelector(".caret")?.remove();
    for (const l of lines) {
      const d = document.createElement("div");
      d.innerHTML = l || "&nbsp;";
      termBody.appendChild(d);
      termBody.scrollTop = termBody.scrollHeight;
      if (gap) await sleep(gap);
    }
  },
  /** A prompt, then `cmd` typed out at a human pace, then a newline. */
  async type(cmd: string, { prompt = "~ %", perChar = 55 } = {}) {
    termBody.querySelector(".caret")?.remove();
    const line = document.createElement("div");
    line.innerHTML = `<span class="p">${esc(prompt)}</span> `;
    const text = document.createElement("span");
    const caret = document.createElement("span");
    caret.className = "caret";
    line.append(text, caret);
    termBody.appendChild(line);
    await sleep(350);
    for (const ch of cmd) {
      text.textContent += ch;
      termBody.scrollTop = termBody.scrollHeight;
      await sleep(perChar * (0.6 + Math.random() * 0.8));
    }
    await sleep(300);
    caret.remove();
  },
  /** curl's --progress-bar: a row of # filling over `ms`, percent at the end. */
  async bar(ms = 2000, cols = 70) {
    const d = document.createElement("div");
    termBody.appendChild(d);
    const steps = Math.max(10, Math.round(ms / 60));
    for (let i = 1; i <= steps; i++) {
      const f = i / steps;
      const n = Math.round(cols * f);
      d.textContent = `${"#".repeat(n)}${" ".repeat(cols - n)} ${(f * 100).toFixed(1).padStart(5)}%`;
      await sleep(ms / steps);
    }
  },
  /** An idle prompt with a blinking caret. */
  idle(prompt = "~ %") {
    termBody.querySelector(".caret")?.remove();
    const line = document.createElement("div");
    line.innerHTML = `<span class="p">${esc(prompt)}</span> <span class="caret"></span>`;
    termBody.appendChild(line);
  },
};

/** A cursor in STAGE pixels, for what lies outside the app window (the menu
 *  bar): the window's own cursor is clipped to the window. */
const sc = $("scursor");
let sx = 1500;
let sy = 700;
const splace = () => (sc.style.transform = `translate(${sx - 2.6}px, ${sy - 2.6}px)`);
splace();
const scursor = {
  show(on = true) {
    sc.style.opacity = on ? "1" : "0";
  },
  at(x: number, y: number) {
    sx = x;
    sy = y;
    splace();
  },
  moveTo(x: number, y: number, ms = 700) {
    const [x0, y0] = [sx, sy];
    const t0 = performance.now();
    return new Promise<void>((done) => {
      const tick = (now: number) => {
        const t = Math.min(1, (now - t0) / ms);
        const e = ease(t);
        sx = x0 + (x - x0) * e;
        sy = y0 + (y - y0) * e - Math.sin(Math.PI * e) * Math.min(50, Math.hypot(x - x0, y - y0) * 0.1);
        splace();
        if (t < 1) requestAnimationFrame(tick);
        else done();
      };
      requestAnimationFrame(tick);
    });
  },
  async press() {
    sc.classList.add("down");
    await sleep(110);
    sc.classList.remove("down");
  },
};

/** The tray: the menu bar, rexenv's icon, and its menu. Items are strings;
 *  "-" is a separator, a leading "~" greys an item out, "›" marks a submenu. */
const tray = {
  bar(on = true) {
    $("menubar").classList.toggle("shown", on);
  },
  open(items: string[], sub?: { at: number; items: string[] }) {
    document.querySelectorAll(".traymenu").forEach((e) => e.remove());
    const icon = $("trayicon");
    icon.classList.add("on");
    const r = icon.getBoundingClientRect();
    const menu = (list: string[], left: number, top: number) => {
      const m = document.createElement("div");
      m.className = "traymenu";
      m.style.left = `${left}px`;
      m.style.top = `${top}px`;
      for (const it of list) {
        if (it === "-") {
          m.appendChild(document.createElement("hr"));
          continue;
        }
        const d = document.createElement("div");
        d.className = "i" + (it.startsWith("~") ? " dis" : "");
        const [label, right] = it.replace(/^~/, "").split("\t");
        d.innerHTML = `<span>${label}</span><span>${right ?? ""}</span>`;
        m.appendChild(d);
      }
      document.body.appendChild(m);
      return m;
    };
    const main = menu(items, Math.min(r.left - 10, 1920 - 350), r.bottom + 4);
    if (sub) {
      const row = main.querySelectorAll("div.i")[sub.at] as HTMLElement;
      const rr = row.getBoundingClientRect();
      menu(sub.items, main.getBoundingClientRect().left - 346, rr.top - 8);
    }
  },
  /** Highlight item `i` of menu `m` (0 = main, 1 = submenu); returns its centre. */
  hl(i: number, m = 0) {
    const menus = document.querySelectorAll(".traymenu");
    menus.forEach((x) => x.querySelectorAll("div.i").forEach((d) => d.classList.remove("hl")));
    const d = menus[m]?.querySelectorAll("div.i")[i] as HTMLElement | undefined;
    if (!d) return null;
    d.classList.add("hl");
    const r = d.getBoundingClientRect();
    return { x: r.left + r.width * 0.35, y: r.top + r.height / 2 };
  },
  close() {
    document.querySelectorAll(".traymenu").forEach((e) => e.remove());
    $("trayicon").classList.remove("on");
  },
  iconAt() {
    const r = $("trayicon").getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  },
  closeButtonAt() {
    const r = ($("lights").firstElementChild as HTMLElement).getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  },
};

(window as unknown as { stage: typeof stage & { terminal: typeof terminal } }).stage = Object.assign(stage, { terminal, scursor, tray });
