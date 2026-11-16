// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import type * as React from "react"
import { cn } from "@/lib/utils"

type ButtonVariant = "solid" | "outline" | "ghost" | "subtle"
type ButtonSize = "xs" | "sm" | "md" | "lg" | "icon-xs" | "icon-sm" | "icon-md" | "icon-lg"

const base = cn(
  "chatcn-focus inline-flex shrink-0 items-center justify-center gap-1.5 font-medium whitespace-nowrap select-none",
  "ease-chatcn-standard transition-[background-color,color,box-shadow,scale] duration-150",
  "active:scale-(--chatcn-press-scale) active:duration-100 disabled:pointer-events-none disabled:opacity-50",
  "[&_svg]:pointer-events-none [&_svg]:shrink-0",
)

const variants: Record<ButtonVariant, string> = {
  solid: "bg-chatcn-primary text-chatcn-primary-fg hover:bg-chatcn-primary-hover",
  outline: "bg-chatcn-card text-chatcn-fg shadow-chatcn-xs hover:bg-chatcn-fill-hover",
  ghost: "text-chatcn-fg-muted hover:bg-chatcn-fill-hover hover:text-chatcn-fg",
  subtle: "bg-chatcn-fill text-chatcn-fg hover:bg-chatcn-fill-hover",
}

// Heights follow the control scale: 24, 28, 32, 36. Radii stay concentric inside 12 px surfaces.
const sizes: Record<ButtonSize, string> = {
  xs: "h-6 rounded-chatcn-sm px-2 chatcn-text-12 [&_svg]:size-3.5",
  sm: "h-7 rounded-chatcn-sm px-2.5 chatcn-text-13 [&_svg]:size-3.5",
  md: "h-8 rounded-chatcn-md px-3 chatcn-text-14 [&_svg]:size-4",
  lg: "h-9 rounded-chatcn-md px-3.5 chatcn-text-14 [&_svg]:size-4",
  "icon-xs": "size-6 rounded-chatcn-sm active:scale-(--chatcn-press-scale-icon) [&_svg]:size-3.5",
  "icon-sm": "size-7 rounded-chatcn-sm active:scale-(--chatcn-press-scale-icon) [&_svg]:size-3.5",
  "icon-md": "size-8 rounded-chatcn-md active:scale-(--chatcn-press-scale-icon) [&_svg]:size-4",
  "icon-lg": "size-9 rounded-chatcn-md active:scale-(--chatcn-press-scale-icon) [&_svg]:size-4",
}

/** Class names for anything that should look like a button, such as a link. */
function buttonVariants({
  variant = "ghost",
  size = "md",
}: {
  variant?: ButtonVariant
  size?: ButtonSize
} = {}) {
  return cn(base, variants[variant], sizes[size])
}

interface ButtonProps extends React.ComponentProps<"button"> {
  variant?: ButtonVariant
  size?: ButtonSize
}

function Button({
  variant = "ghost",
  size = "md",
  type = "button",
  className,
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      data-slot="button"
      data-variant={variant}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  )
}

export type { ButtonProps, ButtonSize, ButtonVariant }
export { Button, buttonVariants }
