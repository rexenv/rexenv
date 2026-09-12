import { useEffect, useMemo, useState } from "react";
import { toastBackendError } from "@/lib/toast";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCheck, ChevronRight, Globe, Mail as MailIcon, Search, Trash2 } from "lucide-react";
import { PreferredBrowserIcon } from "@/components/ui/open-in";
import { cn, TECH_INPUT } from "@/lib/utils";
import { TopBar } from "@/components/shell/TopBar";
import { StatusPill } from "@/components/common/StatusPill";
// In-house dialog, NOT window.confirm: tauri-plugin-dialog replaces the native
// confirm with an ASYNC override (a bare `if (!confirm(...))` never blocks).
import { confirm } from "@/components/ui/dialog";
import {
  listSites,
  mailpitClear,
  mailpitDelete,
  mailpitMarkAllRead,
  mailpitMessage,
  mailpitMessageRaw,
  mailpitMessages,
  mailpitStatus,
  openExternal,
} from "@/lib/ipc";
import type { MailList, MailSummary, Site } from "@/types";

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

const OTHER_GROUP_KEY = "__other__";

interface MailGroup {
  key: string; // site id, or OTHER_GROUP_KEY for unmatched mail
  name: string;
  domain: string | null;
  messages: MailSummary[];
  unread: number;
  latest: number; // most recent message timestamp (group sort key)
}

function addressDomain(address: string): string | null {
  const at = address.lastIndexOf("@");
  return at === -1 ? null : address.slice(at + 1).toLowerCase();
}

/** True when a mail domain belongs to a site — exact, or a subdomain (multisite). */
function domainMatches(mailDomain: string, siteDomain: string): boolean {
  return mailDomain === siteDomain || mailDomain.endsWith(`.${siteDomain}`);
}

/** Match a message to a site by sender domain first, then any recipient domain. */
function siteFor(m: MailSummary, sites: Site[]): Site | null {
  const domains = [m.from.address, ...m.to.map((a) => a.address)]
    .map(addressDomain)
    .filter((d): d is string => d !== null);
  for (const d of domains) {
    const site = sites.find((s) => domainMatches(d, s.domain.toLowerCase()));
    if (site) return site;
  }
  return null;
}

/** Group messages by site, groups sorted by latest activity, messages newest first. */
function groupBySite(messages: MailSummary[], sites: Site[]): MailGroup[] {
  const byKey = new Map<string, MailGroup>();
  for (const m of messages) {
    const site = siteFor(m, sites);
    const key = site?.id ?? OTHER_GROUP_KEY;
    let g = byKey.get(key);
    if (!g) {
      g = {
        key,
        name: site?.name ?? "Other",
        domain: site?.domain ?? null,
        messages: [],
        unread: 0,
        latest: 0,
      };
      byKey.set(key, g);
    }
    g.messages.push(m);
    if (!m.read) g.unread += 1;
    const t = new Date(m.created).getTime();
    if (!Number.isNaN(t) && t > g.latest) g.latest = t;
  }
  const groups = [...byKey.values()];
  for (const g of groups) {
    g.messages.sort((a, b) => new Date(b.created).getTime() - new Date(a.created).getTime());
  }
  // "Other" always sinks below real sites; sites sort by most recent activity.
  groups.sort((a, b) => {
    if ((a.key === OTHER_GROUP_KEY) !== (b.key === OTHER_GROUP_KEY)) {
      return a.key === OTHER_GROUP_KEY ? 1 : -1;
    }
    return b.latest - a.latest;
  });
  return groups;
}

export function Mail() {
  const qc = useQueryClient();
  const [search, setSearch] = useState("");
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [siteFilter, setSiteFilter] = useState<string>("all");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [tab, setTab] = useState<PreviewTab>("html");

  const { data: mp } = useQuery({
    queryKey: ["mailpit-status"],
    queryFn: mailpitStatus,
    refetchInterval: 5000,
  });
  // The unread filter is part of the KEY, not something applied to the result:
  // it is a Mailpit search (`is:unread`), so the server decides what comes back.
  // Filtering the returned page here would only hide the unread messages that
  // happened to be on it — which is the "I can't find the new ones" problem the
  // filter exists to solve, wearing a fix.
  const { data: list } = useQuery({
    queryKey: ["mailpit-messages", search, unreadOnly],
    queryFn: () => mailpitMessages(search, unreadOnly),
    refetchInterval: 5000,
  });
  const { data: sites } = useQuery({ queryKey: ["sites"], queryFn: listSites });

  // The message being read STAYS on the list while it is open, even after the
  // preview marks it read and the unread filter would drop it. Without this the
  // row vanished under the cursor on the next poll and took the preview with it
  // — the filter would punish you for using it.
  const [pinned, setPinned] = useState<MailSummary | null>(null);
  const messages = useMemo(() => {
    const base = list?.messages ?? [];
    if (!pinned || base.some((m) => m.id === pinned.id)) return base;
    // Only while the filter is what removed it. If the message is gone for real
    // (deleted, inbox cleared), it must disappear like any other.
    if (!unreadOnly || !list) return base;
    return [{ ...pinned, read: true }, ...base];
  }, [list, pinned, unreadOnly]);
  const groups = useMemo(() => groupBySite(messages, sites ?? []), [messages, sites]);
  const visibleGroups = siteFilter === "all" ? groups : groups.filter((g) => g.key === siteFilter);

  // Drop a stale site filter once its group no longer exists (e.g. after Clear all).
  useEffect(() => {
    if (siteFilter !== "all" && messages.length > 0 && !groups.some((g) => g.key === siteFilter)) {
      setSiteFilter("all");
    }
  }, [groups, messages.length, siteFilter]);

  const toggleGroup = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  // Keep a valid selection as the inbox changes. `messages` already carries the
  // pinned row, so a message that only left the LIST (because it is now read
  // and the unread filter is on) keeps its preview open.
  useEffect(() => {
    if (selectedId && !messages.some((m) => m.id === selectedId)) {
      setSelectedId(null);
      setPinned(null);
    }
  }, [messages, selectedId]);
  // Prune checked IDs that no longer exist (deleted elsewhere / new search).
  useEffect(() => {
    setChecked((prev) => {
      const live = new Set(messages.map((m) => m.id));
      const next = new Set([...prev].filter((id) => live.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [messages]);

  const clear = useMutation({
    mutationFn: mailpitClear,
    onSuccess: () => {
      setSelectedId(null);
      setChecked(new Set());
      qc.invalidateQueries({ queryKey: ["mailpit-messages"] });
    },
    onError: (e) => toastBackendError(e),
  });
  const markAllRead = useMutation({
    mutationFn: mailpitMarkAllRead,
    // Patch what is on screen before the refetch lands, for the same reason the
    // preview does (see `Preview`): the poll is 5s wide, and a button whose
    // effect appears somewhere in the next five seconds reads as a button that
    // didn't work.
    onSuccess: () => {
      qc.setQueriesData<MailList>({ queryKey: ["mailpit-messages"] }, (old) =>
        old ? { ...old, unread: 0, messages: old.messages.map((m) => ({ ...m, read: true })) } : old,
      );
      // No invalidate here on purpose. The PUT succeeded, so the patch IS the
      // truth, and the 5s poll reconciles anyway. Refetching immediately only
      // opens a window where an in-flight list answers with the state from
      // before the write — which is how a button that worked reads as a button
      // that didn't.
    },
    onError: (e) => toastBackendError(e),
  });
  const del = useMutation({
    mutationFn: mailpitDelete,
    onSuccess: (_res, ids) => {
      if (selectedId && ids.includes(selectedId)) setSelectedId(null);
      setChecked((prev) => new Set([...prev].filter((id) => !ids.includes(id))));
      qc.invalidateQueries({ queryKey: ["mailpit-messages"] });
    },
    onError: (e) => toastBackendError(e),
  });
  const toggleChecked = (id: string) =>
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const deleteChecked = async () => {
    if (checked.size === 0) return;
    const n = checked.size;
    const ok = await confirm({
      title: n === 1 ? "Delete the selected message?" : `Delete ${n} selected messages?`,
      danger: true,
      confirmLabel: "Delete",
    });
    if (ok) del.mutate([...checked]);
  };
  const clearAll = async () => {
    const ok = await confirm({
      title: "Clear the inbox?",
      message: `All ${list?.total ?? messages.length} captured messages will be deleted.`,
      danger: true,
      confirmLabel: "Delete all",
    });
    if (ok) clear.mutate();
  };

  const running = !!mp?.running;
  const apiPort = (() => {
    try {
      return new URL(mp?.uiUrl ?? "").port || "18025";
    } catch {
      return "18025";
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
              onClick={() => void openExternal(mp.uiUrl).catch(toastBackendError)}
              disabled={!running}
              className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:opacity-40"
            >
              <PreferredBrowserIcon className="h-3.5 w-3.5" />
              Open Mailpit
            </button>
          )}
          {checked.size > 0 && (
            <button
              onClick={deleteChecked}
              disabled={del.isPending}
              className="flex items-center gap-1.5 rounded-lg border border-status-error/60 bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-status-error-bright transition-colors hover:bg-status-error/10 disabled:opacity-40"
            >
              <Trash2 className="h-3.5 w-3.5" />
              Delete selected ({checked.size})
            </button>
          )}
          <button
            onClick={() => markAllRead.mutate()}
            disabled={markAllRead.isPending || (list?.unread ?? 0) === 0}
            title={
              (list?.unread ?? 0) === 0
                ? "Nothing unread"
                : "Mark every captured message read — the way to make the NEXT mail your site sends stand out"
            }
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border"
          >
            <CheckCheck className="h-3.5 w-3.5" />
            Mark all read
          </button>
          <button
            onClick={clearAll}
            disabled={clear.isPending || messages.length === 0}
            className="flex items-center gap-1.5 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5 py-1.5 text-[0.75rem] text-rex-text transition-colors hover:border-status-error/60 hover:text-status-error-bright disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-rex-border disabled:hover:text-rex-text"
          >
            <Trash2 className="h-3.5 w-3.5" />
            Clear all
          </button>
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* List pane */}
        <div className="flex w-[344px] shrink-0 flex-col border-r border-rex-border">
          <div className="flex flex-col gap-2 border-b border-rex-border p-2.5">
            <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-2 px-2.5">
              <Search className="h-3.5 w-3.5 text-rex-text-muted" />
              <input {...TECH_INPUT}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search mail…"
                className="h-[34px] flex-1 bg-transparent text-[0.78125rem] text-rex-text outline-none placeholder:text-rex-text-muted"
              />
            </div>
            {/* All / Unread. A segmented pair rather than a checkbox: which one
                you are looking at has to be readable at a glance, and the count
                belongs on the thing that filters BY it. `list.unread` stays
                mailbox-wide while filtering (Mailpit's own contract), so this
                number never becomes "unread among the unread". */}
            <div className="flex items-center gap-1 rounded-lg border border-rex-border bg-rex-surface-2 p-0.5">
              {([false, true] as const).map((only) => (
                <button
                  key={String(only)}
                  onClick={() => setUnreadOnly(only)}
                  aria-pressed={unreadOnly === only}
                  // A stable accessible name: the visible label carries a live
                  // COUNT, so "Unread 4" would rename the control every time a
                  // mail arrived — for a screen reader and for anything else
                  // that addresses it by name.
                  aria-label={only ? "Show unread only" : "Show all mail"}
                  className={cn(
                    "flex flex-1 items-center justify-center gap-1.5 rounded-[6px] px-2 py-1 text-[0.75rem] transition-colors",
                    unreadOnly === only
                      ? "bg-rex-surface-1 font-medium text-rex-text"
                      : "text-rex-text-muted hover:text-rex-text",
                  )}
                >
                  {only ? "Unread" : "All"}
                  {only && (list?.unread ?? 0) > 0 && (
                    <span className="rounded-full bg-brand px-1.5 py-px font-mono text-[0.59375rem] font-semibold text-white">
                      {list?.unread}
                    </span>
                  )}
                </button>
              ))}
            </div>
            <select
              value={siteFilter}
              onChange={(e) => setSiteFilter(e.target.value)}
              className="h-[30px] w-full rounded-lg border border-rex-border bg-rex-surface-2 px-2 text-[0.75rem] text-rex-text outline-none"
            >
              <option value="all">All sites</option>
              {groups.map((g) => (
                <option key={g.key} value={g.key}>
                  {g.domain ? `${g.name} · ${g.domain}` : g.name} ({g.messages.length})
                </option>
              ))}
            </select>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {visibleGroups.length === 0 ? (
              <div className="p-6 text-center text-[0.78125rem] text-rex-text-muted">
                {unreadOnly
                  ? search
                    ? "No unread messages match your search."
                    : "Nothing unread — every captured message has been opened."
                  : search
                    ? "No messages match your search."
                    : "No mail captured yet."}
              </div>
            ) : (
              visibleGroups.map((g) => (
                <div key={g.key}>
                  <button
                    onClick={() => toggleGroup(g.key)}
                    className="sticky top-0 z-10 flex w-full items-center gap-2 border-b border-rex-border-subtle bg-rex-surface-1 px-3 py-2 text-left transition-colors hover:bg-rex-surface-2/50"
                  >
                    <ChevronRight
                      className={cn(
                        "h-3.5 w-3.5 shrink-0 text-rex-text-muted transition-transform",
                        !collapsed.has(g.key) && "rotate-90",
                      )}
                    />
                    <Globe className="h-3.5 w-3.5 shrink-0 text-rex-text-dim" />
                    <span className="truncate text-[0.75rem] font-semibold text-rex-text">{g.name}</span>
                    {g.domain && (
                      <span className="truncate font-mono text-[0.65625rem] text-rex-text-muted">{g.domain}</span>
                    )}
                    <span className="ml-auto flex shrink-0 items-center gap-1.5">
                      {g.unread > 0 && (
                        <span className="rounded-full bg-brand px-1.5 py-px font-mono text-[0.59375rem] font-semibold text-white">
                          {g.unread}
                        </span>
                      )}
                      <span className="rounded-md border border-rex-border-strong bg-rex-surface-2 px-1.5 py-px font-mono text-[0.625rem] text-rex-text-muted">
                        {g.messages.length}
                      </span>
                    </span>
                  </button>
                  {!collapsed.has(g.key) &&
                    g.messages.map((m) => (
                      <MessageRow
                        key={m.id}
                        m={m}
                        active={m.id === selectedId}
                        checked={checked.has(m.id)}
                        onCheck={() => toggleChecked(m.id)}
                        onDelete={() => del.mutate([m.id])}
                        onClick={() => {
                          setSelectedId(m.id);
                          setPinned(m);
                          setTab("html");
                        }}
                      />
                    ))}
                </div>
              ))
            )}
          </div>
          {list && (
            <div className="flex-none border-t border-rex-border px-3 py-2 font-mono text-[0.65625rem] text-rex-text-muted">
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
              <div className="flex h-[46px] w-[46px] items-center justify-center rounded-xl border border-rex-border-strong bg-rex-surface-1 text-rex-text-muted">
                <MailIcon className="h-[22px] w-[22px]" strokeWidth={1.6} />
              </div>
              <div>
                <div className="text-[0.875rem] font-semibold text-rex-text">
                  {messages.length ? "Nothing selected" : "No emails yet"}
                </div>
                <div className="mt-1 text-[0.78125rem] text-rex-text-muted">
                  {messages.length
                    ? "Pick a message on the left to preview it."
                    : "Outgoing email from your sites is captured here."}
                </div>
              </div>
              <span className="rounded-md border border-rex-border-strong bg-rex-surface-1 px-2.5 py-1 font-mono text-[0.65625rem] text-rex-text-muted">
                SMTP · 127.0.0.1:{mp?.smtpPort ?? 11025} · auto-configured
              </span>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function MessageRow({
  m,
  active,
  checked,
  onCheck,
  onDelete,
  onClick,
}: {
  m: MailSummary;
  active: boolean;
  checked: boolean;
  onCheck: () => void;
  onDelete: () => void;
  onClick: () => void;
}) {
  const recipient = m.to[0]?.address ?? "—";
  // A <div role="button">, not a <button>: the row contains its own interactive
  // children (select checkbox, delete button) and nested buttons are invalid.
  return (
    <div
      role="button"
      tabIndex={0}
      // Unread is otherwise carried ONLY by a coloured dot and a heavier font —
      // nothing a screen reader announces, and nothing a probe can assert on.
      // The label says it, and `data-read` is what the WebKit check reads to
      // prove the state flips on click rather than on the next poll.
      aria-label={`${m.read ? "Read" : "Unread"} message: ${m.subject || "(no subject)"} from ${m.from.address}`}
      data-read={m.read ? "1" : "0"}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      className={cn(
        "group relative flex w-full cursor-pointer items-start gap-2 border-b border-rex-border-subtle py-2.5 pl-3 pr-3 text-left transition-colors",
        active ? "bg-brand-active" : "hover:bg-rex-surface-2/50",
      )}
    >
      {(active || !m.read) && (
        <span className="absolute left-0 top-0 h-full w-[2.5px] bg-brand" />
      )}
      <input
        type="checkbox"
        checked={checked}
        onChange={onCheck}
        onClick={(e) => e.stopPropagation()}
        aria-label={`Select "${m.subject || "(no subject)"}"`}
        className="mt-[3px] h-[15px] w-[15px] shrink-0 cursor-pointer accent-brand"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-2">
          {!m.read && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-brand" />}
          <span className={`flex-1 truncate text-[0.78125rem] ${m.read ? "text-rex-text-muted" : "font-semibold text-rex-text"}`}>
            {m.from.name || m.from.address}
          </span>
          <span className="shrink-0 font-mono text-[0.65625rem] text-rex-text-muted">{shortTime(m.created)}</span>
        </div>
        <div className="truncate text-[0.75rem] text-rex-text">{m.subject || "(no subject)"}</div>
        <div className="truncate font-mono text-[0.6875rem] text-rex-text-muted">
          <span className="text-rex-text-muted">to </span>
          {recipient}
        </div>
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          onDelete();
        }}
        title="Delete message"
        aria-label={`Delete "${m.subject || "(no subject)"}"`}
        className="absolute bottom-2 right-2 hidden h-6 w-6 items-center justify-center rounded-md border border-rex-border bg-rex-surface-2 text-rex-text-muted transition-colors hover:border-status-error/60 hover:text-status-error-bright group-hover:flex"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
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
  const qc = useQueryClient();
  const { data: msg } = useQuery({ queryKey: ["mailpit-message", id], queryFn: () => mailpitMessage(id) });

  // Fetching the detail is WHAT MARKS THE MESSAGE READ in Mailpit — it is that
  // request's documented side effect. Until now nothing told the list, so the
  // row kept its unread dot until the next 5s poll happened to come round:
  // sometimes instant, sometimes five seconds, which reads as "clicking the
  // subject works but clicking the sender doesn't". The list already knows
  // everything it needs; patch it here, at the moment the fact became true,
  // and let the poll reconcile.
  const readId = msg?.id;
  useEffect(() => {
    if (!readId) return;
    qc.setQueriesData<MailList>({ queryKey: ["mailpit-messages"] }, (old) => {
      if (!old) return old;
      const target = old.messages.find((m) => m.id === readId);
      if (!target || target.read) return old; // already read: never double-count
      return {
        ...old,
        unread: Math.max(0, old.unread - 1),
        messages: old.messages.map((m) => (m.id === readId ? { ...m, read: true } : m)),
      };
    });
  }, [readId, qc]);
  const { data: raw } = useQuery({
    queryKey: ["mailpit-raw", id],
    queryFn: () => mailpitMessageRaw(id),
    enabled: tab === "raw",
  });

  // WordPress mail is usually plain-text only — land on the part that exists
  // (once per message; manual tab clicks still win afterwards).
  const msgHtml = msg ? !!msg.html : null;
  useEffect(() => {
    if (msgHtml === false && tab === "html") onTab("text");
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only on message load
  }, [id, msgHtml]);

  if (!msg) return <div className="p-6 text-[0.78125rem] text-rex-text-muted">Loading…</div>;

  const tabs: PreviewTab[] = ["html", "text", "raw", "headers"];
  const avatar = avatarColor(msg.from.address);
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-start gap-3 border-b border-rex-border p-4">
        <span
          className="flex h-[30px] w-[30px] flex-none items-center justify-center rounded-lg text-[0.75rem] font-semibold"
          style={{ background: avatar.bg, color: avatar.color }}
        >
          {initial(msg.from)}
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[0.875rem] font-semibold text-rex-text">{msg.subject || "(no subject)"}</div>
        <div className="mt-1.5 flex flex-col gap-0.5 text-[0.75rem] text-rex-text-muted">
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
            className={`-mb-px border-b-2 px-3 py-2 text-[0.78125rem] transition-colors ${
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
            <pre className="whitespace-pre-wrap p-4 font-mono text-[0.75rem] text-rex-text">{msg.text}</pre>
          ) : (
            <Empty label="No plain-text part" />
          ))}
        {tab === "raw" && (
          <pre className="whitespace-pre-wrap p-4 font-mono text-[0.71875rem] text-rex-text-muted">
            {raw ?? "Loading…"}
          </pre>
        )}
        {tab === "headers" && (
          <div className="flex flex-col gap-1 p-4">
            {msg.headers.map((h, i) => (
              <div key={`${h.name}-${i}`} className="flex gap-2 font-mono text-[0.71875rem]">
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
  return <div className="p-6 text-[0.78125rem] text-rex-text-muted">{label}</div>;
}

function addr(a: { name: string; address: string }): string {
  return a.name ? `${a.name} <${a.address}>` : a.address;
}

function shortTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
