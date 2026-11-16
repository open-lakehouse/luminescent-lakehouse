// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"
import { Button } from "@/components/ui/chatcn/button"
import { useConversation } from "@/components/ui/chatcn/conversation"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { CircleAlertIcon, CircleStopIcon, ClockIcon, RotateCcwIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types and context
 * -----------------------------------------------------------------------------------------------*/

type MessageRole = "user" | "assistant" | "system"
type MessageLayout = "auto" | "bubble" | "flat" | "compact"
type MessageState = "streaming" | "done" | "edited" | "error" | "stopped" | "queued"

interface MessageContextValue {
  from: MessageRole
  /** Resolved layout: `auto` becomes `bubble` for users and `flat` for everyone else. */
  layout: Exclude<MessageLayout, "auto">
  state: MessageState
  /** The newest message: its actions stay visible. */
  latest: boolean
}

const MessageContext = React.createContext<MessageContextValue | null>(null)

/** Role, layout and state of the nearest `Message`. Returns null outside one. */
function useMessage() {
  return React.useContext(MessageContext)
}

/* -------------------------------------------------------------------------------------------------
 * Message
 * -----------------------------------------------------------------------------------------------*/

interface MessageProps extends React.ComponentProps<"div"> {
  /** Who wrote it. Named `from` so the ARIA `role` attribute stays free. */
  from: MessageRole
  /**
   * `auto` gives users a subtle bubble and assistants a flat, document-style block. `compact`
   * is the dense log style for agent traces and side panels. Defaults to `auto`.
   */
  layout?: MessageLayout
  /** Lifecycle state. Defaults to `done`. */
  state?: MessageState
  /** Follows a message from the same author: tightens the gap and hides the avatar. */
  grouped?: boolean
  /** The newest message. Keeps its actions visible instead of revealing them on hover. */
  latest?: boolean
  /**
   * Enter animation. Defaults to animating only messages that arrive after the conversation
   * mounted, so restored history appears at once.
   */
  animate?: boolean
  /** Announce the text to screen readers when streaming finishes. Defaults to true. */
  announce?: boolean
}

/**
 * One turn in the conversation. Compose `MessageAvatar`, `MessageContent`, `MessageMeta`,
 * `MessageStatus` and `MessageActions` inside it.
 */
function Message({
  from,
  layout: layoutProp = "auto",
  state = "done",
  grouped = false,
  latest = false,
  animate,
  announce = true,
  className,
  children,
  ...props
}: MessageProps) {
  const conversation = useConversation()
  const layout: MessageContextValue["layout"] =
    layoutProp === "auto" ? (from === "user" ? "bubble" : "flat") : layoutProp
  const [enter] = React.useState(() => animate ?? conversation?.ready ?? false)
  const ref = React.useRef<HTMLDivElement>(null)

  // Screen readers hear the finished reply once, never token by token.
  const previous = React.useRef(state)
  React.useEffect(() => {
    const was = previous.current
    previous.current = state
    if (!announce || was !== "streaming" || state === "streaming") return
    const text = ref.current?.querySelector('[data-slot="message-content"]')?.textContent?.trim()
    if (text) conversation?.announce(text)
  }, [state, announce, conversation])

  const value = React.useMemo(
    () => ({ from, layout, state, latest }),
    [from, layout, state, latest],
  )

  return (
    <MessageContext.Provider value={value}>
      <div
        ref={ref}
        data-slot="message"
        data-role={from}
        data-layout={layout}
        data-message-state={state}
        data-grouped={grouped ? "" : undefined}
        data-latest={latest ? "" : undefined}
        aria-busy={state === "streaming" || undefined}
        className={cn(
          "group/message relative flex min-w-0 flex-col",
          layout === "compact" ? "gap-1" : "gap-2",
          layout === "bubble" ? "items-end" : "items-start",
          from === "system" && layout !== "compact" && "items-center text-center",
          grouped && (layout === "compact" ? "-mt-2" : "-mt-4"),
          // An avatar sits in the start gutter (the end gutter for bubbles).
          layout === "bubble"
            ? "has-[>[data-slot=message-avatar]]:pe-9"
            : "has-[>[data-slot=message-avatar]]:ps-9",
          enter &&
            (from === "user"
              ? "ease-chatcn-out-soft transition-[opacity,translate] duration-200 starting:translate-y-2 starting:opacity-0 motion-reduce:starting:translate-y-0"
              : "ease-chatcn-out-soft transition-opacity duration-150 starting:opacity-0"),
          className,
        )}
        {...props}
      >
        {children}
      </div>
    </MessageContext.Provider>
  )
}

/* -------------------------------------------------------------------------------------------------
 * MessageContent
 * -----------------------------------------------------------------------------------------------*/

/**
 * The text of a message, capped at a 68ch measure. Plain text keeps its line breaks. Markdown
 * renderers drop in as children and inherit the 15/24 prose size.
 */
function MessageContent({ className, children, ...props }: React.ComponentProps<"div">) {
  const ctx = useMessage()
  const layout = ctx?.layout ?? "flat"
  const from = ctx?.from ?? "assistant"
  const state = ctx?.state ?? "done"
  const plain = typeof children === "string"

  return (
    <div
      data-slot="message-content"
      className={cn(
        "text-chatcn-fg min-w-0 [overflow-wrap:anywhere]",
        // Bubbles fill their lines: pretty wrapping would leave a wide, half-empty bubble.
        layout !== "bubble" && "text-pretty",
        plain && "whitespace-pre-wrap",
        layout === "bubble" &&
          "chatcn-text-15 rounded-chatcn-xl bg-chatcn-fill max-w-[80%] px-4 py-2.5 text-start",
        layout === "flat" && "chatcn-text-15 w-full max-w-[68ch]",
        layout === "compact" &&
          (from === "user"
            ? "chatcn-text-14 rounded-chatcn-md bg-chatcn-fill max-w-full px-3 py-1.5"
            : "chatcn-text-14 w-full max-w-[68ch]"),
        from === "system" && "chatcn-text-13 text-chatcn-fg-muted max-w-[60ch]",
        // Queued turns are not sent yet: an outline instead of a fill says "pending".
        state === "queued" &&
          from === "user" &&
          "text-chatcn-fg-muted border-chatcn-border-strong border border-dashed bg-transparent",
        state === "error" && from === "user" && "text-chatcn-fg-muted",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * MessageAvatar
 * -----------------------------------------------------------------------------------------------*/

interface MessageAvatarProps extends React.ComponentProps<"span"> {
  /** Image URL. Falls back to initials, then to the role default. */
  src?: string
  /** Name used for initials and the image alt text. */
  name?: string
  /**
   * `motif` is the kit's square mark, the default for assistants. `initials` is the default for
   * users. Pass children for a provider mark.
   */
  variant?: "motif" | "initials"
}

function initialsOf(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean)
  return ((words[0]?.[0] ?? "") + (words.length > 1 ? (words.at(-1)?.[0] ?? "") : ""))
    .toUpperCase()
    .slice(0, 2)
}

/** Avatar in the message gutter. Hidden, but still spaced, on grouped messages. */
function MessageAvatar({ src, name, variant, className, children, ...props }: MessageAvatarProps) {
  const ctx = useMessage()
  const [failed, setFailed] = React.useState(false)
  const kind = variant ?? (ctx?.from === "assistant" ? "motif" : "initials")
  const compact = ctx?.layout === "compact"

  let body: React.ReactNode
  if (children) body = children
  else if (src && !failed)
    body = (
      <img
        src={src}
        alt={name ?? ""}
        onError={() => setFailed(true)}
        className="size-full object-cover"
      />
    )
  else if (kind === "motif")
    body = <span aria-hidden="true" className="bg-chatcn-fg size-2.5 rounded-[3px]" />
  // Two letters do not fit the 20 px compact avatar: it shows the first initial only.
  else
    body = <span aria-hidden="true">{name ? initialsOf(name).slice(0, compact ? 1 : 2) : "Y"}</span>

  return (
    <span
      data-slot="message-avatar"
      title={name}
      className={cn(
        "absolute top-0 grid shrink-0 place-items-center overflow-hidden rounded-full select-none",
        compact ? "chatcn-text-12 size-5" : "chatcn-text-12 size-6",
        // Centered on the first line of text: bubbles and compact user turns are padded.
        ctx?.layout === "bubble"
          ? "end-0 top-2.5"
          : compact && ctx?.from === "user"
            ? "start-0 top-1.5"
            : "start-0",
        kind === "motif" && !children && !src
          ? "bg-transparent"
          : "bg-chatcn-fill text-chatcn-fg-muted font-medium shadow-[inset_0_0_0_1px_var(--chatcn-alpha-200)]",
        "group-data-[grouped]/message:invisible",
        className,
      )}
      {...props}
    >
      {body}
    </span>
  )
}

/* -------------------------------------------------------------------------------------------------
 * MessageMeta
 * -----------------------------------------------------------------------------------------------*/

const NBSP = "\u00a0"

function toDate(value: Date | number | string) {
  return value instanceof Date ? value : new Date(value)
}

function relativeTime(date: Date, now: number) {
  const s = Math.round((now - date.getTime()) / 1000)
  if (s < 45) return "Just now"
  const m = Math.round(s / 60)
  if (m < 60) return `${m}${NBSP}min ago`
  const h = Math.round(m / 60)
  if (h < 24) return `${h}${NBSP}h ago`
  const d = Math.round(h / 24)
  if (d === 1) return "Yesterday"
  if (d < 7) return `${d}${NBSP}days ago`
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" })
}

function formatLatency(ms: number) {
  return ms < 1000 ? `${Math.round(ms)}${NBSP}ms` : `${(ms / 1000).toFixed(1)}${NBSP}s`
}

interface MessageMetaProps extends React.ComponentProps<"div"> {
  /** When it was sent. Shown relative, absolute on hover. */
  time?: Date | number | string
  /** Model that wrote the reply. */
  model?: string
  /** Total tokens for the turn. */
  tokens?: number
  /** Time to finish, in ms. */
  durationMs?: number
  /** Cost in US dollars. */
  cost?: number
}

/** Quiet facts about a message: time, model, tokens, latency, cost. Extra children append. */
function MessageMeta({
  time,
  model,
  tokens,
  durationMs,
  cost,
  className,
  children,
  ...props
}: MessageMetaProps) {
  const ctx = useMessage()
  // Starts empty and reads the clock after mount: a render must not read it (Next.js prerender).
  const [now, setNow] = React.useState<number | null>(null)
  React.useEffect(() => {
    if (time === undefined) return
    setNow(Date.now())
    const id = window.setInterval(() => setNow(Date.now()), 30_000)
    return () => window.clearInterval(id)
  }, [time])

  const items: React.ReactNode[] = []
  if (time !== undefined) {
    const date = toDate(time)
    const absolute = date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
    items.push(
      <Tooltip key="time" content={absolute}>
        <time dateTime={date.toISOString()}>{relativeTime(date, now ?? date.getTime())}</time>
      </Tooltip>,
    )
  }
  if (model) items.push(<span key="model">{model}</span>)
  if (tokens !== undefined)
    items.push(<span key="tokens">{`${tokens.toLocaleString()}${NBSP}tokens`}</span>)
  if (durationMs !== undefined) items.push(<span key="duration">{formatLatency(durationMs)}</span>)
  if (cost !== undefined)
    items.push(<span key="cost">{`$${cost < 0.01 ? cost.toFixed(4) : cost.toFixed(2)}`}</span>)
  if (ctx?.state === "edited") items.push(<span key="edited">Edited</span>)
  React.Children.forEach(children, (child) => {
    if (child !== null && child !== undefined && child !== false) items.push(child)
  })
  if (items.length === 0) return null

  return (
    <div
      data-slot="message-meta"
      className={cn(
        "chatcn-text-12 text-chatcn-fg-subtle flex min-h-5 flex-wrap items-center gap-x-1.5 tabular-nums",
        className,
      )}
      {...props}
    >
      {items.map((item, i) => (
        <React.Fragment key={i}>
          {i > 0 ? <span aria-hidden="true">·</span> : null}
          {item}
        </React.Fragment>
      ))}
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * MessageStatus
 * -----------------------------------------------------------------------------------------------*/

interface MessageStatusProps extends React.ComponentProps<"div"> {
  /** Resend a user message that failed to send. */
  onRetry?: () => void
  /** Continue a reply that was stopped. */
  onContinue?: () => void
}

/**
 * The line under a message that is not simply done: queued, not sent, stopped or edited.
 * Renders nothing for other states. Failed replies use `MessageError` instead.
 */
function MessageStatus({ onRetry, onContinue, className, ...props }: MessageStatusProps) {
  const ctx = useMessage()
  const state = ctx?.state
  const isUser = ctx?.from === "user"

  let body: React.ReactNode = null
  if (state === "queued") {
    body = (
      <>
        <ClockIcon aria-hidden="true" />
        <span>Queued, sends after the current reply</span>
      </>
    )
  } else if (state === "error" && isUser) {
    body = (
      <>
        <CircleAlertIcon aria-hidden="true" className="text-chatcn-red" />
        <span>Not sent</span>
        {onRetry ? (
          <Button size="xs" onClick={onRetry} className="ms-0.5 -me-2">
            <RotateCcwIcon />
            Retry
          </Button>
        ) : null}
      </>
    )
  } else if (state === "stopped") {
    body = (
      <>
        <CircleStopIcon aria-hidden="true" />
        <span>Stopped</span>
        {onContinue ? (
          <Button size="xs" onClick={onContinue} className="ms-0.5">
            Resume reply
          </Button>
        ) : null}
      </>
    )
  }
  if (!body) return null

  return (
    <div
      role={state === "error" ? "alert" : "status"}
      data-slot="message-status"
      className={cn(
        "chatcn-text-12 text-chatcn-fg-subtle flex min-h-6 items-center gap-1.5 [&>svg]:size-3.5 [&>svg]:shrink-0",
        className,
      )}
      {...props}
    >
      {body}
    </div>
  )
}

export type {
  MessageAvatarProps,
  MessageContextValue,
  MessageLayout,
  MessageMetaProps,
  MessageProps,
  MessageRole,
  MessageState,
  MessageStatusProps,
}
export { Message, MessageAvatar, MessageContent, MessageMeta, MessageStatus, useMessage }
