import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { StartStopToggle } from "@/components/common/StartStopToggle";
import { createSite, listBlueprints, listPhpVersions, wpMultisiteConvert } from "@/lib/ipc";
import type { MultisiteMode, SiteType, WebServer } from "@/types";

/** Web servers selectable in Phase 2 (Apache/OpenLiteSpeed are deferred). */
const SERVERS: { value: WebServer; label: string }[] = [
  { value: "nginx", label: "Nginx" },
  { value: "frankenphp", label: "FrankenPHP" },
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
  <span className="font-mono text-[14px] font-semibold leading-none">&lt;?</span>
);
const WP_GLYPH = (
  <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="block">
    <circle cx="12" cy="12" r="9.2" />
    <path d="M3.2 9.5l4.4 11.3M12 3.2 8.3 14.6 5.7 6.4M20.4 8.4c.4 1 .4 2.4-.2 4l-2.9 8M14.6 5.1l3.4 9.9" />
  </svg>
);
const LARAVEL_GLYPH = (
  <svg width="21" height="21" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" className="block">
    <path d="M3 6.5 7.5 4l4.5 2.5v5L7.5 14 3 11.5z" />
    <path d="M12 11.5 16.5 9 21 11.5v5L16.5 19 12 16.5z" />
    <path d="M7.5 14v5" />
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
  { value: "php", label: "Blank PHP", desc: "A clean document root. Bring your own framework or write plain PHP.", icon: PHP_GLYPH, bg: "rgba(125,128,185,0.16)", border: "rgba(125,128,185,0.30)", color: "#A7AADD" },
  { value: "wordpress", label: "WordPress", desc: "Latest WordPress, installed and ready — admin account and database set up for you.", icon: WP_GLYPH, bg: "rgba(74,134,170,0.16)", border: "rgba(74,134,170,0.30)", color: "#7DB8D8" },
  { value: "laravel", label: "Laravel", desc: "A fresh Laravel app via the installer, wired to a database with your .env ready.", icon: LARAVEL_GLYPH, bg: "rgba(224,82,77,0.14)", border: "rgba(224,82,77,0.28)", color: "#EE837C" },
];

/**
 * New Site dialog — a 2-step wizard (Type → Configure). Creates a Blank-PHP or
 * **WordPress** site with a chosen PHP version and web server; WordPress adds a
 * one-click install. (Phase 2 §1.6 + §4.2 · Phase 3 §1.2.)
 */
export function NewSiteDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const { data: versions = [] } = useQuery({ queryKey: ["php-versions"], queryFn: listPhpVersions });
  const installed = useMemo(() => versions.filter((v) => v.installed), [versions]);
  const defaultVersion = installed.find((v) => v.isDefault)?.minor ?? installed[0]?.minor ?? "8.3";
  const { data: blueprints = [] } = useQuery({ queryKey: ["blueprints"], queryFn: listBlueprints });

  const [step, setStep] = useState<1 | 2>(1);
  const [name, setName] = useState("");
  const [domain, setDomain] = useState("");
  const [siteType, setSiteType] = useState<SiteType>("php");
  const [phpVersion, setPhpVersion] = useState(defaultVersion);
  const [webServer, setWebServer] = useState<WebServer>("nginx");
  const [domainEdited, setDomainEdited] = useState(false);
  const [blueprintId, setBlueprintId] = useState("");

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
  const [adminPassword, setAdminPassword] = useState("");
  const [language, setLanguage] = useState("");
  const [multisite, setMultisite] = useState<MultisiteMode>("none");

  useEffect(() => setPhpVersion(defaultVersion), [defaultVersion]);

  const suggestedDomain = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const effectiveDomain = domainEdited ? domain : suggestedDomain ? `${suggestedDomain}.test` : "";
  const isWordpress = siteType === "wordpress";

  const create = useMutation({
    mutationFn: async () => {
      const site = await createSite(
        { name: name.trim(), domain: effectiveDomain.trim(), type: siteType, phpVersion, webServer, path: "" },
        isWordpress
          ? { title: name.trim(), adminUser: adminUser.trim(), adminEmail: adminEmail.trim(), adminPassword, language }
          : undefined,
        blueprintId || undefined,
      );
      // Convert to multisite after the one-click install (§10.1).
      if (site && isWordpress && multisite !== "none") await wpMultisiteConvert(site.id, multisite);
      return site;
    },
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={onClose}>
      <div
        className="flex max-h-[88vh] w-[516px] flex-col overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header + step indicator */}
        <div className="flex items-center gap-3 border-b border-rex-border-subtle px-5 py-[18px]">
          <div className="flex-1">
            <div className="text-[15.5px] font-semibold text-rex-text">New site</div>
            <div className="mt-px text-[12px] text-rex-text-muted">
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
              blueprintId={blueprintId}
              onPickBlueprint={onPickBlueprint}
              name={name}
              setName={setName}
              effectiveDomain={effectiveDomain}
              onDomain={(v) => {
                setDomainEdited(true);
                setDomain(v);
              }}
              installed={installed}
              defaultVersion={defaultVersion}
              phpVersion={phpVersion}
              setPhpVersion={setPhpVersion}
              webServer={webServer}
              setWebServer={setWebServer}
              isWordpress={isWordpress}
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

        {/* Footer */}
        <div className="flex items-center justify-between gap-3 border-t border-rex-border-subtle px-5 py-[14px]">
          <div>
            {step === 2 && (
              <button
                onClick={() => setStep(1)}
                className="flex h-9 items-center gap-1.5 rounded-[9px] px-3 text-[13px] font-medium text-rex-text-bright transition-colors hover:bg-white/[0.05]"
              >
                <ChevronLeft className="h-[15px] w-[15px]" strokeWidth={2} />
                Back
              </button>
            )}
          </div>
          <div className="flex items-center gap-2.5">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            {step === 1 ? (
              <Button variant="primary" onClick={() => setStep(2)}>
                Continue
                <ChevronRight className="h-[15px] w-[15px]" strokeWidth={2} />
              </Button>
            ) : (
              <Button variant="primary" disabled={!canSubmit} onClick={() => create.mutate()}>
                {create.isPending ? "Creating…" : "Create site"}
              </Button>
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
        "flex items-center gap-[7px] font-mono text-[10.5px]",
        active ? "text-rex-text-bright" : "text-rex-text-dim",
      )}
    >
      <span
        className={cn(
          "flex h-[18px] w-[18px] items-center justify-center rounded-full text-[10px]",
          current ? "bg-brand text-white" : active ? "bg-brand-tint-bg text-brand-tint" : "bg-rex-surface-3 text-rex-text-dim",
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
        <span className="block text-[14px] font-semibold text-rex-text">{card.label}</span>
        <span className="mt-[3px] block text-[12.5px] text-rex-text-muted">{card.desc}</span>
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
  "h-9 w-full rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[13px] text-rex-text outline-none transition-colors focus:border-brand";
const FIELD_SELECT =
  "h-9 w-full rounded-[9px] border border-rex-border-strong bg-rex-well px-[11px] text-[12.5px] text-rex-text outline-none transition-colors focus:border-brand";

function Step2(p: {
  blueprints: import("@/types").Blueprint[];
  blueprintId: string;
  onPickBlueprint: (id: string) => void;
  name: string;
  setName: (v: string) => void;
  effectiveDomain: string;
  onDomain: (v: string) => void;
  installed: import("@/types").PhpVersion[];
  defaultVersion: string;
  phpVersion: string;
  setPhpVersion: (v: string) => void;
  webServer: WebServer;
  setWebServer: (v: WebServer) => void;
  isWordpress: boolean;
  adminUser: string;
  setAdminUser: (v: string) => void;
  adminEmail: string;
  setAdminEmail: (v: string) => void;
  adminPassword: string;
  setAdminPassword: (v: string) => void;
  language: string;
  setLanguage: (v: string) => void;
  multisite: MultisiteMode;
  setMultisite: (v: MultisiteMode) => void;
}) {
  const domainBase = p.effectiveDomain.replace(/\.test$/, "") || "my-site";
  return (
    <div className="flex flex-col gap-3">
      {p.blueprints.length > 0 && (
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

      <div className="grid grid-cols-2 gap-[13px]">
        <Field label="Site name">
          <input autoFocus value={p.name} placeholder="my-site" onChange={(e) => p.setName(e.target.value)} className={FIELD_INPUT} />
        </Field>
        <Field label="Domain">
          <input
            value={p.effectiveDomain}
            placeholder="my-site.test"
            onChange={(e) => p.onDomain(e.target.value)}
            className={cn(FIELD_INPUT, "font-mono text-[12.5px]")}
          />
        </Field>
      </div>

      <div className="grid grid-cols-2 gap-[13px]">
        <Field label="PHP version">
          <select value={p.phpVersion} onChange={(e) => p.setPhpVersion(e.target.value)} className={cn(FIELD_SELECT, "font-mono")}>
            {p.installed.length === 0 && <option value={p.defaultVersion}>{p.defaultVersion}</option>}
            {p.installed.map((v) => (
              <option key={v.minor} value={v.minor}>
                {v.minor}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Web server">
          <select value={p.webServer} onChange={(e) => p.setWebServer(e.target.value as WebServer)} className={FIELD_SELECT}>
            {SERVERS.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </select>
        </Field>
      </div>

      {p.isWordpress && (
        <div className="mt-1 flex flex-col gap-[14px] border-t border-rex-border-subtle pt-[15px]">
          <div className="font-mono text-[10px] uppercase tracking-[0.13em] text-rex-text-label">
            WordPress install
          </div>
          <div className="grid grid-cols-2 gap-[13px]">
            <Field label="Admin username">
              <input value={p.adminUser} placeholder="admin" onChange={(e) => p.setAdminUser(e.target.value)} className={cn(FIELD_INPUT, "font-mono text-[12.5px]")} />
            </Field>
            <Field label="Admin email">
              <input value={p.adminEmail} placeholder="you@example.com" onChange={(e) => p.setAdminEmail(e.target.value)} className={cn(FIELD_INPUT, "font-mono text-[12.5px]")} />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-[13px]">
            <Field label="Admin password">
              <input type="password" value={p.adminPassword} placeholder="••••••••" onChange={(e) => p.setAdminPassword(e.target.value)} className={cn(FIELD_INPUT, "font-mono text-[12.5px]")} />
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

          {/* Multisite */}
          <div className="rounded-[11px] border border-rex-border-subtle bg-rex-well p-[13px]">
            <div className="flex items-center gap-[11px]">
              <div className="flex-1">
                <div className="text-[13px] font-medium text-rex-text">Multisite network</div>
                <div className="mt-0.5 text-[11.5px] text-rex-text-muted">
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
                  example={`site1.${domainBase}.test`}
                  selected={p.multisite === "subdomain"}
                  onClick={() => p.setMultisite("subdomain")}
                />
                <MultiCard
                  label="Subdirectory"
                  example={`${domainBase}.test/site1`}
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

function MultiCard({ label, example, selected, onClick }: { label: string; example: string; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "rounded-[10px] border px-3 py-[11px] text-left transition-colors",
        selected ? "border-brand" : "border-rex-border-subtle hover:border-rex-border-strong",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-[12.5px] font-semibold text-rex-text">{label}</span>
        <CheckCircle2
          className="h-4 w-4 flex-none"
          style={{ color: selected ? "var(--rex-brand)" : "var(--rex-border-strong)" }}
          strokeWidth={2}
        />
      </div>
      <div className="mt-[5px] font-mono text-[11px] text-rex-text-muted">{example}</div>
    </button>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <label className="mb-1.5 block text-[12px] text-rex-text-muted">{label}</label>
      {children}
    </div>
  );
}
