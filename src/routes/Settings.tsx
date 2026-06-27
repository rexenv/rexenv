import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { TopBar } from "@/components/shell/TopBar";
import { Button } from "@/components/ui/button";
import { getSetting, setSetting, sitesFolder } from "@/lib/ipc";

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

export function Settings() {
  return (
    <>
      <TopBar title="Settings" showSearch={false} />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="mx-auto max-w-2xl">
          <Card title="General">
            <SitesFolderSetting />
          </Card>
        </div>
      </div>
    </>
  );
}
