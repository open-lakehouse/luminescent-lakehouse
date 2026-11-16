// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"

type SubmitOn = "enter" | "mod-enter" | "auto"

interface UseChatcnImeSubmitOptions {
  /** Called on Enter (or Cmd/Ctrl+Enter). Never called while an IME is composing. */
  onSubmit: () => void
  /** Called on Escape while `streaming` is true. */
  onStop?: () => void
  streaming?: boolean
  /**
   * Which key sends.
   * - `enter`: Enter sends, Shift+Enter inserts a newline. Cmd/Ctrl+Enter also sends.
   * - `mod-enter`: Enter inserts a newline, Cmd/Ctrl+Enter sends.
   * - `auto` (default): `enter` on fine pointers, `mod-enter` on touch screens, where the
   *   on-screen keyboard has no Shift+Enter and the send button is right there.
   */
  submitOn?: SubmitOn
  /** Swallow Enter without sending. */
  disabled?: boolean
}

function isCoarsePointer() {
  return typeof window !== "undefined" && window.matchMedia?.("(pointer: coarse)").matches === true
}

/**
 * Keyboard handling for chat inputs that is safe with input method editors (Japanese, Chinese,
 * Korean and others). Enter that confirms a composition never sends.
 *
 * Safari fires `compositionend` before the confirming `keydown`, so `isComposing` is already
 * false there. The hook also checks `keyCode === 229` and holds a flag until the next task.
 *
 * Spread the returned handlers on a textarea or input.
 */
function useChatcnImeSubmit(options: UseChatcnImeSubmitOptions) {
  const optionsRef = React.useRef(options)
  optionsRef.current = options
  const composing = React.useRef(false)

  const onCompositionStart = React.useCallback(() => {
    composing.current = true
  }, [])

  const onCompositionEnd = React.useCallback(() => {
    window.setTimeout(() => {
      composing.current = false
    }, 0)
  }, [])

  const onKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement | HTMLInputElement>) => {
      if (event.defaultPrevented) return
      const native = event.nativeEvent
      // biome-ignore lint/suspicious/noExplicitAny: keyCode 229 is the only Safari IME signal
      if (native.isComposing || composing.current || (native as any).keyCode === 229) return

      const { onSubmit, onStop, streaming, submitOn = "auto", disabled } = optionsRef.current

      if (event.key === "Escape" && streaming && onStop) {
        event.preventDefault()
        onStop()
        return
      }
      if (event.key !== "Enter" || event.shiftKey || event.altKey) return

      const mod = event.metaKey || event.ctrlKey
      const mode = submitOn === "auto" ? (isCoarsePointer() ? "mod-enter" : "enter") : submitOn
      if (mode === "mod-enter" && !mod) return

      event.preventDefault()
      if (disabled) return
      onSubmit()
    },
    [],
  )

  return { onKeyDown, onCompositionStart, onCompositionEnd }
}

export type { SubmitOn, UseChatcnImeSubmitOptions }
export { useChatcnImeSubmit }
