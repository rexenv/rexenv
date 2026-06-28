import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { createSite, listPhpVersions } from "@/lib/ipc";
import type { SiteType, WebServer } from "@/types";

/** Web servers selectable in Phase 2 (Apache/OpenLiteSpeed are deferred). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
];

/** Site types selectable now (Laravel one-click is a later phase). */
const TYPES: { value: SiteType; label: string }[] = [
  { value: "php", label: "Blank PHP" },
  { value: "wordpress", label: "WordPress" },
];

/** WordPress locales offered in the dialog ("" → default en_US). */
const LANGUAGES: { value: string; label: string }[] = [
  { value: "", label: "English (United States)" },
  { value: "en_GB", label: "English (UK)" },
  { value: "es_ES", label: "Español" },
  { value: "fr_FR", label: "Français" },
  { value: "de_DE", label: "Deutsch" },
];

/**
 * New Site dialog (Phase 2 §1.6 + §4.2 · Phase 3 §1.2). Creates a Blank-PHP or
 * **WordPress** site with a chosen PHP version and web server. WordPress adds a
 * one-click install (site title + admin account + language); the backend brings
 * MySQL up and runs the installer. (Multisite lands in §10.)
 */
export function NewSiteDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const { data: versions = [] } = useQuery({
    queryKey: ["php-versions"],
    queryFn: listPhpVersions,
  });
  const installed = useMemo(() => versions.filter((v) => v.installed), [versions]);
  const defaultVersion = installed.find((v) => v.isDefault)?.minor ?? installed[0]?.minor ?? "8.3";

  const [name, setName] = useState("");
  const [domain, setDomain] = useState("");
  const [siteType, setSiteType] = useState<SiteType>("php");
  const [phpVersion, setPhpVersion] = useState(defaultVersion);
  const [webServer, setWebServer] = useState<WebServer>("nginx");
  const [domainEdited, setDomainEdited] = useState(false);

  // WordPress one-click fields (used only when siteType === "wordpress").
  const [adminUser, setAdminUser] = useState("admin");
  const [adminEmail, setAdminEmail] = useState("");
  const [adminPassword, setAdminPassword] = useState("");
  const [language, setLanguage] = useState("");

  useEffect(() => setPhpVersion(defaultVersion), [defaultVersion]);

  // Suggest a domain from the name until the user edits it.
  const suggestedDomain = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const effectiveDomain = domainEdited ? domain : suggestedDomain ? `${suggestedDomain}.test` : "";
  const isWordpress = siteType === "wordpress";

  const create = useMutation({
    mutationFn: () =>
      createSite(
        {
          name: name.trim(),
          domain: effectiveDomain.trim(),
          type: siteType,
          phpVersion,
          webServer,
          path: "",
        },
        isWordpress
          ? {
              title: name.trim(),
              adminUser: adminUser.trim(),
              adminEmail: adminEmail.trim(),
              adminPassword,
              language,
            }
          : undefined,
      ),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites"] });
      onClose();
    },
    onError: (e) => window.alert(String(e)),
  });

  const canSubmit =
    name.trim() !== "" &&
    effectiveDomain.trim() !== "" &&
    (!isWordpress || (adminUser.trim() !== "" && adminPassword !== "")) &&
    !create.isPending;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-[440px] rounded-xl border border-rex-border bg-rex-surface-1 p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <div className="text-[15px] font-semibold text-rex-text">New site</div>
          <Button variant="ghost" size="icon" aria-label="Close" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </div>

        <div className="flex flex-col gap-3">
          <Field label="Name">
            <input
              autoFocus
              type="text"
              value={name}
              placeholder="My Site"
              onChange={(e) => setName(e.target.value)}
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 text-[13px] text-rex-text outline-none focus:border-brand"
            />
          </Field>

          <Field label="Domain">
            <input
              type="text"
              value={effectiveDomain}
              placeholder="my-site.test"
              onChange={(e) => {
                setDomainEdited(true);
                setDomain(e.target.value);
              }}
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none focus:border-brand"
            />
          </Field>

          <Field label="Type">
            <select
              value={siteType}
              onChange={(e) => setSiteType(e.target.value as SiteType)}
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 text-[12.5px] text-rex-text outline-none focus:border-brand"
            >
              {TYPES.map((t) => (
                <option key={t.value} value={t.value}>
                  {t.label}
                </option>
              ))}
            </select>
          </Field>

          <Field label="PHP version">
            <select
              value={phpVersion}
              onChange={(e) => setPhpVersion(e.target.value)}
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12.5px] text-rex-text outline-none focus:border-brand"
            >
              {installed.length === 0 && <option value={defaultVersion}>PHP {defaultVersion}</option>}
              {installed.map((v) => (
                <option key={v.minor} value={v.minor}>
                  PHP {v.minor}
                </option>
              ))}
            </select>
          </Field>

          <Field label="Web server">
            <select
              value={webServer}
              onChange={(e) => setWebServer(e.target.value as WebServer)}
              className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 text-[12.5px] text-rex-text outline-none focus:border-brand"
            >
              {SERVERS.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </Field>

          {isWordpress && (
            <div className="mt-1 flex flex-col gap-3 rounded-lg border border-rex-border bg-rex-surface-2/40 p-3">
              <div className="text-[12px] font-medium text-rex-text-muted">WordPress install</div>
              <Field label="Admin username">
                <input
                  type="text"
                  value={adminUser}
                  placeholder="admin"
                  onChange={(e) => setAdminUser(e.target.value)}
                  className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none focus:border-brand"
                />
              </Field>
              <Field label="Admin email">
                <input
                  type="email"
                  value={adminEmail}
                  placeholder={effectiveDomain ? `admin@${effectiveDomain}` : "admin@my-site.test"}
                  onChange={(e) => setAdminEmail(e.target.value)}
                  className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none focus:border-brand"
                />
              </Field>
              <Field label="Admin password">
                <input
                  type="password"
                  value={adminPassword}
                  placeholder="••••••••"
                  onChange={(e) => setAdminPassword(e.target.value)}
                  className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none focus:border-brand"
                />
              </Field>
              <Field label="Language">
                <select
                  value={language}
                  onChange={(e) => setLanguage(e.target.value)}
                  className="h-[34px] w-full rounded border border-rex-border bg-rex-surface-2 px-2 text-[12.5px] text-rex-text outline-none focus:border-brand"
                >
                  {LANGUAGES.map((l) => (
                    <option key={l.value} value={l.value}>
                      {l.label}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
          )}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!canSubmit} onClick={() => create.mutate()}>
            {create.isPending ? "Creating…" : "Create site"}
          </Button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="mb-1.5 block text-[12px] text-rex-text-muted">{label}</label>
      {children}
    </div>
  );
}
