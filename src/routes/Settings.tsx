import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { TopBar } from "@/components/shell/TopBar";
import { Button } from "@/components/ui/button";
import {
  getSetting,
  listPhpVersions,
  setPhpVersionInstalled,
  setSetting,
  sitesFolder,
} from "@/lib/ipc";
import type { PhpVersion } from "@/types";

const SITES_DIR_KEY = "sites_dir";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="mb-3 text-[13px] font-semibold text-rex-text">{title}</div>
      {children}
    </div>
  );
}

function SitesFolderSetting() {
  const qc = useQueryClient();
  // Resolved folder (for the placeholder) + the raw override setting.
  const { data: resolved } = useQuery({ queryKey: ["sites-folder"], queryFn: sitesFolder });
  const { data: override } = useQuery({
    queryKey: ["setting", SITES_DIR_KEY],
    queryFn: () => getSetting(SITES_DIR_KEY),
  });
  const [value, setValue] = useState("");
  useEffect(() => {
    if (override != null) setValue(override);
  }, [override]);

  const save = useMutation({
    mutationFn: (v: string) => setSetting(SITES_DIR_KEY, v),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["sites-folder"] });
      qc.invalidateQueries({ queryKey: ["setting", SITES_DIR_KEY] });
    },
  });

  return (
    <div>
      <label className="mb-1.5 block text-[12.5px] text-rex-text-muted">
        Sites folder
      </label>
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={value}
          placeholder={resolved ?? "default"}
          onChange={(e) => setValue(e.target.value)}
          className="h-[34px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-3 font-mono text-[12.5px] text-rex-text outline-none transition-colors focus:border-brand"
        />
        <Button
          variant="primary"
          disabled={save.isPending}
          onClick={() => save.mutate(value.trim())}
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
      <div className="mt-2 font-mono text-[11px] text-rex-text-dim">
        New sites are created under: {resolved ?? "…"}
        {!override && " (default)"}
      </div>
    </div>
  );
}

function PhpVersionRow({
  v,
  busy,
  onToggle,
}: {
  v: PhpVersion;
  busy: boolean;
  onToggle: (installed: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle py-2.5 last:border-b-0">
      <div className="min-w-0 flex-1">
        <span className="font-mono text-[13px] text-rex-text">PHP {v.minor}</span>
        <span className="ml-2 font-mono text-[11px] text-rex-text-dim">{v.patch}</span>
        {v.isDefault && (
          <span className="ml-2 rounded border border-brand/40 bg-brand/10 px-1.5 py-0.5 text-[10px] font-medium text-brand">
            Default
          </span>
        )}
      </div>
      {v.installed ? (
        <>
          <span className="text-[11.5px] text-status-running">Installed</span>
          {!v.isDefault && (
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => onToggle(false)}
              className="hover:text-status-error"
            >
              {busy ? "…" : "Remove"}
            </Button>
          )}
        </>
      ) : (
        <Button variant="primary" disabled={busy} onClick={() => onToggle(true)}>
          {busy ? "…" : "Install"}
        </Button>
      )}
    </div>
  );
}

function PhpVersionsSetting() {
  const qc = useQueryClient();
  const { data: versions = [], isLoading } = useQuery({
    queryKey: ["php-versions"],
    queryFn: listPhpVersions,
  });
  const toggle = useMutation({
    mutationFn: ({ minor, installed }: { minor: string; installed: boolean }) =>
      setPhpVersionInstalled(minor, installed),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["php-versions"] }),
    onError: (e) => window.alert(String(e)),
  });

  if (isLoading) {
    return <div className="text-[12.5px] text-rex-text-muted">Loading…</div>;
  }
  return (
    <div>
      {versions.map((v) => (
        <PhpVersionRow
          key={v.minor}
          v={v}
          busy={toggle.isPending && toggle.variables?.minor === v.minor}
          onToggle={(installed) => toggle.mutate({ minor: v.minor, installed })}
        />
      ))}
      <div className="mt-2.5 text-[11px] text-rex-text-dim">
        Installed versions each run a php-fpm pool; a site picks its version in its detail view.
      </div>
    </div>
  );
}

export function Settings() {
  return (
    <>
      <TopBar title="Settings" showSearch={false} />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          <Card title="General">
            <SitesFolderSetting />
          </Card>
          <Card title="PHP versions">
            <PhpVersionsSetting />
          </Card>
        </div>
      </div>
    </>
  );
}
