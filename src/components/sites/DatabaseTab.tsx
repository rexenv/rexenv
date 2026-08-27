/**
 * The Database tab's body — extracted verbatim from SiteDetail so the dev
 * harness can render the REAL content inside a replica of the route's
 * region chain (UI-REVIEW §C2: the Adminer sizing bugs live in exactly this
 * composition, so the harness must exercise the same code, not a copy).
 */
import { Database } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import { AdminerFrame } from "@/components/database/AdminerFrame";
import { DbImportCard } from "@/components/sites/DbImportCard";
import { adminerFrameSrc, adminerUrl } from "@/lib/adminer";
import type { Site } from "@/types";

export function DatabaseTab({ site }: { site: Site }) {
  // A flex column that participates in the tab region's height (the old
  // `space-y-4` block severed the percentage chain, collapsing the iframe to
  // its ~150px intrinsic default — §C2.2 — and with the cards above it the
  // frame left the clipped region entirely — §C2.3). Cards take their natural
  // height; the frame flexes to what remains, with a 420px floor that makes
  // the region scroll instead of squashing it as the cards keep growing.
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      {site.docrootManaged === false && (
        <div className="flex-none">
          <DbImportCard site={site} />
        </div>
      )}
      {site.type === "php" && !site.starterDb ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <Placeholder
            icon={<Database className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="No database"
            hint="This Blank PHP site was created without one. Pick MySQL or MariaDB in the Database field when you create a Blank PHP site to get one, seeded and wired."
          />
        </div>
      ) : (
        /* A FLEX column, not a plain div: the frame fills via flex-grow, and
           a plain wrapper would leave its child's flex-1 inert (measured as
           iframeH=0 during the §C2 fix). min-h is the scroll floor. */
        <div className="flex min-h-[420px] flex-1 flex-col">
          <AdminerFrame
            src={adminerFrameSrc({ engine: site.dbEngine, db: site.dbName })}
            externalUrl={adminerUrl({ engine: site.dbEngine, db: site.dbName })}
          />
        </div>
      )}
    </div>
  );
}
