import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type * as React from "react";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-[4px] border border-transparent text-[0.8125rem] font-medium whitespace-nowrap transition-[background-color,border-color,color] duration-100 outline-none focus-visible:border-primary focus-visible:ring-1 focus-visible:ring-primary/25 disabled:pointer-events-none disabled:opacity-45 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default:
          "border-primary/35 bg-primary/12 text-primary hover:border-primary/55 hover:bg-primary/18 active:bg-primary/24",
        destructive:
          "border-destructive/40 bg-destructive/12 text-destructive hover:border-destructive/60 hover:bg-destructive/18 focus-visible:border-destructive focus-visible:ring-destructive/20",
        outline:
          "border-border bg-secondary/55 text-foreground hover:border-foreground/20 hover:bg-secondary active:bg-secondary/75",
        secondary:
          "border-border/80 bg-secondary/75 text-secondary-foreground hover:border-foreground/20 hover:bg-secondary",
        ghost:
          "text-muted-foreground hover:border-border/70 hover:bg-secondary/70 hover:text-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-8 px-3 has-[>svg]:px-2.5",
        xs: "h-6 gap-1 px-2 text-xs has-[>svg]:px-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-7 px-2.5 has-[>svg]:px-2",
        lg: "h-9 px-4 has-[>svg]:px-3.5",
        icon: "size-8",
        "icon-xs": "size-6 [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-7",
        "icon-lg": "size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot.Root : "button";

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
