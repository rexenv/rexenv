/**
 * Helpers for the embedded Adminer database browser (§5.2). Adminer is served by
 * the backend on a fixed internal host; the UI just builds pre-filled login URLs.
 */

/** Internal host Adminer is served on (mirrors `core::adminer::ADMINER_HOST`). */
export const ADMINER_HOST = "adminer.rexenv.test";

// Canonical fixed loopback DB ports (see CLAUDE.md / core::db). Not user-configurable.
const MYSQL_PORT = 13306;
const POSTGRES_PORT = 15432;

/** Build a pre-filled Adminer URL for an engine (+ optional database). */
export function adminerUrl(opts: { engine: "mysql" | "postgres"; db?: string }): string {
  const params = new URLSearchParams();
  if (opts.engine === "postgres") {
    params.set("pgsql", `127.0.0.1:${POSTGRES_PORT}`);
    params.set("username", "postgres");
  } else {
    params.set("server", `127.0.0.1:${MYSQL_PORT}`);
    params.set("username", "root");
  }
  if (opts.db) params.set("db", opts.db);
  return `https://${ADMINER_HOST}/?${params.toString()}`;
}

/** A site's MySQL database name (mirrors `core::wordpress::db_name_for`). */
export function siteDbName(domain: string): string {
  return "wp_" + domain.replace(/[^a-zA-Z0-9]/g, "_");
}
