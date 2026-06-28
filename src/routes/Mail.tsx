import { useQuery } from "@tanstack/react-query";
import { ExternalLink, Mail as MailIcon } from "lucide-react";
import { TopBar } from "@/components/shell/TopBar";
import { Placeholder } from "@/components/common/Placeholder";
import { mailpitStatus, openExternal } from "@/lib/ipc";

export function Mail() {
  // Mailpit health drives the status pill; the two-pane inbox lands in §2.3.
  const { data: mp } = useQuery({
    queryKey: ["mailpit-status"],
    queryFn: mailpitStatus,
    refetchInterval: 5000,
  });
  const running = !!mp?.running;

  return (
    <>
      <TopBar title="Mail" subtitle="Mailpit" showSearch={false} />
      <div className="min-h-0 flex-1 overflow-auto p-[18px]">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2.5">
                <span
                  className={`h-2 w-2 rounded-full ${running ? "bg-emerald-500" : "bg-rex-text-muted"}`}
                />
                <span className="text-[13px] font-semibold text-rex-text">
                  Mailpit {running ? "running" : "stopped"}
                </span>
              </div>
              {mp && (
                <button
                  onClick={() => openExternal(mp.uiUrl)}
                  disabled={!running}
                  className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-3 py-1.5 text-[12.5px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border"
                >
                  <ExternalLink className="h-4 w-4" />
                  Open Mailpit
                </button>
              )}
            </div>
            {mp && (
              <div className="mt-3 flex flex-col gap-1.5 border-t border-rex-border-subtle pt-3">
                <Row label="SMTP" value={`127.0.0.1:${mp.smtpPort}`} />
                <Row label="Web / API" value={mp.uiUrl} />
              </div>
            )}
          </div>

          <Placeholder
            icon={<MailIcon className="h-[22px] w-[22px]" strokeWidth={1.6} />}
            label="Inbox"
            hint="Captured outgoing email (Mailpit) renders here in §2.3"
          />
        </div>
      </div>
    </>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-[12.5px] text-rex-text-muted">{label}</span>
      <span className="font-mono text-[12px] text-rex-text">{value}</span>
    </div>
  );
}
