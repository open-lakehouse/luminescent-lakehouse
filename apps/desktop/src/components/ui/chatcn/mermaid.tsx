// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"
import { Button } from "@/components/ui/chatcn/button"
import { CopyButton } from "@/components/ui/chatcn/copy-button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/chatcn/dialog"
import { Spinner } from "@/components/ui/chatcn/spinner"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { CircleAlertIcon, CodeIcon, Maximize2Icon, MinusIcon, PlusIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Mermaid, loaded on first use, themed from the chatcn tokens
 * -----------------------------------------------------------------------------------------------*/

type MermaidApi = typeof import("mermaid")["default"]
let mermaidTask: Promise<MermaidApi> | undefined
const loadMermaid = () => {
  mermaidTask ??= import("mermaid").then((m) => m.default)
  return mermaidTask
}

const TOKENS = {
  bg: "--chatcn-bg",
  bgSubtle: "--chatcn-bg-subtle",
  surface: "--chatcn-surface",
  fill: "--chatcn-fill",
  fillActive: "--chatcn-fill-active",
  fg: "--chatcn-fg",
  fgMuted: "--chatcn-fg-muted",
  fgSubtle: "--chatcn-fg-subtle",
  border: "--chatcn-border",
  borderStrong: "--chatcn-border-strong",
  red: "--chatcn-red",
} as const

type Palette = Record<keyof typeof TOKENS, string> & { font: string; dark: boolean }

/**
 * Mermaid mixes colors with a library that only reads hex, so each token is resolved where the
 * diagram lives (scoped themes work) and converted through a 1 px canvas.
 */
function readPalette(scope: HTMLElement): Palette {
  const probe = document.createElement("span")
  probe.style.display = "none"
  scope.appendChild(probe)
  const canvas = document.createElement("canvas")
  canvas.width = canvas.height = 1
  const ctx = canvas.getContext("2d", { willReadFrequently: true })
  const hex = (variable: string) => {
    probe.style.color = `var(${variable})`
    const value = getComputedStyle(probe).color
    if (!ctx) return value
    ctx.clearRect(0, 0, 1, 1)
    ctx.fillStyle = value
    ctx.fillRect(0, 0, 1, 1)
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data
    return `#${[r, g, b].map((n) => n.toString(16).padStart(2, "0")).join("")}`
  }
  const out = Object.fromEntries(
    Object.entries(TOKENS).map(([key, variable]) => [key, hex(variable)]),
  ) as Record<keyof typeof TOKENS, string>
  const font = getComputedStyle(scope).fontFamily
  probe.remove()
  const lum = Number.parseInt(out.bg.slice(1, 3), 16)
  return { ...out, font, dark: lum < 128 }
}

function mermaidConfig(p: Palette) {
  return {
    startOnLoad: false,
    // A failed chart reports through our error state, never Mermaid's own error graphic.
    suppressErrorRendering: true,
    securityLevel: "strict" as const,
    theme: "base" as const,
    darkMode: p.dark,
    fontFamily: p.font,
    // Nodes hug their labels (Mermaid 12 widens every node to 120 px), so a chart stays compact
    // enough to read inline without panning.
    flowchart: { minNodeWidth: 0 },
    themeVariables: {
      darkMode: p.dark,
      fontFamily: p.font,
      fontSize: "14px",
      background: p.bgSubtle,
      primaryColor: p.surface,
      primaryTextColor: p.fg,
      primaryBorderColor: p.borderStrong,
      secondaryColor: p.fill,
      secondaryTextColor: p.fg,
      secondaryBorderColor: p.borderStrong,
      tertiaryColor: p.bgSubtle,
      tertiaryTextColor: p.fg,
      tertiaryBorderColor: p.border,
      mainBkg: p.surface,
      nodeBorder: p.borderStrong,
      nodeTextColor: p.fg,
      lineColor: p.fgSubtle,
      textColor: p.fg,
      titleColor: p.fg,
      clusterBkg: p.bg,
      clusterBorder: p.border,
      edgeLabelBackground: p.bgSubtle,
      noteBkgColor: p.fill,
      noteTextColor: p.fg,
      noteBorderColor: p.border,
      actorBkg: p.surface,
      actorBorder: p.borderStrong,
      actorTextColor: p.fg,
      actorLineColor: p.border,
      signalColor: p.fgMuted,
      signalTextColor: p.fg,
      labelBoxBkgColor: p.surface,
      labelBoxBorderColor: p.borderStrong,
      labelTextColor: p.fg,
      loopTextColor: p.fgMuted,
      activationBkgColor: p.fill,
      activationBorderColor: p.borderStrong,
      sequenceNumberColor: p.bg,
      errorBkgColor: p.fill,
      errorTextColor: p.red,
      git0: p.fg,
      pie1: p.fg,
      pie2: p.fgMuted,
      pie3: p.fgSubtle,
      pie4: p.borderStrong,
      pieStrokeColor: p.bg,
      pieOuterStrokeColor: p.border,
    },
    themeCSS: [
      ".node rect, .node polygon, .cluster rect { rx: 6px; ry: 6px; }",
      ".node rect, .node polygon, .node circle { stroke-width: 1px; }",
      ".node, .node *, .cluster rect, .actor, .note { filter: none !important; }",
      ".flowchart-link, .messageLine0, .messageLine1 { stroke-width: 1.25px; }",
      ".edgeLabel, .edgeLabel rect { font-size: 12px; }",
      "text { font-weight: 400; }",
    ].join("\n"),
  }
}

let renderCount = 0

type DiagramResult = { svg: string } | { error: string }

/** Renders a chart once it is complete, and again when the theme changes. */
function useMermaidSvg(
  chart: string,
  enabled: boolean,
  scope: React.RefObject<HTMLElement | null>,
) {
  const [result, setResult] = React.useState<DiagramResult>()
  const [themeKey, setThemeKey] = React.useState("")

  // Theme flips (class on <html>, or the OS scheme) re-render with the new palette.
  React.useEffect(() => {
    const read = () => {
      if (scope.current) setThemeKey(JSON.stringify(readPalette(scope.current)))
    }
    read()
    const observer = new MutationObserver(read)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "style", "data-theme"],
    })
    const media = window.matchMedia("(prefers-color-scheme: dark)")
    media.addEventListener("change", read)
    return () => {
      observer.disconnect()
      media.removeEventListener("change", read)
    }
  }, [scope])

  React.useEffect(() => {
    if (!enabled || !themeKey) return
    let alive = true
    const palette = JSON.parse(themeKey) as Palette
    loadMermaid()
      .then(async (mermaid) => {
        mermaid.initialize(mermaidConfig(palette))
        renderCount += 1
        const id = `chatcn-mermaid-${renderCount}`
        try {
          const { svg } = await mermaid.render(id, chart)
          if (alive) setResult({ svg })
        } finally {
          // Mermaid measures in a node on <body> and can leave it behind when a chart fails.
          for (const leftover of [id, `d${id}`, `i${id}`])
            document.getElementById(leftover)?.remove()
        }
      })
      .catch((error: unknown) => {
        if (!alive) return
        const message = error instanceof Error ? error.message : String(error)
        setResult({ error: message.split("\n")[0]?.replace(/:\s*$/, "").trim() ?? "" })
      })
    return () => {
      alive = false
    }
  }, [chart, enabled, themeKey])

  return result
}

/* -------------------------------------------------------------------------------------------------
 * Pan and zoom
 * -----------------------------------------------------------------------------------------------*/

interface View {
  x: number
  y: number
  scale: number
  /** Pointer-driven changes from buttons animate; drags, wheel and keys never do. */
  animate: boolean
}

const MIN = 0.25
const MAX = 4
const clamp = (n: number) => Math.min(MAX, Math.max(MIN, n))

/** Gap kept between a panned diagram and the frame edge, the canvas padding. */
const EDGE = 16

/**
 * How far the diagram can move from where it is, so it never leaves the frame: a side only pans
 * while content is hidden past it, and a diagram that fits does not move at all.
 */
function panRoom(frame: HTMLElement | null) {
  const svg = frame?.querySelector("svg")
  if (!frame || !svg) return null
  const a = frame.getBoundingClientRect()
  const b = svg.getBoundingClientRect()
  return {
    left: Math.max(0, b.right - (a.right - EDGE)),
    right: Math.max(0, a.left + EDGE - b.left),
    up: Math.max(0, b.bottom - (a.bottom - EDGE)),
    down: Math.max(0, a.top + EDGE - b.top),
  }
}

function usePanZoom() {
  const [view, setView] = React.useState<View>({ x: 0, y: 0, scale: 1, animate: false })
  const viewport = React.useRef<HTMLDivElement>(null)

  const zoomAt = React.useCallback((factor: number, cx?: number, cy?: number, animate = false) => {
    setView((v) => {
      const scale = clamp(v.scale * factor)
      const el = viewport.current
      const px = cx ?? (el ? el.clientWidth / 2 : 0)
      const py = cy ?? (el ? el.clientHeight / 2 : 0)
      const k = scale / v.scale
      return { scale, x: px - (px - v.x) * k, y: py - (py - v.y) * k, animate }
    })
  }, [])

  const reset = React.useCallback(
    (animate = false) => setView({ x: 0, y: 0, scale: 1, animate }),
    [],
  )

  // Moves by (dx, dy), held inside the room measured when the gesture or key press started.
  const pan = React.useCallback((dx: number, dy: number, room = panRoom(viewport.current)) => {
    if (!room) return
    const x = Math.min(room.right, Math.max(-room.left, dx))
    const y = Math.min(room.down, Math.max(-room.up, dy))
    if (x === 0 && y === 0) return
    setView((v) => ({ ...v, x: v.x + x, y: v.y + y, animate: false }))
  }, [])

  // Ctrl or Cmd plus wheel (and trackpad pinch) zooms at the cursor. A plain wheel scrolls the page.
  React.useEffect(() => {
    const el = viewport.current
    if (!el) return
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      const rect = el.getBoundingClientRect()
      zoomAt(Math.exp(-event.deltaY * 0.01), event.clientX - rect.left, event.clientY - rect.top)
    }
    el.addEventListener("wheel", onWheel, { passive: false })
    return () => el.removeEventListener("wheel", onWheel)
  }, [zoomAt])

  const drag = React.useRef<{
    id: number
    x: number
    y: number
    room: NonNullable<ReturnType<typeof panRoom>>
  } | null>(null)
  const end = (event: React.PointerEvent<HTMLDivElement>) => {
    if (drag.current?.id === event.pointerId) drag.current = null
  }
  const handlers = {
    onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
      if (event.button !== 0) return
      const room = panRoom(viewport.current)
      if (!room) return
      drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, room }
      event.currentTarget.setPointerCapture(event.pointerId)
    },
    onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
      const d = drag.current
      if (!d || d.id !== event.pointerId) return
      const dx = Math.min(d.room.right, Math.max(-d.room.left, event.clientX - d.x))
      const dy = Math.min(d.room.down, Math.max(-d.room.up, event.clientY - d.y))
      d.x += dx
      d.y += dy
      // The room shrinks on the side it moved toward and grows on the other.
      d.room = {
        left: d.room.left + dx,
        right: d.room.right - dx,
        up: d.room.up + dy,
        down: d.room.down - dy,
      }
      if (dx !== 0 || dy !== 0) setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy, animate: false }))
    },
    onPointerUp: end,
    // A vertical swipe on touch hands the gesture to the page scroll.
    onPointerCancel: end,
    onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
      const step = 48
      const keys: Record<string, () => void> = {
        "+": () => zoomAt(1.25),
        "=": () => zoomAt(1.25),
        "-": () => zoomAt(0.8),
        "0": () => reset(),
        ArrowLeft: () => pan(step, 0),
        ArrowRight: () => pan(-step, 0),
        ArrowUp: () => pan(0, step),
        ArrowDown: () => pan(0, -step),
      }
      const action = keys[event.key]
      if (!action || event.metaKey || event.ctrlKey || event.altKey) return
      event.preventDefault()
      action()
    },
  }

  return { view, viewport, handlers, zoomAt, reset }
}

function ZoomControls({
  scale,
  onZoom,
  onReset,
}: {
  scale: number
  onZoom: (factor: number) => void
  onReset: () => void
}) {
  return (
    <div className="flex items-center gap-0.5">
      <Button
        size="icon-sm"
        aria-label="Zoom out"
        onClick={() => onZoom(0.8)}
        disabled={scale <= MIN}
      >
        <MinusIcon />
      </Button>
      <Tooltip content="Reset view">
        <button
          type="button"
          onClick={onReset}
          aria-label={`Zoom ${Math.round(scale * 100)} percent, reset view`}
          className={cn(
            "chatcn-focus chatcn-text-12 rounded-chatcn-sm text-chatcn-fg-subtle h-7 w-11 tabular-nums",
            "hover:bg-chatcn-fill-hover hover:text-chatcn-fg transition-colors duration-150",
          )}
        >
          {Math.round(scale * 100)}%
        </button>
      </Tooltip>
      <Button
        size="icon-sm"
        aria-label="Zoom in"
        onClick={() => onZoom(1.25)}
        disabled={scale >= MAX}
      >
        <PlusIcon />
      </Button>
    </div>
  )
}

/** The diagram's own width, from its viewBox. */
function naturalWidth(svg: string) {
  const match = /viewBox="[-\d.]+[ ,]+[-\d.]+[ ,]+([\d.]+)/.exec(svg)
  return match ? Number(match[1]) : undefined
}

type Hidden = { start: boolean; end: boolean; top: boolean; bottom: boolean }
const NONE: Hidden = { start: false, end: false, top: false, bottom: false }

/** A soft edge on each side that hides part of the diagram, so the cut reads as "there is more". */
function edgeMask(hidden: Hidden, rtl: boolean) {
  const left = rtl ? hidden.end : hidden.start
  const right = rtl ? hidden.start : hidden.end
  if (!left && !right && !hidden.top && !hidden.bottom) return undefined
  const fade = (from: boolean, to: boolean, side: string) =>
    `linear-gradient(to ${side}, ${from ? "transparent" : "black"}, black 32px, black calc(100% - 32px), ${to ? "transparent" : "black"})`
  const image = `${fade(left, right, "right")}, ${fade(hidden.top, hidden.bottom, "bottom")}`
  return {
    maskImage: image,
    WebkitMaskImage: image,
    maskComposite: "intersect",
    WebkitMaskComposite: "source-in",
  } as const
}

function DiagramCanvas({
  svg,
  label,
  className,
  panZoom,
  minScale,
}: {
  svg: string
  label: string
  className?: string
  panZoom: ReturnType<typeof usePanZoom>
  /** Never shrink below this share of the natural size: wide diagrams overflow and pan instead. */
  minScale?: number
}) {
  const { view, viewport, handlers } = panZoom
  const natural = minScale ? naturalWidth(svg) : undefined
  const [hidden, setHidden] = React.useState<Hidden>(NONE)
  const [rtl, setRtl] = React.useState(false)
  // The hint shows until the first pan: after that the reader knows.
  const [panned, setPanned] = React.useState(false)
  const origin = React.useRef({ x: view.x, y: view.y })
  if (!panned && (view.x !== origin.current.x || view.y !== origin.current.y)) setPanned(true)

  // Which sides hide content: read on resize, after each pan, and when a zoom settles.
  const read = React.useCallback(() => {
    const frame = viewport.current
    const content = frame?.querySelector("svg")
    if (!frame || !content) return
    const a = frame.getBoundingClientRect()
    const b = content.getBoundingClientRect()
    const left = b.left < a.left - 0.5
    const right = b.right > a.right + 0.5
    const isRtl = getComputedStyle(frame).direction === "rtl"
    const next = {
      start: isRtl ? right : left,
      end: isRtl ? left : right,
      top: b.top < a.top - 0.5,
      bottom: b.bottom > a.bottom + 0.5,
    }
    setRtl(isRtl)
    setHidden((h) =>
      h.start === next.start && h.end === next.end && h.top === next.top && h.bottom === next.bottom
        ? h
        : next,
    )
  }, [viewport])

  React.useLayoutEffect(() => {
    const frame = viewport.current
    if (!frame || !svg) return
    read()
    const observer = new ResizeObserver(read)
    observer.observe(frame)
    frame.addEventListener("transitionend", read)
    return () => {
      observer.disconnect()
      frame.removeEventListener("transitionend", read)
    }
  }, [svg, viewport, read])

  // A pan or zoom moves the diagram: read again.
  React.useLayoutEffect(read, [view.x, view.y, view.scale, read])

  const more = hidden.start || hidden.end || hidden.top || hidden.bottom
  return (
    <div
      ref={viewport}
      role="img"
      aria-label={label}
      // biome-ignore lint/a11y/noNoninteractiveTabindex: arrow keys pan and plus or minus zoom
      tabIndex={0}
      {...handlers}
      data-pannable={more || undefined}
      className={cn(
        "relative overflow-hidden outline-none select-none",
        "focus-visible:shadow-[inset_0_0_0_2px_var(--chatcn-accent)]",
        // Only a diagram with something to reveal grabs the pointer. On touch, a vertical swipe
        // still scrolls the page unless the reader has zoomed in.
        more && "cursor-grab active:cursor-grabbing",
        view.scale > 1 ? "touch-none" : "touch-pan-y",
        className,
      )}
    >
      <div className="flex min-h-full" style={edgeMask(hidden, rtl)}>
        <div
          className={cn(
            // Safe centering: a diagram wider than the frame starts at its start edge, not off screen.
            "flex w-full origin-top-left items-center justify-center-safe p-4",
            "[&_svg]:h-auto [&_svg]:max-h-[min(30rem,66vh)] [&_svg]:max-w-full",
            natural && "[&_svg]:min-w-(--diagram-min) [&_svg]:shrink-0",
            view.animate &&
              "ease-chatcn-out transition-transform duration-200 motion-reduce:transition-none",
          )}
          style={
            {
              transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`,
              ...(natural && minScale ? { "--diagram-min": `${natural * minScale}px` } : {}),
            } as React.CSSProperties
          }
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      </div>
      <span
        aria-hidden="true"
        data-slot="mermaid-pan-hint"
        className={cn(
          "chatcn-text-12 text-chatcn-fg-subtle bg-chatcn-bg-subtle rounded-chatcn-xs pointer-events-none absolute end-2 bottom-2 px-1.5 leading-5",
          "ease-chatcn-out transition-opacity duration-150",
          more && !panned ? "opacity-100" : "opacity-0",
        )}
      >
        <span className="pointer-coarse:hidden">Drag to pan</span>
        <span className="hidden pointer-coarse:inline">Swipe to pan</span>
      </span>
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * Mermaid
 * -----------------------------------------------------------------------------------------------*/

interface MermaidProps extends Omit<React.ComponentProps<"figure">, "children"> {
  /** Mermaid source. */
  chart: string
  /** Header label and accessible name. Defaults to "Diagram". */
  title?: string
  /** True while the source streams in: shows the source and renders once it is complete. */
  streaming?: boolean
  /** Start on the source view. */
  defaultView?: "diagram" | "source"
}

/**
 * A Mermaid diagram in the chatcn palette. Mermaid loads on first use. Streams as source, renders
 * when complete, re-renders on theme change, and offers source, zoom, pan and fullscreen.
 */
function Mermaid({
  chart,
  title = "Diagram",
  streaming = false,
  defaultView = "diagram",
  className,
  ...props
}: MermaidProps) {
  const scope = React.useRef<HTMLElement>(null)
  const source = chart.trim()
  const result = useMermaidSvg(source, !streaming && source.length > 0, scope)
  const svg = result && "svg" in result ? result.svg : undefined
  const error = result && "error" in result ? result.error : undefined
  const [view, setView] = React.useState(defaultView)
  const [fullscreen, setFullscreen] = React.useState(false)
  const inline = usePanZoom()
  const large = usePanZoom()

  const rendering = !streaming && !svg && !error
  const showSource = view === "source" || !svg

  return (
    <figure
      ref={scope}
      data-slot="mermaid"
      data-mermaid-state={
        streaming ? "streaming" : svg ? "rendered" : error ? "error" : "rendering"
      }
      aria-busy={streaming || rendering || undefined}
      className={cn(
        "group/mermaid rounded-chatcn-lg bg-chatcn-bg-subtle shadow-chatcn-border overflow-hidden",
        "forced-colors:outline-1 forced-colors:outline-solid",
        className,
      )}
      {...props}
    >
      <div className="flex h-9 items-center gap-2 ps-3.5 pe-1.5">
        <figcaption className="chatcn-text-12 text-chatcn-fg-subtle flex min-w-0 flex-1 items-center gap-2 truncate">
          {title}
          {rendering || streaming ? <Spinner className="size-3" /> : null}
        </figcaption>
        <div className="flex shrink-0 items-center gap-0.5">
          {svg && view === "diagram" ? (
            <ZoomControls
              scale={inline.view.scale}
              onZoom={(f) => inline.zoomAt(f, undefined, undefined, true)}
              onReset={() => inline.reset(true)}
            />
          ) : null}
          {svg ? (
            <Tooltip content={view === "source" ? "Show diagram" : "Show source"}>
              <Button
                size="icon-sm"
                aria-label="Show source"
                aria-pressed={view === "source"}
                onClick={() => setView(view === "source" ? "diagram" : "source")}
                className={cn(view === "source" && "bg-chatcn-fill-active text-chatcn-fg")}
              >
                <CodeIcon />
              </Button>
            </Tooltip>
          ) : null}
          {svg ? (
            <Tooltip content="Fullscreen">
              <Button
                size="icon-sm"
                aria-label="Open fullscreen"
                onClick={() => setFullscreen(true)}
              >
                <Maximize2Icon />
              </Button>
            </Tooltip>
          ) : null}
          <CopyButton value={source} label="Copy Mermaid source" />
        </div>
      </div>

      {error ? (
        <p className="chatcn-text-13 text-chatcn-fg-muted flex items-start gap-2 px-3.5 pb-2">
          <CircleAlertIcon aria-hidden="true" className="text-chatcn-red mt-0.5 size-4 shrink-0" />
          <span>
            Could not render this diagram. Check the syntax below.{" "}
            <span className="text-chatcn-fg-subtle">{error}</span>
          </span>
        </p>
      ) : null}

      {showSource ? (
        <pre
          dir="ltr"
          className={cn(
            "chatcn-text-13 chatcn-scroll overflow-x-auto px-3.5 pt-0.5 pb-3.5 font-mono whitespace-pre",
            // Holds the diagram's height while it streams, so the swap to SVG does not jump. A
            // failed diagram stays as source, so it has nothing to hold space for.
            !error && "min-h-60",
            svg ? "text-chatcn-fg" : "text-chatcn-fg-muted",
          )}
        >
          <code>{source}</code>
        </pre>
      ) : (
        <DiagramCanvas
          svg={svg ?? ""}
          label={title}
          panZoom={inline}
          // Labels stay readable inline; drag, zoom or open full screen to see the whole picture.
          minScale={0.75}
          className="bg-chatcn-bg-subtle min-h-40"
        />
      )}

      <Dialog
        open={fullscreen}
        onOpenChange={(open) => {
          setFullscreen(open)
          if (open) large.reset()
        }}
      >
        <DialogContent size="full" className="bg-chatcn-bg">
          <div className="flex h-12 shrink-0 items-center gap-3 ps-5 pe-14">
            <DialogTitle className="chatcn-text-14 min-w-0 flex-1 truncate font-medium">
              {title}
            </DialogTitle>
            <ZoomControls
              scale={large.view.scale}
              onZoom={(f) => large.zoomAt(f, undefined, undefined, true)}
              onReset={() => large.reset(true)}
            />
          </div>
          {svg ? (
            <DiagramCanvas
              svg={svg}
              label={title}
              panZoom={large}
              className="bg-chatcn-bg-subtle min-h-0 flex-1 [&_svg]:max-h-none [&>div]:min-h-full"
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </figure>
  )
}

export type { MermaidProps }
export { Mermaid }
