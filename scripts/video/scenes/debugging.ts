/** Debugging Agency Blog: Xdebug per site (`set_site_xdebug` — the site moves
 *  to a debug pool, mode debug,develop, IDE on 9003), WP_DEBUG from Tools
 *  (on = WP_DEBUG + WP_DEBUG_LOG true, WP_DEBUG_DISPLAY false —
 *  `core/wordpress.rs`), the Logs tab's debug.log (polled every 2 s), and the
 *  Terminal tab: a shell in the site folder with the bundled php + wp on PATH,
 *  fed as raw bytes on `terminal://output/<id>` (`commands/terminal.rs`). */
import type { SceneCtx } from "../demo-backend";
import { wpHandlers } from "../wp-fixtures";

type Args = Record<string, unknown> | undefined;

const PLUGIN = "/Users/demo/Sites/agency-blog/wp-content/plugins/agency-tweaks/agency-tweaks.php";
const LOG = [
  `[30-Sep-2026 05:12:41 UTC] PHP Notice:  Function _load_textdomain_just_in_time was called <strong>incorrectly</strong>. Translation loading for the <code>agency-tweaks</code> domain was triggered too early. in /Users/demo/Sites/agency-blog/wp-includes/functions.php on line 6131`,
  `[30-Sep-2026 05:12:44 UTC] PHP Warning:  Undefined array key "price" in ${PLUGIN} on line 27`,
  `[30-Sep-2026 05:13:02 UTC] PHP Deprecated:  strlen(): Passing null to parameter #1 ($string) of type string is deprecated in ${PLUGIN} on line 41`,
];
const LATE = `[30-Sep-2026 05:13:19 UTC] PHP Fatal error:  Uncaught Error: Call to undefined function agency_price() in ${PLUGIN}:58`;

const PROMPT = "demo@MacBook-Pro agency-blog % ";
const PLUGIN_TABLE = [
  "+----------------+----------+-----------+---------+----------------+-------------+",
  "| name           | status   | update    | version | update_version | auto_update |",
  "+----------------+----------+-----------+---------+----------------+-------------+",
  "| akismet        | active   | none      | 5.5     |                | off         |",
  "| contact-form-7 | active   | none      | 6.1.2   |                | off         |",
  "| wordpress-seo  | active   | available | 26.1    | 26.2           | off         |",
  "| hello-dolly    | inactive | none      | 1.7.2   |                | off         |",
  "+----------------+----------+-----------+---------+----------------+-------------+",
];
const REPLIES: Record<string, string[]> = {
  "wp --version": ["WP-CLI 2.12.0"],
  "wp plugin list": PLUGIN_TABLE,
};

export default function debugging(ctx: SceneCtx) {
  // `?wpdebug=1`: WP_DEBUG already on (the tour shows the log without the Tools step).
  let wpDebug = new URLSearchParams(location.search).has("wpdebug");
  const log = [...LOG];
  const enc = new TextEncoder();
  const out = (id: string, s: string) => void ctx.emit(`terminal://output/${id}`, Array.from(enc.encode(s)));
  const typed: Record<string, string> = {};

  (window as unknown as { __scene: object }).__scene = {
    logMore() {
      log.push(LATE);
    },
  };

  return {
    ...wpHandlers(ctx),
    site_cert_info: () => ({
      notBefore: "2026-08-21 09:30:00",
      notAfter: "2027-09-23 09:30:00",
      daysLeft: 358,
      sans: ["agency-blog.rex", "*.agency-blog.rex"],
      certDir: "/Users/demo/Library/Application Support/dev.rexenv.rexenv/certs/agency-blog.rex",
    }),
    site_domains: (a: Args) => [ctx.sites.find((x) => x.id === a?.id)?.domain],
    list_site_env: () => [],
    set_site_xdebug: async (a: Args) => {
      await ctx.sleep(1500);
      const s = ctx.sites.find((x) => x.id === a?.id)!;
      s.xdebug = Boolean(a?.enabled);
      return s;
    },
    wp_debug_get: () => wpDebug,
    wp_debug_set: async (a: Args) => {
      await ctx.sleep(900);
      wpDebug = Boolean(a?.on);
      return null;
    },
    wp_debug_flag_get: (a: Args) => (a?.name === "WP_DEBUG_LOG" ? wpDebug : false),
    wp_debug_log_status: () => ({
      debug: wpDebug,
      logEnabled: wpDebug,
      path: "/Users/demo/Sites/agency-blog/wp-content/debug.log",
      exists: wpDebug,
      sizeBytes: log.join("\n").length,
      indeterminate: false,
    }),
    wp_debug_log_tail: () => (wpDebug ? log : []),
    terminal_open: () => {
      const id = "term-demo-1";
      typed[id] = "";
      setTimeout(() => out(id, PROMPT), 300);
      return id;
    },
    terminal_write: (a: Args) => {
      const id = String(a?.id);
      const data = String(a?.data);
      for (const ch of data) {
        if (ch === "\r") {
          const cmd = typed[id].trim();
          typed[id] = "";
          const reply = REPLIES[cmd] ?? [`zsh: command not found: ${cmd}`];
          out(id, "\r\n" + reply.join("\r\n") + "\r\n" + PROMPT);
        } else if (ch === "\u007f") {
          typed[id] = typed[id].slice(0, -1);
          out(id, "\b \b");
        } else {
          typed[id] += ch;
          out(id, ch);
        }
      }
      return null;
    },
    terminal_resize: () => null,
    terminal_close: () => null,
  };
}
