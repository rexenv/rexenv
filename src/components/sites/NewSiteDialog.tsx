import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { createSite, listPhpVersions } from "@/lib/ipc";

/**
 * New Site dialog (Phase 2 §1.6). Creates a Blank-PHP site with a chosen PHP
 * version; the server dropdown lands in §4.2 and WordPress one-click is a
 * follow-up, so this sends type=php / webServer=nginx.
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
  const [phpVersion, setPhpVersion] = useState(defaultVersion);
  const [domainEdited, setDomainEdited] = useState(false);

  useEffect(() => setPhpVersion(defaultVersion), [defaultVersion]);

  // Suggest a domain from the name until the user edits it.
  const suggestedDomain = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const effectiveDomain = domainEdited ? domain : suggestedDomain ? `${suggestedDomain}.test` : "";

  const create = useMutation({
    mutationFn: () =>
      createSite({
        name: name.trim(),
        domain: effectiveDomain.trim(),
        type: "php",
        phpVersion,
        webServer: "nginx",
        path: "",
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites"] });
      onClose();
    },
    onError: (e) => window.alert(String(e)),
  });

  const canSubmit = name.trim() !== "" && effectiveDomain.trim() !== "" && !create.isPending;

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
