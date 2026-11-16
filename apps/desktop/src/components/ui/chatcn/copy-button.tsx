// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"
import { Button, type ButtonProps } from "@/components/ui/chatcn/button"
import { CheckIcon, CopyIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

interface CopyButtonProps extends Omit<ButtonProps, "value" | "onCopy"> {
  value: string | (() => string)
  label?: string
  onCopy?: (value: string) => void
}

/** Copy swaps to a check with a 150 ms blur crossfade, holds for 1.5 s and announces itself. */
function CopyButton({
  value,
  label = "Copy",
  onCopy,
  onClick,
  children,
  size = "icon-sm",
  className,
  ...props
}: CopyButtonProps) {
  // A counter, not a flag: copying again while the check shows restarts the 1.5 s hold.
  const [copies, setCopies] = React.useState(0)
  const [copied, setCopied] = React.useState(false)
  React.useEffect(() => {
    if (!copies) return
    setCopied(true)
    const id = window.setTimeout(() => setCopied(false), 1500)
    return () => window.clearTimeout(id)
  }, [copies])

  // Reduced motion keeps the crossfade and drops the scale and blur.
  const layer = cn(
    "ease-chatcn-out absolute transition-[opacity,scale,filter] duration-150",
    "motion-reduce:scale-100 motion-reduce:blur-none motion-reduce:transition-opacity",
  )
  return (
    <Button
      {...props}
      size={size}
      aria-label={children ? undefined : copied ? "Copied" : label}
      onClick={(event) => {
        // Compose with handlers injected by wrappers such as Tooltip instead of replacing them.
        onClick?.(event)
        if (event.defaultPrevented) return
        const text = typeof value === "function" ? value() : value
        navigator.clipboard
          ?.writeText(text)
          .then(() => {
            setCopies((n) => n + 1)
            onCopy?.(text)
          })
          // Clipboard access can be denied (insecure context, permissions). Stay quiet and unchanged.
          .catch(() => {})
      }}
      className={cn("relative", className)}
    >
      {children ? (
        <>
          <span className="relative grid size-3.5 place-items-center">
            <CopyIcon
              className={cn(
                layer,
                copied ? "scale-75 opacity-0 blur-[2px]" : "scale-100 opacity-100",
              )}
            />
            <CheckIcon
              className={cn(
                layer,
                copied ? "scale-100 opacity-100" : "scale-75 opacity-0 blur-[2px]",
              )}
            />
          </span>
          {children}
        </>
      ) : null}
      {children ? null : (
        <>
          <CopyIcon
            className={cn(
              layer,
              copied ? "scale-75 opacity-0 blur-[2px]" : "scale-100 opacity-100",
            )}
          />
          <CheckIcon
            className={cn(
              layer,
              copied ? "scale-100 opacity-100" : "scale-75 opacity-0 blur-[2px]",
            )}
          />
        </>
      )}
      <span aria-live="polite" className="sr-only">
        {copied ? "Copied" : ""}
      </span>
    </Button>
  )
}

export type { CopyButtonProps }
export { CopyButton }
