import { useEffect, useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Mail as MailIcon, Search, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { StatusPill } from "@/components/common/StatusPill";
import {
  mailpitClear,
  mailpitMessage,
  mailpitMessageRaw,
  mailpitMessages,
  mailpitStatus,
  openExternal,
} from "@/lib/ipc";
import type { MailSummary } from "@/types";

type PreviewTab = "html" | "text" | "raw" | "headers";

const TAB_LABEL: Record<PreviewTab, string> = {
  html: "HTML",
  text: "Text",
  raw: "Raw source",
  headers: "Headers",
};

const AVATAR_COLORS = [
  { color: "var(--rex-accent-blue)", bg: "var(--rex-accent-blue-bg)" },
  { color: "var(--rex-accent-red)", bg: "var(--rex-accent-red-bg)" },
  { color: "var(--rex-accent-periwinkle)", bg: "var(--rex-accent-periwinkle-bg)" },
  { color: "var(--rex-accent-teal)", bg: "var(--rex-accent-teal-bg)" },
  { color: "var(--rex-accent-amber)", bg: "var(--rex-accent-amber-bg)" },
];

/** A stable per-sender accent so each correspondent reads as a distinct avatar. */
function avatarColor(seed: string) {
  let h = 0;
  for (const ch of seed) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return AVATAR_COLORS[h % AVATAR_COLORS.length];
}

function initial(a: { name: string; address: string }): string {
  return (a.name || a.address).trim().charAt(0).toUpperCase() || "?";
}

export function Mail() {
  const qc = useQueryClient();
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<PreviewTab>("html");

  const { data: mp } = useQuery({
    queryKey: ["mailpit-status"],
    queryFn: mailpitStatus,
    refetchInterval: 5000,
  });
  const { data: list } = useQuery({
    queryKey: ["mailpit-messages", search],
    queryFn: () => mailpitMessages(search),
    refetchInterval: 5000,
  });

  const messages = list?.messages ?? [];
  // Keep a valid selection as the inbox changes.
  useEffect(() => {
    if (selectedId && !messages.some((m) => m.id === selectedId)) setSelectedId(null);
  }, [messages, selectedId]);

  const clear = useMutation({
    mutationFn: mailpitClear,
    onSuccess: () => {
      setSelectedId(null);
      qc.invalidateQueries({ queryKey: ["mailpit-messages"] });
    },
    onError: (e) => toastBackendError(e),
  });

  const running = !!mp?.running;
  const apiPort = (() => {
    try {
      return new URL(mp?.uiUrl ?? "").port || "8025";
    } catch {
      return "8025";
    }
  })();

  return (
    <>
      <TopBar
        title="Mail"
        subtitle={list ? `${list.total} captured · ${list.unread} unread` : "Mailpit"}
        showSearch={false}
      />

      <div className="flex items-center justify-between gap-3 border-b border-rex-border px-[18px] py-2.5">
        <StatusPill
          status={running ? "running" : "stopped"}
          label={running ? `Mailpit · :${apiPort}` : "Mailpit stopped"}
        />
        <div className="flex items-center gap-2">
          {mp && (
            <button
              onClick={() => openExternal(mp.uiUrl)}
              disabled={!running}
              className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-brand disabled:opacity-40"
            >
              <ExternalLink className="h-3.5 w-3.5" />
              Open Mailpit
            </button>
          )}
          <button
            onClick={() => clear.mutate()}
            disabled={clear.isPending || messages.length === 0}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[12px] text-rex-text transition-colors hover:border-status-error/60 hover:text-status-error-bright disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border disabled:hover:text-rex-text"
          >
            <Trash2 className="h-3.5 w-3.5" />
            Clear all
          </button>
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* List pane */}
        <div className="flex w-[344px] shrink-0 flex-col border-r border-rex-border">
          <div className="border-b border-rex-border p-2.5">
            <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5">
              <Search className="h-3.5 w-3.5 text-rex-text-muted" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search mail…"
                className="h-[34px] flex-1 bg-transparent text-[12.5px] text-rex-text outline-none placeholder:text-rex-text-muted"
              />
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {messages.length === 0 ? (
              <div className="p-6 text-center text-[12.5px] text-rex-text-muted">
                {search ? "No messages match your search." : "No mail captured yet."}
              </div>
            ) : (
              messages.map((m) => (
                <MessageRow
                  key={m.id}
                  m={m}
                  active={m.id === selectedId}
                  onClick={() => {
                    setSelectedId(m.id);
                    setTab("html");
                  }}
                />
              ))
            )}
          </div>
          {list && (
            <div className="flex-none border-t border-rex-border px-3 py-2 font-mono text-[10.5px] text-rex-text-dim">
              {list.total} messages · {list.unread} unread
            </div>
          )}
        </div>

        {/* Preview pane */}
        <div className="min-h-0 flex-1 overflow-hidden">
          {selectedId ? (
            <Preview id={selectedId} tab={tab} onTab={setTab} />
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
              <div className="flex h-[46px] w-[46px] items-center justify-center rounded-xl border border-rex-border-strong bg-rex-surface-1 text-rex-text-faint">
                <MailIcon className="h-[22px] w-[22px]" strokeWidth={1.6} />
              </div>
              <div>
                <div className="text-[14px] font-semibold text-rex-text">
                  {messages.length ? "Nothing selected" : "No emails yet"}
                </div>
                <div className="mt-1 text-[12.5px] text-rex-text-muted">
                  {messages.length
                    ? "Pick a message on the left to preview it."
                    : "Outgoing email from your sites is captured here."}
                </div>
              </div>
              <span className="rounded-md border border-rex-border-strong bg-rex-surface-1 px-2.5 py-1 font-mono text-[10.5px] text-rex-text-dim">
                SMTP · 127.0.0.1:{mp?.smtpPort ?? 1025} · auto-configured
              </span>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function MessageRow({ m, active, onClick }: { m: MailSummary; active: boolean; onClick: () => void }) {
  const recipient = m.to[0]?.address ?? "—";
  return (
    <button
      onClick={onClick}
      className={cn(
        "relative flex w-full flex-col gap-0.5 border-b border-rex-border-subtle py-2.5 pl-4 pr-3 text-left transition-colors",
        active ? "bg-brand-active" : "hover:bg-rex-surface-2/50",
      )}
    >
      {(active || !m.read) && (
        <span className="absolute left-0 top-0 h-full w-[2.5px] bg-brand" />
      )}
      <div className="flex items-center gap-2">
        {!m.read && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-brand" />}
        <span className={`flex-1 truncate text-[12.5px] ${m.read ? "text-rex-text-muted" : "font-semibold text-rex-text"}`}>
          {m.from.name || m.from.address}
        </span>
        <span className="shrink-0 font-mono text-[10.5px] text-rex-text-muted">{shortTime(m.created)}</span>
      </div>
      <div className="truncate text-[12px] text-rex-text">{m.subject || "(no subject)"}</div>
      <div className="truncate font-mono text-[11px] text-rex-text-muted">
        <span className="text-rex-text-dim">to </span>
        {recipient}
      </div>
    </button>
  );
}

function Preview({
  id,
  tab,
  onTab,
}: {
  id: string;
  tab: PreviewTab;
  onTab: (t: PreviewTab) => void;
}) {
  const { data: msg } = useQuery({ queryKey: ["mailpit-message", id], queryFn: () => mailpitMessage(id) });
  const { data: raw } = useQuery({
    queryKey: ["mailpit-raw", id],
    queryFn: () => mailpitMessageRaw(id),
    enabled: tab === "raw",
  });

  if (!msg) return <div className="p-6 text-[12.5px] text-rex-text-muted">Loading…</div>;

  const tabs: PreviewTab[] = ["html", "text", "raw", "headers"];
  const avatar = avatarColor(msg.from.address);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-start gap-3 border-b border-rex-border p-4">
        <span
          className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg text-[12px] font-semibold"
          style={{ background: avatar.bg, color: avatar.color }}
        >
          {initial(msg.from)}
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[14px] font-semibold text-rex-text">{msg.subject || "(no subject)"}</div>
        <div className="mt-1.5 flex flex-col gap-0.5 text-[12px] text-rex-text-muted">
          <span>
            <span className="text-rex-text-muted">From </span>
            <span className="font-mono text-rex-text">{addr(msg.from)}</span>
          </span>
          <span>
            <span className="text-rex-text-muted">To </span>
            <span className="font-mono text-rex-text">{msg.to.map(addr).join(", ")}</span>
          </span>
          {msg.cc.length > 0 && (
            <span>
              <span className="text-rex-text-muted">Cc </span>
              <span className="font-mono text-rex-text">{msg.cc.map(addr).join(", ")}</span>
            </span>
          )}
          </div>
        </div>
      </div>

      <div className="flex gap-1 border-b border-rex-border px-4">
        {tabs.map((t) => (
          <button
            key={t}
            onClick={() => onTab(t)}
            className={`-mb-px border-b-2 px-3 py-2 text-[12.5px] transition-colors ${
              tab === t
                ? "border-brand font-medium text-brand-tint"
                : "border-transparent text-rex-text-muted hover:text-rex-text"
            }`}
          >
            {TAB_LABEL[t]}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "html" &&
          (msg.html ? (
            // Sandboxed: isolates the email's styles/scripts from the app.
            <iframe title="HTML preview" sandbox="" srcDoc={msg.html} className="h-full w-full bg-white" />
          ) : (
            <Empty label="No HTML part" />
          ))}
        {tab === "text" &&
          (msg.text ? (
            <pre className="whitespace-pre-wrap p-4 font-mono text-[12px] text-rex-text">{msg.text}</pre>
          ) : (
            <Empty label="No plain-text part" />
          ))}
        {tab === "raw" && (
          <pre className="whitespace-pre-wrap p-4 font-mono text-[11.5px] text-rex-text-muted">
            {raw ?? "Loading…"}
          </pre>
        )}
        {tab === "headers" && (
          <div className="flex flex-col gap-1 p-4">
            {msg.headers.map((h, i) => (
              <div key={`${h.name}-${i}`} className="flex gap-2 font-mono text-[11.5px]">
                <span className="w-[120px] shrink-0 text-rex-text-muted">{h.name}</span>
                <span className="break-all text-rex-text">{h.value}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function Empty({ label }: { label: string }) {
  return <div className="p-6 text-[12.5px] text-rex-text-muted">{label}</div>;
}

function addr(a: { name: string; address: string }): string {
  return a.name ? `${a.name} <${a.address}>` : a.address;
}

function shortTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
