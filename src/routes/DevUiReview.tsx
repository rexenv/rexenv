/** DEV-ONLY WebKit render harness for the UI review (`#/dev/ui-review`).
 *  Mounted only when `import.meta.env.DEV` (see App.tsx) — never part of a
 *  production bundle. Mocks the Tauri IPC layer (`mockIPC`) with canned
 *  responses so the Stage 2/3 surfaces — including the ones that have NEVER
 *  rendered anywhere (the connected-delete dialog variants, verifyFailed,
 *  the borrowed-resolver row) — can be screenshotted in Playwright WebKit
 *  with zero backend and zero contact with the real app.
 *
 *  Views (`?view=…`):
 *    card     — DbImportCard.   `rec=imported|connected|connectedHttp`,
 *               `preview=ready|refused|noop`, `root=1`, `cache=1`,
 *               `backup=1`, `engine=mariadb`,
 *               `apply=fileChanged|engineStopped|verifyFailed|applied`,
 *               `revert=refusedEdited|backupMissing|reverted`
 *    delete   — DeleteSiteDialog. `kind=connected|preexisting|wp|imported|linked`
 *    badges   — a column of SiteRow variants (badge crowding at narrow widths)
 *    resolver — the borrowed-resolver hand-back row (Settings card context)
 *    toast    — the long revert-then-delete refusal toast + friends
 */
import { useEffect, useState } from "react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { DbImportCard } from "@/components/sites/DbImportCard";
import { DeleteSiteDialog } from "@/components/sites/DeleteSiteDialog";
import { SiteRow } from "@/routes/Sites";
import { ResolverHandBackRow } from "@/routes/Import";
import { toast } from "@/lib/toast";
import type { DbImportRecord, ResolverTldStatus, RewriteApplied, RewritePreview, RewriteRevertOutcome, Site } from "@/types";

const params = new URLSearchParams(window.location.search);

function fixtureSite(over: Partial<Site> = {}): Site {
  return {
    id: "s-ea",
    name: "ea",
    domain: "ea.test",
    type: "wordpress",
    status: "running",
    phpVersion: "8.3",
    webServer: "nginx",
    ssl: true,
    path: "/Users/wpdev/code/ea",
    createdAt: "2026-07-26 00:00:00",
    multisite: "none",
    dbName: "ea",
    dbEngine: "mysql",
    xdebug: false,
    provisioned: true,
    docrootManaged: false,
    dbCreated: true,
    ...over,
  };
}

const RECORD_BASE = {
  siteId: "s-ea",
  dbName: "ea",
  tableCount: 48,
  sizeBytes: 24 * 1024 * 1024,
  sourceLabel: "MySQL 8.0.27 at 127.0.0.1:3306",
  mirroredUser: null as string | null,
  importedAt: "2026-07-27 10:00:00",
};

function record(): DbImportRecord | null {
  switch (params.get("rec")) {
    case "imported":
      return { ...RECORD_BASE, state: "imported" };
    case "connected":
      return { ...RECORD_BASE, mirroredUser: "rex_ea_test", state: "connected", verified: "signin" };
    case "connectedHttp":
      return { ...RECORD_BASE, mirroredUser: "rex_ea_test", state: "connected", verified: "signin+http" };
    default:
      return null;
  }
}

const WP_DIFF = [
  { sign: "-", line: 4, text: "define( 'DB_HOST', '127.0.0.1' );" },
  { sign: "+", line: 4, text: "define( 'DB_HOST', '127.0.0.1:13306' );" },
  { sign: "-", line: 3, text: "define( 'DB_USER', 'root' );" },
  { sign: "+", line: 3, text: "define( 'DB_USER', 'rex_ea_test' );" },
];

function preview(): RewritePreview {
  if (params.get("preview") === "refused") {
    return {
      status: "refused",
      reason:
        "DB_HOST is set more than once (lines 40 and 61), so rexenv can't tell which one this site actually uses.",
      file: "/Users/wpdev/code/ea/wp-config.php",
    };
  }
  return {
    status: "ready",
    file: "/Users/wpdev/code/ea/wp-config.php",
    diff: params.get("preview") === "noop" ? [] : WP_DIFF,
    fingerprint: "f".repeat(64),
    createsUser: params.get("root") === "1" ? "rex_ea_test" : null,
    backupExists: params.get("backup") === "1",
    laravelCacheWarning: params.get("cache") === "1",
    target: "127.0.0.1:13306",
  };
}

function applied(): RewriteApplied {
  switch (params.get("apply")) {
    case "fileChanged":
      return {
        status: "fileChanged",
        message:
          "/Users/wpdev/code/ea/wp-config.php changed since the diff was shown — nothing was written. Re-open the preview to see the current change.",
      };
    case "engineStopped":
      return {
        status: "engineStopped",
        message:
          "this change points ea.test at rexenv's own MySQL (127.0.0.1:13306), which isn't running — start it from the Databases page, then apply again.",
      };
    case "verifyFailed":
      return {
        status: "verifyFailed",
        reason:
          "the server refused the file's own credentials (Access denied for user 'rex_ea_test'@'localhost') — the site isn't marked connected until a sign-in succeeds.",
        message:
          "the change was applied and backed up, but the sign-in check didn't pass — ea.test is not marked connected. You can revert the change from this card.",
      };
    default:
      return {
        status: "applied",
        record: { ...RECORD_BASE, mirroredUser: "rex_ea_test", state: "connected", verified: "signin+http" },
        message: "verified: the rewritten settings sign in to the rexenv copy of `ea`.",
      };
  }
}

function reverted(): RewriteRevertOutcome {
  switch (params.get("revert")) {
    case "refusedEdited":
      return {
        status: "refusedEdited",
        file: "/Users/wpdev/code/ea/wp-config.php",
        reason: "editedSinceRewrite",
        message:
          "/Users/wpdev/code/ea/wp-config.php was edited after the rewrite — restoring the backup would replace those edits. Choose \"restore anyway\" to proceed.",
      };
    case "backupMissing":
      return {
        status: "backupMissing",
        file: "/Users/wpdev/code/ea/wp-config.php",
        message:
          "rexenv's copy of the original is gone; your file was left exactly as it is. To go back to the old database, edit /Users/wpdev/code/ea/wp-config.php yourself.",
      };
    default:
      return {
        status: "reverted",
        file: "/Users/wpdev/code/ea/wp-config.php",
        message:
          "/Users/wpdev/code/ea/wp-config.php was restored to the original, byte for byte; the site is back on its previous connection settings.",
      };
  }
}

const BORROWED: ResolverTldStatus = {
  tld: "test",
  owner: "borrowed",
  path: "/etc/resolver/test",
  theirContent: "nameserver 127.0.0.1\n",
  ourContent: "nameserver 127.0.0.1\nport 15353\n",
  rexenvSites: 7,
};

function DeleteView() {
  const kind = params.get("kind") ?? "connected";
  // `long=1`: the worst realistic case — the longest domain on the real
  // machine's Valet tree plus a collision-suffixed db name and a deep path.
  const long = params.get("long") === "1";
  const longOver: Partial<Site> = long
    ? {
        name: "storeware-reviews-staging",
        domain: "storeware-reviews-staging.test",
        dbName: "wp_storeware_reviews_staging_9a1b2c3d",
        path: "/Users/wpdev/Projects/clients/storeware/storeware-reviews-staging",
      }
    : {};
  const site =
    kind === "preexisting"
      ? fixtureSite({ dbCreated: false })
      : kind === "wp"
        ? fixtureSite({ docrootManaged: true, dbCreated: null })
        : kind === "imported"
          ? fixtureSite({ type: "laravel", dbCreated: true, dbName: "lms" })
          : kind === "linked"
            ? fixtureSite({ dbCreated: null })
            : fixtureSite(longOver);
  const dbState = kind === "connected" || kind === "preexisting" ? ("connected" as const) : undefined;
  return (
    <DeleteSiteDialog
      site={site}
      dbState={dbState}
      onPlainDelete={() => {}}
      onRevertThenDelete={() => {}}
      onCancel={() => {}}
    />
  );
}

function BadgesView() {
  const noop = () => {};
  const rows: Array<{ site: Site; dbState?: DbImportRecord["state"] }> = [
    { site: fixtureSite({ name: "plain", domain: "plain.rex", docrootManaged: true }) },
    { site: fixtureSite({ name: "external", domain: "linked.test" }) },
    { site: fixtureSite({ name: "imported-db", domain: "lms.test" }), dbState: "imported" },
    { site: fixtureSite({ name: "connected-db", domain: "ea.test" }), dbState: "connected" },
    {
      site: fixtureSite({ name: "half", domain: "half.test", provisioned: false }),
      dbState: "imported",
    },
  ];
  return (
    <div className="space-y-2">
      {rows.map((r) => (
        <SiteRow
          key={r.site.domain}
          site={r.site}
          status={r.site.status}
          dbState={r.dbState}
          onOpen={noop}
          onDelete={noop}
          onOpenDatabase={noop}
          onOpenWordpress={noop}
          onRename={noop}
          onDuplicate={noop}
          onRetry={noop}
        />
      ))}
    </div>
  );
}

function ToastView() {
  useEffect(() => {
    toast.info(
      "Not deleted — /Users/wpdev/code/lms/wp-config.php was edited after the rewrite — restoring the backup would replace those edits. Choose \"restore anyway\" to proceed. Resolve it on the site's Database tab, or delete without reverting.",
    );
    toast.success("verified: the rewritten settings sign in to the rexenv copy of `ea`.");
    toast.error(
      "the edge is running, but Herd answers port 443 in front of it — every site is unreachable until you quit Herd",
      "osascript -e 'quit app \"Herd\"'",
    );
  }, []);
  return <p className="text-xs text-rex-text-muted">toasts pushed — see overlay.</p>;
}

export function DevUiReview() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "db_import_record":
          return record();
        case "db_import_state":
          return null;
        case "rewrite_preview":
          return preview();
        case "rewrite_apply":
          return applied();
        case "rewrite_revert":
          return reverted();
        case "list_editors":
          return [];
        case "get_setting":
          return null;
        default:
          return 1; // plugin:event|listen etc. — accept quietly.
      }
    });
    setReady(true);
  }, []);
  if (!ready) return null;

  const view = params.get("view") ?? "card";
  return (
    <div className="min-h-screen bg-rex-bg p-6">
      <div className="mx-auto max-w-[860px] space-y-3">
        <h1 className="text-[0.8125rem] font-medium text-rex-text-muted">
          DEV harness — UI review ({view}, mocked IPC)
        </h1>
        {view === "card" && <DbImportCard site={fixtureSite({ dbEngine: params.get("engine") === "mariadb" ? "mariadb" : "mysql" })} />}
        {view === "delete" && <DeleteView />}
        {view === "badges" && <BadgesView />}
        {view === "resolver" && (
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
            <div className="text-[0.8125rem] font-medium text-rex-text">Valet / Herd</div>
            <div className="mt-3 flex flex-col gap-2">
              <ResolverHandBackRow tld={BORROWED} />
            </div>
          </div>
        )}
        {view === "toast" && <ToastView />}
      </div>
    </div>
  );
}
