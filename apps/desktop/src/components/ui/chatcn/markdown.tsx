// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { type Components, Streamdown, useIsCodeFenceIncomplete } from "streamdown"
import { Button } from "@/components/ui/chatcn/button"
import { CodeBlock, type CodeBlockOptions } from "@/components/ui/chatcn/code-block"
import { CopyButton } from "@/components/ui/chatcn/copy-button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/chatcn/dialog"
import { Image } from "@/components/ui/chatcn/image"
import { InlineCode } from "@/components/ui/chatcn/inline-code"
import { MathFormula } from "@/components/ui/chatcn/math"
import { Mermaid } from "@/components/ui/chatcn/mermaid"
import { Table } from "@/components/ui/chatcn/table"
import { useChatcnAnnounce } from "@/components/ui/chatcn/hooks/use-chatcn-announce"
import { useChatcnCadence } from "@/components/ui/chatcn/hooks/use-chatcn-cadence"
import { CheckIcon, ExternalLinkIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Math: $$…$$, \[…\] and \(…\) become code nodes the renderer routes to <MathFormula>.
 *
 * Display math turns into a ```math fence, so it streams like any code block: its space is held
 * from the first line and it renders once closed. Inline math becomes inline code tagged with a
 * private-use character. Fenced code and inline code are left alone.
 * -----------------------------------------------------------------------------------------------*/

const MATH = ""
const MATH_PARTIAL = ""
const FENCE = /^ {0,3}(`{3,}|~{3,})/

function inlineMath(tex: string, partial = false) {
  const body = `${partial ? MATH_PARTIAL : MATH}${tex}`
  return body.includes("`") ? `\`\` ${body} \`\`` : `\`${body}\``
}

function convertInline(line: string, singleDollar: boolean, isLast: boolean) {
  // Split out inline code spans, which must stay literal.
  const parts = line.split(/(`+[^`]*?`+)/g)
  return parts
    .map((part, i) => {
      if (i % 2 === 1) return part
      let out = part
        .replace(/\$\$([^$\n]+?)\$\$/g, (_, tex: string) => inlineMath(tex))
        .replace(/\\\((.+?)\\\)/g, (_, tex: string) => inlineMath(tex))
      if (singleDollar) {
        out = out.replace(
          /(^|[^\\$\w])\$([^\s$](?:[^$\n]*?[^\s$])?)\$(?![\w$])/g,
          (_, lead: string, tex: string) => `${lead}${inlineMath(tex)}`,
        )
      }
      // An inline formula still being written: show it as math-in-progress, not raw dollars.
      if (isLast && i === parts.length - 1) {
        out = out
          .replace(/\$\$([^$\n]*)$/, (_, tex: string) => (tex ? inlineMath(tex, true) : ""))
          .replace(/\\\(([^\n]*)$/, (_, tex: string) => (tex ? inlineMath(tex, true) : ""))
      }
      return out
    })
    .join("")
}

/** Rewrites math delimiters into code nodes. Exported for tests. */
function prepareMath(markdown: string, { singleDollar = false } = {}) {
  if (!/\$\$|\\\[|\\\(|\$/.test(markdown)) return markdown
  const lines = markdown.split("\n")
  const out: string[] = []
  let fence: string | null = null
  let display: "$$" | "\\]" | null = null
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] as string
    const isLast = i === lines.length - 1
    const trimmed = line.trim()
    if (display) {
      if (trimmed === display || trimmed.endsWith(display)) {
        const rest = trimmed.slice(0, trimmed.length - display.length)
        if (rest) out.push(rest)
        out.push("```")
        display = null
      } else out.push(line)
      continue
    }
    const fenceMatch = FENCE.exec(line)
    if (fence) {
      if (
        fenceMatch &&
        fenceMatch[1]?.[0] === fence[0] &&
        (fenceMatch[1]?.length ?? 0) >= fence.length
      )
        fence = null
      out.push(line)
      continue
    }
    if (fenceMatch) {
      fence = fenceMatch[1] ?? "```"
      out.push(line)
      continue
    }
    // One-line display math: $$ x $$ or \[ x \] alone on a line.
    const oneLine = /^\$\$(.+)\$\$$/.exec(trimmed) ?? /^\\\[(.+)\\\]$/.exec(trimmed)
    if (oneLine) {
      out.push("```math", (oneLine[1] ?? "").trim(), "```")
      continue
    }
    if (trimmed.startsWith("$$") || trimmed.startsWith("\\[")) {
      const open = trimmed.startsWith("$$") ? "$$" : "\\["
      const rest = trimmed.slice(2).trim()
      if (open === "$$" && rest && !isLast && /\$\$/.test(rest)) {
        out.push(convertInline(line, singleDollar, isLast))
        continue
      }
      display = open === "$$" ? "$$" : "\\]"
      out.push("```math")
      if (rest) out.push(rest)
      continue
    }
    out.push(convertInline(line, singleDollar, isLast))
  }
  return out.join("\n")
}

/* -------------------------------------------------------------------------------------------------
 * Context: settings the element renderers read. Kept out of `components` so the components map
 * stays one stable object and finished blocks never re-render.
 * -----------------------------------------------------------------------------------------------*/

interface LinkSafetyOptions {
  /** Return true for URLs that may open without asking, for example your own domains. */
  onLinkCheck?: (url: string) => boolean | Promise<boolean>
}

interface MarkdownSettings {
  linkSafety: false | LinkSafetyOptions
  codeBlock: Pick<CodeBlockOptions, "showLineNumbers" | "collapseAfter" | "defaultWrap">
}

const SettingsContext = React.createContext<MarkdownSettings>({ linkSafety: false, codeBlock: {} })

/* -------------------------------------------------------------------------------------------------
 * Element renderers
 * -----------------------------------------------------------------------------------------------*/

type NodeProps<T extends keyof React.JSX.IntrinsicElements> = React.JSX.IntrinsicElements[T] & {
  node?: { tagName?: string; properties?: Record<string, unknown> }
}

/** Drops the hast `node` Streamdown passes, so it never reaches the DOM. */
function withoutNode<P extends { node?: unknown }>(props: P): Omit<P, "node"> {
  const rest: Partial<P> = { ...props }
  delete rest.node
  return rest as Omit<P, "node">
}

const heading = "text-chatcn-fg font-semibold text-balance scroll-mt-20"

function textOf(children: React.ReactNode): string | null {
  if (typeof children === "string") return children
  if (Array.isArray(children) && children.every((c) => typeof c === "string"))
    return children.join("")
  if (React.isValidElement<{ children?: React.ReactNode }>(children))
    return textOf(children.props.children)
  return null
}

/** Reads `title="file.ts"`, `{1,4-6}` and `showLineNumbers` from a fence's meta string. */
function parseMeta(meta: unknown) {
  if (typeof meta !== "string") return {}
  const title = /(?:title|filename|file)=(?:"([^"]+)"|'([^']+)'|(\S+))/.exec(meta)
  const lines = /\{([\d,\s-]+)\}/.exec(meta)
  return {
    filename: title ? (title[1] ?? title[2] ?? title[3]) : undefined,
    highlightLines: lines?.[1]?.replace(/\s/g, ""),
    showLineNumbers: /\bshowLineNumbers\b/.test(meta) ? true : undefined,
  }
}

function MarkdownCode({ node, className, children, ...props }: NodeProps<"code">) {
  const incomplete = useIsCodeFenceIncomplete()
  const settings = React.useContext(SettingsContext)
  const block = "data-block" in props

  if (!block) {
    const text = textOf(children)
    if (text?.startsWith(MATH)) return <MathFormula tex={text.slice(1)} />
    if (text?.startsWith(MATH_PARTIAL)) return <MathFormula tex={text.slice(1)} streaming />
    return <InlineCode>{children}</InlineCode>
  }

  const language = /language-([^\s]+)/.exec(className ?? "")?.[1] ?? ""
  const code = textOf(children) ?? ""
  if (language === "math")
    return <MathFormula tex={code} display streaming={incomplete} className="my-4" />
  if (language === "mermaid")
    return <Mermaid chart={code} streaming={incomplete} className="my-5" />
  const meta = parseMeta(node?.properties?.metastring)
  return (
    <CodeBlock
      code={code}
      language={language || undefined}
      filename={meta.filename}
      highlightLines={meta.highlightLines}
      streaming={incomplete}
      showLineNumbers={meta.showLineNumbers ?? settings.codeBlock.showLineNumbers}
      collapseAfter={settings.codeBlock.collapseAfter}
      defaultWrap={settings.codeBlock.defaultWrap}
      className="my-5"
    />
  )
}

function hostOf(url: string) {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}

function MarkdownLink(allProps: NodeProps<"a">) {
  const { href, children, className, ...props } = withoutNode(allProps)
  const { linkSafety } = React.useContext(SettingsContext)
  const [confirming, setConfirming] = React.useState(false)
  const openRef = React.useRef<HTMLButtonElement>(null)
  const linkClass = cn(
    "text-chatcn-fg decoration-chatcn-border-strong underline decoration-1 underline-offset-[0.22em]",
    "ease-chatcn-standard hover:decoration-chatcn-accent transition-[text-decoration-color] duration-150",
    "chatcn-focus rounded-[2px] [overflow-wrap:anywhere]",
    className,
  )

  // Remend marks a link whose URL has not arrived yet. It reads as a link but goes nowhere.
  if (!href || href.startsWith("streamdown:")) {
    return (
      <span
        data-slot="markdown-link"
        data-pending=""
        className={cn(linkClass, "decoration-dotted")}
      >
        {children}
      </span>
    )
  }

  const external = /^https?:\/\//i.test(href)
  const open = () => window.open(href, "_blank", "noopener,noreferrer")

  return (
    <>
      <a
        data-slot="markdown-link"
        href={href}
        className={linkClass}
        {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
        onClick={async (event) => {
          if (!external || !linkSafety || event.metaKey || event.ctrlKey || event.shiftKey) return
          event.preventDefault()
          if (linkSafety.onLinkCheck && (await linkSafety.onLinkCheck(href))) open()
          else setConfirming(true)
        }}
        {...props}
      >
        {children}
        {external ? <span className="sr-only"> (opens in a new tab)</span> : null}
      </a>
      {linkSafety && external ? (
        <Dialog open={confirming} onOpenChange={setConfirming}>
          <DialogContent size="sm" initialFocus={openRef}>
            <DialogHeader>
              <DialogTitle>Open external link?</DialogTitle>
              <DialogDescription>
                This goes to <span className="text-chatcn-fg font-medium">{hostOf(href)}</span>.
                Check the address before you continue.
              </DialogDescription>
            </DialogHeader>
            <p className="rounded-chatcn-md bg-chatcn-fill chatcn-text-13 text-chatcn-fg-muted mx-5 mt-4 px-3 py-2.5 font-mono break-all">
              {href}
            </p>
            <DialogFooter className="pt-4">
              <CopyButton value={href} label="Copy link" className="me-auto" />
              <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                Cancel
              </Button>
              <Button
                ref={openRef}
                size="sm"
                variant="solid"
                onClick={() => {
                  open()
                  setConfirming(false)
                }}
              >
                Open link
                <ExternalLinkIcon />
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
    </>
  )
}

function MarkdownImage({ src, alt, title }: NodeProps<"img">) {
  const url = typeof src === "string" ? src : ""
  // Remend's placeholder for an image whose URL is still streaming: hold the space, request nothing.
  if (!url || url.startsWith("streamdown:")) {
    return (
      <span
        data-slot="markdown-image"
        data-pending=""
        className="bg-chatcn-fill rounded-chatcn-lg my-5 block aspect-video w-full"
      />
    )
  }
  return (
    <Image
      src={url}
      alt={alt ?? ""}
      caption={title || undefined}
      aspectRatio={16 / 9}
      className="my-5"
    />
  )
}

/** Paragraphs that hold only an image or a block render their child alone (no <p> around a figure). */
function MarkdownParagraph(allProps: NodeProps<"p">) {
  const { children, className, ...props } = withoutNode(allProps)
  const items = React.Children.toArray(children).filter((c) => c !== "" && c !== "\n")
  const only = items.length === 1 ? items[0] : undefined
  if (React.isValidElement<{ node?: { tagName?: string } }>(only)) {
    const tag = only.props.node?.tagName
    if (tag === "img") return <>{children}</>
  }
  return (
    <p className={cn("my-4 text-pretty", className)} {...props}>
      {children}
    </p>
  )
}

/** GFM task list boxes: the native disabled checkbox renders washed out, so draw our own. */
function MarkdownInput(allProps: NodeProps<"input">) {
  const { className, type, checked, ...props } = withoutNode(allProps)
  if (type !== "checkbox") return <input type={type} className={className} {...props} />
  return (
    <span
      data-slot="markdown-task"
      className="relative me-2 inline-grid size-3.5 place-items-center align-[-0.125em]"
    >
      <input
        {...props}
        type="checkbox"
        checked={Boolean(checked)}
        // The item text follows the box; the box itself says its state.
        aria-label={checked ? "Done" : "To do"}
        disabled
        readOnly
        className={cn(
          "peer absolute inset-0 m-0 appearance-none rounded-[4px]",
          "shadow-[inset_0_0_0_1px_var(--chatcn-border-strong)]",
          "checked:bg-chatcn-accent checked:shadow-none forced-colors:appearance-auto",
          className,
        )}
      />
      <CheckIcon
        aria-hidden="true"
        strokeWidth={2}
        className="text-chatcn-accent-fg pointer-events-none relative size-3 opacity-0 peer-checked:opacity-100 forced-colors:hidden"
      />
    </span>
  )
}

const strip =
  <T extends keyof React.JSX.IntrinsicElements>(Tag: T, base?: string) =>
  (allProps: NodeProps<T>) => {
    const { className, ...props } = withoutNode(allProps) as NodeProps<T> & { className?: string }
    return React.createElement(Tag, { ...props, className: cn(base, className) })
  }

const components: Components = {
  h1: strip("h1", cn(heading, "chatcn-text-24 mt-8 mb-4")),
  h2: strip("h2", cn(heading, "chatcn-text-20 mt-8 mb-3")),
  h3: strip("h3", cn(heading, "chatcn-text-16 mt-7 mb-2")),
  h4: strip("h4", cn(heading, "chatcn-text-15 mt-6 mb-2")),
  h5: strip("h5", cn(heading, "chatcn-text-15 mt-6 mb-2")),
  h6: strip("h6", cn(heading, "chatcn-text-15 text-chatcn-fg-muted mt-6 mb-2")),
  p: MarkdownParagraph,
  a: MarkdownLink,
  strong: strip("strong", "text-chatcn-fg font-semibold"),
  em: strip("em", "italic"),
  del: strip("del", "text-chatcn-fg-subtle decoration-chatcn-fg-subtle"),
  ul: strip(
    "ul",
    "my-4 list-disc ps-6 marker:text-chatcn-fg-subtle [&_ol]:my-1.5 [&_ul]:my-1.5 [&_ul]:list-[circle]",
  ),
  ol: strip(
    "ol",
    "my-4 list-decimal ps-6 marker:text-chatcn-fg-subtle marker:tabular-nums [&_ol]:my-1.5 [&_ul]:my-1.5",
  ),
  li: strip(
    "li",
    "my-1.5 ps-1 [&>p]:my-2 has-[>[data-slot=markdown-task]]:list-none has-[>[data-slot=markdown-task]]:-ms-5",
  ),
  blockquote: strip(
    "blockquote",
    "border-chatcn-border-strong text-chatcn-fg-muted my-5 border-s ps-4 [&>p]:my-2",
  ),
  hr: strip("hr", "border-chatcn-border my-8 border-0 border-t"),
  code: MarkdownCode,
  img: MarkdownImage,
  input: MarkdownInput,
  table: ({ children }: NodeProps<"table">) => <Table className="my-5">{children}</Table>,
  thead: strip("thead"),
  tbody: strip("tbody"),
  tr: strip("tr"),
  th: strip("th"),
  td: strip("td"),
}

/* -------------------------------------------------------------------------------------------------
 * Caret: a CSS square after the last line of text while streaming. Code blocks draw their own.
 * Rendered once per page through React's style hoisting.
 * -----------------------------------------------------------------------------------------------*/

const root = "[data-slot=markdown][data-caret] > [data-slot=markdown-content]"
const caretCss = `
${root} > :is(p, h1, h2, h3, h4, h5, h6):last-child::after,
${root} > :is(ul, ol):last-child > li:last-child::after,
${root} > blockquote:last-child > p:last-child::after {
  content: "";
  display: inline-block;
  inline-size: 0.55em;
  block-size: 0.55em;
  margin-inline-start: 0.125rem;
  border-radius: 0.12em;
  background-color: var(--chatcn-accent);
  vertical-align: baseline;
  translate: 0 0.04em;
}
[data-slot=markdown][data-caret=waiting] > [data-slot=markdown-content] > :last-child::after,
[data-slot=markdown][data-caret=waiting] > [data-slot=markdown-content] > :last-child > :last-child::after {
  animation: chatcn-caret-blink 1.2s steps(1, end) infinite;
}
@media (prefers-reduced-motion: reduce) {
  [data-slot=markdown] [data-slot=markdown-content] *::after { animation: none; }
}
@media (forced-colors: active) {
  [data-slot=markdown] [data-slot=markdown-content] *::after { background-color: CanvasText; }
}
`

/* -------------------------------------------------------------------------------------------------
 * Markdown
 * -----------------------------------------------------------------------------------------------*/

interface MarkdownProps extends Omit<React.ComponentProps<"div">, "children"> {
  /** Markdown source. While streaming, pass the text so far. */
  children: string
  /** True while more text may arrive. Heals unfinished syntax and shows the caret. */
  streaming?: boolean
  /**
   * Smooth streaming: releases bursty chunks at a steady pace with bounded lag. Off by default;
   * `true` uses the default pace.
   */
  cadence?: boolean | { charsPerSecond?: number; maxLagMs?: number }
  /** Square caret after the last line while streaming. On by default. */
  caret?: boolean
  /** Announce the finished answer to screen readers once. On by default. */
  announce?: boolean
  /**
   * Confirm before opening external links, showing the full address. Off by default. Pass
   * `{ onLinkCheck }` to let trusted URLs through without asking.
   */
  linkSafety?: boolean | LinkSafetyOptions
  /** Render $$…$$, \[…\] and \(…\) with KaTeX. On by default; `singleDollar` also allows $…$. */
  math?: boolean | { singleDollar?: boolean }
  /** Defaults for fenced code blocks. */
  codeBlock?: MarkdownSettings["codeBlock"]
  /** `comfortable` sets prose at 16/26 instead of 15/24. */
  density?: "default" | "comfortable"
  /** Override element renderers. Pass a stable object (a module constant or memoized). */
  components?: Components
}

/**
 * Streaming Markdown for assistant answers. Built on Streamdown: unfinished syntax heals as it
 * streams, finished blocks never re-render, code block chrome appears with the opening fence, and
 * math and diagrams load only when an answer uses them.
 */
function Markdown({
  children,
  streaming = false,
  cadence = false,
  caret = true,
  announce = true,
  linkSafety = false,
  math = true,
  codeBlock,
  density = "default",
  components: overrides,
  className,
  ...props
}: MarkdownProps) {
  const ref = React.useRef<HTMLDivElement>(null)
  const pace = typeof cadence === "object" ? cadence : {}
  const shown = useChatcnCadence(children, { streaming, enabled: cadence !== false, ...pace })
  const live = shown.revealing
  const message = useChatcnAnnounce(
    live,
    () => ref.current?.querySelector("[data-slot=markdown-content]")?.textContent,
    {
      enabled: announce,
    },
  )

  const singleDollar = typeof math === "object" && math.singleDollar === true
  const source = React.useMemo(
    () => (math ? prepareMath(shown.text, { singleDollar }) : shown.text),
    [shown.text, math, singleDollar],
  )

  // Caret blinks only while waiting on the next token.
  const [waiting, setWaiting] = React.useState(false)
  React.useEffect(() => {
    setWaiting(false)
    if (!live) return
    const id = window.setTimeout(() => setWaiting(true), 600)
    return () => window.clearTimeout(id)
  }, [shown.text, live])

  const settings = React.useMemo<MarkdownSettings>(
    () => ({
      linkSafety: linkSafety === true ? {} : linkSafety === false ? false : linkSafety,
      codeBlock: codeBlock ?? {},
    }),
    [linkSafety, codeBlock],
  )
  const merged = React.useMemo(
    () => (overrides ? { ...components, ...overrides } : components),
    [overrides],
  )
  const empty = source.trim().length === 0

  return (
    <SettingsContext.Provider value={settings}>
      <div
        ref={ref}
        data-slot="markdown"
        data-streaming={live ? "" : undefined}
        data-caret={live && caret ? (waiting ? "waiting" : "streaming") : undefined}
        aria-busy={live || undefined}
        className={cn(
          "text-chatcn-fg min-w-0 break-words",
          density === "comfortable" ? "chatcn-text-16 leading-[1.625rem]" : "chatcn-text-15",
          className,
        )}
        {...props}
      >
        <style href="chatcn-markdown-caret" precedence="default">
          {caretCss}
        </style>
        {empty && live && caret ? (
          // The same mark, on the same baseline, as the caret that follows the first token, so it
          // never hops when text lands. It blinks while the first token is slow to arrive.
          <span
            aria-hidden="true"
            data-slot="markdown-caret"
            className={cn(
              "bg-chatcn-accent inline-block size-[0.55em] translate-y-[0.04em] rounded-[0.12em] align-baseline",
              "forced-colors:bg-[CanvasText]",
              waiting &&
                "animate-[chatcn-caret-blink_1.2s_steps(1,end)_infinite] motion-reduce:animate-none",
            )}
          />
        ) : (
          <Streamdown
            data-slot="markdown-content"
            mode="streaming"
            isAnimating={live}
            controls={false}
            linkSafety={{ enabled: false }}
            remend={{ katex: false }}
            components={merged}
            className="space-y-0 [&>*:first-child]:mt-0 [&>*:last-child]:mb-0"
          >
            {source}
          </Streamdown>
        )}
        <span aria-live="polite" aria-atomic="true" className="sr-only">
          {message}
        </span>
      </div>
    </SettingsContext.Provider>
  )
}

export type { LinkSafetyOptions, MarkdownProps }
export { Markdown, prepareMath }
