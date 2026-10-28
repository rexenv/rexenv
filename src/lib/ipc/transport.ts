/**
 * Which backend this page talks to (docs/PLAN-headless-web.md T6).
 *
 * - `tauri` — inside the app's own window: Tauri's IPC bridge.
 * - `web`   — served by `rexenv --web` (`web_server.rs`): the SAME commands, POSTed to
 *             `/ipc/<cmd>`, and the same events over one server-sent event stream. The server
 *             marks the page it serves with `<meta name="rexenv-transport" content="web">`
 *             (`web_rules::TRANSPORT_META`) — a meta and not a script, so the page's CSP stays
 *             without inline script.
 * - `none`  — a plain `vite dev` page, a review route, a wk-check: the ipc wrappers' mocks.
 *
 * Components never look at this: they call the wrappers in `./index.ts`, which is the rule
 * ("UI never calls raw `invoke`") that let a second transport land in one file.
 */
export type Transport = "tauri" | "web" | "none";

export function transport(): Transport {
  if (typeof window === "undefined") return "none";
  if ("__TAURI_INTERNALS__" in window) return "tauri";
  if (document.querySelector('meta[name="rexenv-transport"][content="web"]')) return "web";
  return "none";
}

/** Run one command over HTTP. Resolves with its value, rejects with its error — exactly what
 *  Tauri's `invoke` does with the same command's `Result`. */
export async function webInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const res = await fetch(`/ipc/${encodeURIComponent(cmd)}`, {
    method: "POST",
    credentials: "same-origin",
    headers: webHeaders({ "content-type": "application/json" }),
    body: JSON.stringify(args ?? {}),
  });
  let reply: { ok: boolean; data?: T; error?: unknown };
  try {
    reply = await res.json();
  } catch {
    throw `rexenv's web server answered ${res.status} with no JSON`;
  }
  if (!reply.ok) throw reply.error;
  return reply.data as T;
}

/** Headers every web request carries. T7 adds the CSRF header here, once. */
export function webHeaders(extra: Record<string, string> = {}): Record<string, string> {
  return { ...extra };
}

/** The shape Tauri's `listen` hands a callback — kept so the wrappers do not change. */
export interface WebEvent<T> {
  event: string;
  id: number;
  payload: T;
}

type Handler = (e: WebEvent<unknown>) => void;
interface Registration {
  event: string;
  handler: Handler;
  /** The server's id for this listener on the CURRENT stream, once registered. */
  serverId: number | null;
}

let source: EventSource | null = null;
let streamId: string | null = null;
let ready: Promise<void> | null = null;
const registrations = new Set<Registration>();

/** Open the one stream, or reuse it. A reconnect gets a NEW stream id from the server, and every
 *  listener is registered again on it — the old stream's listeners died with it server-side. */
function connect(): Promise<void> {
  if (ready) return ready;
  ready = new Promise<void>((resolve) => {
    source = new EventSource("/events", { withCredentials: true });
    source.addEventListener("hello", (e) => {
      streamId = (JSON.parse((e as MessageEvent).data) as { stream: string }).stream;
      for (const r of registrations) {
        r.serverId = null;
        void register(r);
      }
      resolve();
    });
    source.addEventListener("message", (e) => {
      const msg = JSON.parse((e as MessageEvent).data) as WebEvent<unknown>;
      for (const r of registrations) {
        if (r.serverId === msg.id) r.handler(msg);
      }
    });
  });
  return ready;
}

async function post(path: string, body: unknown): Promise<{ ok: boolean; id?: number; error?: string }> {
  const res = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: webHeaders({ "content-type": "application/json" }),
    body: JSON.stringify(body),
  });
  return res.json();
}

async function register(r: Registration): Promise<void> {
  if (!streamId) return;
  const reply = await post("/events/listen", { stream: streamId, event: r.event });
  if (reply.ok && typeof reply.id === "number" && registrations.has(r)) r.serverId = reply.id;
}

/** `listen` over the web transport: the same event names the window listens to. */
export async function webListen<T>(event: string, handler: (e: WebEvent<T>) => void): Promise<() => void> {
  const r: Registration = { event, handler: handler as Handler, serverId: null };
  registrations.add(r);
  await connect();
  await register(r);
  return () => {
    registrations.delete(r);
    if (r.serverId !== null && streamId) void post("/events/unlisten", { stream: streamId, id: r.serverId });
  };
}
