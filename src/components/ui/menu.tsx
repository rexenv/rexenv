import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

const MenuCtx = createContext<{ close: () => void }>({ close: () => {} });

/**
 * A lightweight dropdown menu. The popover is portaled to <body> and
 * fixed-positioned from the trigger rect, so it never gets clipped by an
 * ancestor's `overflow: hidden`. Closes on outside-click, Escape, scroll, resize.
 */
export function Menu({
  trigger,
  children,
  width = 186,
  align = "right",
}: {
  trigger: ReactNode;
  children: ReactNode;
  width?: number;
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const wrapRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const place = () => {
    const r = wrapRef.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ top: r.bottom + 6, left: align === "right" ? r.right - width : r.left });
  };

  // Reposition once the menu has a real height: FLIP above the trigger when
  // there isn't room below (the last rows of a long list rendered the menu
  // past the viewport bottom — and the scroll listener closes it, so it
  // couldn't even be scrolled into view), and clamp horizontally. Runs
  // before paint, so the wrong position is never visible.
  useLayoutEffect(() => {
    if (!open) return;
    const r = wrapRef.current?.getBoundingClientRect();
    const h = menuRef.current?.offsetHeight ?? 0;
    if (!r || !h) return;
    let top = r.bottom + 6;
    if (top + h > window.innerHeight - 8) top = Math.max(8, r.top - 6 - h);
    let left = align === "right" ? r.right - width : r.left;
    left = Math.min(Math.max(8, left), window.innerWidth - width - 8);
    setPos({ top, left });
  }, [open, align, width]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (
        !menuRef.current?.contains(e.target as Node) &&
        !wrapRef.current?.contains(e.target as Node)
      )
        setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    const onMove = () => setOpen(false);
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onMove);
    window.addEventListener("scroll", onMove, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onMove);
      window.removeEventListener("scroll", onMove, true);
    };
  }, [open]);

  return (
    <div
      ref={wrapRef}
      className="relative inline-flex"
      onClick={() => {
        if (!open) place();
        setOpen((o) => !o);
      }}
    >
      {trigger}
      {open &&
        createPortal(
          <MenuCtx.Provider value={{ close: () => setOpen(false) }}>
            <div
              ref={menuRef}
              onClick={(e) => e.stopPropagation()}
              style={{ position: "fixed", top: pos.top, left: pos.left, width }}
              className="z-[60] rounded-[11px] border border-rex-border-strong bg-rex-surface-2 p-[5px] shadow-menu"
            >
              {children}
            </div>
          </MenuCtx.Provider>,
          document.body,
        )}
    </div>
  );
}

export function MenuItem({
  icon,
  children,
  onSelect,
  danger,
  action,
}: {
  icon?: ReactNode;
  children: ReactNode;
  onSelect?: () => void;
  danger?: boolean;
  /** A SECOND target on the same row, split off by a divider — "…and open it
   *  privately". Rendered as a SIBLING button, never nested inside the row's
   *  own button: nested buttons are invalid HTML and WebKit swallows the inner
   *  click (the same reason `SplitButton` is two buttons). Each half hovers on
   *  its own, so which one is about to fire is never a guess. */
  action?: { icon: ReactNode; label: string; onSelect: () => void };
}) {
  const { close } = useContext(MenuCtx);
  const row = (
    <button
      type="button"
      onClick={() => {
        onSelect?.();
        close();
      }}
      className={cn(
        "flex items-center gap-2.5 px-[9px] py-[7px] text-left text-[0.78125rem] transition-colors",
        // min-w-0 keeps a truncating child truncating inside the flex row.
        action ? "min-w-0 flex-1 rounded-l-[7px]" : "w-full rounded-[7px]",
        danger
          ? "text-status-error-bright hover:bg-status-error-bg"
          : "text-rex-text-bright hover:bg-rex-hover",
      )}
    >
      {icon}
      {children}
    </button>
  );
  if (!action) return row;
  return (
    <div className="flex w-full items-stretch">
      {row}
      <div className="my-[5px] w-px flex-none bg-rex-border-strong" />
      <button
        type="button"
        aria-label={action.label}
        title={action.label}
        onClick={() => {
          action.onSelect();
          close();
        }}
        className="flex flex-none items-center rounded-r-[7px] px-[9px] text-rex-text-dim transition-colors hover:bg-rex-hover hover:text-rex-text-bright"
      >
        {action.icon}
      </button>
    </div>
  );
}

export function MenuSeparator() {
  return <div className="mx-[6px] my-[5px] h-px bg-rex-border-strong" />;
}
