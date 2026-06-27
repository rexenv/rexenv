import { useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { LayoutGrid } from "lucide-react";
import { listPhpVersions, listSites, setSitePhpVersion } from "@/lib/ipc";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
      <div className="mb-3 text-[13px] font-semibold text-rex-text">{title}</div>
      {children}
    </div>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between border-b border-rex-border-subtle py-2 last:border-b-0">
      <span className="text-[12.5px] text-rex-text-muted">{label}</span>
      <span className="font-mono text-[12px] text-rex-text">{value}</span>
    </div>
  );
}

export function SiteDetail() {
  const { id } = useParams();
  const qc = useQueryClient();

  const { data: sites = [] } = useQuery({ queryKey: ["sites"], queryFn: listSites });
  const { data: versions = [] } = useQuery({
    queryKey: ["php-versions"],
    queryFn: listPhpVersions,
  });
  const site = sites.find((s) => s.id === id);

  const switchPhp = useMutation({
    mutationFn: (version: string) => setSitePhpVersion(id!, version),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sites"] }),
    onError: (e) => window.alert(String(e)),
  });

  if (!site) {
    return (
      <>
        <TopBar title="Site" showSearch={false} />
        <Placeholder
          icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
          label="Site not found"
          hint="It may have been deleted."
        />
      </>
    );
  }

  // Installed versions, plus the site's current one (so the select always shows it).
  const options = versions.filter((v) => v.installed).map((v) => v.minor);
  if (!options.includes(site.phpVersion)) options.unshift(site.phpVersion);

  return (
    <>
      <TopBar title={site.name} subtitle={site.domain} showSearch={false} />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          <Card title="Configuration">
            <div className="flex items-center justify-between border-b border-rex-border-subtle py-2">
              <span className="text-[12.5px] text-rex-text-muted">PHP version</span>
              <select
                value={site.phpVersion}
                disabled={switchPhp.isPending}
                onChange={(e) => switchPhp.mutate(e.target.value)}
                className="h-[30px] rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none transition-colors focus:border-brand disabled:opacity-50"
              >
                {options.map((m) => (
                  <option key={m} value={m}>
                    PHP {m}
                  </option>
                ))}
              </select>
            </div>
            <Field label="Web server" value={site.webServer} />
            <Field label="Type" value={site.type} />
            <Field label="Domain" value={site.domain} />
            <Field label="Document root" value={site.path} />
          </Card>
          <Placeholder
            icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Site detail · more tabs"
            hint="WordPress, Database, Logs render here"
          />
        </div>
      </div>
    </>
  );
}
