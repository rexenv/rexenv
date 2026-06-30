import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 focus-visible:outline-none",
  {
    variants: {
      variant: {
        primary:
          "bg-primary text-primary-foreground shadow-glow-primary hover:bg-brand-hover active:bg-brand-strong active:translate-y-px",
        secondary:
          "bg-rex-surface-2 text-rex-text border border-rex-border-strong hover:bg-rex-surface-2-hover hover:border-rex-border-strong-hover active:translate-y-px",
        ghost:
          "text-rex-text-muted hover:bg-white/[0.045] hover:text-rex-text active:bg-white/[0.085] active:translate-y-px",
        danger:
          "bg-danger text-white hover:bg-danger-hover active:bg-danger-active active:translate-y-px",
      },
      size: {
        sm: "h-8 px-3 text-[13px]",
        default: "h-[34px] px-3.5 text-[13px]",
        lg: "h-10 px-5 text-sm",
        icon: "h-[34px] w-[34px]",
      },
    },
    defaultVariants: {
      variant: "secondary",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        className={cn(buttonVariants({ variant, size, className }))}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
