/**
 * Helpers for phpMyAdmin, the second database console (mirrors `src/lib/adminer.ts`).
 * phpMyAdmin is served by the backend on a fixed internal host with a GENERATED config
 * whose servers are rexenv's two loopback engines (`auth_type = config`, so there is no
 * login form and no host field — the passwordless gate, ledger #864). MySQL and
 * MariaDB only: phpMyAdmin has no PostgreSQL driver.
 */

/** Internal host phpMyAdmin is served on (mirrors `core::phpmyadmin::PHPMYADMIN_HOST`). */
export const PHPMYADMIN_HOST = "phpmyadmin.rexenv.rex";

/** `$cfg['Servers'][1]` is MySQL, `[2]` MariaDB — the generator's contract
 *  (`core::phpmyadmin::SERVER_MYSQL` / `SERVER_MARIADB`). */
const SERVER_INDEX = { mysql: 1, mariadb: 2 } as const;

export interface PhpMyAdminTarget {
  engine: "mysql" | "mariadb";
  db?: string;
}

function phpmyadminQuery(opts: PhpMyAdminTarget): string {
  const params = new URLSearchParams();
  params.set("server", String(SERVER_INDEX[opts.engine]));
  if (opts.db) {
    params.set("route", "/database/structure");
    params.set("db", opts.db);
  }
  return params.toString();
}

/** phpMyAdmin deep link for an EXTERNAL browser (first-party page). */
export function phpmyadminUrl(opts: PhpMyAdminTarget): string {
  return `https://${PHPMYADMIN_HOST}/index.php?${phpmyadminQuery(opts)}`;
}

/** phpMyAdmin src for the IN-APP `<iframe>`, through its OWN proxy origin
 *  (`PlatformWords.phpmyadminOrigin`: `rexpma://localhost` on macOS/Linux,
 *  `http://rexpma.localhost` on Windows) — it cannot share Adminer's, because its
 *  pages use absolute paths. The origin is the platform's, never spelled here (#703). */
export function phpmyadminFrameSrc(opts: PhpMyAdminTarget, origin: string): string {
  return `${origin}/index.php?${phpmyadminQuery(opts)}`;
}
