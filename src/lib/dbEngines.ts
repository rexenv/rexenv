/**
 * What a site's database engine is CALLED, where it listens, and who connects
 * to it — one table, because these three move together and the UI had been
 * carrying them as `dbEngine === "mariadb" ? … : …` ternaries in five places.
 *
 * That shape is only correct while there are two engines. PostgreSQL made it
 * wrong everywhere at once, and wrong in the quiet direction: a PG-backed site
 * read as "MySQL · 127.0.0.1:13306", which is a sentence a developer would copy
 * into their config and then debug for an hour.
 *
 * Ports mirror `core::db` (CLAUDE.md: fixed, not user-configurable), and the
 * usernames are the local-dev superusers each engine's datadir is initialized
 * with — the same values `starter::StarterDb::for_engine` and
 * `laravel::DbSettings::for_engine` write into generated files.
 */
import type { SiteDbEngine } from "@/types";

interface EngineFacts {
  /** Display name, as the engine's own project spells it. */
  label: string;
  /** Fixed loopback port. */
  port: number;
  /** The superuser a local-dev connection uses. */
  user: string;
  /** Laravel's `DB_CONNECTION` value / PDO's DSN prefix. */
  driver: "mysql" | "pgsql";
}

const ENGINES: Record<SiteDbEngine, EngineFacts> = {
  mysql: { label: "MySQL", port: 13306, user: "root", driver: "mysql" },
  // MariaDB speaks the MySQL protocol and Laravel has no separate driver for it
  // before 11.x — only the port differs.
  mariadb: { label: "MariaDB", port: 13307, user: "root", driver: "mysql" },
  postgres: { label: "PostgreSQL", port: 15432, user: "postgres", driver: "pgsql" },
};

export function engineFacts(engine: SiteDbEngine): EngineFacts {
  return ENGINES[engine] ?? ENGINES.mysql;
}

export function engineLabel(engine: SiteDbEngine): string {
  return engineFacts(engine).label;
}

export function enginePort(engine: SiteDbEngine): number {
  return engineFacts(engine).port;
}
