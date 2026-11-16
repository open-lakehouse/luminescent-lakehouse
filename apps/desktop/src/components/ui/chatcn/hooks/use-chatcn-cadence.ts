// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"

/** Options for `useChatcnCadence`. */
export interface UseChatcnCadenceOptions {
  /** True while more text may arrive. When false, any backlog drains quickly and the hook settles. */
  streaming?: boolean
  /** Turn smoothing on. Off returns the text as is. Defaults to true. */
  enabled?: boolean
  /** Floor speed in characters per second. The reveal speeds up to keep lag bounded. Default 90. */
  charsPerSecond?: number
  /** Most lag the reveal may build before it catches up, in ms. Default 240. */
  maxLagMs?: number
}

/** Return value of `useChatcnCadence`. */
export interface UseChatcnCadenceResult {
  /** The part of the text to render now. */
  text: string
  /** True while the reveal is behind the source or the source is still streaming. */
  revealing: boolean
}

function prefersReducedMotion() {
  return (
    typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
  )
}

/**
 * Smooth streaming. Network chunks arrive in bursts; this releases them at a steady pace so text
 * flows instead of jumping, without ever falling far behind: the speed scales with the backlog,
 * so lag stays under `maxLagMs`. It reveals whole words where it can, never animates a character,
 * and returns history (text that was complete at mount) immediately.
 */
export function useChatcnCadence(
  source: string,
  {
    streaming = false,
    enabled = true,
    charsPerSecond = 90,
    maxLagMs = 240,
  }: UseChatcnCadenceOptions = {},
): UseChatcnCadenceResult {
  // Text already present when the component mounts is history: show it at once.
  const [shown, setShown] = React.useState(() => source.length)
  const shownRef = React.useRef(shown)
  const sourceRef = React.useRef(source)
  const streamingRef = React.useRef(streaming)
  sourceRef.current = source
  streamingRef.current = streaming

  const active = enabled && !prefersReducedMotion()

  // A replaced (not appended) source starts over from its full length.
  const previous = React.useRef(source)
  if (!source.startsWith(previous.current.slice(0, shownRef.current))) {
    shownRef.current = source.length
  }
  previous.current = source

  const behind = active && shownRef.current < source.length

  React.useEffect(() => {
    if (!behind) return
    let frame = 0
    let last = performance.now()
    const tick = (now: number) => {
      const dt = Math.min(64, now - last)
      last = now
      const target = sourceRef.current
      const backlog = target.length - shownRef.current
      if (backlog <= 0) return
      // Floor speed, or fast enough to clear the backlog within maxLag (faster once the stream ends).
      const windowMs = streamingRef.current ? maxLagMs : maxLagMs / 2
      const perMs = Math.max(charsPerSecond / 1000, backlog / windowMs)
      let next = Math.min(target.length, shownRef.current + Math.max(1, Math.round(perMs * dt)))
      // Finish the current word rather than cutting it, when the word end is close.
      const space = target.slice(next, next + 12).search(/\s/)
      if (space > 0) next += space
      shownRef.current = Math.min(next, target.length)
      setShown(shownRef.current)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [behind, charsPerSecond, maxLagMs])

  if (!active) {
    shownRef.current = source.length
    return { text: source, revealing: streaming }
  }
  const visible = Math.min(Math.max(shown, shownRef.current), source.length)
  return {
    text: source.slice(0, visible),
    revealing: streaming || visible < source.length,
  }
}
