/**
 * ONE place that answers "which console, and at what URL" for an engine, so no screen
 * switches on the console name itself (`docs/PLAN-db-console-open-and-phpmyadmin.md`
 * §3.2). The choice is the backend's effective answer (`db_console_status`: phpMyAdmin
 * only when chosen AND staged); PostgreSQL always gets Adminer — phpMyAdmin has no
 * driver for it — whatever the setting says.
 */
import { useQuery } from "@tanstack/react-query";
import { dbConsoleStatus } from "@/lib/ipc";
import { adminerFrameSrc, adminerUrl } from "@/lib/adminer";
import { phpmyadminFrameSrc, phpmyadminUrl } from "@/lib/phpmyadmin";
import type { DbConsole, DbConsoleStatus, PlatformWords } from "@/types";

export type ConsoleEngine = "mysql" | "mariadb" | "postgres";

export const DB_CONSOLE_QUERY = ["db-console"] as const;

/** The console row, cached; `undefined` until the first answer. */
export function useDbConsole(): DbConsoleStatus | undefined {
  const { data } = useQuery({ queryKey: DB_CONSOLE_QUERY, queryFn: dbConsoleStatus, staleTime: 60_000 });
  return data ?? undefined;
}

/** The console an engine opens in under `setting`. Postgres is pinned to Adminer. */
export function consoleFor(engine: ConsoleEngine, setting: DbConsole | undefined): DbConsole {
  if (engine === "postgres") return "adminer";
  return setting ?? "adminer";
}

export const CONSOLE_LABEL: Record<DbConsole, string> = { adminer: "Adminer", phpmyadmin: "phpMyAdmin" };

/** First-party https deep link for a real browser. */
export function consoleUrl(engine: ConsoleEngine, setting: DbConsole | undefined, db?: string): string {
  if (consoleFor(engine, setting) === "phpmyadmin" && engine !== "postgres") {
    return phpmyadminUrl({ engine, db });
  }
  return adminerUrl({ engine, db });
}

/** The in-app frame src, through the console's own proxy origin — both from the
 *  platform's words (#703). */
export function consoleFrameSrc(
  engine: ConsoleEngine,
  setting: DbConsole | undefined,
  words: Pick<PlatformWords, "dbBrowserOrigin" | "phpmyadminOrigin">,
  db?: string,
): string {
  if (consoleFor(engine, setting) === "phpmyadmin" && engine !== "postgres") {
    return phpmyadminFrameSrc({ engine, db }, words.phpmyadminOrigin);
  }
  return adminerFrameSrc({ engine, db }, words.dbBrowserOrigin);
}
