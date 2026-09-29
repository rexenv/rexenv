/** The stage's controls, called by the recorder through `window.stage`.
 *  Coordinates are APP pixels (the iframe's own CSS px, what the recorder
 *  reads off `getBoundingClientRect` inside the app); the stage scales them. */
import "@fontsource/inter/500.css";
import "@fontsource/space-grotesk/600.css";
import "@fontsource/space-grotesk/700.css";
import "@fontsource/jetbrains-mono/500.css";

const STAGE = { w: 1920, h: 1080 };
const APP = { w: 1180, h: 760 };
// The window's size on the stage: big enough to read the UI at 1080p, with a
// margin of background so it reads as a window rather than a screen grab.
const SCALE = 1.3;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const cam = $("cam");
const win = $("win");
const cursor = $("cursor");
const caption = $("caption");

const winLeft = (STAGE.w - APP.w * SCALE) / 2;
const winTop = (STAGE.h - APP.h * SCALE) / 2 - 6;
win.style.left = `${winLeft}px`;
win.style.top = `${winTop}px`;
win.style.transform = `scale(${SCALE})`;

let cx = APP.w * 0.62;
let cy = APP.h * 0.72;
const place = () => (cursor.style.transform = `translate(${cx - 2}px, ${cy - 2}px)`);
place();

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

const stage = {
  ready: () => new Promise<void>((r) => (document.fonts ? document.fonts.ready.then(() => r()) : r())),

  showWindow() {
    win.classList.add("shown");
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

  /** A pulsing ring around an app-px rect; null hides it. */
  spotlight(rect: { x: number; y: number; w: number; h: number } | null, pad = 6) {
    const spot = $("spot");
    if (!rect) {
      spot.classList.remove("shown");
      return;
    }
    Object.assign(spot.style, {
      left: `${rect.x - pad}px`,
      top: `${rect.y - pad}px`,
      width: `${rect.w + 2 * pad}px`,
      height: `${rect.h + 2 * pad}px`,
    });
    spot.classList.add("shown");
  },

  /** Zoom the camera so the app-px rect fills ~`fill` of the stage; null resets. */
  camera(rect: { x: number; y: number; w: number; h: number } | null, fill = 0.72) {
    if (!rect) {
      cam.style.transform = "none";
      return;
    }
    const z = Math.min(2.2, Math.max(1, Math.min((STAGE.w * fill) / (rect.w * SCALE), (STAGE.h * fill) / (rect.h * SCALE))));
    const c = { x: winLeft + (rect.x + rect.w / 2) * SCALE, y: winTop + (rect.y + rect.h / 2) * SCALE };
    // Keep the zoomed stage covering the frame — never pan past an edge.
    const tx = Math.min(0, Math.max(STAGE.w - STAGE.w * z, STAGE.w / 2 - z * c.x));
    const ty = Math.min(0, Math.max(STAGE.h - STAGE.h * z, STAGE.h / 2 - z * c.y));
    cam.style.transform = `translate(${tx}px, ${ty}px) scale(${z})`;
  },

  title(h: string | null, p = "") {
    const t = $("title");
    if (h) {
      $("title-h").textContent = h;
      $("title-p").textContent = p;
      t.classList.add("shown");
    } else {
      t.classList.remove("shown");
    }
  },
};

(window as unknown as { stage: typeof stage }).stage = stage;
