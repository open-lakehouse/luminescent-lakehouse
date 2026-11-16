// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import {
  type ChatcnTokenLine,
  createLineTokenizer,
  isLanguageReady,
  loadLanguage,
  plainLines,
  resolveLanguage,
  tokenize,
} from "@/components/ui/chatcn/lib/chatcn-highlighter"

/** Return value of `useChatcnHighlight`. */
export interface UseChatcnHighlightResult {
  lines: ChatcnTokenLine[]
  /** False while the grammar loads (lines are plain) or for unknown languages. */
  highlighted: boolean
}

/**
 * Tokenizes code with the shared lazy Shiki. Renders plain lines first, then swaps to highlighted
 * lines on the frame the grammar is ready. Same text, same metrics: the swap never moves layout.
 * While `streaming`, only the line being written is re-tokenized on each update.
 */
export function useChatcnHighlight(
  code: string,
  language?: string,
  { streaming = false }: { streaming?: boolean } = {},
): UseChatcnHighlightResult {
  const lang = resolveLanguage(language)
  const [, setVersion] = React.useState(0)
  const ready = isLanguageReady(lang)

  React.useEffect(() => {
    if (ready) return
    let alive = true
    void loadLanguage(lang).then(() => {
      if (alive) setVersion((v) => v + 1)
    })
    return () => {
      alive = false
    }
  }, [lang, ready])

  // One incremental tokenizer per language, kept across renders.
  const tokenizer = React.useMemo(() => createLineTokenizer(lang), [lang])

  return React.useMemo(() => {
    const lines = streaming ? tokenizer.update(code) : tokenize(code, lang)
    if (lines) return { lines, highlighted: ready }
    return { lines: plainLines(code), highlighted: false }
    // `ready` re-runs this once the grammar arrives.
  }, [code, lang, streaming, tokenizer, ready])
}
