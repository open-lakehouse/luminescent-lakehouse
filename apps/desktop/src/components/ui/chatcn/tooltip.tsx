// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import { Tooltip as TooltipPrimitive } from "radix-ui"
import type * as React from "react"
import { cn } from "@/lib/utils"

/**
 * Wrap the app once. After the first tooltip opens, the next ones open instantly, the way
 * native toolbars behave.
 */
function TooltipProvider({
  delayDuration: delay = 500,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider skipDelayDuration={400} delayDuration={delay} {...props} />
}

interface TooltipProps {
  content: React.ReactNode
  children: React.ReactElement
  side?: "top" | "bottom" | "left" | "right"
  align?: "start" | "center" | "end"
  /** A shortcut hint shown after the label, for example `⌘C`. */
  shortcut?: React.ReactNode
}

function Tooltip({ content, children, side = "top", align = "center", shortcut }: TooltipProps) {
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          collisionPadding={5}
          side={side}
          align={align}
          sideOffset={6}
          data-slot="tooltip"
          className={cn(
            "z-50",
            "rounded-chatcn-sm bg-chatcn-fg chatcn-text-12 text-chatcn-bg shadow-chatcn-md flex max-w-xs origin-(--radix-tooltip-content-transform-origin) items-center gap-2 px-2 py-1 font-medium text-pretty",
            "ease-chatcn-out transition-[opacity,scale] duration-125",
            "data-[state=closed]:scale-97 data-[state=closed]:opacity-0 data-[state=closed]:duration-100 starting:scale-97 starting:opacity-0 data-[state=closed]:animate-[chatcn-exit-hold_var(--tw-duration,150ms)]",
            // Once one tooltip is open, the next appears without animation. Reduced motion keeps
            // only the fade.
            "data-[state=instant-open]:transition-none motion-reduce:transition-opacity motion-reduce:data-[state=closed]:scale-100 motion-reduce:starting:scale-100 data-[state=closed]:animate-[chatcn-exit-hold_var(--tw-duration,150ms)]",
          )}
        >
          {content}
          {shortcut ? <span className="font-normal opacity-60">{shortcut}</span> : null}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  )
}

export type { TooltipProps }
export { Tooltip, TooltipProvider }
