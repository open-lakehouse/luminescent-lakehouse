// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { cn } from "@/lib/utils"

interface InlineCodeProps extends Omit<React.ComponentProps<"code">, "onCopy"> {
  /** Click to copy. For commands, package names and ids the reader will paste somewhere. */
  copyable?: boolean
  /** Text to copy. Defaults to the rendered text. */
  copyValue?: string
  onCopy?: (value: string) => void
}

const base = cn(
  "rounded-chatcn-xs bg-chatcn-fill text-chatcn-fg px-[0.3em] py-[0.1em] font-mono text-[0.9em]",
  "[box-decoration-break:clone] [overflow-wrap:anywhere] shadow-[inset_0_0_0_1px_var(--chatcn-alpha-300)]",
)

/** Mono at 0.9em on a quiet fill. Copyable code turns into a button that says what it did. */
function InlineCode({
  copyable,
  copyValue,
  onCopy,
  className,
  children,
  ...props
}: InlineCodeProps) {
  const ref = React.useRef<HTMLElement>(null)
  const [copied, setCopied] = React.useState(false)
  React.useEffect(() => {
    if (!copied) return
    const id = window.setTimeout(() => setCopied(false), 1500)
    return () => window.clearTimeout(id)
  }, [copied])

  if (!copyable) {
    return (
      <code data-slot="inline-code" className={cn(base, className)} {...props}>
        {children}
      </code>
    )
  }

  const copy = () => {
    const text = copyValue ?? ref.current?.textContent ?? ""
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(true)
      onCopy?.(text)
    })
  }

  return (
    <Tooltip content={copied ? "Copied" : "Copy"}>
      <button
        type="button"
        onClick={copy}
        data-slot="inline-code"
        data-copied={copied ? "" : undefined}
        className={cn(
          "chatcn-focus rounded-chatcn-xs inline cursor-copy text-start align-baseline",
          "[&>code]:ease-chatcn-standard [&>code]:transition-[background-color,box-shadow] [&>code]:duration-150",
          "hover:[&>code]:bg-chatcn-fill-hover data-[copied]:[&>code]:bg-chatcn-fill-active",
          // Copied: the hairline takes the accent for a moment. Seen on touch too, where the
          // tooltip never opens, and nothing in the sentence moves.
          "data-[copied]:[&>code]:shadow-[inset_0_0_0_1px_var(--chatcn-accent)]",
        )}
      >
        <code ref={ref} className={cn(base, className)} {...props}>
          {children}
        </code>
        <span aria-live="polite" className="sr-only">
          {copied ? "Copied" : ""}
        </span>
      </button>
    </Tooltip>
  )
}

export type { InlineCodeProps }
export { InlineCode }
