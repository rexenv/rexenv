/**
 * Helpers for the embedded Adminer database browser (§5.2). Adminer is served by
 * the backend on a fixed internal host; the UI just builds pre-filled login URLs.
 */

/** Internal host Adminer is served on (mirrors `core::adminer::ADMINER_HOST`) —
 *  on the .rex backbone TLD, whose resolver onboarding installs. */
export const ADMINER_HOST = "adminer.rexenv.rex";

// Canonical fixed loopback DB ports (see CLAUDE.md / core::db). Not user-configurable.
const MYSQL_PORT = 13306;
const MARIADB_PORT = 13307;
const POSTGRES_PORT = 15432;

/** Build a per-site Adminer deep-link for an engine (+ optional database). By
 *  default it requests a one-click scoped session (`rexenv_auto`) so the backend
 *  wrapper auto-logs-in and lands straight in the DB (§11.4); pass
 *  `autoLogin: false` for a plain pre-filled login form. */
interface AdminerTarget {
  engine: "mysql" | "mariadb" | "postgres";
  db?: string;
  autoLogin?: boolean;
}

function adminerQuery(opts: AdminerTarget): string {
  const params = new URLSearchParams();
  if (opts.engine === "postgres") {
    params.set("pgsql", `127.0.0.1:${POSTGRES_PORT}`);
    params.set("username", "postgres");
  } else {
    // MariaDB speaks the MySQL protocol — same Adminer driver, its own port.
    const port = opts.engine === "mariadb" ? MARIADB_PORT : MYSQL_PORT;
    params.set("server", `127.0.0.1:${port}`);
    params.set("username", "root");
  }
  if (opts.db) params.set("db", opts.db);
  // Scoped one-click session by default (the wrapper auto-submits Adminer's own
  // CSRF-tokened form); opt out with autoLogin:false.
  if (opts.autoLogin !== false) params.set("rexenv_auto", "1");
  return params.toString();
}

/** Adminer deep-link for an EXTERNAL browser (first-party page → cookies work). */
export function adminerUrl(opts: AdminerTarget): string {
  return `https://${ADMINER_HOST}/?${adminerQuery(opts)}`;
}

/** Adminer src for the IN-APP `<iframe>`. Goes through the `rexdb://` custom
 *  protocol (Rust-side cookie jar) because WebKit withholds third-party cookies
 *  in cross-site iframes — a direct `https://` src loses the session on the
 *  login POST and every login bounces back to the form. (Windows webviews use
 *  `http://rexdb.localhost/` for custom schemes — Phase 4.) */
export function adminerFrameSrc(opts: AdminerTarget): string {
  return `rexdb://localhost/?${adminerQuery(opts)}`;
}

// NOTE: a site's MySQL database name must come from `site.dbName` (stored at
// creation) — never derive it from the domain, which can change (§1.4).
