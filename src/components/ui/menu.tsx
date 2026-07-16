import {
  createContext,
  useContext,
  useEffect,
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
}: {
  icon?: ReactNode;
  children: ReactNode;
  onSelect?: () => void;
  danger?: boolean;
}) {
  const { close } = useContext(MenuCtx);
  return (
    <button
      type="button"
      onClick={() => {
        onSelect?.();
        close();
      }}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-[7px] px-[9px] py-[7px] text-left text-[0.78125rem] transition-colors",
        danger
          ? "text-status-error-bright hover:bg-status-error-bg"
          : "text-rex-text-bright hover:bg-rex-hover",
      )}
    >
      {icon}
      {children}
    </button>
  );
}

export function MenuSeparator() {
  return <div className="mx-[6px] my-[5px] h-px bg-rex-border-strong" />;
}
