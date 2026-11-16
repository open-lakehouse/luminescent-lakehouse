// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { Spinner } from "@/components/ui/chatcn/spinner"
import { ArrowUpIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

type SendButtonStatus = "idle" | "submitting" | "streaming"

interface SendButtonProps extends Omit<React.ComponentProps<"button">, "children"> {
  /**
   * - `idle`: Send. Disabled only when `empty`.
   * - `submitting`: a spinner while the request is on its way (or files are still uploading).
   * - `streaming`: Stop. Clicking calls `onStop`.
   */
  status?: SendButtonStatus
  /** Nothing to send. The one reason Send is ever disabled. */
  empty?: boolean
  onStop?: () => void
  shape?: "round" | "square"
  /** 28, 32 or 36 px. Touch screens get a 44 px hit area either way. */
  size?: "sm" | "md" | "lg"
  /** Accessible names. Defaults: Send message, Stop generating, Sending. */
  labels?: { send?: string; stop?: string; submitting?: string }
}

const sizes = {
  sm: { box: "size-7", icon: "size-3.5", square: "size-2" },
  md: { box: "size-8", icon: "size-4", square: "size-2.5" },
  lg: { box: "size-9", icon: "size-4", square: "size-2.5" },
}

// Each glyph is its own layer; state changes crossfade them with a 2 px blur so the button reads
// as one object changing, never as a swap. Same footprint, so nothing around it moves.
const layer = cn(
  "pointer-events-none absolute grid place-items-center",
  "ease-chatcn-out transition-[opacity,scale,translate,filter] duration-150",
  "motion-reduce:translate-none! motion-reduce:scale-none! motion-reduce:blur-none!",
)
const shown = "opacity-100 blur-0 scale-100 translate-y-0"

/**
 * Send that becomes Stop in the same footprint. The arrow lifts away with the message and blurs
 * out while the kit's signature mark settles in: the same rounded square as the streaming caret,
 * so the thing that is writing is the thing you press to stop it. When the turn ends the next
 * arrow rises in from below, never back down from where the last one left. Escape in the composer
 * does the same as clicking Stop.
 */
function SendButton({
  status = "idle",
  empty = false,
  onStop,
  shape = "round",
  size = "md",
  labels,
  type = "submit",
  className,
  onClick,
  disabled,
  ...props
}: SendButtonProps) {
  const s = sizes[size]
  const streaming = status === "streaming"
  const submitting = status === "submitting"
  const inactive = !streaming && !submitting && empty

  // Count returns to idle. The arrow layer remounts on each one, so its @starting-style places the
  // fresh arrow below the centre and it rises into place: the old one left upward with the message.
  const [seen, setSeen] = React.useState(status)
  const [returns, setReturns] = React.useState(0)
  if (seen !== status) {
    setSeen(status)
    if (status === "idle") setReturns((n) => n + 1)
  }

  const label = streaming
    ? (labels?.stop ?? "Stop generating")
    : submitting
      ? (labels?.submitting ?? "Sending")
      : (labels?.send ?? "Send message")

  return (
    <button
      type={streaming ? "button" : type}
      aria-label={label}
      aria-keyshortcuts={streaming ? "Escape" : "Enter"}
      // Empty stays focusable (aria-disabled, not disabled): after Stop the input is often empty,
      // and a disabled button would drop keyboard focus onto the page.
      aria-disabled={submitting || inactive || undefined}
      disabled={disabled}
      data-slot="send-button"
      data-send-state={status}
      data-empty={inactive ? "" : undefined}
      onClick={(event) => {
        if (submitting || inactive) {
          event.preventDefault()
          return
        }
        if (streaming) {
          event.preventDefault()
          onStop?.()
        }
        onClick?.(event)
      }}
      className={cn(
        "chatcn-focus relative grid shrink-0 place-items-center select-none",
        "ease-chatcn-standard transition-[background-color,color,scale] duration-150",
        "active:scale-(--chatcn-press-scale-icon) active:duration-100",
        "bg-chatcn-primary text-chatcn-primary-fg hover:bg-chatcn-primary-hover",
        "data-[empty]:bg-chatcn-fill-active data-[empty]:text-chatcn-fg-faint data-[empty]:hover:bg-chatcn-fill-active data-[empty]:active:scale-100",
        "disabled:cursor-default disabled:opacity-50 aria-disabled:cursor-default data-[send-state=submitting]:cursor-progress",
        // 44 px hit area on touch without changing the visual size.
        "after:absolute after:-inset-1.5 after:content-[''] pointer-fine:after:hidden",
        "forced-colors:border forced-colors:border-[ButtonText]",
        s.box,
        shape === "round" ? "rounded-full" : "rounded-chatcn-md",
        className,
      )}
      {...props}
    >
      <span
        key={returns}
        aria-hidden="true"
        data-send-layer="arrow"
        className={cn(
          layer,
          streaming || submitting ? "-translate-y-1 scale-90 opacity-0 blur-[2px]" : shown,
          returns > 0 &&
            "starting:translate-y-1 starting:scale-90 starting:opacity-0 starting:blur-[2px]",
        )}
      >
        <ArrowUpIcon strokeWidth={2} className={s.icon} />
      </span>
      <span
        aria-hidden="true"
        className={cn(layer, submitting ? shown : "scale-75 opacity-0 blur-[2px]")}
      >
        {submitting ? <Spinner className={s.icon} /> : null}
      </span>
      <span
        aria-hidden="true"
        className={cn(layer, streaming ? shown : "scale-50 opacity-0 blur-[2px]")}
      >
        {/* 22% corners: the streaming caret's ratio (0.12em on 0.55em), at every size. */}
        <span className={cn("rounded-[22%] bg-current", s.square)} />
      </span>
    </button>
  )
}

export type { SendButtonProps, SendButtonStatus }
export { SendButton }
