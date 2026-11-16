// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"

/**
 * Screen reader announcement for streamed content: silent while it streams, one polite message
 * when it completes. Render the returned `message` in an `aria-live="polite"` element.
 *
 * `read` is called once, at completion, so it can pull text from the DOM. The message clears
 * after a few seconds so the copy does not linger next to the visible content.
 */
export function useChatcnAnnounce(
  streaming: boolean,
  read: () => string | null | undefined,
  { enabled = true, clearAfterMs = 6000 }: { enabled?: boolean; clearAfterMs?: number } = {},
): string {
  const [message, setMessage] = React.useState("")
  const was = React.useRef(streaming)
  const readRef = React.useRef(read)
  readRef.current = read

  React.useEffect(() => {
    const started = streaming && !was.current
    const finished = !streaming && was.current
    was.current = streaming
    if (started) setMessage("")
    if (!finished || !enabled) return
    setMessage(readRef.current()?.trim() ?? "")
    const id = window.setTimeout(() => setMessage(""), clearAfterMs)
    return () => window.clearTimeout(id)
  }, [streaming, enabled, clearAfterMs])

  return message
}
