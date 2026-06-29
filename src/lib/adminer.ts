/**
 * Helpers for the embedded Adminer database browser (§5.2). Adminer is served by
 * the backend on a fixed internal host; the UI just builds pre-filled login URLs.
 */

/** Internal host Adminer is served on (mirrors `core::adminer::ADMINER_HOST`). */
export const ADMINER_HOST = "adminer.rexenv.test";

// Canonical fixed loopback DB ports (see CLAUDE.md / core::db). Not user-configurable.
const MYSQL_PORT = 13306;
const POSTGRES_PORT = 15432;

/** Build a per-site Adminer deep-link for an engine (+ optional database). By
 *  default it requests a one-click scoped session (`rexenv_auto`) so the backend
 *  wrapper auto-logs-in and lands straight in the DB (§11.4); pass
 *  `autoLogin: false` for a plain pre-filled login form. */
export function adminerUrl(opts: {
  engine: "mysql" | "postgres";
  db?: string;
  autoLogin?: boolean;
}): string {
  const params = new URLSearchParams();
  if (opts.engine === "postgres") {
    params.set("pgsql", `127.0.0.1:${POSTGRES_PORT}`);
    params.set("username", "postgres");
  } else {
    params.set("server", `127.0.0.1:${MYSQL_PORT}`);
    params.set("username", "root");
  }
  if (opts.db) params.set("db", opts.db);
  // Scoped one-click session by default (the wrapper auto-submits Adminer's own
  // CSRF-tokened form); opt out with autoLogin:false.
  if (opts.autoLogin !== false) params.set("rexenv_auto", "1");
  return `https://${ADMINER_HOST}/?${params.toString()}`;
}

/** A site's MySQL database name (mirrors `core::wordpress::db_name_for`). */
export function siteDbName(domain: string): string {
  return "wp_" + domain.replace(/[^a-zA-Z0-9]/g, "_");
}
