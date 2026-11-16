// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import { Tabs } from "radix-ui"
import * as React from "react"
import { Button } from "@/components/ui/chatcn/button"
import { CopyButton } from "@/components/ui/chatcn/copy-button"
import { StreamingCaret } from "@/components/ui/chatcn/streaming-caret"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { useChatcnHighlight } from "@/components/ui/chatcn/hooks/use-chatcn-highlight"
import { type ChatcnTokenLine, chatcnTokenClass, languageLabel } from "@/components/ui/chatcn/lib/chatcn-highlighter"
import { ChevronDownIcon, ChevronUpIcon, WrapTextIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types
 * -----------------------------------------------------------------------------------------------*/

interface CodeBlockOptions {
  /** Line numbers in a sticky gutter. */
  showLineNumbers?: boolean
  /** Number of the first line, for excerpts. Defaults to 1. */
  startLine?: number
  /** Lines to mark, by number: `[3, 4]` or `"3-4,9"`. Counted from `startLine`. */
  highlightLines?: number[] | string
  /** Wrap long lines. Horizontal scroll is the default. */
  wrap?: boolean
  defaultWrap?: boolean
  onWrapChange?: (wrap: boolean) => void
  /**
   * Blocks longer than this collapse behind "Show N more lines". Blocks that stream in never
   * collapse, so the reader never loses their place. `false` turns it off. Defaults to 24.
   */
  collapseAfter?: number | false
}

interface CodeBlockProps
  extends CodeBlockOptions,
    Omit<React.ComponentProps<"figure">, "children"> {
  code: string
  /** Fence language: `tsx`, `bash`, `python`. Aliases such as `ts` and `sh` resolve. */
  language?: string
  /** Shown instead of the language in the header. */
  filename?: string
  /** True while the model is writing this block: highlights incrementally and shows the caret. */
  streaming?: boolean
  /** Extra header actions, placed before Copy. */
  actions?: React.ReactNode
  /** Hide the header row (language, wrap and copy). */
  hideHeader?: boolean
}

/* -------------------------------------------------------------------------------------------------
 * Helpers
 * -----------------------------------------------------------------------------------------------*/

/** `"1,3-5"` or `[1, 3, 4, 5]` to a set of line numbers. */
function parseLineRanges(value: number[] | string | undefined): Set<number> {
  const out = new Set<number>()
  if (!value) return out
  if (Array.isArray(value)) {
    for (const n of value) out.add(n)
    return out
  }
  for (const part of value.split(",")) {
    const [a, b] = part.split("-").map((s) => Number.parseInt(s.trim(), 10))
    if (a === undefined || Number.isNaN(a)) continue
    const end = b === undefined || Number.isNaN(b) ? a : b
    for (let n = Math.min(a, end); n <= Math.max(a, end); n++) out.add(n)
  }
  return out
}

function useControllable<T>(value: T | undefined, defaultValue: T, onChange?: (v: T) => void) {
  const [inner, setInner] = React.useState(defaultValue)
  const current = value ?? inner
  const set = React.useCallback(
    (next: T) => {
      if (value === undefined) setInner(next)
      onChange?.(next)
    },
    [value, onChange],
  )
  return [current, set] as const
}

/** In a `diff` fence, `+` and `-` lines (not the `+++` and `---` file headers) get a tint. */
function diffKind(tokens: ChatcnTokenLine): LineProps["diff"] {
  const text = tokens.map((t) => t.content).join("")
  if (text.startsWith("+") && !text.startsWith("+++")) return "add"
  if (text.startsWith("-") && !text.startsWith("---")) return "del"
  return undefined
}

const sameLine = (a: ChatcnTokenLine, b: ChatcnTokenLine) =>
  a === b ||
  (a.length === b.length && a.every((t, i) => t.content === b[i]?.content && t.role === b[i]?.role))

/* -------------------------------------------------------------------------------------------------
 * Lines
 * -----------------------------------------------------------------------------------------------*/

interface LineProps {
  tokens: ChatcnTokenLine
  number: number
  numbered: boolean
  marked: boolean
  caret: boolean
  /** Added or removed line in a `diff` fence. The +/- glyph stays as the non-color backup. */
  diff?: "add" | "del"
}

// Finished lines keep their props, so a streaming block only re-renders the line being written.
const Line = React.memo(
  function Line({ tokens, number, numbered, marked, caret, diff }: LineProps) {
    return (
      <span
        data-line={number}
        data-highlighted={marked ? "" : undefined}
        data-diff={diff}
        className={cn(
          // The block's surface is inherited all the way down and tints are painted over it, so
          // the sticky gutter is opaque and code never shows through it when scrolled sideways.
          "flex min-h-5 bg-inherit",
          (diff || marked) && "bg-[linear-gradient(var(--row),var(--row))]",
          diff === "add" && "[&_span]:text-chatcn-diff-add-fg [--row:var(--chatcn-diff-add-bg)]",
          diff === "del" && "[&_span]:text-chatcn-diff-del-fg [--row:var(--chatcn-diff-del-bg)]",
          marked && "shadow-[inset_2px_0_0_var(--chatcn-fg-subtle)] [--row:var(--chatcn-fill)]",
        )}
      >
        {numbered ? (
          <span
            aria-hidden="true"
            className={cn(
              "sticky start-0 shrink-0 bg-inherit [background-image:inherit] ps-4 pe-4 text-end tabular-nums [box-shadow:inherit] select-none",
              "w-[calc(var(--chatcn-code-digits)*1ch+2rem)]",
              marked ? "text-chatcn-fg-muted" : "text-chatcn-fg-subtle",
            )}
          >
            {number}
          </span>
        ) : null}
        <span className={cn("min-w-0 flex-1 pe-4", !numbered && "ps-4")}>
          {tokens.map((token, i) => (
            <span key={i} className={chatcnTokenClass[token.role]}>
              {token.content}
            </span>
          ))}
          {caret ? <StreamingCaret /> : null}
        </span>
      </span>
    )
  },
  (a, b) =>
    a.number === b.number &&
    a.numbered === b.numbered &&
    a.marked === b.marked &&
    a.caret === b.caret &&
    a.diff === b.diff &&
    sameLine(a.tokens, b.tokens),
)

/* -------------------------------------------------------------------------------------------------
 * Frame and header
 * -----------------------------------------------------------------------------------------------*/

function CodeBlockFrame({ className, ...props }: React.ComponentProps<"figure">) {
  return (
    <figure
      data-slot="code-block"
      className={cn(
        // `clip`, not `hidden`: rounds the corners without becoming a scroll container, so the
        // header can stay pinned while a long block scrolls past.
        "group/code-block rounded-chatcn-lg bg-chatcn-bg-subtle shadow-chatcn-border relative isolate overflow-clip",
        "chatcn-text-13 text-chatcn-fg forced-colors:outline-1 forced-colors:outline-solid",
        className,
      )}
      {...props}
    />
  )
}

/**
 * Marks the header `data-stuck` while it rides over the code, so it gains a hairline only then.
 * Listens to scrolling only while the block is on screen.
 */
function useStuck(ref: React.RefObject<HTMLDivElement | null>) {
  React.useEffect(() => {
    const header = ref.current
    const frame = header?.parentElement?.closest("[data-slot=code-block]")
    if (!header || !frame || typeof IntersectionObserver === "undefined") return
    let frameId = 0
    const check = () => {
      frameId = 0
      const offset = header.getBoundingClientRect().top - frame.getBoundingClientRect().top
      header.toggleAttribute("data-stuck", offset > 1)
    }
    const onScroll = () => {
      frameId ||= requestAnimationFrame(check)
    }
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) {
        document.addEventListener("scroll", onScroll, { capture: true, passive: true })
        check()
      } else {
        document.removeEventListener("scroll", onScroll, { capture: true })
        header.removeAttribute("data-stuck")
      }
    })
    observer.observe(frame)
    return () => {
      observer.disconnect()
      document.removeEventListener("scroll", onScroll, { capture: true })
      cancelAnimationFrame(frameId)
    }
  }, [ref])
}

function CodeBlockHeader({ className, ...props }: React.ComponentProps<"div">) {
  const ref = React.useRef<HTMLDivElement>(null)
  useStuck(ref)
  return (
    <div
      ref={ref}
      data-slot="code-block-header"
      className={cn(
        // Pinned to the top of the scroll area while the block is in view: Copy is always one
        // click away on a long file. Set `--chatcn-code-sticky-top` under a fixed page header.
        "bg-chatcn-bg-subtle sticky top-(--chatcn-code-sticky-top,0px) z-10 flex h-9 min-w-0 items-center gap-2 ps-3.5 pe-1.5",
        "data-[stuck]:shadow-[0_1px_0_var(--chatcn-border)]",
        className,
      )}
      {...props}
    />
  )
}

function WrapToggle({ wrap, onChange }: { wrap: boolean; onChange: (wrap: boolean) => void }) {
  return (
    <Tooltip content={wrap ? "Scroll long lines" : "Wrap long lines"}>
      <Button
        size="icon-sm"
        aria-label="Wrap lines"
        aria-pressed={wrap}
        onClick={() => onChange(!wrap)}
        className={cn(wrap && "bg-chatcn-fill-active text-chatcn-fg")}
      >
        <WrapTextIcon />
      </Button>
    </Tooltip>
  )
}

/* -------------------------------------------------------------------------------------------------
 * Body
 * -----------------------------------------------------------------------------------------------*/

interface CodeBlockBodyProps extends CodeBlockOptions {
  code: string
  language?: string
  streaming?: boolean
  wrap: boolean
  label: string
}

function CodeBlockBody({
  code,
  language,
  streaming = false,
  wrap,
  label,
  showLineNumbers = false,
  startLine = 1,
  highlightLines,
  collapseAfter = 24,
}: CodeBlockBodyProps) {
  // A trailing newline is a fence artifact, not a line the reader should see.
  const source = code.endsWith("\n") ? code.slice(0, -1) : code
  const { lines } = useChatcnHighlight(source, language, { streaming })
  const marked = React.useMemo(() => parseLineRanges(highlightLines), [highlightLines])

  // Decide once, at mount: a block that arrived streaming stays open when it finishes.
  const [streamedIn] = React.useState(streaming)
  const limit = collapseAfter === false ? Number.POSITIVE_INFINITY : collapseAfter
  const collapsible = !streamedIn && !streaming && lines.length > limit + 3
  const [expanded, setExpanded] = React.useState(false)
  const collapsed = collapsible && !expanded
  const visible = collapsed ? lines.slice(0, limit) : lines
  const hidden = lines.length - visible.length
  const lastNumber = startLine + lines.length - 1
  const isDiff = language !== undefined && /^(diff|patch)$/i.test(language)

  // Long lines fade at the edge they continue past, so the reader can tell there is more.
  // Attributes, not state: scrolling never re-renders the lines.
  const preRef = React.useRef<HTMLPreElement>(null)
  const measureEdges = React.useCallback(() => {
    const el = preRef.current
    if (!el) return
    el.toggleAttribute("data-scroll-start", el.scrollLeft > 1)
    el.toggleAttribute("data-scroll-end", el.scrollLeft + el.clientWidth < el.scrollWidth - 1)
  }, [])
  React.useEffect(() => {
    const el = preRef.current
    if (!el || typeof ResizeObserver === "undefined") return
    measureEdges()
    const observer = new ResizeObserver(measureEdges)
    observer.observe(el)
    if (el.firstElementChild) observer.observe(el.firstElementChild)
    return () => observer.disconnect()
  }, [measureEdges])

  return (
    <div data-slot="code-block-body" className="relative bg-inherit">
      <section aria-label={`${label} code`} className="bg-inherit">
        <pre
          ref={preRef}
          onScroll={measureEdges}
          dir="ltr"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: scrollable regions must be reachable by keyboard
          tabIndex={0}
          data-wrap={wrap ? "" : undefined}
          className={cn(
            "chatcn-scroll overflow-x-auto overscroll-x-contain bg-inherit pt-0.5 pb-3.5 font-mono outline-none",
            "focus-visible:shadow-[inset_0_0_0_2px_var(--chatcn-accent)]",
            wrap ? "[overflow-wrap:anywhere] break-words whitespace-pre-wrap" : "whitespace-pre",
            // Side fades start after the line-number gutter, so numbers never fade.
            "[--edge-end:black] [--edge-start:black] [--fade-end:0px] [--fade-start:0px]",
            "data-[scroll-start]:[--edge-start:transparent] data-[scroll-start]:[--fade-start:2rem]",
            "data-[scroll-end]:[--edge-end:transparent] data-[scroll-end]:[--fade-end:2rem]",
            showLineNumbers
              ? "[--gutter:calc(var(--chatcn-code-digits)*1ch+2rem)]"
              : "[--gutter:0px]",
            "[--sides:linear-gradient(to_right,black_var(--gutter),var(--edge-start)_var(--gutter),black_calc(var(--gutter)+var(--fade-start)),black_calc(100%-var(--fade-end)),var(--edge-end))]",
            collapsed
              ? "[mask-image:var(--sides),linear-gradient(to_bottom,black_62%,transparent)] pb-0"
              : "[mask-image:var(--sides)]",
            // The focus ring is inset, so the side fades step aside while the block has focus.
            "[mask-composite:intersect]",
            collapsed
              ? "focus-visible:[mask-image:linear-gradient(to_bottom,black_62%,transparent)]"
              : "focus-visible:[mask-image:none]",
          )}
          style={{ "--chatcn-code-digits": String(lastNumber).length } as React.CSSProperties}
        >
          <code className={cn("flex min-w-full flex-col bg-inherit", !wrap && "w-max")}>
            {visible.map((tokens, i) => (
              <Line
                key={i}
                tokens={tokens}
                number={startLine + i}
                numbered={showLineNumbers}
                marked={marked.has(startLine + i)}
                caret={streaming && i === lines.length - 1}
                diff={isDiff ? diffKind(tokens) : undefined}
              />
            ))}
          </code>
        </pre>
      </section>
      {collapsible ? (
        <div
          className={cn(
            "flex justify-center pb-2.5",
            collapsed ? "absolute inset-x-0 bottom-0" : "pt-1",
          )}
        >
          <Button
            size="xs"
            variant="outline"
            aria-expanded={!collapsed}
            onClick={() => setExpanded((v) => !v)}
            className="rounded-full ps-2.5 pe-2"
          >
            {collapsed ? (
              <>
                Show {hidden} more {hidden === 1 ? "line" : "lines"}
                <ChevronDownIcon />
              </>
            ) : (
              <>
                Show fewer lines
                <ChevronUpIcon />
              </>
            )}
          </Button>
        </div>
      ) : null}
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * CodeBlock
 * -----------------------------------------------------------------------------------------------*/

function CodeBlock({
  code,
  language,
  filename,
  streaming = false,
  actions,
  hideHeader = false,
  wrap: wrapProp,
  defaultWrap = false,
  onWrapChange,
  showLineNumbers,
  startLine,
  highlightLines,
  collapseAfter,
  className,
  ...props
}: CodeBlockProps) {
  const [wrap, setWrap] = useControllable(wrapProp, defaultWrap, onWrapChange)
  const label = languageLabel(language)

  return (
    <CodeBlockFrame
      data-language={language}
      data-streaming={streaming ? "" : undefined}
      aria-busy={streaming || undefined}
      className={className}
      {...props}
    >
      {hideHeader ? null : (
        <CodeBlockHeader>
          <figcaption
            className={cn(
              "min-w-0 flex-1 truncate",
              filename
                ? "chatcn-text-12 text-chatcn-fg-muted font-mono"
                : "chatcn-text-12 text-chatcn-fg-subtle",
            )}
          >
            {filename ?? label}
          </figcaption>
          <div className="flex shrink-0 items-center gap-0.5">
            {actions}
            <WrapToggle wrap={wrap} onChange={setWrap} />
            <CopyButton value={code.replace(/\n$/, "")} label="Copy code" />
          </div>
        </CodeBlockHeader>
      )}
      <CodeBlockBody
        code={code}
        language={language}
        streaming={streaming}
        wrap={wrap}
        label={filename ?? label}
        showLineNumbers={showLineNumbers}
        startLine={startLine}
        highlightLines={highlightLines}
        collapseAfter={collapseAfter}
      />
    </CodeBlockFrame>
  )
}

/* -------------------------------------------------------------------------------------------------
 * CodeBlockTabs: several files in one block
 * -----------------------------------------------------------------------------------------------*/

interface CodeBlockFile {
  /** File name, shown on the tab. Also the tab value. */
  name: string
  code: string
  language?: string
  streaming?: boolean
  highlightLines?: number[] | string
}

interface CodeBlockTabsProps
  extends Omit<CodeBlockOptions, "highlightLines">,
    Omit<React.ComponentProps<"figure">, "children" | "defaultValue"> {
  files: CodeBlockFile[]
  value?: string
  defaultValue?: string
  onValueChange?: (name: string) => void
  actions?: React.ReactNode
}

/** Infers a language from a file name: `route.ts` to `typescript`. */
function languageFromName(name: string) {
  const ext = name.split(".").pop()?.toLowerCase()
  return ext && ext !== name.toLowerCase() ? ext : undefined
}

function CodeBlockTabs({
  files,
  value: valueProp,
  defaultValue,
  onValueChange,
  actions,
  wrap: wrapProp,
  defaultWrap = false,
  onWrapChange,
  showLineNumbers,
  startLine,
  collapseAfter,
  className,
  ...props
}: CodeBlockTabsProps) {
  const [wrap, setWrap] = useControllable(wrapProp, defaultWrap, onWrapChange)
  const [value, setValue] = useControllable(
    valueProp,
    defaultValue ?? files[0]?.name ?? "",
    onValueChange,
  )
  const active = files.find((f) => f.name === value) ?? files[0]

  return (
    <CodeBlockFrame
      data-variant="tabs"
      aria-busy={active?.streaming || undefined}
      className={className}
      {...props}
    >
      <Tabs.Root
        activationMode="manual"
        value={value}
        onValueChange={(v) => setValue(String(v))}
        className="bg-inherit"
      >
        {/* 6 px around the tabs, so the active tab sits concentric in the 12 px frame. */}
        <CodeBlockHeader className="h-10 ps-1.5">
          <Tabs.List
            aria-label="Files"
            className="flex min-w-0 flex-1 [scrollbar-width:none] items-center gap-0.5 overflow-x-auto"
          >
            {files.map((file) => (
              <Tabs.Trigger
                key={file.name}
                value={file.name}
                className={cn(
                  "chatcn-focus chatcn-text-12 rounded-chatcn-sm text-chatcn-fg-subtle h-7 shrink-0 px-2.5 font-mono whitespace-nowrap",
                  "ease-chatcn-standard transition-colors duration-150",
                  "hover:text-chatcn-fg data-[state=active]:bg-chatcn-fill-active data-[state=active]:text-chatcn-fg",
                )}
              >
                {file.name}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
          <div className="flex shrink-0 items-center gap-0.5">
            {actions}
            <WrapToggle wrap={wrap} onChange={setWrap} />
            <CopyButton value={() => active?.code.replace(/\n$/, "") ?? ""} label="Copy code" />
          </div>
        </CodeBlockHeader>
        {files.map((file) => (
          <Tabs.Content key={file.name} value={file.name} className="bg-inherit outline-none">
            <CodeBlockBody
              code={file.code}
              language={file.language ?? languageFromName(file.name)}
              streaming={file.streaming}
              wrap={wrap}
              label={file.name}
              showLineNumbers={showLineNumbers}
              startLine={startLine}
              highlightLines={file.highlightLines}
              collapseAfter={collapseAfter}
            />
          </Tabs.Content>
        ))}
      </Tabs.Root>
    </CodeBlockFrame>
  )
}

export type { CodeBlockFile, CodeBlockOptions, CodeBlockProps, CodeBlockTabsProps }
export { CodeBlock, CodeBlockTabs, parseLineRanges }
