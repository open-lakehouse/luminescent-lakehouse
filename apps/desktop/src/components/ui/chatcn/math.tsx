// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { CopyButton } from "@/components/ui/chatcn/copy-button"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { CircleAlertIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * KaTeX, loaded on first use
 * -----------------------------------------------------------------------------------------------*/

type Katex = typeof import("katex")["default"]

let stylesheet: string | false | undefined
let katexTask: Promise<{ katex: Katex; styled: boolean }> | undefined

/**
 * Where KaTeX's stylesheet comes from. By default chatcn adds it from jsDelivr, pinned to the
 * installed KaTeX version, the first time math renders, unless the page already has it. Pass a
 * URL to self-host, or `false` when you import `katex/dist/katex.min.css` yourself.
 */
function configureMath(options: { stylesheet?: string | false }) {
  stylesheet = options.stylesheet
}

/** True when KaTeX's CSS is already on the page (imported by the app or added earlier). */
function hasKatexStyles() {
  const probe = document.createElement("span")
  probe.className = "katex"
  probe.style.visibility = "hidden"
  const inner = document.createElement("span")
  inner.className = "katex-mathml"
  probe.appendChild(inner)
  document.body.appendChild(probe)
  const present = getComputedStyle(inner).position === "absolute"
  probe.remove()
  return present
}

/** Resolves true when KaTeX's CSS is on the page, false when it could not be loaded. */
function ensureStyles(version: string): Promise<boolean> {
  if (stylesheet === false || typeof document === "undefined") return Promise.resolve(true)
  const existing = document.querySelector<HTMLLinkElement>("link[data-chatcn-katex]")
  if (!existing && hasKatexStyles()) return Promise.resolve(true)
  const link =
    existing ??
    Object.assign(document.createElement("link"), {
      rel: "stylesheet",
      href: stylesheet ?? `https://cdn.jsdelivr.net/npm/katex@${version}/dist/katex.min.css`,
      crossOrigin: "anonymous",
    })
  if (!existing) {
    link.dataset.chatcnKatex = ""
    document.head.appendChild(link)
  }
  if (link.sheet) return Promise.resolve(true)
  // Rendered HTML math without its CSS shows every formula twice. Wait for the sheet, but never
  // forever: when it fails (offline, a CSP, a blocked CDN) formulas render as native MathML.
  return new Promise((resolve) => {
    link.addEventListener("load", () => resolve(true), { once: true })
    link.addEventListener("error", () => resolve(false), { once: true })
    window.setTimeout(() => resolve(hasKatexStyles()), 3000)
  })
}

/** Loads the fonts most formulas use before the first render, so glyphs do not swap and reflow. */
function preloadFonts(): Promise<void> {
  if (typeof document === "undefined" || !document.fonts) return Promise.resolve()
  const faces = ["1em KaTeX_Main", "italic 1em KaTeX_Math", "1em KaTeX_Size1", "1em KaTeX_Size2"]
  const loading = Promise.all(faces.map((f) => document.fonts.load(f).catch(() => []))).then(
    () => {},
  )
  return Promise.race([loading, new Promise<void>((r) => window.setTimeout(r, 1500))])
}

function loadKatex() {
  katexTask ??= import("katex").then(async (mod) => {
    const katex = mod.default
    const styled = await ensureStyles(katex.version)
    if (styled) await preloadFonts()
    return { katex, styled }
  })
  return katexTask
}

type RenderResult = { html: string } | { error: string }

/** Typeset output takes the source's place with a short fade, in the same footprint. */
const arrive = "ease-chatcn-out transition-opacity duration-150 starting:opacity-0"

function useKatex(tex: string, display: boolean, enabled: boolean) {
  const [result, setResult] = React.useState<RenderResult>()
  React.useEffect(() => {
    if (!enabled) return
    let alive = true
    loadKatex()
      .then(({ katex, styled }) => {
        if (!alive) return
        try {
          const html = katex.renderToString(tex, {
            displayMode: display,
            throwOnError: true,
            strict: "ignore",
            trust: false,
            // Without KaTeX's CSS the HTML output breaks apart; the browser lays out MathML itself.
            output: styled ? "htmlAndMathml" : "mathml",
          })
          setResult({ html })
        } catch (error) {
          setResult({ error: error instanceof Error ? error.message : String(error) })
        }
      })
      .catch(() => alive && setResult({ error: "KaTeX could not be loaded." }))
    return () => {
      alive = false
    }
  }, [tex, display, enabled])
  return enabled ? result : undefined
}

/* -------------------------------------------------------------------------------------------------
 * MathFormula
 * -----------------------------------------------------------------------------------------------*/

interface MathProps extends Omit<React.ComponentProps<"span">, "children"> {
  /** TeX source, without delimiters. */
  tex: string
  /** Display (block) math, centered on its own line. Inline otherwise. */
  display?: boolean
  /** True while the formula is still arriving: shows the source until it is complete. */
  streaming?: boolean
  /** Copy LaTeX button on display math. On by default. */
  copyable?: boolean
}

/**
 * KaTeX math, inline or display. KaTeX loads the first time a formula appears. Until it is ready,
 * and while a formula streams, the TeX source holds the space so the line does not jump.
 */
function MathFormula({
  tex,
  display = false,
  streaming = false,
  copyable = true,
  className,
  ...props
}: MathProps) {
  const source = tex.trim()
  const result = useKatex(source, display, !streaming && source.length > 0)
  const html = result && "html" in result ? result.html : undefined
  const error = result && "error" in result ? result.error : undefined

  const fallback = (
    <span
      className={cn(
        "text-chatcn-fg-muted font-mono text-[0.9em] [overflow-wrap:anywhere]",
        // Centred like the typeset formula, so finishing never moves it sideways.
        display && "w-full text-center text-balance whitespace-pre-wrap",
      )}
    >
      {display ? source : `$${source}$`}
    </span>
  )

  if (!display) {
    return (
      <span
        data-slot="math"
        data-display="inline"
        data-math-state={html ? "rendered" : error ? "error" : "pending"}
        className={cn("[&_.katex]:text-[1.06em]", className)}
        {...props}
      >
        {html ? (
          <span className={arrive} dangerouslySetInnerHTML={{ __html: html }} />
        ) : error ? (
          <Tooltip content="Could not render this formula">{fallback}</Tooltip>
        ) : (
          fallback
        )}
      </span>
    )
  }

  return (
    <span
      data-slot="math"
      data-display="block"
      data-math-state={html ? "rendered" : error ? "error" : "pending"}
      className={cn("group/math relative block", className)}
      {...props}
    >
      <span
        className={cn(
          // Reserves the height of a typical one-line formula, so rendering never moves the text below.
          "chatcn-scroll flex min-h-18 items-center justify-center overflow-x-auto overflow-y-hidden px-10 py-2",
          "[&_.katex]:text-[1.15em] [&_.katex-display]:m-0 [&_.katex-display]:w-full",
        )}
      >
        {html ? (
          <span className={cn("w-full", arrive)} dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          fallback
        )}
      </span>
      {error ? (
        <span className="chatcn-text-12 text-chatcn-fg-subtle flex items-center justify-center gap-1.5 pb-1">
          <CircleAlertIcon aria-hidden="true" className="text-chatcn-red size-3.5" />
          Could not render this formula. Showing the source.
        </span>
      ) : null}
      {copyable && !streaming ? (
        <span
          className={cn(
            "absolute end-0 top-2 flex",
            "pointer-fine:opacity-0 pointer-fine:group-focus-within/math:opacity-100 pointer-fine:group-hover/math:opacity-100",
          )}
        >
          <CopyButton value={source} label="Copy LaTeX" size="icon-xs" />
        </span>
      ) : null}
    </span>
  )
}

export type { MathProps }
export { configureMath, MathFormula }
