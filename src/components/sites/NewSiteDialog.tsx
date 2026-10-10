import { baseName } from "@/lib/path";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { toastBackendError } from "@/lib/toast";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, Check, CheckCircle2, ChevronLeft, ChevronRight, Eye, EyeOff, RefreshCw, X as XIcon } from "lucide-react";
import { cn, TECH_INPUT } from "@/lib/utils";
import { eolNote, eolTag } from "@/lib/php";
import { Button } from "@/components/ui/button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { dbEngineRefusals, defaultTld, inspectLinkedFolder, listBlueprints, listPhpVersions, listSites, liveSyncCreateFromLive, liveSyncPreview, offeredDbEngines, offeredWebServers, pickFolder, repoProbe, siteProvisionCancel, siteProvisionJob, wpMultisiteConvert } from "@/lib/ipc";
import { usePlatformWords } from "@/lib/usePlatformWords";
import { SiteProvisionCard, useSiteProvision } from "@/components/sites/SiteProvisionCard";
import { RefPicker, type RefGroup } from "@/components/wordpress/RefPicker";
import { useDownloads } from "@/lib/useDownloads";
import type { LinkedFolderInfo, LiveSyncPreview, MultisiteMode, RepoProbeResult, SiteDbEngine, SiteProvisionState, SiteType, WebServer } from "@/types";

/** Where a new site's files come from — the three sources a docroot has.
 *  Mutually exclusive by construction, which is also the backend's rule:
 *  `git_url` beside a linked `path` is refused, never ranked. */
type DocrootSource = "new" | "git" | "existing" | "live";

function generatePassword(): string {
  const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!@#$%";
  const buf = new Uint32Array(16);
  crypto.getRandomValues(buf);
  return Array.from(buf, (n) => chars[n % chars.length]).join("");
}

/** Labels for the web servers; WHICH are offered is core's answer (`offeredWebServers`). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
  { value: "apache", label: "Apache (.htaccess)" },
  { value: "openlitespeed", label: "OpenLiteSpeed (.htaccess + LSCache)" },
];

/** WordPress locales offered in the dialog ("" → default en_US). */
const LANGUAGES: { value: string; label: string }[] = [
  { value: "", label: "English (United States)" },
  { value: "en_GB", label: "English (UK)" },
  { value: "es_ES", label: "Español" },
  { value: "fr_FR", label: "Français" },
  { value: "de_DE", label: "Deutsch" },
];

const PHP_GLYPH = (
  <span className="font-mono text-[0.875rem] font-semibold leading-none">&lt;?</span>
);
/** Official WordPress mark (simple-icons path) — filled, inherits the card colour. */
const WP_GLYPH = (
  <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" className="block" aria-hidden="true">
    <path d="M21.469 6.825c.84 1.537 1.318 3.3 1.318 5.175 0 3.979-2.156 7.456-5.363 9.325l3.295-9.527c.615-1.54.82-2.771.82-3.864 0-.405-.026-.78-.07-1.11m-7.981.105c.647-.03 1.232-.105 1.232-.105.582-.075.514-.93-.067-.899 0 0-1.755.135-2.88.135-1.064 0-2.85-.15-2.85-.15-.585-.03-.661.855-.075.885 0 0 .54.061 1.125.09l1.68 4.605-2.37 7.08L5.354 6.9c.649-.03 1.234-.1 1.234-.1.585-.075.516-.93-.065-.896 0 0-1.746.138-2.874.138-.2 0-.438-.008-.69-.015C4.911 3.15 8.235 1.215 12 1.215c2.809 0 5.365 1.072 7.286 2.833-.046-.003-.091-.009-.141-.009-1.06 0-1.812.923-1.812 1.914 0 .89.513 1.643 1.06 2.531.411.72.89 1.643.89 2.977 0 .923-.354 1.994-.821 3.479l-1.075 3.585-3.9-11.61.001.014zM12 22.784c-1.059 0-2.081-.153-3.048-.437l3.237-9.406 3.315 9.087c.024.053.05.101.078.149-1.12.393-2.325.609-3.582.609M1.211 12c0-1.564.336-3.05.935-4.39L7.29 21.709C3.694 19.96 1.211 16.271 1.211 12M12 0C5.385 0 0 5.385 0 12s5.385 12 12 12 12-5.385 12-12S18.615 0 12 0" />
  </svg>
);
/** Official Laravel mark (simple-icons path) — filled, inherits the card colour. */
const LARAVEL_GLYPH = (
  <svg width="21" height="21" viewBox="0 0 24 24" fill="currentColor" className="block" aria-hidden="true">
    <path d="M23.642 5.43a.364.364 0 01.014.1v5.149c0 .135-.073.26-.189.327l-4.323 2.49v4.934a.378.378 0 01-.188.326L9.93 23.949a.316.316 0 01-.066.027c-.008.002-.016.008-.024.01a.348.348 0 01-.192 0c-.011-.002-.02-.008-.03-.012-.02-.008-.042-.014-.062-.025L.533 18.756a.376.376 0 01-.189-.326V2.974c0-.033.005-.066.014-.098.003-.012.01-.02.014-.032a.369.369 0 01.023-.058c.004-.013.015-.022.023-.033l.033-.045c.012-.01.025-.018.037-.027.014-.012.027-.024.041-.034H.53L5.043.05a.375.375 0 01.375 0L9.93 2.647h.002c.015.01.027.021.04.033l.038.027c.013.014.02.03.033.045.008.011.02.021.025.033.01.02.017.038.024.058.003.011.01.021.013.032.01.031.014.064.014.098v9.652l3.76-2.164V5.527c0-.033.004-.066.013-.098.003-.01.01-.02.013-.032a.487.487 0 01.024-.059c.007-.012.018-.02.025-.033.012-.015.021-.03.033-.043.012-.012.025-.02.037-.028.015-.01.028-.023.043-.033h.001l4.513-2.598a.375.375 0 01.375 0l4.513 2.598c.016.01.029.021.043.032.012.01.025.018.036.028.013.014.022.03.034.044.008.012.019.021.024.033.01.02.017.038.024.058.004.012.01.022.014.033zm-.74 5.032V6.179l-1.578.908-2.182 1.256v4.283zm-4.51 7.75v-4.287l-2.147 1.225-6.126 3.498v4.325zM1.093 3.624v14.588l8.273 4.761v-4.325l-4.322-2.445-.002-.003H5.04c-.014-.01-.025-.021-.04-.031-.011-.01-.024-.018-.035-.027l-.001-.002c-.013-.012-.021-.025-.031-.04-.01-.011-.021-.022-.028-.036h-.002c-.008-.014-.013-.031-.02-.047-.006-.016-.014-.027-.018-.043a.49.49 0 01-.008-.057c-.002-.014-.006-.027-.006-.041V5.789l-2.18-1.257zM5.23.81L1.47 2.974l3.76 2.164 3.758-2.164zm1.956 13.505l2.182-1.256V3.624l-1.58.91-2.181 1.255v9.435zm11.581-10.95l-3.76 2.163 3.76 2.164 3.759-2.164zm-.376 4.978L16.21 7.087 14.63 6.18v4.283l2.182 1.256 1.58.908zm-9.55 9.372l5.514-3.148 2.756-1.572-3.757-2.163-4.323 2.489-3.941 2.27z" />
  </svg>
);

/** Step-1 type cards (Laravel added in §12.2). */
type TypeCard = {
  value: SiteType;
  label: string;
  desc: string;
  icon: ReactNode;
  bg: string;
  border: string;
  color: string;
};
const TYPE_CARDS: TypeCard[] = [
  { value: "php", label: "Blank PHP", desc: "A clean document root. Bring your own framework or write plain PHP.", icon: PHP_GLYPH, bg: "var(--rex-accent-periwinkle-bg)", border: "var(--rex-accent-periwinkle-border)", color: "var(--rex-accent-periwinkle)" },
  { value: "wordpress", label: "WordPress", desc: "Latest WordPress, installed and ready — admin account and database set up for you.", icon: WP_GLYPH, bg: "var(--rex-accent-blue-bg)", border: "var(--rex-accent-blue-border)", color: "var(--rex-accent-blue)" },
  { value: "laravel", label: "Laravel", desc: "A fresh Laravel app via the installer, wired to a database with your .env ready.", icon: LARAVEL_GLYPH, bg: "var(--rex-accent-red-bg)", border: "var(--rex-accent-red-border)", color: "var(--rex-accent-red)" },
];

/**
 * New Site dialog — a 2-step wizard (Type → Configure). Creates a Blank-PHP or
 * **WordPress** site with a chosen PHP version and web server; WordPress adds a
 * one-click install. (Phase 2 §1.6 + §4.2 · Phase 3 §1.2.)
 */
/** Prefill for "Duplicate" — clones a source site's setup (not its content). */
export type NewSiteInitial = {
  name?: string;
  siteType?: SiteType;
  phpVersion?: string;
  webServer?: WebServer;
};

export function NewSiteDialog({
  onClose,
  initial,
  onStarted,
}: {
  onClose: () => void;
  initial?: NewSiteInitial;
  /** The job just started, for the Sites route's own `useSiteProvision`: its
   *  job card then shows behind the dialog and stays after "Close (keeps
   *  running)" — the route otherwise adopts a job only when it mounts. */
  onStarted?: (snap: SiteProvisionState) => void;
}) {
  const qc = useQueryClient();
  const words = usePlatformWords();
  const { data: versions = [] } = useQuery({ queryKey: ["php-versions"], queryFn: listPhpVersions });
  const installed = useMemo(() => versions.filter((v) => v.installed), [versions]);
  const defaultVersion = installed.find((v) => v.isDefault)?.minor ?? installed[0]?.minor ?? "8.3";
  const { data: blueprints = [] } = useQuery({ queryKey: ["blueprints"], queryFn: listBlueprints });
  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites });
  // The default TLD for new sites (Settings → DNS & SSL). ".rex" until loaded.
  const { data: tld = "rex" } = useQuery({ queryKey: ["default-tld"], queryFn: defaultTld });

  // When prefilled (Duplicate), the type is known → jump straight to Configure.
  const [step, setStep] = useState<1 | 2>(initial ? 2 : 1);
  const [name, setName] = useState(initial?.name ?? "");
  const [domain, setDomain] = useState(""); // the base, without the TLD suffix
  const [siteType, setSiteType] = useState<SiteType>(initial?.siteType ?? "php");
  const [phpVersion, setPhpVersion] = useState(initial?.phpVersion ?? defaultVersion);
  const [webServer, setWebServer] = useState<WebServer>(initial?.webServer ?? "nginx");
  const [dbEngine, setDbEngine] = useState<SiteDbEngine>("mysql");
  // A Blank PHP site gets a starter database by DEFAULT: the generated page
  // then opens on real rows out of a real engine, which is the difference
  // between "PHP works" and "your stack works". Turned off by picking None,
  // and only offered for a folder rexenv creates — a linked folder is the
  // user's, and a clone brings its own code and its own connection.
  const [starterDb, setStarterDb] = useState(true);
  const [domainEdited, setDomainEdited] = useState(false);
  const [blueprintId, setBlueprintId] = useState("");
  const [wpTitle, setWpTitle] = useState("");
  const [showPassword, setShowPassword] = useState(false);

  // Where the site's files come from — the three sources a docroot has:
  // rexenv makes it empty, a repository fills it, or the user points at a
  // folder they already have.
  const [source, setSource] = useState<DocrootSource>("new");
  const useExisting = source === "existing";
  const fromGit = source === "git";
  // From a live site (docs/PLAN-wp-live-sync.md §2.7): the pasted key is proven
  // against the live site BEFORE anything is created (`liveSyncPreview` stores
  // nothing); `livePreview` doubles as "this key reaches its site": Create
  // stays disabled until it lands. The install's admin fields are not asked —
  // the pull replaces every table, users included.
  const fromLive = source === "live";
  const [liveKey, setLiveKey] = useState("");
  const [liveUser, setLiveUser] = useState("");
  const [livePassword, setLivePassword] = useState("");
  const [livePreview, setLivePreview] = useState<LiveSyncPreview | null>(null);
  const livePreviewQ = useMutation({
    mutationFn: () => liveSyncPreview(liveKey.trim(), liveUser.trim() || undefined, livePassword || undefined),
    onSuccess: (pv) => {
      setLivePreview(pv);
      if (!name.trim()) setName(pv.suggestedName);
      if (installed.some((v) => v.minor === pv.phpMinor)) setPhpVersion(pv.phpMinor);
    },
    onError: (e) => {
      setLivePreview(null);
      toastBackendError(e);
    },
  });
  // Link an existing folder: rexenv serves it in place and never writes to,
  // moves, or deletes it. `link` holds the inspected result; a rejected pick
  // shows its reason inline rather than as a toast, next to the button.
  const [link, setLink] = useState<LinkedFolderInfo | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [linking, setLinking] = useState(false);
  // Clone from Git: the URL is probed (`git ls-remote`) BEFORE anything is
  // created, so a typo, a private repo your keys can't reach, or a branch that
  // doesn't exist fails here — with no site row, folder or certificate to
  // clean up. `probed` doubles as "this URL is real": Create stays disabled
  // until it lands.
  const [gitUrl, setGitUrl] = useState("");
  const [gitRef, setGitRef] = useState("");
  const [probed, setProbed] = useState<RepoProbeResult | null>(null);
  // Migrations default ON: this job creates the database and it is empty, so
  // there is nothing a migration can lose. Offered as a choice anyway — a
  // repo whose migrations need seeded data or an external service would
  // otherwise leave the site "setup incomplete" with no way to say "skip it".
  const [gitMigrate, setGitMigrate] = useState(true);
  // Assets default ON: a Laravel app with Vite throws "Unable to locate file in
  // Vite manifest" on page one until `npm run build` has run, so the default
  // that produces a WORKING site is the one that builds them. Disclosed rather
  // than hidden — the box below Create lists every command that will run — and
  // a checkbox for the developer who'd rather drive their own toolchain.
  const [gitBuildAssets, setGitBuildAssets] = useState(true);
  const probe = useMutation({
    mutationFn: (raw: string) => repoProbe(raw),
    onSuccess: (p) => {
      setProbed(p);
      // A pasted `/tree/<ref>` URL names a branch, but branch names contain
      // slashes — trust it only when it matches a ref the remote really has.
      const known = [...p.branches, ...p.tags];
      const candidate = p.refCandidate && known.includes(p.refCandidate) ? p.refCandidate : null;
      setGitRef(candidate ?? p.defaultBranch ?? "");
      if (!name.trim()) setName(p.dirName);
    },
    onError: (e) => {
      setProbed(null);
      toastBackendError(e);
    },
  });
  // A linked folder that already holds an app is ADOPTED — we install nothing,
  // so the WordPress install fields would be collecting credentials we'd never
  // use. (The backend skips those phases regardless; this keeps the UI honest.)
  const adopting = useExisting && !!link?.existingInstall;

  const pickExisting = async () => {
    const picked = await pickFolder("Choose the folder to serve");
    if (!picked) return;
    setLinking(true);
    setLinkError(null);
    try {
      const info = await inspectLinkedFolder(picked);
      setLink(info);
      setSiteType(info.siteType);
      if (!name.trim()) setName(baseName(info.root));
    } catch (e) {
      setLink(null);
      setLinkError(e instanceof Error ? e.message : String(e));
    } finally {
      setLinking(false);
    }
  };

  const onPickBlueprint = (id: string) => {
    setBlueprintId(id);
    const bp = blueprints.find((b) => b.id === id);
    if (bp) {
      setSiteType(bp.spec.siteType);
      setWebServer(bp.spec.webServer);
      if (installed.some((v) => v.minor === bp.spec.phpVersion)) setPhpVersion(bp.spec.phpVersion);
    }
  };

  // WordPress one-click fields (used only when siteType === "wordpress").
  const [adminUser, setAdminUser] = useState("admin");
  const [adminEmail, setAdminEmail] = useState("");
  // Default local-dev credentials admin/admin (consistent with site reset);
  // the tunnels UI warns if a site still accepting them is shared publicly.
  const [adminPassword, setAdminPassword] = useState("admin");
  const [language, setLanguage] = useState("");
  const [multisite, setMultisite] = useState<MultisiteMode>("none");

  // Track the default PHP version as it loads — but not when the dialog was
  // prefilled (Duplicate), where we keep the source site's version.
  useEffect(() => {
    if (!initial) setPhpVersion(defaultVersion);
  }, [defaultVersion, initial]);

  const slug = (s: string) => s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const domainBase = domainEdited ? slug(domain) : slug(name);
  const effectiveDomain = domainBase ? `${domainBase}.${tld}` : "";
  const isWordpress = siteType === "wordpress";
  // WordPress is INSTALLED only when we're making the folder ourselves. Adopting
  // an existing install means we touch nothing inside it — no core download, no
  // wp-config, no database.
  const installingWp = isWordpress && !adopting;
  // The admin fields are asked only when the install's users will be kept.
  const asksAdmin = installingWp && !fromLive;

  // Streamed provision job: submit STARTS it (prepare runs inline — a bad or
  // duplicate domain rejects here with nothing created), then the card below
  // streams phases. Multisite conversion runs after the job settles ok
  // (§10.1), then the dialog closes. On failure/cancel the dialog stays open
  // with the frozen card; the Sites list carries the "setup incomplete"
  // badge from here on. Closing the dialog mid-run abandons NOTHING — the
  // job continues and the Sites route re-adopts its card.
  const downloads = useDownloads();
  const prov = useSiteProvision((settled) => {
    if (settled.status !== "ok") return;
    void (async () => {
      if (settled.siteId && installingWp && multisite !== "none") {
        await wpMultisiteConvert(settled.siteId, multisite).catch(toastBackendError);
      }
      qc.invalidateQueries({ queryKey: ["sites"] });
      onClose();
    })();
  });
  // The Database field is a CHOICE only for a Blank PHP site in a folder rexenv
  // creates. Everywhere else it is either required (WordPress, Laravel) or
  // absent, and this one expression decides both what renders and what is sent.
  const starterDbOffered = siteType === "php" && source === "new";
  // The engine picker offers what create would ACCEPT, from the same gate that refuses
  // (`sites::ensure_engine_available_on`). MySQL and MariaDB used to be listed unconditionally, so
  // on Windows the dialog offered MariaDB and create refused it — offered-then-refused, the shape
  // already removed for web servers (W10, ledger #648). Undefined until the backend answers, and
  // until then everything shows: that is what the dialog did before.
  const { data: offeredEngines } = useQuery({
    queryKey: ["offered-db-engines"],
    queryFn: offeredDbEngines,
    staleTime: Infinity,
  });
  // PostgreSQL: never for WordPress, and only on a PHP that can reach it — the
  // flag comes from core (`php::pdo_pgsql_supported`), never from a list of
  // minors written here, which is the second-copy shape ledger #545 and the
  // Xdebug toggle before it both paid for.
  // Two different reasons PostgreSQL can be missing, each with its OWN note: the
  // chosen PHP lacks the driver (the PDO note), or this Mac's macOS cannot run
  // the engine at all (the refusal note, core's sentence). The first macOS 13
  // run (23 Sep 2026) showed BOTH notes for a PHP 8.3 that has pdo_pgsql —
  // because one boolean carried both facts. They are kept apart here.
  const phpHasPgsql = installed.find((v) => v.minor === phpVersion)?.postgresSupported ?? false;
  const postgresShipsHere = !offeredEngines || offeredEngines.includes("postgres");
  const postgresOffered = siteType !== "wordpress" && phpHasPgsql && postgresShipsHere;
  // …and a choice that stops being legal must not survive as a silent payload.
  // Switching to WordPress, or to a PHP without the driver, with PostgreSQL
  // already picked would otherwise send an engine the backend refuses — the
  // dialog would look fine and the job would fail at prepare.
  useEffect(() => {
    if (!postgresOffered && dbEngine === "postgres") setDbEngine("mysql");
  }, [postgresOffered, dbEngine]);

  // …and an engine that stops being offered must not survive as a silent payload.
  useEffect(() => {
    if (offeredEngines && offeredEngines.length > 0 && !offeredEngines.includes(dbEngine)) {
      setDbEngine(offeredEngines[0]);
    }
  }, [offeredEngines, dbEngine]);
  // …and an engine this Mac's macOS cannot run says so IN core's words, ahead of
  // the platform note: the sentence is the same one the Databases page shows.
  const { data: refusals = {} } = useQuery({
    queryKey: ["db-engine-refusals"],
    queryFn: dbEngineRefusals,
    staleTime: Infinity,
  });
  const missingEngines = offeredEngines
    ? (["mysql", "mariadb"] as SiteDbEngine[]).filter((e) => !offeredEngines.includes(e) && !refusals[e])
    : [];
  const refusedNote = Object.entries(refusals)
    .filter(([key]) => key === "postgres" ? siteType !== "wordpress" : key === "mysql" || key === "mariadb")
    .map(([key, reason]) => `${key === "mariadb" ? "MariaDB" : key === "postgres" ? "PostgreSQL" : "MySQL"}: ${reason}`)
    .join(" ");
  const platformNote =
    missingEngines.length === 0
      ? undefined
      : `${missingEngines.map((e) => (e === "mariadb" ? "MariaDB" : "MySQL")).join(" and ")} ${
          missingEngines.length === 1 ? "isn't" : "aren't"
        } part of rexenv on ${words.osName} yet — rexenv only offers an engine it has a pinned build for.`;
  const engineNote = [refusedNote || undefined, platformNote].filter(Boolean).join(" ") || undefined;

  // The web-server picker offers what create would ACCEPT. The list comes from core
  // (`sites::offered_web_servers`, the same gate that refuses); SERVERS above is only where
  // the labels live. On Windows neither Apache nor FrankenPHP has a pin (D4), so they are
  // ABSENT rather than offered and then refused at create time (W10, ledger #643).
  // Until the first answer arrives the picker shows the full list — what it always showed —
  // and narrows when core replies. A default built from SERVERS itself would be this module
  // deciding again, which is the shape the guard forbids.
  const { data: offered } = useQuery({
    queryKey: ["offered-web-servers"],
    queryFn: offeredWebServers,
    staleTime: Infinity,
  });
  const offeredServers = offered ? SERVERS.filter((s) => offered.includes(s.value)) : SERVERS;
  const missingServers = offered ? SERVERS.filter((s) => !offered.includes(s.value)) : [];
  // …and a server that stops being offered must not survive as a silent payload — a
  // blueprint can set one this build cannot serve.
  useEffect(() => {
    if (offered && offeredServers.length > 0 && !offered.includes(webServer)) {
      setWebServer(offeredServers[0].value);
    }
  }, [offered, offeredServers, webServer]);
  // OpenLiteSpeed is not "not yet" anywhere it is missing — it has no Windows build at all —
  // so its sentence is core's (`PlatformWords::openlitespeed_unavailable`), not this one.
  const notYet = missingServers.filter((s) => s.value !== "openlitespeed");
  const serverNote =
    [
      notYet.length === 0
        ? undefined
        : `${notYet.map((s) => s.label.replace(/ \(.*\)$/, "")).join(" and ")} ${
            notYet.length === 1 ? "isn't" : "aren't"
          } part of rexenv on ${words.osName} yet — rexenv only offers a server it has a pinned build for.`,
      missingServers.some((s) => s.value === "openlitespeed") ? words.openlitespeedUnavailable : undefined,
    ]
      .filter(Boolean)
      .join(" ") || undefined;
  const create = useMutation({
    mutationFn: () =>
      fromLive
        ? liveSyncCreateFromLive({
            key: liveKey.trim(),
            basicAuthUser: liveUser.trim() || undefined,
            basicAuthPassword: livePassword || undefined,
            name: name.trim(),
            domain: effectiveDomain.trim(),
            phpVersion,
            webServer,
          })
        : siteProvisionJob(
        {
          name: name.trim(),
          domain: effectiveDomain.trim(),
          type: siteType,
          phpVersion,
          webServer,
          // Non-empty = link that folder in place. We send the SERVE path, which
          // for a framework is the docroot subfolder, not the project root.
          path: useExisting ? (link?.servePath ?? "") : "",
          dbEngine,
          // Sent only where the field was OFFERED. The backend records the same
          // rule on the row, so the two cannot disagree about a site that was
          // linked or cloned — but a payload that says "yes" for a shape that
          // ignores it is a lie the log would carry.
          starterDb: starterDbOffered && starterDb,
          // The URL the PROBE returned, not the raw paste: it is the one the
          // branch list above actually came from. (The backend re-parses it
          // anyway — the UI's copy is display state, never a trust boundary.)
          gitUrl: fromGit ? (probed?.url ?? gitUrl.trim()) : "",
          gitRef: fromGit && gitRef ? gitRef : null,
          gitMigrate,
          gitBuildAssets,
        },
        installingWp
          ? { title: wpTitle.trim() || name.trim(), adminUser: adminUser.trim(), adminEmail: adminEmail.trim(), adminPassword, language }
          : undefined,
        // Gated on the SAME fact that renders the field, not on the field being
        // visible right now: picking a blueprint and then stepping back to
        // choose Laravel would otherwise submit a stale id the backend refuses.
        (installingWp && blueprintId) || undefined,
      ),
    onSuccess: (snap) => {
      prov.start(snap);
      onStarted?.(snap);
    },
    onError: (e) => toastBackendError(e),
  });
  const pending = create.isPending || prov.running;

  // The backend persists the site row at job START, and the sidebar's 2s poll
  // refreshes ["sites"] — so while the job runs, the domain being created
  // would flag itself as "already in use". Freeze the check while the job is
  // live; after a failed job the row legitimately exists (setup incomplete —
  // Retry lives on the Sites list, not on a resubmit of this form).
  const domainTaken =
    !pending &&
    prov.job == null &&
    effectiveDomain !== "" &&
    sites.some((s) => s.domain.toLowerCase() === effectiveDomain.toLowerCase());
  const domainOk = effectiveDomain !== "" && !domainTaken;

  const canSubmit =
    name.trim() !== "" &&
    domainOk &&
    (!asksAdmin || (adminUser.trim() !== "" && adminPassword !== "")) &&
    // From a live site: the key has reached its site, and it is not a network.
    (!fromLive || (!!livePreview && !livePreview.multisite)) &&
    // Linking is chosen but no usable folder picked yet.
    (!useExisting || !!link) &&
    // Cloning is chosen but the repository hasn't been reached yet. Gating on
    // the PROBE, not on the field being non-empty, is what keeps "created" from
    // meaning "a site row, a folder and a certificate exist for a URL that
    // turned out to be a typo".
    (!fromGit || !!probed) &&
    !pending &&
    prov.job == null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        className="flex max-h-[88vh] w-[516px] flex-col overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header + step indicator */}
        <div className="flex items-center gap-3 border-b border-rex-border-subtle px-5 py-[18px]">
          <div className="flex-1">
            <div className="text-[0.96875rem] font-semibold text-rex-text">New site</div>
            <div className="mt-px text-[0.75rem] text-rex-text-muted">
              {step === 1 ? "Choose what to build" : "Configure your site"}
            </div>
          </div>
          <div className="flex items-center gap-[7px]">
            <StepDot n={1} label="Type" active={step >= 1} current={step === 1} />
            <span className="h-px w-[14px] bg-rex-border-strong" />
            <StepDot n={2} label="Configure" active={step >= 2} current={step === 2} />
          </div>
        </div>

        {/* Content */}
        <div className="min-h-0 flex-1 overflow-auto px-5 py-[18px]">
          {step === 1 ? (
            <div className="flex flex-col gap-[10px]">
              {TYPE_CARDS.map((c) => (
                <TypeCardButton key={c.value} card={c} selected={siteType === c.value} onClick={() => setSiteType(c.value)} />
              ))}
            </div>
          ) : (
            <Step2
              blueprints={blueprints}
              showBlueprints={installingWp}
              blueprintId={blueprintId}
              onPickBlueprint={onPickBlueprint}
              name={name}
              setName={setName}
              tld={tld}
              domainBase={domainBase}
              onDomainBase={(v) => {
                setDomainEdited(true);
                // Lowercase AS TYPED (not on submit): domains are lowercase by
                // convention, and macOS auto-capitalize/paste would otherwise
                // land "Test1" in the field.
                setDomain(v.toLowerCase());
              }}
              domainOk={domainOk}
              domainTaken={domainTaken}
              installed={installed}
              defaultVersion={defaultVersion}
              phpVersion={phpVersion}
              setPhpVersion={setPhpVersion}
              webServer={webServer}
              setWebServer={setWebServer}
              offeredServers={offeredServers}
              serverNote={serverNote}
              offeredEngines={offeredEngines}
              engineNote={engineNote}
              dbEngine={dbEngine}
              setDbEngine={setDbEngine}
              postgresOffered={postgresOffered}
              phpHasPgsql={phpHasPgsql}
              postgresShipsHere={postgresShipsHere}
              needsDb={siteType !== "php" && !adopting}
              starterDbOffered={starterDbOffered}
              starterDb={starterDb}
              setStarterDb={setStarterDb}
              source={source}
              setSource={(v) => {
                setSource(v);
                // Switching source drops the other one's answer: a stale
                // folder or probe would otherwise be submitted invisibly.
                setLink(null);
                setLinkError(null);
                setProbed(null);
                setGitRef("");
                setLivePreview(null);
              }}
              gitAllowed
              siteType={siteType}
              gitUrl={gitUrl}
              setGitUrl={(v) => {
                setGitUrl(v);
                // Editing the URL invalidates the branch list it produced.
                setProbed(null);
                setGitRef("");
              }}
              gitRef={gitRef}
              setGitRef={setGitRef}
              gitMigrate={gitMigrate}
              setGitMigrate={setGitMigrate}
              gitBuildAssets={gitBuildAssets}
              setGitBuildAssets={setGitBuildAssets}
              probed={probed}
              probing={probe.isPending}
              onProbe={() => probe.mutate(gitUrl.trim())}
              link={link}
              linkError={linkError}
              linking={linking}
              pickExisting={() => void pickExisting()}
              isWordpress={asksAdmin}
              live={{
                key: liveKey,
                setKey: (v) => {
                  setLiveKey(v);
                  setLivePreview(null);
                },
                user: liveUser,
                setUser: setLiveUser,
                password: livePassword,
                setPassword: setLivePassword,
                preview: livePreview,
                checking: livePreviewQ.isPending,
                onCheck: () => livePreviewQ.mutate(),
              }}
              wpTitle={wpTitle}
              setWpTitle={setWpTitle}
              showPassword={showPassword}
              setShowPassword={setShowPassword}
              onGeneratePassword={() => {
                setAdminPassword(generatePassword());
                setShowPassword(true);
              }}
              adminUser={adminUser}
              setAdminUser={setAdminUser}
              adminEmail={adminEmail}
              setAdminEmail={setAdminEmail}
              adminPassword={adminPassword}
              setAdminPassword={setAdminPassword}
              language={language}
              setLanguage={setLanguage}
              multisite={multisite}
              setMultisite={setMultisite}
            />
          )}
        </div>

        {/* Streamed provision card — replaces the old opaque disabled-button
            wait. Stays after a failure/cancel (frozen bar + failing phase);
            closing the dialog leaves the job running (Sites re-adopts it). */}
        {prov.job && (
          <div className="border-t border-rex-border-subtle px-5 py-3">
            <SiteProvisionCard
              job={prov.job}
              lines={prov.lines}
              downloads={downloads}
              onCancel={() => void siteProvisionCancel(prov.job!.id).catch(toastBackendError)}
            />
          </div>
        )}

        {/* Footer */}
        <div className="flex items-center justify-between gap-3 border-t border-rex-border-subtle px-5 py-[14px]">
          <div>
            {step === 2 && !prov.job && (
              <button
                onClick={() => setStep(1)}
                className="flex h-9 items-center gap-1.5 rounded-[9px] px-3 text-[0.8125rem] font-medium text-rex-text-bright transition-colors hover:bg-rex-hover"
              >
                <ChevronLeft className="h-[15px] w-[15px]" strokeWidth={2} />
                Back
              </button>
            )}
          </div>
          <div className="flex items-center gap-2.5">
            <Button variant="secondary" onClick={onClose}>
              {prov.running ? "Close (keeps running)" : "Cancel"}
            </Button>
            {step === 1 ? (
              <Button variant="primary" onClick={() => setStep(2)}>
                Continue
                <ChevronRight className="h-[15px] w-[15px]" strokeWidth={2} />
              </Button>
            ) : (
              !prov.job && (
                <Button variant="primary" disabled={!canSubmit} onClick={() => create.mutate()}>
                  {create.isPending
                    ? "Starting…"
                    : fromLive
                      ? "Create and pull"
                      : installingWp
                        ? "Install WordPress"
                      : adopting
                        ? "Link site"
                        : "Create site"}
                </Button>
              )
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function StepDot({ n, label, active, current }: { n: number; label: string; active: boolean; current: boolean }) {
  return (
    <span
      className={cn(
        "flex items-center gap-[7px] font-mono text-[0.65625rem]",
        active ? "text-rex-text-bright" : "text-rex-text-muted",
      )}
    >
      <span
        className={cn(
          "flex h-[18px] w-[18px] items-center justify-center rounded-full text-[0.625rem]",
          current ? "bg-brand text-white" : active ? "bg-brand-tint-bg text-brand-tint" : "bg-rex-surface-3 text-rex-text-muted",
        )}
      >
        {n}
      </span>
      {label}
    </span>
  );
}

function TypeCardButton({ card, selected, onClick }: { card: TypeCard; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-start gap-[13px] rounded-[12px] border bg-rex-surface-1 p-[15px] text-left transition-colors",
        selected ? "border-brand shadow-glow-primary" : "border-rex-border-subtle hover:border-rex-border-strong",
      )}
    >
      <span
        className="flex h-10 w-10 flex-none items-center justify-center rounded-[10px] border"
        style={{ background: card.bg, borderColor: card.border, color: card.color }}
      >
        {card.icon}
      </span>
      <span className="flex-1">
        <span className="block text-[0.875rem] font-semibold text-rex-text">{card.label}</span>
        <span className="mt-[3px] block text-[0.78125rem] text-rex-text-muted">{card.desc}</span>
      </span>
      <CheckCircle2
        className="mt-0.5 h-[19px] w-[19px] flex-none"
        style={{ color: selected ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
        strokeWidth={2}
      />
    </button>
  );
}

const FIELD_INPUT =
  "h-9 w-full rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[0.8125rem] text-rex-text outline-none transition-colors focus:border-brand";
const FIELD_SELECT =
  "h-9 w-full rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[0.78125rem] text-rex-text outline-none transition-colors focus:border-brand";

interface LiveSourceProps {
  key: string;
  setKey: (v: string) => void;
  user: string;
  setUser: (v: string) => void;
  password: string;
  setPassword: (v: string) => void;
  preview: LiveSyncPreview | null;
  checking: boolean;
  onCheck: () => void;
}

/** The "From a live site" source (`docs/PLAN-wp-live-sync.md` §2.7): paste the
 *  key from the rexenv Sync plugin → Check (the manifest, signed — proves the
 *  key AND reaches the site before anything is created) → the live site's
 *  WordPress, PHP and table count show, the name and PHP version prefill. The
 *  site is installed, then pulled: its users, content and files are live's. */
function LiveSourceFields({ p }: { p: LiveSourceProps }) {
  const [showAuth, setShowAuth] = useState(false);
  const isKey = p.key.trim().startsWith("rexsync1:");
  return (
    <div className="mt-[9px] space-y-[7px]">
      <p className="text-[0.71875rem] leading-[1.5] text-rex-text-muted">
        On the live site: install the <span className="font-mono">rexenv Sync</span> plugin, Tools → rexenv Sync → <em>Connect to rexenv</em>, and paste the key here.
        The live site is only read from.
      </p>
      <div className="flex items-start gap-[7px]">
        <textarea
          {...TECH_INPUT}
          value={p.key}
          onChange={(e) => p.setKey(e.target.value)}
          placeholder="rexsync1:…"
          rows={2}
          aria-label="Connection key"
          className={cn("min-w-0 flex-1 rounded-[9px] border border-rex-border-strong bg-rex-well px-2.5 py-1.5 font-mono text-[0.71875rem] text-rex-text outline-none focus:border-brand")}
        />
        <Button variant="secondary" size="sm" onClick={p.onCheck} disabled={p.checking || !isKey}>
          {p.checking ? "Checking…" : p.preview ? "Re-check" : "Check"}
        </Button>
      </div>
      <button type="button" className="text-[0.6875rem] text-rex-accent-blue hover:underline" onClick={() => setShowAuth((s) => !s)}>
        {showAuth ? "No HTTP password" : "The live site is behind an HTTP password…"}
      </button>
      {showAuth && (
        <div className="flex gap-[7px]">
          <input {...TECH_INPUT} value={p.user} onChange={(e) => p.setUser(e.target.value)} placeholder="user" aria-label="HTTP auth user" className={FIELD_INPUT} />
          <input {...TECH_INPUT} type="password" value={p.password} onChange={(e) => p.setPassword(e.target.value)} placeholder="password" aria-label="HTTP auth password" className={FIELD_INPUT} />
        </div>
      )}
      {p.preview && (
        <div
          className={cn(
            "rounded-[9px] border px-[11px] py-2 text-[0.71875rem]",
            p.preview.multisite ? "border-status-error-border bg-status-error-bg/40 text-status-error-bright" : "border-rex-border-subtle bg-rex-surface-1 text-rex-text-muted",
          )}
          data-testid="live-preview"
        >
          <span className="font-mono text-rex-text">{p.preview.siteUrl}</span> — WordPress {p.preview.wp}, PHP {p.preview.php}, {p.preview.tables} tables.
          {p.preview.multisite
            ? " A multisite network: rexenv cannot copy a network yet."
            : p.preview.phpExact
              ? ` This site will run PHP ${p.preview.phpMinor}, as the live one does.`
              : ` rexenv does not ship PHP ${p.preview.phpMinor === p.preview.php ? p.preview.php : p.preview.php.split(".").slice(0, 2).join(".")}; this site will run ${p.preview.phpMinor}.`}
        </div>
      )}
    </div>
  );
}

/** The "From Git" source: paste a URL → Fetch (`git ls-remote`, which validates
 *  the URL AND your access before anything is created) → pick a branch or tag.
 *  The disclosure sits above Create, not behind it: creating this site runs the
 *  repository's own code, and that is stated where the decision is made. */
function GitSourceFields({
  p,
}: {
  p: {
    siteType: SiteType;
    gitUrl: string;
    setGitUrl: (v: string) => void;
    gitRef: string;
    setGitRef: (v: string) => void;
    gitMigrate: boolean;
    setGitMigrate: (v: boolean) => void;
    gitBuildAssets: boolean;
    setGitBuildAssets: (v: boolean) => void;
    probed: RepoProbeResult | null;
    probing: boolean;
    onProbe: () => void;
  };
}) {
  const groups: RefGroup[] = p.probed
    ? [
        {
          label: null,
          items: p.probed.branches.map((b) => ({
            value: b,
            hint: b === p.probed?.defaultBranch ? "default" : undefined,
          })),
        },
        ...(p.probed.tags.length > 0
          ? [{ label: "Tags", items: p.probed.tags.map((t) => ({ value: t })) }]
          : []),
      ]
    : [];
  return (
    <div className="mt-[9px] space-y-[7px]">
      <div className="flex items-center gap-[7px]">
        <input
          value={p.gitUrl}
          onChange={(e) => p.setGitUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && p.gitUrl.trim() && !p.probing) {
              e.preventDefault();
              p.onProbe();
            }
          }}
          placeholder="https://github.com/you/your-app"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          className={cn(TECH_INPUT, "h-9 min-w-0 flex-1 rounded-[9px] border border-rex-border-strong bg-rex-well px-2.5 text-[0.78125rem] text-rex-text outline-none focus:border-brand")}
        />
        <Button variant="secondary" size="sm" onClick={p.onProbe} disabled={p.probing || p.gitUrl.trim() === ""}>
          {p.probing ? "Fetching…" : p.probed ? "Re-fetch" : "Fetch"}
        </Button>
      </div>
      {p.probed ? (
        <>
          <div className="flex items-center gap-[7px]">
            <span className="text-[0.6875rem] text-rex-text-muted">Branch or tag</span>
            <RefPicker
              value={p.gitRef}
              onChange={p.setGitRef}
              groups={groups}
              ariaLabel="Branch or tag to check out"
            />
          </div>
          {p.siteType === "wordpress" && (
            /* The promise this flow can and cannot keep, said BEFORE Create.
               "Clone my site" and "clone my site's code" are different things,
               and only the second one is on offer — the database that makes a
               WordPress site a site lives nowhere in a repository. */
            <div className="rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] leading-[1.5] text-status-warning-bright">
              Your <span className="font-medium">code</span> comes from the repository; the{" "}
              <span className="font-medium">database is new and empty</span>. WordPress is
              installed into it with the admin account below — none of your posts, options or
              users come along. Import a dump from the site's Database tab afterwards if you
              want them.
            </div>
          )}
          {p.siteType === "laravel" && (
            <label className="flex cursor-pointer items-start gap-2">
              <input
                type="checkbox"
                checked={p.gitMigrate}
                onChange={(e) => p.setGitMigrate(e.target.checked)}
                className="mt-[2px] h-3.5 w-3.5 flex-none accent-brand"
              />
              <span className="text-[0.6875rem] leading-[1.5] text-rex-text-muted">
                Run <span className="font-mono text-rex-text-bright">php artisan migrate</span> —
                this site's database is created empty by the same job, so there is nothing to
                lose. Turn it off for a project whose migrations need seed data or a service
                that isn't running yet.
              </span>
            </label>
          )}
          <label className="flex cursor-pointer items-start gap-2">
            <input
              type="checkbox"
              checked={p.gitBuildAssets}
              onChange={(e) => p.setGitBuildAssets(e.target.checked)}
              className="mt-[2px] h-3.5 w-3.5 flex-none accent-brand"
            />
            <span className="text-[0.6875rem] leading-[1.5] text-rex-text-muted">
              Build front-end assets — a Vite app shows{" "}
              <span className="font-mono">Unable to locate file in Vite manifest</span> on its
              first page until it has been built. Uses the Node in your own shell (nvm
              included); if that fails the site is still created and says so.
            </span>
          </label>
          <div className="rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5 text-[0.6875rem] leading-[1.5] text-rex-text-muted">
            Creating this site runs the repository's own code:{" "}
            <span className="font-mono text-rex-text-bright">composer install</span> (which runs
            the project's Composer scripts
            {p.siteType !== "laravel" && ", and is skipped when there is no composer.json"})
            {p.siteType === "laravel" && (
              <>
                , then <span className="font-mono text-rex-text-bright">artisan key:generate</span>
                {p.gitMigrate && (
                  <>
                    {" "}and <span className="font-mono text-rex-text-bright">artisan migrate</span>
                  </>
                )}
              </>
            )}
            {p.gitBuildAssets && (
              <>
                , then the repository's package manager —{" "}
                <span className="font-mono text-rex-text-bright">install</span> (postinstall
                scripts included) and{" "}
                <span className="font-mono text-rex-text-bright">run build</span>
              </>
            )}
            .
          </div>
        </>
      ) : (
        <div className="text-[0.6875rem] leading-[1.5] text-rex-text-muted">
          Paste an https or <span className="font-mono">git@</span> URL — or just{" "}
          <span className="font-mono">owner/repo</span> for GitHub. Fetch checks the URL and your
          access before anything is created; private repos use the SSH keys and agent you
          already have.
        </div>
      )}
    </div>
  );
}

function Step2(p: {
  blueprints: import("@/types").Blueprint[];
  /** WordPress-only, and only when we're the ones installing it. */
  showBlueprints: boolean;
  blueprintId: string;
  onPickBlueprint: (id: string) => void;
  name: string;
  setName: (v: string) => void;
  tld: string;
  domainBase: string;
  onDomainBase: (v: string) => void;
  domainOk: boolean;
  domainTaken: boolean;
  installed: import("@/types").PhpVersion[];
  defaultVersion: string;
  phpVersion: string;
  setPhpVersion: (v: string) => void;
  webServer: WebServer;
  setWebServer: (v: WebServer) => void;
  /** The servers core will accept on this build — never the module's own list (W10). */
  offeredServers: { value: WebServer; label: string }[];
  /** Why a server the user might look for is absent, or undefined when none is. */
  serverNote?: string;
  /** The engines core will accept on this build — undefined until it answers (W10). */
  offeredEngines?: SiteDbEngine[];
  /** Why an engine the user might look for is absent, or undefined when none is. */
  engineNote?: string;
  dbEngine: SiteDbEngine;
  setDbEngine: (v: SiteDbEngine) => void;
  needsDb: boolean;
  /** Blank PHP in a folder rexenv creates: the Database field is a choice,
   *  MySQL / MariaDB / None, rather than a fixed engine or a dead "None". */
  starterDbOffered: boolean;
  /** Whether PostgreSQL is a legal choice for THIS site — the site type and the
   *  chosen PHP together (see the Database field). */
  postgresOffered: boolean;
  /** The chosen PHP has a working pdo_pgsql — the PDO note shows only when this is false. */
  phpHasPgsql: boolean;
  /** This build/host can run PostgreSQL at all — when false the reason is in `engineNote`, not here. */
  postgresShipsHere: boolean;
  starterDb: boolean;
  setStarterDb: (v: boolean) => void;
  source: DocrootSource;
  setSource: (v: DocrootSource) => void;
  gitAllowed: boolean;
  siteType: SiteType;
  gitUrl: string;
  setGitUrl: (v: string) => void;
  gitRef: string;
  setGitRef: (v: string) => void;
  gitMigrate: boolean;
  setGitMigrate: (v: boolean) => void;
  gitBuildAssets: boolean;
  setGitBuildAssets: (v: boolean) => void;
  probed: RepoProbeResult | null;
  probing: boolean;
  onProbe: () => void;
  link: LinkedFolderInfo | null;
  linkError: string | null;
  linking: boolean;
  pickExisting: () => void;
  isWordpress: boolean;
  live: LiveSourceProps;
  wpTitle: string;
  setWpTitle: (v: string) => void;
  adminUser: string;
  setAdminUser: (v: string) => void;
  adminEmail: string;
  setAdminEmail: (v: string) => void;
  adminPassword: string;
  setAdminPassword: (v: string) => void;
  showPassword: boolean;
  setShowPassword: (v: boolean) => void;
  onGeneratePassword: () => void;
  language: string;
  setLanguage: (v: string) => void;
  multisite: MultisiteMode;
  setMultisite: (v: MultisiteMode) => void;
}) {
  const domainBase = p.domainBase || "my-site";
  // The chosen version's row, only when core says it is past its security-
  // support end. `find` on `installed` and not a literal — which minors are
  // dead is core's answer (`core::php::eol_since`), computed against today.
  const eolChoice = p.installed.find((v) => v.minor === p.phpVersion && v.eolSince) as
    | (import("@/types").PhpVersion & { eolSince: string })
    | undefined;
  return (
    <div className="flex flex-col gap-3">
      {/* Blueprints are a WORDPRESS preset (plugins, themes, multisite mode,
          WP_DEBUG, language) and the backend applies them only to a managed
          WordPress site — offering the field for Laravel or Blank PHP, or for
          a folder we merely adopt, promised something that then silently did
          nothing. `installingWp` is the same fact the WordPress fields below
          use, so the two can't drift. */}
      {p.showBlueprints && p.blueprints.length > 0 && (
        <Field label="Start from blueprint">
          <select value={p.blueprintId} onChange={(e) => p.onPickBlueprint(e.target.value)} className={FIELD_SELECT}>
            <option value="">None (custom)</option>
            {p.blueprints.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
              </option>
            ))}
          </select>
        </Field>
      )}

      {/* Where the files come from — the three sources a docroot has. Default:
          rexenv makes the folder and owns it. From Git: rexenv makes the folder
          and fills it from a remote. Existing folder: a project that already
          exists, served in place, never written to, never deleted with the
          site. */}
      <Field label="Files">
        <div className="flex gap-[7px]">
          {([
            { v: "new" as const, label: "New folder" },
            ...(p.gitAllowed ? [{ v: "git" as const, label: "From Git" }] : []),
            { v: "existing" as const, label: "Existing folder" },
            ...(p.siteType === "wordpress" ? [{ v: "live" as const, label: "From a live site" }] : []),
          ]).map((o) => (
            <button
              key={o.v}
              type="button"
              onClick={() => p.setSource(o.v)}
              className={cn(
                "h-9 flex-1 rounded-[9px] border text-[0.78125rem] font-medium transition-colors",
                p.source === o.v
                  ? "border-brand bg-brand/10 text-rex-text-bright"
                  : "border-rex-border-strong bg-rex-well text-rex-text-muted hover:text-rex-text",
              )}
            >
              {o.label}
            </button>
          ))}
        </div>
        {p.source === "git" && <GitSourceFields p={p} />}
        {p.source === "live" && <LiveSourceFields p={p.live} />}
        {p.source === "existing" && (
          <div className="mt-[9px] space-y-[7px]">
            <div className="flex items-center gap-[7px]">
              <Button variant="secondary" size="sm" onClick={p.pickExisting} disabled={p.linking}>
                {p.linking ? "Checking…" : p.link ? "Choose another…" : "Choose folder…"}
              </Button>
              {p.link && (
                <span className="min-w-0 flex-1 truncate font-mono text-[0.6875rem] text-rex-text-muted" title={p.link.root}>
                  {p.link.root}
                </span>
              )}
            </div>
            {p.linkError && (
              <div className="rounded-md border border-status-error-border bg-status-error-bg px-2.5 py-1.5 text-[0.6875rem] text-status-error-bright">
                {p.linkError}
              </div>
            )}
            {p.link && (
              <>
                <div className="rounded-md border border-rex-border-subtle bg-rex-well px-2.5 py-1.5 text-[0.6875rem] text-rex-text-muted">
                  Detected <span className="text-rex-text-bright">{p.link.label}</span>
                  {p.link.docrootRel && (
                    <>
                      {" "}· serving <span className="font-mono text-rex-text-bright">{p.link.docrootRel}/</span>
                    </>
                  )}
                  {p.link.existingInstall
                    ? " · adopted as-is, nothing is installed into it"
                    : " · nothing to serve yet — add files and reload"}
                </div>
                {p.link.hasCustomValetDriver && (
                  <div className="rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] text-status-warning-bright">
                    This project has a LocalValetDriver.php, which picks its document root by
                    running PHP. rexenv detects folders without executing them, so check the
                    served folder above matches what Valet used.
                  </div>
                )}
                <div className="text-[0.6875rem] leading-[1.5] text-rex-text-muted">
                  The folder stays where it is — you keep your own git workflow, and deleting
                  the site never deletes it. rexenv keeps its own certificate, server config and
                  database outside it. WordPress features that write inside the folder still do:
                  the mu-plugins for public sharing and one-click login, plugins or themes added
                  from Git, and anything run through WP-CLI.
                </div>
              </>
            )}
          </div>
        )}
      </Field>

      <div className="grid grid-cols-2 gap-[13px]">
        <Field label="Site name">
          <input {...TECH_INPUT} autoFocus value={p.name} placeholder="my-site" onChange={(e) => p.setName(e.target.value)} className={FIELD_INPUT} />
        </Field>
        <Field label="Domain">
          <div
            className={cn(
              "flex h-9 items-center rounded-[9px] border bg-rex-well px-[11px] transition-colors",
              "has-[input:focus]:shadow-[0_0_0_3px_var(--rex-focus-ring)]",
              p.domainTaken ? "border-status-error" : "border-rex-border-strong has-[input:focus]:border-brand",
            )}
          >
            <input {...TECH_INPUT}
              value={p.domainBase}
              // Pasting a full domain drops the suffix shown next to the field
              // (the TLD is [a-z]+ by policy, so it's regex-safe).
              onChange={(e) => p.onDomainBase(e.target.value.replace(new RegExp(`\\.${p.tld}$`), ""))}
              className="min-w-0 flex-1 bg-transparent font-mono text-[0.78125rem] text-rex-text outline-none focus-visible:shadow-none"
            />
            <span className="flex-none font-mono text-[0.78125rem] text-rex-text-muted">.{p.tld}</span>
            {p.domainOk && <Check className="ml-2 h-[15px] w-[15px] flex-none text-status-running" strokeWidth={2} />}
            {p.domainTaken && <XIcon className="ml-2 h-[15px] w-[15px] flex-none text-status-error" strokeWidth={2} />}
          </div>
        </Field>
      </div>
      {p.domainTaken && (
        <div className="-mt-1.5 flex items-center gap-[7px] text-[0.71875rem] text-status-error-bright">
          <AlertCircle className="h-[13px] w-[13px] flex-none" strokeWidth={2} />
          <span>
            <span className="font-mono">{domainBase}.{p.tld}</span> is already in use. Try another name.
          </span>
        </div>
      )}

      <div className="grid grid-cols-3 gap-[13px]">
        <Field label="PHP version">
          <select value={p.phpVersion} onChange={(e) => p.setPhpVersion(e.target.value)} className={cn(FIELD_SELECT, "font-mono")}>
            {p.installed.length === 0 && <option value={p.defaultVersion}>{p.defaultVersion}</option>}
            {p.installed.map((v) => (
              <option key={v.minor} value={v.minor}>
                {v.minor}
                {eolTag(v.eolSince)}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Web server">
          <select value={p.webServer} onChange={(e) => p.setWebServer(e.target.value as WebServer)} className={FIELD_SELECT}>
            {p.offeredServers.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </select>
        </Field>
        {/* Engine is chosen at create and immutable after — the database
            lives in that engine's datadir. For a Blank PHP site the same field
            also answers WHETHER: None is a real option there, and the only one
            that skips the database engine download entirely.

            PostgreSQL is offered only where the backend will ACCEPT it —
            never WordPress (`wpdb` speaks MySQL alone) and only on a PHP whose
            build has a working `pdo_pgsql` (`postgresSupported`, derived in
            core). Both rules live in `sites::ensure_engine_supports`; this
            renders them rather than restating them, so an option that would
            come back as an error is simply not there. */}
        <Field label="Database">
          {p.needsDb || p.starterDbOffered ? (
            <select
              value={p.starterDbOffered && !p.starterDb ? "none" : p.dbEngine}
              onChange={(e) => {
                const v = e.target.value;
                p.setStarterDb(v !== "none");
                if (v !== "none") p.setDbEngine(v as SiteDbEngine);
              }}
              className={FIELD_SELECT}
            >
              {(!p.offeredEngines || p.offeredEngines.includes("mysql")) && (
                <option value="mysql">MySQL</option>
              )}
              {(!p.offeredEngines || p.offeredEngines.includes("mariadb")) && (
                <option value="mariadb">MariaDB</option>
              )}
              {p.postgresOffered && <option value="postgres">PostgreSQL</option>}
              {p.starterDbOffered && <option value="none">None</option>}
            </select>
          ) : (
            <div className={cn(FIELD_INPUT, "flex items-center text-[0.78125rem] text-rex-text-muted")}>
              None
            </div>
          )}
        </Field>
      </div>

      {/* What "MySQL" in that field actually buys, said before the click rather
          than discovered after it — the site's first page is not a phpinfo dump
          but real rows out of this database, and the engine is a download the
          None option skips. */}
      {/* Why PostgreSQL is missing, said where it would have been — a control
          that silently has fewer options than it did on another site reads as a
          bug. Only shown for the type that COULD have it: telling a WordPress
          user about a PHP version would be answering a question they cannot
          ask. */}
      {/* Why a web server is missing, said where it would have been — the same rule as the
          PostgreSQL note below: a control with fewer options than it had elsewhere reads as a
          bug unless it says why. */}
      {p.serverNote && (
        <div className="-mt-1 text-[0.6875rem] leading-[1.5] text-rex-text-muted">{p.serverNote}</div>
      )}
      {/* Why an engine is missing, said where it would have been — the same rule as the two notes
          around it: a control with fewer options than it had elsewhere reads as a bug unless it
          says why. */}
      {(p.needsDb || p.starterDbOffered) && p.engineNote && (
        <div className="-mt-1 text-[0.6875rem] leading-[1.5] text-rex-text-muted">{p.engineNote}</div>
      )}
      {(p.needsDb || p.starterDbOffered) && p.siteType !== "wordpress" && p.postgresShipsHere && !p.phpHasPgsql && (
        <div className="-mt-1 text-[0.6875rem] leading-[1.5] text-rex-text-muted">
          PostgreSQL needs PDO, and the PHP {p.phpVersion} build has no working{" "}
          <span className="font-mono text-rex-text-bright">pdo_pgsql</span> — pick a newer PHP
          above to use it.
        </div>
      )}
      {p.starterDbOffered && (
        <div className="-mt-1 text-[0.6875rem] leading-[1.5] text-rex-text-muted">
          {p.starterDb ? (
            <>
              rexenv creates the database, seeds a sample table and writes{" "}
              <span className="font-mono text-rex-text-bright">db.php</span> — the generated{" "}
              <span className="font-mono text-rex-text-bright">index.php</span> opens on those rows.
              The engine is downloaded on first use.
            </>
          ) : (
            <>
              No database, and no engine download. The generated{" "}
              <span className="font-mono text-rex-text-bright">index.php</span> still shows what this
              site is running.
            </>
          )}
        </div>
      )}

      {/* The sentence in front of the button that starts it. An EOL runtime is
          a legitimate choice — legacy projects are why it is offered — but it
          is not one to make unknowingly, and for WordPress it also pre-empts a
          nag the user would otherwise report as a rexenv bug. */}
      {eolChoice && (
        <div className="-mt-1 rounded-md border border-status-warning-border bg-status-warning-bg px-2.5 py-1.5 text-[0.6875rem] leading-[1.5] text-status-warning-bright">
          {eolNote(eolChoice.minor, eolChoice.eolSince, { wordpress: p.isWordpress })}
        </div>
      )}

      {p.isWordpress && (
        <div className="mt-1 flex flex-col gap-[14px] border-t border-rex-border-subtle pt-[15px]">
          <div className="font-mono text-[0.625rem] uppercase tracking-[0.13em] text-rex-text-muted">
            WordPress install
          </div>
          <div className="grid grid-cols-2 gap-[13px]">
            <Field label="Site title">
              <input {...TECH_INPUT} value={p.wpTitle} placeholder="My WordPress Site" onChange={(e) => p.setWpTitle(e.target.value)} className={FIELD_INPUT} />
            </Field>
            <Field label="Language">
              <select value={p.language} onChange={(e) => p.setLanguage(e.target.value)} className={FIELD_SELECT}>
                {LANGUAGES.map((l) => (
                  <option key={l.value} value={l.value}>
                    {l.label}
                  </option>
                ))}
              </select>
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-[13px]">
            <Field label="Admin username">
              <input {...TECH_INPUT} value={p.adminUser} placeholder="admin" onChange={(e) => p.setAdminUser(e.target.value)} className={cn(FIELD_INPUT, "font-mono text-[0.78125rem]")} />
            </Field>
            <Field label="Admin email">
              <input {...TECH_INPUT} value={p.adminEmail} placeholder="you@example.com" onChange={(e) => p.setAdminEmail(e.target.value)} className={cn(FIELD_INPUT, "font-mono text-[0.78125rem]")} />
            </Field>
          </div>
          <Field label="Admin password">
            <div className="flex h-9 items-center rounded-[9px] border border-rex-border-strong bg-rex-well pl-[11px] pr-1.5 transition-colors has-[input:focus]:border-brand has-[input:focus]:shadow-[0_0_0_3px_var(--rex-focus-ring)]">
              <input {...TECH_INPUT}
                autoComplete="new-password"
                type={p.showPassword ? "text" : "password"}
                value={p.adminPassword}
                placeholder="••••••••"
                onChange={(e) => p.setAdminPassword(e.target.value)}
                className="min-w-0 flex-1 bg-transparent font-mono text-[0.78125rem] text-rex-text outline-none focus-visible:shadow-none"
              />
              <button
                onClick={() => p.setShowPassword(!p.showPassword)}
                aria-label="Toggle password visibility"
                className="flex h-7 w-7 flex-none items-center justify-center rounded-[7px] text-rex-text-muted transition-colors hover:bg-rex-hover-strong hover:text-rex-text"
              >
                {p.showPassword ? <EyeOff className="h-[15px] w-[15px]" /> : <Eye className="h-[15px] w-[15px]" />}
              </button>
              <button
                onClick={p.onGeneratePassword}
                className="flex h-7 flex-none items-center gap-1.5 rounded-[7px] px-[9px] text-[0.71875rem] text-brand-tint transition-colors hover:bg-brand-tint-bg"
              >
                <RefreshCw className="h-[13px] w-[13px]" strokeWidth={1.9} />
                Generate
              </button>
            </div>
          </Field>

          {/* Multisite */}
          <div className="rounded-[11px] border border-rex-border-subtle bg-rex-well p-[13px]">
            <div className="flex items-center gap-[11px]">
              <div className="flex-1">
                <div className="text-[0.8125rem] font-medium text-rex-text">Multisite network</div>
                <div className="mt-0.5 text-[0.71875rem] text-rex-text-muted">
                  Run many sites from one WordPress install.
                </div>
              </div>
              <StartStopToggle
                running={p.multisite !== "none"}
                variant="setting"
                onToggle={() => p.setMultisite(p.multisite === "none" ? "subdomain" : "none")}
                label="Enable multisite"
              />
            </div>
            {p.multisite !== "none" && (
              <div className="mt-[13px] grid grid-cols-2 gap-[10px]">
                <MultiCard
                  label="Subdomain"
                  example={`site1.${domainBase}.${p.tld}`}
                  selected={p.multisite === "subdomain"}
                  onClick={() => p.setMultisite("subdomain")}
                />
                <MultiCard
                  label="Subdirectory"
                  example={`${domainBase}.${p.tld}/site1`}
                  selected={p.multisite === "subdirectory"}
                  onClick={() => p.setMultisite("subdirectory")}
                />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export function MultiCard({
  label,
  example,
  selected,
  onClick,
  disabled = false,
  disabledNote,
}: {
  label: string;
  example: string;
  selected: boolean;
  onClick: () => void;
  disabled?: boolean;
  disabledNote?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={disabled ? disabledNote : undefined}
      className={cn(
        "rounded-[10px] border px-3 py-[11px] text-left transition-colors",
        disabled
          ? "cursor-not-allowed border-rex-border-subtle opacity-45"
          : selected
            ? "border-brand"
            : "border-rex-border-subtle hover:border-rex-border-strong",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-[0.78125rem] font-semibold text-rex-text">{label}</span>
        <CheckCircle2
          className="h-4 w-4 flex-none"
          style={{ color: selected && !disabled ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
          strokeWidth={2}
        />
      </div>
      <div className="mt-[5px] font-mono text-[0.6875rem] text-rex-text-muted">
        {disabled && disabledNote ? disabledNote : example}
      </div>
    </button>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <label className="mb-1.5 block text-[0.75rem] text-rex-text-muted">{label}</label>
      {children}
    </div>
  );
}
