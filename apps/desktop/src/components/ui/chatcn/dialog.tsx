// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

// Radix variant of registry/ui/dialog.tsx, written by hand: Radix onOpenChange carries no event,
// so keyboard opens are detected from the last input, and initialFocus/finalFocus map to the
// Radix auto-focus events. Same exports, props and look as the Base UI version.

import { Dialog as DialogPrimitive } from "radix-ui"
import * as React from "react"
import { buttonVariants } from "@/components/ui/chatcn/button"
import { XIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Motion
 *
 * Scrim and popup share one duration and curve: 220 ms in, 160 ms out, ease-chatcn-out. The popup
 * grows from 0.96 at the center and leaves at 0.98 (exits travel less). Opened or closed from the
 * keyboard, nothing animates: Enter and Esc should feel instant.
 * -----------------------------------------------------------------------------------------------*/

const InstantContext = React.createContext(false)

type InteractionType = "mouse" | "touch" | "pen" | "keyboard" | ""

/** Last input on the page. Radix does not say how a dialog was opened or closed. */
let lastInteraction: InteractionType = ""
let listening = false

function trackInteraction() {
  if (listening || typeof document === "undefined") return
  listening = true
  document.addEventListener(
    "pointerdown",
    (event) => {
      lastInteraction = (event.pointerType || "mouse") as InteractionType
    },
    true,
  )
  document.addEventListener(
    "keydown",
    (event) => {
      if (!event.metaKey && !event.ctrlKey && !event.altKey) lastInteraction = "keyboard"
    },
    true,
  )
}

type DialogProps = React.ComponentProps<typeof DialogPrimitive.Root>

function Dialog({ onOpenChange, ...props }: DialogProps) {
  const [instant, setInstant] = React.useState(false)
  React.useEffect(trackInteraction, [])
  return (
    <InstantContext.Provider value={instant}>
      <DialogPrimitive.Root
        onOpenChange={(open) => {
          setInstant(lastInteraction === "keyboard")
          onOpenChange?.(open)
        }}
        {...props}
      />
    </InstantContext.Provider>
  )
}

type FocusTarget =
  | boolean
  | React.RefObject<HTMLElement | null>
  | ((type: InteractionType) => boolean | HTMLElement | null | undefined)

/** Base UI initialFocus/finalFocus as a Radix onOpenAutoFocus/onCloseAutoFocus handler. */
function focusHandler(target: FocusTarget | undefined, user?: (event: Event) => void) {
  return (event: Event) => {
    user?.(event)
    if (event.defaultPrevented || target === undefined || target === true) return
    const resolved =
      typeof target === "function"
        ? target(lastInteraction)
        : typeof target === "boolean"
          ? target
          : target.current
    if (resolved === true || resolved == null) return
    event.preventDefault()
    if (resolved instanceof HTMLElement) resolved.focus({ preventScroll: true })
  }
}

function DialogTrigger({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Trigger>) {
  return <DialogPrimitive.Trigger data-slot="dialog-trigger" className={className} {...props} />
}

type DialogSize = "sm" | "md" | "lg" | "xl" | "full"

const sizes: Record<DialogSize, string> = {
  sm: "w-[min(24rem,calc(100vw-2rem))]",
  md: "w-[min(32rem,calc(100vw-2rem))]",
  lg: "w-[min(44rem,calc(100vw-2rem))]",
  xl: "w-[min(64rem,calc(100vw-2rem))]",
  full: "h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)]",
}

interface DialogContentProps extends React.ComponentProps<typeof DialogPrimitive.Content> {
  /** What to focus on open: `false` keeps focus on the trigger, a ref or a callback picks one. */
  initialFocus?: FocusTarget
  /** What to focus on close: `false` leaves focus where it is. */
  finalFocus?: FocusTarget
  size?: DialogSize
  /** Render the close button in the corner. On by default. */
  showClose?: boolean
  /** Classes for the scrim behind the popup. */
  backdropClassName?: string
  container?: React.ComponentProps<typeof DialogPrimitive.Portal>["container"]
}

function DialogContent({
  size = "md",
  showClose = true,
  backdropClassName,
  container,
  initialFocus,
  finalFocus,
  onOpenAutoFocus,
  onCloseAutoFocus,
  className,
  children,
  ...props
}: DialogContentProps) {
  const instant = React.useContext(InstantContext)
  return (
    <DialogPrimitive.Portal container={container}>
      <DialogPrimitive.Overlay
        data-slot="dialog-backdrop"
        data-dialog-instant={instant ? "" : undefined}
        className={cn(
          "bg-chatcn-scrim fixed inset-0 z-50",
          "ease-chatcn-out transition-opacity duration-220",
          "data-[state=closed]:opacity-0 data-[state=closed]:duration-160 starting:opacity-0",
          // Radix unmounts at once unless an animation runs; this keyframe holds the exit.
          "data-[state=closed]:animate-[chatcn-shimmer_var(--tw-duration,150ms)]",
          "data-[dialog-instant]:animate-none data-[dialog-instant]:transition-none",
          backdropClassName,
        )}
      />
      <DialogPrimitive.Content
        data-slot="dialog-content"
        onOpenAutoFocus={focusHandler(initialFocus, onOpenAutoFocus)}
        onCloseAutoFocus={focusHandler(finalFocus, onCloseAutoFocus)}
        data-size={size}
        data-dialog-instant={instant ? "" : undefined}
        className={cn(
          "bg-chatcn-surface text-chatcn-fg shadow-chatcn-xl rounded-chatcn-xl fixed top-1/2 left-1/2 z-50 -translate-x-1/2 -translate-y-1/2",
          "flex max-h-[calc(100dvh-2rem)] origin-center flex-col overflow-hidden outline-none",
          sizes[size],
          "ease-chatcn-out transition-[opacity,scale] duration-220",
          "starting:scale-(--chatcn-enter-scale) starting:opacity-0",
          "data-[state=closed]:scale-(--chatcn-exit-scale) data-[state=closed]:opacity-0 data-[state=closed]:duration-160",
          "data-[state=closed]:animate-[chatcn-shimmer_var(--tw-duration,150ms)]",
          "motion-reduce:data-[state=closed]:scale-100",
          "data-[dialog-instant]:animate-none data-[dialog-instant]:transition-none",
          // Forced colors strip shadows, so the hairline needs a real outline.
          "forced-colors:outline-1 forced-colors:outline-solid",
          className,
        )}
        {...props}
      >
        {children}
        {showClose ? (
          <DialogPrimitive.Close
            aria-label="Close"
            data-slot="dialog-close"
            className={cn(
              buttonVariants({ variant: "ghost", size: "icon-sm" }),
              "absolute end-3 top-3 pointer-coarse:size-11",
            )}
          >
            <XIcon />
          </DialogPrimitive.Close>
        ) : null}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  )
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-header"
      className={cn("flex flex-col gap-1.5 px-5 pt-5", className)}
      {...props}
    />
  )
}

function DialogFooter({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn("flex items-center justify-end gap-2 px-5 pt-2 pb-5", className)}
      {...props}
    />
  )
}

function DialogTitle({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      data-slot="dialog-title"
      className={cn("chatcn-text-16 text-chatcn-fg pe-8 font-semibold text-balance", className)}
      {...props}
    />
  )
}

function DialogDescription({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      data-slot="dialog-description"
      className={cn("chatcn-text-14 text-chatcn-fg-muted text-pretty", className)}
      {...props}
    />
  )
}

function DialogClose(props: React.ComponentProps<typeof DialogPrimitive.Close>) {
  return <DialogPrimitive.Close data-slot="dialog-close" {...props} />
}

export type { DialogContentProps, DialogProps, DialogSize }
export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
}
