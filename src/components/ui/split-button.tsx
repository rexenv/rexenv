import type { ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { Button, type ButtonProps } from "@/components/ui/button";
import { Menu } from "@/components/ui/menu";
import { cn } from "@/lib/utils";

/**
 * One default action plus a chevron that opens alternatives — "Open in browser"
 * where the browser is a choice.
 *
 * The two halves are separate `<button>`s (a nested button is invalid HTML and
 * WebKit swallows the inner click), joined by removing the facing corners.
 * With no alternatives to offer, the chevron is not rendered at all rather than
 * opening an empty menu: a control that does nothing is worse than no control.
 */
export function SplitButton({
  children,
  onClick,
  menu,
  menuWidth = 210,
  variant = "secondary",
  size,
  disabled,
  chevronLabel,
  className,
}: {
  /** Contents of the primary half — icon + label. */
  children: ReactNode;
  onClick: () => void;
  /** Menu items. Falsy/empty → no chevron. */
  menu?: ReactNode;
  menuWidth?: number;
  variant?: ButtonProps["variant"];
  size?: ButtonProps["size"];
  disabled?: boolean;
  /** Accessible name for the chevron, e.g. "Choose a browser". */
  chevronLabel: string;
  className?: string;
}) {
  const hasMenu = Array.isArray(menu) ? menu.length > 0 : !!menu;
  if (!hasMenu) {
    return (
      <Button variant={variant} size={size} disabled={disabled} onClick={onClick} className={className}>
        {children}
      </Button>
    );
  }
  return (
    <div className={cn("inline-flex", className)}>
      <Button
        variant={variant}
        size={size}
        disabled={disabled}
        onClick={onClick}
        className="rounded-r-none"
      >
        {children}
      </Button>
      <Menu
        align="right"
        width={menuWidth}
        trigger={
          <Button
            variant={variant}
            size={size}
            disabled={disabled}
            aria-label={chevronLabel}
            title={chevronLabel}
            // -ml-px collapses the two facing 1px borders into one seam.
            className="-ml-px rounded-l-none px-1.5"
          >
            <ChevronDown className="h-[13px] w-[13px]" strokeWidth={2} />
          </Button>
        }
      >
        {menu}
      </Menu>
    </div>
  );
}
