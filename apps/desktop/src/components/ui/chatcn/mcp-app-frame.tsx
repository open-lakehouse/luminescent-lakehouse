// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"
import { Button } from "@/components/ui/chatcn/button"
import { Spinner } from "@/components/ui/chatcn/spinner"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import {
  chatcnHostStyles,
  type McpCallToolParams,
  type McpCallToolResult,
  type McpToolResult,
  useChatcnHostTheme,
  useChatcnMcpApp,
} from "@/components/ui/chatcn/hooks/use-chatcn-genui-mcp-app"
import {
  CircleAlertIcon,
  Maximize2Icon,
  Minimize2Icon,
  PictureInPictureIcon,
  PlugIcon,
  RotateCcwIcon,
  XIcon,
} from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types
 * -----------------------------------------------------------------------------------------------*/

type McpAppDisplayMode = "inline" | "fullscreen" | "pip"

interface McpAppResource {
  /** `ui://server/name`. Shown in the header as the app's address. */
  uri: string
  /** The resource text, MIME `text/html;profile=mcp-app`. */
  html: string
  /** `_meta.ui` from the resource: CSP domains, permissions and the border preference. */
  meta?: {
    csp?: {
      connectDomains?: string[]
      resourceDomains?: string[]
      frameDomains?: string[]
      baseUriDomains?: string[]
    }
    permissions?: {
      camera?: object
      microphone?: object
      geolocation?: object
      clipboardWrite?: object
    }
    prefersBorder?: boolean
  }
}

interface McpAppFrameProps
  extends Omit<React.ComponentProps<"div">, "title" | "onError" | "resource"> {
  // luminescent: the MCP Apps sandbox proxy the View runs in (see the hook).
  /** The sandbox proxy page, on an origin other than the host's (`mcpAppSandboxUrl()`). */
  sandboxUrl: string
  /** The resource to host. Leave it undefined while it is being read from the server. */
  resource?: McpAppResource
  /** App or server name for the header. Defaults to the name the View reports. */
  appName?: string
  /** Server or app mark, user supplied. Defaults to a plug. */
  icon?: React.ReactNode
  /** Tool that produced this app, shown next to the name. */
  toolName?: string
  toolInputPartial?: Record<string, unknown>
  toolInput?: Record<string, unknown>
  toolResult?: McpToolResult
  toolCancelled?: boolean | string
  /** Reading the resource failed. Shown with Retry when `onRetry` is set. */
  error?: React.ReactNode
  onRetry?: () => void
  displayMode?: McpAppDisplayMode
  defaultDisplayMode?: McpAppDisplayMode
  onDisplayModeChange?: (mode: McpAppDisplayMode) => void
  /** Modes the host offers. PiP falls back to full screen on small touch screens. */
  displayModes?: McpAppDisplayMode[]
  /** Height before the app reports its own, in px. */
  initialHeight?: number
  /** Inline height limit in px; taller apps scroll inside. */
  maxHeight?: number
  onMessage?: (text: string, params: { role: "user"; content: unknown[] }) => void | Promise<void>
  onOpenLink?: (url: string) => void | Promise<void>
  onCallTool?: (params: McpCallToolParams) => Promise<McpCallToolResult>
  onUpdateModelContext?: (params: {
    content?: unknown[]
    structuredContent?: Record<string, unknown>
  }) => void
  /** The View asked to close. */
  onClose?: () => void
  /** How long to wait for the View's `ui/initialize` before showing an error. Default 10000. */
  timeoutMs?: number
}

/* -------------------------------------------------------------------------------------------------
 * Motion
 *
 * The iframe never moves in the DOM (moving it would reload the app); display modes restyle the
 * same element. Entering full screen or picture in picture plays a short opacity and scale enter,
 * 220 ms for full screen like a dialog, 180 ms for the floating window. Keyboard-triggered changes
 * skip it.
 * -----------------------------------------------------------------------------------------------*/

const frameCss = `
@keyframes chatcn-mcp-enter { from { opacity: 0; scale: var(--chatcn-enter-scale) } to { opacity: 1; scale: 1 } }
[data-slot=mcp-app-frame][data-mcp-animate=fullscreen] { animation: chatcn-mcp-enter 220ms var(--chatcn-ease-out) both }
[data-slot=mcp-app-frame][data-mcp-animate=pip] { animation: chatcn-mcp-enter 180ms var(--chatcn-ease-out) both; transform-origin: bottom right }
[dir=rtl] [data-slot=mcp-app-frame][data-mcp-animate=pip] { transform-origin: bottom left }
@media (prefers-reduced-motion: reduce) {
  [data-slot=mcp-app-frame][data-mcp-animate] { animation-duration: 150ms }
}
`

function textOf(content: unknown[]) {
  return content
    .map((c) =>
      typeof c === "object" && c && "text" in c ? String((c as { text: unknown }).text) : "",
    )
    .filter(Boolean)
    .join("\n")
}

function useMediaQuery(query: string) {
  const [match, setMatch] = React.useState(false)
  React.useEffect(() => {
    const mq = window.matchMedia(query)
    setMatch(mq.matches)
    const on = () => setMatch(mq.matches)
    mq.addEventListener("change", on)
    return () => mq.removeEventListener("change", on)
  }, [query])
  return match
}

/* -------------------------------------------------------------------------------------------------
 * McpAppFrame
 * -----------------------------------------------------------------------------------------------*/

/**
 * Hosts an MCP App (`ui://` resource, `text/html;profile=mcp-app`) in a sandboxed iframe with no
 * same-origin access and a CSP limited to the domains the resource declares. Our tokens flow in
 * as host style variables, the frame follows the app's content height, and the app can go full
 * screen or float as picture in picture without reloading.
 */
function McpAppFrame({
  sandboxUrl,
  resource,
  appName,
  icon,
  toolName,
  toolInputPartial,
  toolInput,
  toolResult,
  toolCancelled,
  error: errorProp,
  onRetry,
  displayMode: modeProp,
  defaultDisplayMode = "inline",
  onDisplayModeChange,
  displayModes = ["inline", "fullscreen", "pip"],
  initialHeight = 240,
  maxHeight = 640,
  onMessage,
  onOpenLink,
  onCallTool,
  onUpdateModelContext,
  onClose,
  timeoutMs,
  className,
  ...props
}: McpAppFrameProps) {
  const [root, setRoot] = React.useState<HTMLDivElement | null>(null)
  const theme = useChatcnHostTheme(root)
  const narrowTouch = useMediaQuery("(pointer: coarse) and (max-width: 640px)")
  const [modeState, setModeState] = React.useState<McpAppDisplayMode>(defaultDisplayMode)
  const mode = modeProp ?? modeState
  const [animate, setAnimate] = React.useState<McpAppDisplayMode | null>(null)
  const returnFocus = React.useRef<HTMLElement | null>(null)

  const setMode = React.useCallback(
    (next: McpAppDisplayMode, viaKeyboard = false) => {
      // On phones a floating window would cover the composer; hosts coerce PiP to full screen.
      const granted = next === "pip" && narrowTouch ? "fullscreen" : next
      if (!displayModes.includes(granted) && granted !== "inline") return mode
      if (granted !== "inline" && mode === "inline") {
        returnFocus.current = document.activeElement as HTMLElement | null
      }
      setAnimate(viaKeyboard || granted === "inline" ? null : granted)
      if (modeProp === undefined) setModeState(granted)
      onDisplayModeChange?.(granted)
      return granted
    },
    [narrowTouch, displayModes, mode, modeProp, onDisplayModeChange],
  )

  // Styles are recomputed when the theme flips so the View repaints in the new palette.
  const styles = React.useMemo(() => (root ? chatcnHostStyles(root) : undefined), [root, theme])

  const app = useChatcnMcpApp({
    sandboxUrl,
    html: resource?.html,
    csp: resource?.meta?.csp,
    permissions: resource?.meta?.permissions,
    theme,
    styles,
    displayMode: mode,
    availableDisplayModes: displayModes,
    maxHeight: mode === "inline" ? maxHeight : undefined,
    toolInputPartial,
    toolInput,
    toolResult,
    toolCancelled,
    onMessage: onMessage
      ? (params) =>
          onMessage(
            textOf(params.content as unknown[]),
            params as { role: "user"; content: unknown[] },
          )
      : undefined,
    onOpenLink,
    onCallTool,
    onUpdateModelContext: onUpdateModelContext as never,
    onRequestDisplayMode: (requested) => setMode(requested),
    onRequestTeardown: onClose,
    timeoutMs,
  })

  // Escape leaves full screen and picture in picture, then focus goes back where it was.
  React.useEffect(() => {
    if (mode === "inline") {
      returnFocus.current?.focus?.()
      returnFocus.current = null
      return
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) setMode("inline", true)
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [mode, setMode])

  // Full screen keeps keyboard focus inside the frame.
  const trap = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (mode !== "fullscreen" || event.key !== "Tab") return
    const items = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>("button:not([disabled]), iframe"),
    )
    const first = items[0]
    const last = items[items.length - 1]
    if (!first || !last) return
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }

  const name = appName ?? app.appInfo?.name ?? "App"
  const loading = !errorProp && (!resource || app.status === "loading" || app.status === "idle")
  const failed = Boolean(errorProp) || app.status === "error"
  const contentHeight = app.size.height
  const inlineHeight = Math.min(maxHeight, Math.max(96, contentHeight ?? initialHeight))
  const bordered = resource?.meta?.prefersBorder !== false

  // A delayed label: quick loads show only the frame, slow ones say what is happening.
  const [showLabel, setShowLabel] = React.useState(false)
  React.useEffect(() => {
    if (!loading) {
      setShowLabel(false)
      return
    }
    const t = window.setTimeout(() => setShowLabel(true), 300)
    return () => window.clearTimeout(t)
  }, [loading])

  const modeButton = (target: McpAppDisplayMode, label: string, glyph: React.ReactNode) =>
    displayModes.includes(target) ? (
      <Tooltip content={label}>
        <Button
          size="icon-sm"
          aria-label={label}
          onClick={(event) => setMode(target, event.detail === 0)}
          className="pointer-coarse:size-11"
        >
          {glyph}
        </Button>
      </Tooltip>
    ) : null

  const header = (
    <div className="flex h-10 shrink-0 items-center gap-2 ps-3.5 pe-1.5">
      <span className="text-chatcn-fg-subtle grid size-4 shrink-0 place-items-center [&_svg]:size-4 [&>img]:size-4 [&>img]:rounded-[4px]">
        {icon ?? <PlugIcon />}
      </span>
      <span className="chatcn-text-13 text-chatcn-fg shrink-0 font-medium">{name}</span>
      {toolName ? (
        <span className="chatcn-text-12 text-chatcn-fg-subtle min-w-0 truncate font-mono">
          {toolName}
        </span>
      ) : resource?.uri ? (
        <span className="chatcn-text-12 text-chatcn-fg-subtle min-w-0 truncate font-mono">
          {resource.uri}
        </span>
      ) : null}
      <span className="ms-auto flex shrink-0 items-center gap-0.5">
        {mode === "inline" ? (
          failed || loading ? null : (
            <>
              {modeButton("pip", "Picture in picture", <PictureInPictureIcon />)}
              {modeButton("fullscreen", "Full screen", <Maximize2Icon />)}
            </>
          )
        ) : mode === "pip" ? (
          <>
            {modeButton("fullscreen", "Full screen", <Maximize2Icon />)}
            <Tooltip content="Back to the conversation">
              <Button
                size="icon-sm"
                aria-label="Back to the conversation"
                onClick={(event) => setMode("inline", event.detail === 0)}
                className="pointer-coarse:size-11"
              >
                <XIcon />
              </Button>
            </Tooltip>
          </>
        ) : (
          <Button
            size="sm"
            onClick={(event) => setMode("inline", event.detail === 0)}
            className="pointer-coarse:h-11"
          >
            <Minimize2Icon />
            Exit full screen
          </Button>
        )}
      </span>
    </div>
  )

  return (
    <div
      ref={setRoot}
      data-slot="mcp-app-root"
      className={cn("min-w-0", className)}
      // Inline, the root takes the frame's height; out of flow, it keeps it so the answer
      // below does not jump while the app floats or fills the screen.
      style={mode === "inline" ? undefined : { height: inlineHeight + 41 }}
      {...props}
    >
      <style href="chatcn-mcp-app-frame" precedence="default">
        {frameCss}
      </style>

      {mode !== "inline" ? (
        <div className="bg-chatcn-inset rounded-chatcn-lg flex h-full flex-col items-center justify-center gap-2 p-4 text-center">
          <p className="chatcn-text-13 text-chatcn-fg-muted">
            {name} is open {mode === "fullscreen" ? "in full screen" : "in picture in picture"}
          </p>
          <Button
            size="sm"
            variant="outline"
            onClick={(event) => setMode("inline", event.detail === 0)}
          >
            Show it here
          </Button>
        </div>
      ) : null}

      {/* biome-ignore lint/a11y/useAriaPropsSupportedByRole: the role switches to dialog in full screen, where aria-modal applies */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: the key handler only traps Tab inside the full screen dialog */}
      <div
        data-slot="mcp-app-frame"
        data-status={app.status}
        data-display-mode={mode}
        data-mcp-animate={animate && animate === mode ? animate : undefined}
        role={mode === "fullscreen" ? "dialog" : "region"}
        aria-modal={mode === "fullscreen" || undefined}
        aria-label={name}
        aria-busy={loading || undefined}
        onKeyDown={trap}
        className={cn(
          "chatcn-text-14 text-chatcn-fg flex min-w-0 flex-col overflow-hidden",
          mode === "inline" &&
            cn("rounded-chatcn-lg bg-chatcn-card", bordered ? "shadow-chatcn-sm" : ""),
          mode === "fullscreen" && "bg-chatcn-bg fixed inset-0 z-50",
          mode === "pip" &&
            "rounded-chatcn-xl bg-chatcn-card shadow-chatcn-xl dark:shadow-chatcn-border fixed end-4 bottom-4 z-40 w-[min(26rem,calc(100vw-2rem))]",
        )}
      >
        {header}
        <div
          className={cn(
            "border-chatcn-border relative min-h-0 border-t",
            mode === "fullscreen" && "flex-1",
            mode === "inline" &&
              "ease-chatcn-out transition-[height] duration-200 motion-reduce:transition-none",
          )}
          style={
            mode === "inline"
              ? { height: failed ? undefined : inlineHeight }
              : mode === "pip"
                ? { height: Math.min(360, inlineHeight) }
                : undefined
          }
        >
          {failed ? (
            <div role="alert" className="flex flex-col items-start gap-2 p-4">
              <p className="chatcn-text-13 text-chatcn-fg flex items-center gap-2 font-medium">
                <CircleAlertIcon aria-hidden="true" className="text-chatcn-red size-4 shrink-0" />
                Could not load {name}
              </p>
              <p className="chatcn-text-13 text-chatcn-fg-subtle ps-6 text-pretty">
                {errorProp ?? app.error?.message ?? "The app stopped responding."}
              </p>
              {onRetry || app.status === "error" ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="ms-6"
                  onClick={() => {
                    if (errorProp) onRetry?.()
                    else app.reload()
                  }}
                >
                  <RotateCcwIcon />
                  Reload app
                </Button>
              ) : null}
            </div>
          ) : (
            <>
              {resource ? (
                <iframe
                  key={app.frameKey}
                  ref={app.iframeRef}
                  title={`${name}, interactive app`}
                  // luminescent: the sandbox proxy, not a `srcdoc`. It is another origin,
                  // so same-origin only gives it its own storage-less origin; the View's
                  // frame inside it gets scripts and forms only. Links go through
                  // `ui/open-link`, so no popups.
                  src={app.src}
                  sandbox="allow-scripts allow-same-origin allow-forms"
                  allow={app.allow || undefined}
                  referrerPolicy="no-referrer"
                  className={cn(
                    "block size-full border-0 bg-transparent [color-scheme:normal]",
                    "ease-chatcn-out transition-opacity duration-200",
                    loading ? "opacity-0" : "opacity-100",
                  )}
                />
              ) : null}
              {loading ? (
                <div aria-hidden={!showLabel} className="absolute inset-0 flex flex-col gap-3 p-4">
                  <span className="bg-chatcn-fill rounded-chatcn-xs h-3 w-1/3" />
                  <span className="bg-chatcn-fill rounded-chatcn-xs h-3 w-1/2" />
                  <span className="bg-chatcn-fill rounded-chatcn-md w-full flex-1" />
                  <span
                    className={cn(
                      "chatcn-text-12 text-chatcn-fg-subtle flex items-center gap-2",
                      "ease-chatcn-out transition-opacity duration-150",
                      showLabel ? "opacity-100" : "opacity-0",
                    )}
                  >
                    <Spinner />
                    Starting {name}
                  </span>
                </div>
              ) : null}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

export type { McpAppDisplayMode, McpAppFrameProps, McpAppResource }
export { McpAppFrame }
