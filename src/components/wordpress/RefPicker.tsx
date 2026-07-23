/** Searchable ref combobox for the RepoPanel ops row — replaces the plain
 *  <select> that drowned in ~100 branches. Trigger button → portaled popover
 *  (same never-clipped fixed-position approach as ui/menu.tsx) with a filter
 *  input on top of a grouped, keyboard-navigable list (cmdk). Picking an item
 *  only SETS the target — the Checkout button next door still fires the op. */
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Command } from "cmdk";
import { Check, ChevronsUpDown } from "lucide-react";
import { cn, TECH_INPUT } from "@/lib/utils";

export interface RefItem {
  /** Sent to checkout as-is (branch name today; refs/tags/… later). */
  value: string;
  /** Display text; defaults to value. */
  label?: string;
  /** Dim right-aligned annotation, e.g. "current". */
  hint?: string;
}

export interface RefGroup {
  /** null = ungrouped section (local branches). */
  label: string | null;
  items: RefItem[];
  /** Non-interactive status line under the group's items (e.g. "loading
   *  pull requests…"). Keeps the group visible even with zero items. */
  note?: string;
}

const PANEL_WIDTH = 280;

export function RefPicker({
  value,
  onChange,
  groups,
  disabled,
  ariaLabel,
  onOpenChange,
}: {
  value: string;
  onChange: (value: string) => void;
  groups: RefGroup[];
  disabled?: boolean;
  ariaLabel: string;
  /** Fires on every open/close — lets the owner lazy-load network-backed
   *  groups (PR refs) only once the picker is actually opened. */
  onOpenChange?: (open: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    onOpenChange?.(open);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const place = () => {
    const r = triggerRef.current?.getBoundingClientRect();
    if (!r) return;
    setPos({
      top: r.bottom + 6,
      left: Math.min(r.left, window.innerWidth - PANEL_WIDTH - 8),
    });
  };

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (
        !panelRef.current?.contains(e.target as Node) &&
        !triggerRef.current?.contains(e.target as Node)
      )
        setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    const onMove = () => setOpen(false);
    // Scroll-dismiss ONLY for scrolls outside the panel (the fixed-position
    // popover would be orphaned by the page scrolling under it). Menu's
    // unconditional version is wrong here: our own list scrolls — a wheel
    // tick or cmdk's arrow-key scrollIntoView capture-propagates to window
    // and must NOT close the picker.
    const onScroll = (e: Event) => {
      if (panelRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onMove);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open]);

  const select = (v: string) => {
    onChange(v);
    setOpen(false);
    triggerRef.current?.focus();
  };

  // Trigger shows the picked item's display label (a tag reads "v1.2.0",
  // not "refs/tags/v1.2.0"); the raw value is what checkout receives.
  const selected = groups.flatMap((g) => g.items).find((i) => i.value === value);
  const display = selected?.label ?? value;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-label={ariaLabel}
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => {
          if (!open) place();
          setOpen((o) => !o);
        }}
        className="flex h-[28px] max-w-[200px] items-center gap-1.5 rounded border border-rex-border bg-rex-surface-2 px-1.5 font-mono text-[0.71875rem] text-rex-text outline-none focus:border-brand disabled:cursor-not-allowed disabled:opacity-40"
      >
        <span className="truncate">{display || "select ref…"}</span>
        <ChevronsUpDown className="h-3 w-3 flex-none text-rex-text-muted" />
      </button>
      {open &&
        createPortal(
          <div
            ref={panelRef}
            style={{ position: "fixed", top: pos.top, left: pos.left, width: PANEL_WIDTH }}
            className="z-[60] rounded-[11px] border border-rex-border-strong bg-rex-surface-2 shadow-menu"
          >
            <Command label={ariaLabel}>
              <Command.Input
                {...TECH_INPUT}
                autoFocus
                placeholder="Filter refs…"
                className="w-full border-b border-rex-border bg-transparent px-3 py-2 font-mono text-[0.71875rem] text-rex-text placeholder:text-rex-text-dim focus:outline-none"
              />
              <Command.List className="max-h-[260px] overflow-y-auto overscroll-contain p-[5px]">
                <Command.Empty className="px-[9px] py-2 font-mono text-[0.6875rem] text-rex-text-muted">
                  No matching refs.
                </Command.Empty>
                {groups
                  .filter((g) => g.items.length > 0 || g.note)
                  .map((g) => {
                    const items = g.items.map((item) => (
                      <Command.Item
                        key={item.value}
                        value={item.value}
                        // Label as keywords so "PR #123" is findable even
                        // though the value is refs/pull/123/head.
                        keywords={item.label ? [item.label] : undefined}
                        // Close over item.value — cmdk normalizes its onSelect
                        // argument, and branch names are case-sensitive.
                        onSelect={() => select(item.value)}
                        className={cn(
                          "flex cursor-default items-center gap-2 rounded-[7px] px-[9px] py-[5px] font-mono text-[0.71875rem] text-rex-text-bright",
                          "data-[selected=true]:bg-rex-hover",
                        )}
                      >
                        <Check
                          className={cn(
                            "h-3 w-3 flex-none",
                            item.value === value ? "text-brand" : "invisible",
                          )}
                        />
                        <span className="truncate">{item.label ?? item.value}</span>
                        {item.hint && (
                          <span className="ml-auto flex-none text-[0.625rem] text-rex-text-muted">
                            {item.hint}
                          </span>
                        )}
                      </Command.Item>
                    ));
                    // Note lives OUTSIDE Command.Group — cmdk hides a group
                    // whose items are all filtered out, and a status line
                    // ("loading pull requests…") must stay visible even when
                    // the group has no items at all.
                    const note = g.note && (
                      <div
                        key={`note-${g.label}`}
                        className="px-[9px] pb-1.5 pt-0.5 font-mono text-[0.625rem] text-rex-text-muted"
                      >
                        {g.label && g.items.length === 0 && (
                          <div className="pb-1 pt-1 text-[0.625rem] uppercase tracking-wide">
                            {g.label}
                          </div>
                        )}
                        {g.note}
                      </div>
                    );
                    return g.label ? (
                      <div key={g.label}>
                        <Command.Group
                          heading={g.label}
                          className="[&_[cmdk-group-heading]]:px-[9px] [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-1.5 [&_[cmdk-group-heading]]:text-[0.625rem] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-rex-text-muted"
                        >
                          {items}
                        </Command.Group>
                        {note}
                      </div>
                    ) : (
                      <Command.Group key="__ungrouped">{items}</Command.Group>
                    );
                  })}
              </Command.List>
            </Command>
          </div>,
          document.body,
        )}
    </>
  );
}
