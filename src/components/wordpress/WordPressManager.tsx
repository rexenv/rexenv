import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpCircle, Check, LayoutGrid, Palette, Plus, Trash2 } from "lucide-react";
import { Placeholder } from "@/components/common/Placeholder";
import {
  wpPluginActivate,
  wpPluginDeactivate,
  wpPluginDelete,
  wpPluginInstall,
  wpPluginUpdate,
  wpPlugins,
  wpThemeActivate,
  wpThemeDelete,
  wpThemeInstall,
  wpThemeUpdate,
  wpThemes,
} from "@/lib/ipc";
import type { WpPlugin, WpTheme } from "@/types";

type SubTab = "plugins" | "themes" | "users" | "tools";

const BTN =
  "rounded-md border border-rex-border bg-rex-surface-2 px-2.5 py-1 text-[12px] text-rex-text transition-colors hover:border-brand disabled:cursor-not-allowed disabled:opacity-40";

export function WordPressManager({ siteId }: { siteId: string }) {
  const [sub, setSub] = useState<SubTab>("plugins");
  const subs: { key: SubTab; label: string }[] = [
    { key: "plugins", label: "Plugins" },
    { key: "themes", label: "Themes" },
    { key: "users", label: "Users" },
    { key: "tools", label: "Tools" },
  ];

  return (
    <>
      <div className="flex gap-1 rounded-lg border border-rex-border bg-rex-surface-1 p-1">
        {subs.map((s) => (
          <button
            key={s.key}
            onClick={() => setSub(s.key)}
            className={`flex-1 rounded-md px-3 py-1.5 text-[12.5px] transition-colors ${
              sub === s.key ? "bg-rex-surface-2 font-medium text-rex-text" : "text-rex-text-muted hover:text-rex-text"
            }`}
          >
            {s.label}
          </button>
        ))}
      </div>

      {sub === "plugins" && <PluginsPanel siteId={siteId} />}
      {sub === "themes" && <ThemesPanel siteId={siteId} />}
      {(sub === "users" || sub === "tools") && (
        <Placeholder
          icon={<LayoutGrid className="h-[22px] w-[22px]" strokeWidth={1.6} />}
          label={subs.find((s) => s.key === sub)!.label}
          hint="Users & Tools land in §7."
        />
      )}
    </>
  );
}

function ThemesPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(false);

  const { data: themes = [], isLoading } = useQuery({
    queryKey: ["wp-themes", siteId],
    queryFn: () => wpThemes(siteId),
  });

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["wp-themes", siteId] }),
    onError: (e) => window.alert(String(e)),
  });
  const busy = run.isPending;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          placeholder="Theme slug (e.g. twentytwentyfour)"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
          <input type="checkbox" checked={activateOnAdd} onChange={(e) => setActivateOnAdd(e.target.checked)} />
          Activate
        </label>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={busy || !slug.trim()}
          onClick={() => {
            const s = slug.trim();
            run.mutate(() => wpThemeInstall(siteId, s, activateOnAdd).then(() => setSlug("")));
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          Add
        </button>
      </div>

      {isLoading ? (
        <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center text-[12.5px] text-rex-text-muted">
          Loading themes…
        </div>
      ) : themes.length === 0 ? (
        <div className="rounded-xl border border-rex-border bg-rex-surface-1 p-6 text-center text-[12.5px] text-rex-text-muted">
          No themes installed.
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          {themes.map((t) => (
            <ThemeCard
              key={t.name}
              t={t}
              busy={busy}
              onActivate={() => run.mutate(() => wpThemeActivate(siteId, t.name))}
              onUpdate={() => run.mutate(() => wpThemeUpdate(siteId, [t.name]))}
              onDelete={() => {
                if (window.confirm(`Delete theme "${t.name}"?`)) run.mutate(() => wpThemeDelete(siteId, [t.name]));
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ThemeCard({
  t,
  busy,
  onActivate,
  onUpdate,
  onDelete,
}: {
  t: WpTheme;
  busy: boolean;
  onActivate: () => void;
  onUpdate: () => void;
  onDelete: () => void;
}) {
  const active = t.status === "active";
  const updatable = t.update === "available";
  return (
    <div
      className={`flex flex-col rounded-xl border bg-rex-surface-1 ${
        active ? "border-brand/60" : "border-rex-border"
      }`}
    >
      <div className="flex aspect-[4/3] items-center justify-center rounded-t-xl bg-rex-surface-2 text-rex-text-dim">
        <Palette className="h-7 w-7" strokeWidth={1.4} />
      </div>
      <div className="flex flex-1 flex-col gap-2 p-3">
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-rex-text">{t.name}</span>
          {active && (
            <span className="flex items-center gap-1 rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-medium text-emerald-400">
              <Check className="h-3 w-3" />
              Active
            </span>
          )}
          {updatable && !active && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">v{t.version}</div>
        <div className="mt-auto flex items-center gap-1.5">
          {!active && (
            <button className={BTN + " flex-1"} disabled={busy} onClick={onActivate}>
              Activate
            </button>
          )}
          {updatable && (
            <button className={BTN + " flex items-center gap-1"} disabled={busy} onClick={onUpdate} title="Update">
              <ArrowUpCircle className="h-3.5 w-3.5" />
            </button>
          )}
          <button
            className={BTN + " hover:border-red-500/60 hover:text-red-400 disabled:hover:border-rex-border disabled:hover:text-rex-text"}
            disabled={busy || active}
            onClick={onDelete}
            title={active ? "Can't delete the active theme" : "Delete"}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </div>
  );
}

function PluginsPanel({ siteId }: { siteId: string }) {
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [slug, setSlug] = useState("");
  const [activateOnAdd, setActivateOnAdd] = useState(true);

  const { data: plugins = [], isLoading } = useQuery({
    queryKey: ["wp-plugins", siteId],
    queryFn: () => wpPlugins(siteId),
  });

  const run = useMutation({
    mutationFn: (fn: () => Promise<void>) => fn(),
    onSuccess: () => {
      setSelected(new Set());
      qc.invalidateQueries({ queryKey: ["wp-plugins", siteId] });
    },
    onError: (e) => window.alert(String(e)),
  });
  const busy = run.isPending;

  const toggleSel = (name: string) =>
    setSelected((s) => {
      const next = new Set(s);
      next.has(name) ? next.delete(name) : next.add(name);
      return next;
    });
  const selNames = [...selected];

  return (
    <div className="flex flex-col gap-3">
      {/* Add by slug */}
      <div className="flex items-center gap-2 rounded-lg border border-rex-border bg-rex-surface-1 p-2.5">
        <input
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
          placeholder="Plugin slug (e.g. hello-dolly)"
          className="h-[30px] flex-1 rounded border border-rex-border bg-rex-surface-2 px-2 font-mono text-[12px] text-rex-text outline-none focus:border-brand"
        />
        <label className="flex items-center gap-1.5 text-[12px] text-rex-text-muted">
          <input type="checkbox" checked={activateOnAdd} onChange={(e) => setActivateOnAdd(e.target.checked)} />
          Activate
        </label>
        <button
          className={BTN + " flex items-center gap-1.5"}
          disabled={busy || !slug.trim()}
          onClick={() => {
            const s = slug.trim();
            run.mutate(() => wpPluginInstall(siteId, s, activateOnAdd).then(() => setSlug("")));
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          Add
        </button>
      </div>

      {/* Bulk bar */}
      {selNames.length > 0 && (
        <div className="flex items-center gap-2 rounded-lg border border-brand/40 bg-rex-surface-1 p-2.5 text-[12px]">
          <span className="text-rex-text-muted">{selNames.length} selected</span>
          <div className="flex-1" />
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginActivate(siteId, selNames))}>
            Activate
          </button>
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginDeactivate(siteId, selNames))}>
            Deactivate
          </button>
          <button className={BTN} disabled={busy} onClick={() => run.mutate(() => wpPluginUpdate(siteId, selNames))}>
            Update
          </button>
          <button
            className={BTN + " hover:border-red-500/60 hover:text-red-400"}
            disabled={busy}
            onClick={() => {
              if (window.confirm(`Delete ${selNames.length} plugin(s)?`))
                run.mutate(() => wpPluginDelete(siteId, selNames));
            }}
          >
            Delete
          </button>
        </div>
      )}

      {/* List */}
      <div className="overflow-hidden rounded-xl border border-rex-border bg-rex-surface-1">
        {isLoading ? (
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">Loading plugins…</div>
        ) : plugins.length === 0 ? (
          <div className="p-6 text-center text-[12.5px] text-rex-text-muted">No plugins installed.</div>
        ) : (
          plugins.map((p) => (
            <PluginRow
              key={p.name}
              p={p}
              selected={selected.has(p.name)}
              busy={busy}
              onSelect={() => toggleSel(p.name)}
              onActivate={() => run.mutate(() => wpPluginActivate(siteId, [p.name]))}
              onDeactivate={() => run.mutate(() => wpPluginDeactivate(siteId, [p.name]))}
              onUpdate={() => run.mutate(() => wpPluginUpdate(siteId, [p.name]))}
              onDelete={() => {
                if (window.confirm(`Delete plugin "${p.name}"?`)) run.mutate(() => wpPluginDelete(siteId, [p.name]));
              }}
            />
          ))
        )}
      </div>
    </div>
  );
}

function PluginRow({
  p,
  selected,
  busy,
  onSelect,
  onActivate,
  onDeactivate,
  onUpdate,
  onDelete,
}: {
  p: WpPlugin;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onActivate: () => void;
  onDeactivate: () => void;
  onUpdate: () => void;
  onDelete: () => void;
}) {
  const active = p.status === "active" || p.status === "active-network";
  const updatable = p.update === "available";
  return (
    <div className="flex items-center gap-3 border-b border-rex-border-subtle px-3 py-2.5 last:border-b-0">
      <input type="checkbox" checked={selected} onChange={onSelect} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-rex-text">{p.name}</span>
          {updatable && (
            <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-400">
              update
            </span>
          )}
        </div>
        <div className="font-mono text-[11px] text-rex-text-dim">v{p.version}</div>
      </div>
      <span
        className={`rounded-full px-2 py-0.5 text-[11px] ${
          active ? "bg-emerald-500/15 text-emerald-400" : "bg-rex-surface-3 text-rex-text-muted"
        }`}
      >
        {active ? "Active" : "Inactive"}
      </span>
      {updatable && (
        <button className={BTN + " flex items-center gap-1"} disabled={busy} onClick={onUpdate} title="Update">
          <ArrowUpCircle className="h-3.5 w-3.5" />
        </button>
      )}
      <button className={BTN} disabled={busy} onClick={active ? onDeactivate : onActivate}>
        {active ? "Deactivate" : "Activate"}
      </button>
      <button
        className={BTN + " hover:border-red-500/60 hover:text-red-400"}
        disabled={busy}
        onClick={onDelete}
        title="Delete"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
