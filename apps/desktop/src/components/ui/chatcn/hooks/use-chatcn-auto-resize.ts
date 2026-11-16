// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"

const useIsoLayoutEffect = typeof window === "undefined" ? React.useEffect : React.useLayoutEffect

let fieldSizing: boolean | undefined

/** True where the browser sizes textareas to their content on its own (`field-sizing: content`). */
function supportsFieldSizing() {
  if (fieldSizing === undefined) {
    fieldSizing = typeof CSS !== "undefined" && CSS.supports?.("field-sizing", "content") === true
  }
  return fieldSizing
}

/**
 * Sizes a textarea to its content. Pair it with a CSS `max-height` (chatcn uses 40vh) and
 * `overflow-y: auto`: past the cap the textarea scrolls instead of growing.
 */
function resizeTextarea(el: HTMLTextAreaElement) {
  // Measuring resets the scroll position; put it back so the caret stays where the user is.
  const { scrollTop } = el
  const style = window.getComputedStyle(el)
  const borders =
    Number.parseFloat(style.borderTopWidth) + Number.parseFloat(style.borderBottomWidth)
  el.style.height = "auto"
  el.style.height = `${el.scrollHeight + borders}px`
  el.scrollTop = scrollTop
}

interface UseChatcnAutoResizeOptions {
  /** The controlled value. Pass it so the textarea re-measures when the value changes in code. */
  value?: string
  /** Turn sizing off, for example for a fixed-height variant. Default true. */
  enabled?: boolean
}

/**
 * Grows a textarea with its content and shrinks it back when cleared. Uses native
 * `field-sizing: content` where supported (no JS, no layout reads) and measures `scrollHeight`
 * elsewhere, on input, on value changes and when the width changes the wrapping.
 */
function useChatcnAutoResize(
  ref: React.RefObject<HTMLTextAreaElement | null>,
  { value, enabled = true }: UseChatcnAutoResizeOptions = {},
) {
  // Value changes made in code (clear after send, insert a transcript).
  useIsoLayoutEffect(() => {
    const el = ref.current
    if (!el || !enabled || supportsFieldSizing()) return
    resizeTextarea(el)
  }, [ref, value, enabled])

  // Typing in uncontrolled mode, and width changes that re-wrap the text.
  React.useEffect(() => {
    const el = ref.current
    if (!el || !enabled || supportsFieldSizing()) return
    const onInput = () => resizeTextarea(el)
    el.addEventListener("input", onInput)
    let width = el.offsetWidth
    const observer = new ResizeObserver(() => {
      if (el.offsetWidth === width) return
      width = el.offsetWidth
      resizeTextarea(el)
    })
    observer.observe(el)
    return () => {
      el.removeEventListener("input", onInput)
      observer.disconnect()
      el.style.height = ""
    }
  }, [ref, enabled])
}

export type { UseChatcnAutoResizeOptions }
export { resizeTextarea, supportsFieldSizing, useChatcnAutoResize }
