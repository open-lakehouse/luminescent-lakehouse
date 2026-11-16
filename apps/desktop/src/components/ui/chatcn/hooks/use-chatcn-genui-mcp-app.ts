// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import {
  AppBridge,
  buildAllowAttribute,
  type McpUiDisplayMode,
  type McpUiHostContext,
  type McpUiMessageRequest,
  type McpUiResourceCsp,
  type McpUiResourcePermissions,
  type McpUiStyles,
  type McpUiTheme,
  type McpUiToolResultNotification,
  type McpUiUpdateModelContextRequest,
  PostMessageTransport,
} from "@modelcontextprotocol/ext-apps/app-bridge"
import * as React from "react"

/* -------------------------------------------------------------------------------------------------
 * Types
 * -----------------------------------------------------------------------------------------------*/

type CallToolHandler = NonNullable<AppBridge["oncalltool"]>
type McpCallToolParams = Parameters<CallToolHandler>[0]
type McpCallToolResult = Awaited<ReturnType<CallToolHandler>>
type McpToolResult = McpUiToolResultNotification["params"]
type McpAppStatus = "idle" | "loading" | "ready" | "error"

interface UseChatcnMcpAppOptions {
  // luminescent: the View runs inside the MCP Apps sandbox proxy (a second origin:
  // the desktop's `mcp-app://` scheme), not in a `srcdoc` frame of ours, which would
  // inherit the shell's CSP. The proxy builds the View's frame and CSP.
  /** The sandbox proxy page (`mcpAppSandboxUrl()`). */
  sandboxUrl: string
  /** The `ui://` resource HTML (`text/html;profile=mcp-app`). Nothing loads until it is set. */
  html?: string
  /** CSP domains from the resource's `_meta.ui.csp`. Everything else is blocked. */
  csp?: McpUiResourceCsp
  /** Device permissions from `_meta.ui.permissions`, mapped to the iframe `allow` attribute. */
  permissions?: McpUiResourcePermissions
  hostInfo?: { name: string; version: string }
  theme: McpUiTheme
  styles?: McpUiStyles
  displayMode: McpUiDisplayMode
  availableDisplayModes?: McpUiDisplayMode[]
  /** Upper bound for the app's height in inline mode, sent as `containerDimensions.maxHeight`. */
  maxHeight?: number
  /** Arguments while the model is still writing them. */
  toolInputPartial?: Record<string, unknown>
  /** Final tool arguments. */
  toolInput?: Record<string, unknown>
  toolResult?: McpToolResult
  /** Set when the tool call was cancelled. A string is passed on as the reason. */
  toolCancelled?: boolean | string
  /** The app asked to post a message to the chat as the user. */
  onMessage?: (params: McpUiMessageRequest["params"]) => void | Promise<void>
  /** The app asked to open a link. Defaults to a new tab for http and https only. */
  onOpenLink?: (url: string) => void | Promise<void>
  /** The app called a server tool through the host. Without a handler the call fails politely. */
  onCallTool?: (params: McpCallToolParams) => Promise<McpCallToolResult>
  /** The app asked for another display mode. Return the mode you grant. */
  onRequestDisplayMode?: (mode: McpUiDisplayMode) => McpUiDisplayMode
  /** The app updated what the model should know about its state. */
  onUpdateModelContext?: (params: McpUiUpdateModelContextRequest["params"]) => void
  /** The app asked to be closed. */
  onRequestTeardown?: () => void
  /** Milliseconds to wait for `ui/initialize` before reporting an error. Default 10000. */
  timeoutMs?: number
}

interface UseChatcnMcpAppResult {
  /** Attach to the iframe. The bridge connects before the document loads, so no message is lost. */
  iframeRef: (node: HTMLIFrameElement | null) => void
  // luminescent: the proxy's URL replaces `srcDoc`.
  /** The iframe `src`: the sandbox proxy. Undefined until the bridge listens. */
  src: string | undefined
  /** For the iframe `allow` attribute. */
  allow: string
  status: McpAppStatus
  error: Error | null
  /** Content size reported by the app. */
  size: { width?: number; height?: number }
  appInfo: { name: string; version: string } | undefined
  /** Use as the iframe `key`: `reload` remounts the frame so the document really restarts. */
  frameKey: number
  /** Restart the app from scratch. */
  reload: () => void
}

/* -------------------------------------------------------------------------------------------------
 * Theme bridge: chatcn tokens to MCP Apps host style variables
 * -----------------------------------------------------------------------------------------------*/

/** Our token for every host variable a View can read. `px:` values are literal. */
const STYLE_MAP: Record<keyof McpUiStyles, string> = {
  "--color-background-primary": "--chatcn-card",
  "--color-background-secondary": "--chatcn-bg-subtle",
  "--color-background-tertiary": "--chatcn-fill",
  "--color-background-inverse": "--chatcn-fg",
  "--color-background-ghost": "--chatcn-alpha-100",
  "--color-background-info": "mix:--chatcn-blue",
  "--color-background-danger": "mix:--chatcn-red",
  "--color-background-success": "mix:--chatcn-green",
  "--color-background-warning": "mix:--chatcn-amber",
  "--color-background-disabled": "--chatcn-fill",
  "--color-text-primary": "--chatcn-fg",
  "--color-text-secondary": "--chatcn-fg-muted",
  "--color-text-tertiary": "--chatcn-fg-subtle",
  "--color-text-inverse": "--chatcn-bg",
  "--color-text-ghost": "--chatcn-fg-faint",
  "--color-text-info": "--chatcn-blue",
  "--color-text-danger": "--chatcn-red",
  "--color-text-success": "--chatcn-green",
  "--color-text-warning": "--chatcn-amber",
  "--color-text-disabled": "--chatcn-fg-faint",
  "--color-border-primary": "--chatcn-border",
  "--color-border-secondary": "--chatcn-border-hover",
  "--color-border-tertiary": "--chatcn-alpha-300",
  "--color-border-inverse": "--chatcn-fg",
  "--color-border-ghost": "--chatcn-alpha-200",
  "--color-border-info": "--chatcn-blue",
  "--color-border-danger": "--chatcn-red",
  "--color-border-success": "--chatcn-green",
  "--color-border-warning": "--chatcn-amber",
  "--color-border-disabled": "--chatcn-border",
  "--color-ring-primary": "--chatcn-accent",
  "--color-ring-secondary": "--chatcn-border-strong",
  "--color-ring-inverse": "--chatcn-bg",
  "--color-ring-info": "--chatcn-blue",
  "--color-ring-danger": "--chatcn-red",
  "--color-ring-success": "--chatcn-green",
  "--color-ring-warning": "--chatcn-amber",
  "--font-sans": "font:sans",
  "--font-mono": "font:mono",
  "--font-weight-normal": "px:400",
  "--font-weight-medium": "px:500",
  "--font-weight-semibold": "px:600",
  // chatcn never goes above 600, so bold maps to semibold.
  "--font-weight-bold": "px:600",
  "--font-text-xs-size": "px:12px",
  "--font-text-sm-size": "px:13px",
  "--font-text-md-size": "px:14px",
  "--font-text-lg-size": "px:16px",
  "--font-heading-xs-size": "px:14px",
  "--font-heading-sm-size": "px:16px",
  "--font-heading-md-size": "px:18px",
  "--font-heading-lg-size": "px:20px",
  "--font-heading-xl-size": "px:24px",
  "--font-heading-2xl-size": "px:32px",
  "--font-heading-3xl-size": "px:40px",
  "--font-text-xs-line-height": "px:16px",
  "--font-text-sm-line-height": "px:18px",
  "--font-text-md-line-height": "px:20px",
  "--font-text-lg-line-height": "px:24px",
  "--font-heading-xs-line-height": "px:20px",
  "--font-heading-sm-line-height": "px:22px",
  "--font-heading-md-line-height": "px:24px",
  "--font-heading-lg-line-height": "px:28px",
  "--font-heading-xl-line-height": "px:32px",
  "--font-heading-2xl-line-height": "px:40px",
  "--font-heading-3xl-line-height": "px:48px",
  "--border-radius-xs": "px:4px",
  "--border-radius-sm": "px:6px",
  "--border-radius-md": "px:8px",
  "--border-radius-lg": "px:12px",
  "--border-radius-xl": "px:16px",
  "--border-radius-full": "px:9999px",
  "--border-width-regular": "px:1px",
  "--shadow-hairline": "--chatcn-shadow-border",
  "--shadow-sm": "--chatcn-shadow-sm",
  "--shadow-md": "--chatcn-shadow-md",
  "--shadow-lg": "--chatcn-shadow-lg",
}

const SANS = "Geist, ui-sans-serif, system-ui, sans-serif"
const MONO = "'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace"

/**
 * Resolves chatcn tokens on `element` (light or dark, whatever applies there) into the host style
 * variables an MCP App reads. Values are computed, so the View gets real colors, not var() names
 * it cannot resolve inside its sandbox.
 */
function chatcnHostStyles(element?: Element | null): McpUiStyles {
  const target = element ?? (typeof document === "undefined" ? null : document.documentElement)
  const computed = target ? getComputedStyle(target) : null
  const read = (name: string) => computed?.getPropertyValue(name).trim() ?? ""
  const out = {} as McpUiStyles
  for (const [key, source] of Object.entries(STYLE_MAP) as [keyof McpUiStyles, string][]) {
    if (source.startsWith("px:")) out[key] = source.slice(3)
    else if (source === "font:sans") out[key] = computed?.fontFamily || SANS
    else if (source === "font:mono") out[key] = MONO
    else if (source.startsWith("mix:")) {
      const color = read(source.slice(4))
      out[key] = color ? `color-mix(in oklab, ${color} 12%, transparent)` : undefined
    } else out[key] = read(source) || undefined
  }
  return out
}

/** Light or dark for the element, from a `.dark` ancestor. Updates when the class changes. */
function useChatcnHostTheme(element: Element | null): McpUiTheme {
  const [theme, setTheme] = React.useState<McpUiTheme>("light")
  React.useEffect(() => {
    if (!element) return
    const read = () => setTheme(element.closest(".dark") ? "dark" : "light")
    read()
    const observer = new MutationObserver(read)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] })
    return () => observer.disconnect()
  }, [element])
  return theme
}

/* -------------------------------------------------------------------------------------------------
 * Sandbox document
 * -----------------------------------------------------------------------------------------------*/

function list(domains?: string[]) {
  return (domains ?? []).filter((d) => /^https:\/\/[\w.-]+(:\d+)?(\/.*)?$/.test(d)).join(" ")
}

/**
 * Prepends a Content-Security-Policy meta tag. Inline script and style are allowed (Views are
 * single documents); network, frames and base URIs are limited to the declared domains.
 */
// luminescent: unused by the hook here; the sandbox proxy builds the View's CSP the
// same way (src-tauri/assets/mcp-app/sandbox.js).
function withCsp(html: string, csp?: McpUiResourceCsp) {
  const resources = list(csp?.resourceDomains)
  const policy = [
    "default-src 'none'",
    `script-src 'unsafe-inline' ${resources}`,
    `style-src 'unsafe-inline' ${resources}`,
    `img-src data: blob: ${resources}`,
    `font-src data: ${resources}`,
    `media-src data: blob: ${resources}`,
    `connect-src ${list(csp?.connectDomains) || "'none'"}`,
    `frame-src ${list(csp?.frameDomains) || "'none'"}`,
    `base-uri ${list(csp?.baseUriDomains) || "'none'"}`,
    "form-action 'none'",
  ]
    .map((d) => d.trim())
    .join("; ")
  const meta = `<meta http-equiv="Content-Security-Policy" content="${policy}">`
  return /<head[^>]*>/i.test(html)
    ? html.replace(/<head[^>]*>/i, (m) => `${m}${meta}`)
    : `${meta}${html}`
}

/* -------------------------------------------------------------------------------------------------
 * Hook
 * -----------------------------------------------------------------------------------------------*/

/**
 * Hosts one MCP App View in a sandboxed iframe through the official `AppBridge`. Handles the
 * initialize handshake, theme and display mode in the host context, content size, tool input and
 * result passthrough, links, messages and teardown. Pair it with `<McpAppFrame>` or your own UI.
 */
function useChatcnMcpApp(options: UseChatcnMcpAppOptions): UseChatcnMcpAppResult {
  const {
    sandboxUrl,
    html,
    csp,
    permissions,
    hostInfo = { name: "chatcn", version: "0.4.0" },
    theme,
    styles,
    displayMode,
    availableDisplayModes = ["inline", "fullscreen", "pip"],
    maxHeight,
    toolInputPartial,
    toolInput,
    toolResult,
    toolCancelled,
    timeoutMs = 10_000,
  } = options

  // Latest callbacks without reconnecting the bridge on every render.
  const callbacks = React.useRef(options)
  callbacks.current = options

  const [iframe, setIframe] = React.useState<HTMLIFrameElement | null>(null)
  const [bridge, setBridge] = React.useState<AppBridge | null>(null)
  const [status, setStatus] = React.useState<McpAppStatus>("idle")
  const [error, setError] = React.useState<Error | null>(null)
  const [size, setSize] = React.useState<{ width?: number; height?: number }>({})
  const [appInfo, setAppInfo] = React.useState<{ name: string; version: string }>()
  const [generation, setGeneration] = React.useState(0)

  const hostContext = React.useMemo<McpUiHostContext>(
    () => ({
      theme,
      styles: styles ? { variables: styles } : undefined,
      displayMode,
      availableDisplayModes,
      containerDimensions: maxHeight ? { maxHeight } : {},
      locale: typeof navigator === "undefined" ? "en-US" : navigator.language,
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      platform: "web",
      deviceCapabilities:
        typeof window === "undefined"
          ? undefined
          : {
              touch: window.matchMedia("(pointer: coarse)").matches,
              hover: window.matchMedia("(hover: hover)").matches,
            },
    }),
    [theme, styles, displayMode, availableDisplayModes, maxHeight],
  )
  const contextRef = React.useRef(hostContext)
  contextRef.current = hostContext

  // Connect before the document loads: the View sends ui/initialize as soon as its script runs.
  React.useEffect(() => {
    const target = iframe?.contentWindow
    if (!iframe || !target || !html) return
    let disposed = false
    setStatus("loading")
    setError(null)
    setSize({})

    const next = new AppBridge(
      null,
      hostInfo,
      {
        openLinks: {},
        serverTools: {},
        logging: {},
        updateModelContext: { text: {}, structuredContent: {} },
        message: { text: {} },
        sandbox: { csp, permissions },
      },
      { hostContext: contextRef.current },
    )

    next.addEventListener("initialized", () => {
      if (disposed) return
      setAppInfo(next.getAppVersion())
      setStatus("ready")
    })
    next.addEventListener("sizechange", ({ width, height }) => {
      if (!disposed) setSize({ width, height })
    })
    next.addEventListener("requestteardown", () => callbacks.current.onRequestTeardown?.())
    next.onmessage = async (params) => {
      try {
        await callbacks.current.onMessage?.(params)
        return {}
      } catch {
        return { isError: true }
      }
    }
    next.onopenlink = async ({ url }) => {
      let safe = false
      try {
        safe = ["http:", "https:"].includes(new URL(url).protocol)
      } catch {
        safe = false
      }
      if (!safe) return { isError: true }
      if (callbacks.current.onOpenLink) await callbacks.current.onOpenLink(url)
      else window.open(url, "_blank", "noopener,noreferrer")
      return {}
    }
    next.oncalltool = async (params) => {
      const handler = callbacks.current.onCallTool
      if (handler) return handler(params)
      return {
        content: [{ type: "text", text: `This host cannot run ${params.name}.` }],
        isError: true,
      }
    }
    next.onrequestdisplaymode = async ({ mode }) => {
      const granted =
        callbacks.current.onRequestDisplayMode?.(mode) ?? contextRef.current.displayMode ?? "inline"
      return { mode: granted }
    }
    next.onupdatemodelcontext = async (params) => {
      callbacks.current.onUpdateModelContext?.(params)
      return {}
    }

    // luminescent: the proxy is up; hand it the View. It answers only its parent
    // (an allowlisted origin) and puts the HTML in its inner frame under a CSP built
    // from `csp`. Relayed messages come from the proxy's window, so the transport
    // below still talks to `target`.
    next.onsandboxready = () => {
      next.sendSandboxResourceReady({ html, csp, permissions }).catch((err: unknown) => {
        if (disposed) return
        setError(err instanceof Error ? err : new Error(String(err)))
        setStatus("error")
      })
    }

    const transport = new PostMessageTransport(target, target)
    const timer = window.setTimeout(() => {
      if (disposed || next.getAppVersion()) return
      setError(
        new Error(
          "The app did not start. It never sent ui/initialize, so it may not be an MCP App View.",
        ),
      )
      setStatus("error")
    }, timeoutMs)

    next
      .connect(transport)
      .then(() => {
        if (!disposed) setBridge(next)
      })
      .catch((err: unknown) => {
        if (disposed) return
        setError(err instanceof Error ? err : new Error(String(err)))
        setStatus("error")
      })

    return () => {
      disposed = true
      window.clearTimeout(timer)
      setBridge(null)
      // Give the View a chance to save state, then drop the channel.
      next
        .teardownResource({}, { timeout: 500 })
        .catch(() => {})
        .finally(() => {
          transport.close().catch(() => {})
        })
    }
    // hostInfo, csp and permissions are read once per load; changing html or reloading restarts.
  }, [iframe, html, generation, timeoutMs])

  // Host context changes (theme, display mode, size limits) flow to the running View.
  React.useEffect(() => {
    if (bridge && status === "ready") bridge.setHostContext(hostContext)
  }, [bridge, status, hostContext])

  // Tool data passthrough, in protocol order: partial input, input, then result or cancellation.
  React.useEffect(() => {
    if (bridge && status === "ready" && toolInputPartial && !toolInput) {
      bridge.sendToolInputPartial({ arguments: toolInputPartial }).catch(() => {})
    }
  }, [bridge, status, toolInputPartial, toolInput])
  React.useEffect(() => {
    if (bridge && status === "ready" && toolInput) {
      bridge.sendToolInput({ arguments: toolInput }).catch(() => {})
    }
  }, [bridge, status, toolInput])
  React.useEffect(() => {
    if (bridge && status === "ready" && toolInput && toolResult) {
      bridge.sendToolResult(toolResult).catch(() => {})
    }
  }, [bridge, status, toolInput, toolResult])
  React.useEffect(() => {
    if (bridge && status === "ready" && toolCancelled) {
      bridge
        .sendToolCancelled(typeof toolCancelled === "string" ? { reason: toolCancelled } : {})
        .catch(() => {})
    }
  }, [bridge, status, toolCancelled])

  const src = html && status !== "idle" ? sandboxUrl : undefined

  return {
    iframeRef: setIframe,
    src,
    allow: buildAllowAttribute(permissions),
    status,
    error,
    size,
    appInfo,
    frameKey: generation,
    reload: () => {
      setStatus("idle")
      setError(null)
      setGeneration((n) => n + 1)
    },
  }
}

export type {
  McpAppStatus,
  McpCallToolParams,
  McpCallToolResult,
  McpToolResult,
  UseChatcnMcpAppOptions,
  UseChatcnMcpAppResult,
}
export { chatcnHostStyles, useChatcnHostTheme, useChatcnMcpApp, withCsp }
