import { useQuery } from "@tanstack/react-query";
import { Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { caDownload } from "@/lib/ipc";

/**
 * Settings → SSL, over the web UI only (docs/PLAN-headless-web.md T9): this server's local CA for a
 * browser on ANOTHER machine, reaching the sites through `ssh -D`. Every sentence is Rust's — the
 * note with the cost (`core::ssl::REMOTE_CA_NOTE`) and each OS's steps (`PlatformWords`) — so the
 * card renders and holds no rule.
 */
export function RemoteCaCard() {
  const { data } = useQuery({ queryKey: ["ca-download"], queryFn: caDownload });
  if (!data) return null;
  const save = () => {
    const url = URL.createObjectURL(new Blob([data.pem], { type: "application/x-pem-file" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = data.fileName;
    a.click();
    URL.revokeObjectURL(url);
  };
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5 py-4">
      <div className="flex items-start gap-4">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Open this server&apos;s sites in your browser</div>
          <p className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">{data.note}</p>
        </div>
        <Button size="sm" onClick={save} disabled={!data.pem}>
          <Download className="h-3.5 w-3.5" /> {data.fileName}
        </Button>
      </div>
      <div className="mt-3 space-y-2">
        {data.steps.map((s) => (
          <div key={s.os}>
            <div className="text-[0.75rem] font-medium text-rex-text">{s.os}</div>
            <pre className="mt-0.5 whitespace-pre-wrap font-mono text-[0.6875rem] leading-[1.5] text-rex-text-muted">{s.how}</pre>
          </div>
        ))}
      </div>
    </div>
  );
}
