// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import { Collapsible } from "radix-ui"
import * as React from "react"
import { Button } from "@/components/ui/chatcn/button"
import { CopyButton } from "@/components/ui/chatcn/copy-button"
import { Spinner } from "@/components/ui/chatcn/spinner"
import { StreamingCaret } from "@/components/ui/chatcn/streaming-caret"
import {
  BanIcon,
  CheckIcon,
  ChevronDownIcon,
  CircleStopIcon,
  CircleXIcon,
  FileTextIcon,
  GlobeIcon,
  HandIcon,
  PencilIcon,
  PlugIcon,
  RotateCcwIcon,
  SquareTerminalIcon,
  ToolIcon,
  WebSearchIcon,
} from "@/components/ui/chatcn/lib/chatcn-icons"
import { parsePartialJson } from "@/components/ui/chatcn/lib/chatcn-partial-json"
import {
  type ChatcnToolPart,
  getToolDisplayState,
  getToolName,
  type ToolDisplayState,
} from "@/components/ui/chatcn/lib/chatcn-types"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types
 * -----------------------------------------------------------------------------------------------*/

type ApprovalScope = "once" | "always"

interface ToolCallApprovalRequest {
  /** Approval id from the runtime, passed back to `onApprove` and `onDeny`. */
  id: string
  /** Why the agent wants to run this tool. Shown above the actions. */
  reason?: React.ReactNode
}

interface ToolCallProps extends Omit<React.ComponentProps<"div">, "title"> {
  /** Tool name from the model, for example `edit_file`. Picks a default icon and label. */
  name: string
  /** Human label. Defaults to a readable version of `name`. */
  title?: React.ReactNode
  /** Short context next to the title, set in mono. A path, a query or a command. */
  detail?: React.ReactNode
  /** Override the tool icon. */
  icon?: React.ReactNode
  state: ToolDisplayState
  /** Parsed tool input. */
  input?: unknown
  /** Raw input text while it streams. Parsed tolerantly so partial JSON renders as it arrives. */
  inputText?: string
  /** Tool output. Strings render as text, anything else as JSON. Pass a node for a custom view. */
  output?: unknown
  /** What the output copy button copies. Defaults to the output as text; elements are not copied. */
  outputCopyValue?: string
  /** Extra header content before the status label, such as a result count or a +12 -3 summary. */
  meta?: React.ReactNode
  /** Name used in "Always allow …". Defaults to the title with its first letter lowercased. */
  approvalTarget?: string
  /**
   * Replaces the header status label, for example "Blocked by policy" instead of "Failed".
   * A custom label is shown in the neutral tone; the red error tone is only for the default.
   */
  statusLabel?: React.ReactNode
  errorText?: React.ReactNode
  approval?: ToolCallApprovalRequest
  /** The approval decision, shown after the user responds. */
  decision?: { approved: boolean; scope?: ApprovalScope; reason?: string }
  onApprove?: (approvalId: string, scope: ApprovalScope) => void
  onDeny?: (approvalId: string, reason?: string) => void
  onRetry?: () => void
  /** Run time in ms, shown once the tool finishes. */
  durationMs?: number
  /** Epoch ms when the tool started. Drives the live elapsed counter while running. */
  startedAt?: number
  variant?: "card" | "inline"
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
}

/* -------------------------------------------------------------------------------------------------
 * Helpers
 * -----------------------------------------------------------------------------------------------*/

const RUNNING: ToolDisplayState[] = ["input-streaming", "input-available", "approval-responded"]

function isRunning(state: ToolDisplayState, decision?: ToolCallProps["decision"]) {
  if (state === "approval-responded" && decision?.approved === false) return false
  return RUNNING.includes(state)
}

function humanize(name: string) {
  const words = name
    .replace(/^tool-/, "")
    .replace(/[_-]+/g, " ")
    .trim()
  return words.charAt(0).toUpperCase() + words.slice(1)
}

function iconFor(name: string) {
  const n = name.toLowerCase()
  if (/search|query|lookup/.test(n) && /web|internet|google|bing/.test(n)) return <WebSearchIcon />
  if (/fetch|browse|url|http/.test(n)) return <GlobeIcon />
  if (/bash|shell|command|terminal|exec|run/.test(n)) return <SquareTerminalIcon />
  if (/edit|write|patch|replace|create/.test(n)) return <PencilIcon />
  if (/read|open|view|cat|file/.test(n)) return <FileTextIcon />
  if (/search|grep|glob|find/.test(n)) return <WebSearchIcon />
  if (/mcp|connector|plugin/.test(n)) return <PlugIcon />
  return <ToolIcon />
}

const NBSP = "\u00a0"

/**
 * Final durations keep a decimal under 10 s. Live counters (`precise: false`) tick in whole
 * seconds, so the figure turns over once a second and never shows a frozen ".0".
 */
function formatDuration(ms: number, precise = true) {
  if (precise && ms < 1000) return `${Math.max(1, Math.round(ms))}${NBSP}ms`
  const s = ms / 1000
  if (precise && s < 10) return `${s.toFixed(1)}${NBSP}s`
  if (s < 60) return `${precise ? Math.round(s) : Math.floor(s)}${NBSP}s`
  return formatLong(precise ? s : Math.floor(s))
}

/** A minute and up: "4 m 12 s", then "2 h 5 m", then "3 d 4 h", never "1 m 60 s". */
function formatLong(seconds: number) {
  const t = Math.round(seconds)
  if (t < 3600) return `${Math.floor(t / 60)}${NBSP}m ${t % 60}${NBSP}s`
  const h = Math.floor(t / 3600)
  if (h < 24) return `${h}${NBSP}h ${Math.floor((t % 3600) / 60)}${NBSP}m`
  return `${Math.floor(h / 24)}${NBSP}d ${h % 24}${NBSP}h`
}

function useElapsed(startedAt: number | undefined, active: boolean) {
  // Starts empty and reads the clock after mount: a render must not read it (Next.js prerender).
  const [now, setNow] = React.useState<number | null>(null)
  React.useEffect(() => {
    if (!active || startedAt === undefined) return
    setNow(Date.now())
    // Ticks once per second: a counter that changes faster only adds noise.
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [active, startedAt])
  if (!active || startedAt === undefined) return undefined
  return Math.max(0, (now ?? startedAt) - startedAt)
}

/** Measures run time itself when the caller passes no timing, so durations show up for free. */
function useAutoTiming(running: boolean, done: boolean) {
  const [startedAt, setStartedAt] = React.useState<number>()
  const [durationMs, setDurationMs] = React.useState<number>()
  React.useEffect(() => {
    if (running && startedAt === undefined) setStartedAt(Date.now())
    if (done && startedAt !== undefined && durationMs === undefined)
      setDurationMs(Date.now() - startedAt)
  }, [running, done, startedAt, durationMs])
  return { startedAt, durationMs }
}

const DETAIL_KEYS = [
  "path",
  "file",
  "filePath",
  "query",
  "command",
  "cmd",
  "url",
  "pattern",
  "name",
]

function detailFrom(input: unknown): string | undefined {
  if (!input || typeof input !== "object") return undefined
  for (const key of DETAIL_KEYS) {
    const value = (input as Record<string, unknown>)[key]
    if (typeof value === "string" && value.length > 0) return value
  }
  return undefined
}

/* -------------------------------------------------------------------------------------------------
 * Small parts
 * -----------------------------------------------------------------------------------------------*/

/** Status glyph. Swaps with a short blur crossfade so state changes read as one object changing. */
function ToolCallStatus({
  state,
  decision,
  icon,
}: {
  state: ToolDisplayState
  decision?: ToolCallProps["decision"]
  icon: React.ReactNode
}) {
  let glyph: React.ReactNode = icon
  let key = "idle"
  let tone = "text-chatcn-fg-subtle"

  if (isRunning(state, decision)) {
    glyph = <Spinner />
    key = "running"
    tone = "text-chatcn-blue"
  } else if (state === "approval-requested") {
    glyph = <HandIcon />
    key = "approval"
    tone = "text-chatcn-amber"
  } else if (state === "output-error") {
    glyph = <CircleXIcon />
    key = "error"
    tone = "text-chatcn-red"
  } else if (
    state === "output-denied" ||
    (state === "approval-responded" && decision?.approved === false)
  ) {
    glyph = <BanIcon />
    key = "denied"
  } else if (state === "cancelled") {
    glyph = <CircleStopIcon />
    key = "cancelled"
  }

  return (
    <span
      data-slot="tool-call-status"
      className={cn("relative grid size-4 shrink-0 place-items-center [&_svg]:size-4", tone)}
    >
      <span
        key={key}
        className="grid place-items-center motion-safe:animate-[chatcn-swap_150ms_var(--chatcn-ease-out)]"
      >
        {glyph}
      </span>
    </span>
  )
}

function statusLabel(props: {
  state: ToolDisplayState
  decision?: ToolCallProps["decision"]
  durationMs?: number
  elapsed?: number
}) {
  const { state, decision, durationMs, elapsed } = props
  switch (state) {
    case "input-streaming":
    case "input-available":
      return elapsed !== undefined && elapsed >= 1000 ? formatDuration(elapsed, false) : "Running"
    case "approval-requested":
      return "Needs approval"
    case "approval-responded":
      if (decision?.approved === false) return "Denied"
      return elapsed !== undefined && elapsed >= 1000 ? formatDuration(elapsed, false) : "Running"
    case "output-available":
      return durationMs !== undefined ? formatDuration(durationMs) : "Done"
    case "output-error":
      return "Failed"
    case "output-denied":
      return "Denied"
    case "cancelled":
      return "Cancelled"
  }
}

/* -------------------------------------------------------------------------------------------------
 * JSON view: monochrome, weight and gray only.
 * -----------------------------------------------------------------------------------------------*/

function JsonValue({ value, depth }: { value: unknown; depth: number }): React.ReactNode {
  const pad = "  ".repeat(depth + 1)
  const close = "  ".repeat(depth)

  if (value === null) return <span className="text-chatcn-fg-subtle">null</span>
  if (typeof value === "string") {
    return <span className="text-chatcn-fg">{JSON.stringify(value)}</span>
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return <span className="text-chatcn-fg font-medium">{String(value)}</span>
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return <span className="text-chatcn-fg-subtle">[]</span>
    return (
      <>
        <span className="text-chatcn-fg-subtle">[</span>
        {"\n"}
        {value.map((item, i) => (
          <React.Fragment key={i}>
            {pad}
            <JsonValue value={item} depth={depth + 1} />
            {i < value.length - 1 ? <span className="text-chatcn-fg-subtle">,</span> : null}
            {"\n"}
          </React.Fragment>
        ))}
        {close}
        <span className="text-chatcn-fg-subtle">]</span>
      </>
    )
  }
  if (typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
    if (entries.length === 0) return <span className="text-chatcn-fg-subtle">{"{}"}</span>
    return (
      <>
        <span className="text-chatcn-fg-subtle">{"{"}</span>
        {"\n"}
        {entries.map(([k, v], i) => (
          <React.Fragment key={k}>
            {pad}
            <span className="text-chatcn-fg-muted">{JSON.stringify(k)}</span>
            <span className="text-chatcn-fg-subtle">: </span>
            <JsonValue value={v} depth={depth + 1} />
            {i < entries.length - 1 ? <span className="text-chatcn-fg-subtle">,</span> : null}
            {"\n"}
          </React.Fragment>
        ))}
        {close}
        <span className="text-chatcn-fg-subtle">{"}"}</span>
      </>
    )
  }
  return <span className="text-chatcn-fg">{String(value)}</span>
}

function ToolCallCode({
  children,
  streaming,
  className,
}: {
  children: React.ReactNode
  streaming?: boolean
  className?: string
}) {
  // Long input or output scrolls inside the box, so the box joins the tab order exactly when it
  // overflows: keyboard users can then scroll it, and short payloads add no extra tab stop.
  const ref = React.useRef<HTMLPreElement>(null)
  const [scrolls, setScrolls] = React.useState(false)
  React.useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const measure = () =>
      setScrolls(el.scrollHeight > el.clientHeight + 1 || el.scrollWidth > el.clientWidth + 1)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    if (el.firstElementChild) observer.observe(el.firstElementChild)
    return () => observer.disconnect()
  }, [])
  return (
    <pre
      ref={ref}
      data-slot="tool-call-code"
      tabIndex={scrolls ? 0 : undefined}
      className={cn(
        "rounded-chatcn-md bg-chatcn-inset chatcn-scroll chatcn-focus max-h-72 overflow-auto px-3 py-2.5",
        "chatcn-text-12 text-chatcn-fg-muted font-mono [overflow-wrap:anywhere] whitespace-pre-wrap",
        className,
      )}
    >
      <code>
        {children}
        {streaming ? <StreamingCaret /> : null}
      </code>
    </pre>
  )
}

function stringify(value: unknown) {
  if (typeof value === "string") return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function ToolCallSection({
  label,
  action,
  children,
}: {
  label: string
  action?: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <section data-slot="tool-call-section" className="flex flex-col gap-1.5">
      <div className="flex h-6 items-center justify-between">
        <h4 className="chatcn-text-12 text-chatcn-fg-subtle font-medium">{label}</h4>
        {action}
      </div>
      {children}
    </section>
  )
}

/* -------------------------------------------------------------------------------------------------
 * Approval
 * -----------------------------------------------------------------------------------------------*/

function ToolCallApproval({
  approval,
  toolLabel,
  onApprove,
  onDeny,
  inline,
}: {
  /** Inline rows continue the body's guide line, so the reason and actions share its left edge. */
  inline?: boolean
  approval: ToolCallApprovalRequest
  toolLabel: string
  onApprove?: ToolCallProps["onApprove"]
  onDeny?: ToolCallProps["onDeny"]
}) {
  const [always, setAlways] = React.useState(false)
  const [denying, setDenying] = React.useState(false)
  const [reason, setReason] = React.useState("")
  const inputRef = React.useRef<HTMLInputElement>(null)

  React.useEffect(() => {
    if (denying) inputRef.current?.focus({ preventScroll: true })
  }, [denying])

  return (
    <div
      data-slot="tool-call-approval"
      className={cn(
        "flex flex-col gap-2",
        inline
          ? "border-chatcn-border ms-[15.5px] mb-2 border-s ps-[17.5px] pt-3"
          : "border-chatcn-border/70 border-t p-1.5 pl-3.5",
      )}
    >
      {approval.reason ? (
        <p
          className={cn(
            "chatcn-text-13 text-chatcn-fg-muted pr-2 text-pretty",
            !inline && "pt-1.5",
          )}
        >
          {approval.reason}
        </p>
      ) : null}

      {denying ? (
        <form
          className="flex items-center gap-1.5"
          onSubmit={(event) => {
            event.preventDefault()
            onDeny?.(approval.id, reason.trim() || undefined)
          }}
        >
          <input
            ref={inputRef}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation()
                setDenying(false)
              }
            }}
            placeholder="Tell the agent what to do instead…"
            aria-label="Reason for denying"
            className={cn(
              "rounded-chatcn-sm chatcn-text-13 text-chatcn-fg h-7 min-w-0 flex-1 bg-transparent px-1",
              "placeholder:text-chatcn-fg-subtle focus:outline-none",
            )}
          />
          <Button size="sm" onClick={() => setDenying(false)}>
            Back
          </Button>
          <Button type="submit" size="sm" variant="solid">
            Deny
          </Button>
        </form>
      ) : (
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            role="switch"
            aria-checked={always}
            onClick={() => setAlways((v) => !v)}
            className={cn(
              "chatcn-focus group/always rounded-chatcn-sm mr-auto inline-flex h-7 min-w-0 items-center gap-2 pr-2",
              "chatcn-text-13 text-chatcn-fg-muted hover:text-chatcn-fg transition-colors duration-150",
            )}
          >
            <span
              aria-hidden="true"
              className={cn(
                "grid size-3.5 shrink-0 place-items-center rounded-[4px] shadow-[inset_0_0_0_1px_var(--chatcn-border-strong)]",
                "ease-chatcn-out transition-[background-color,box-shadow] duration-150 [&_svg]:size-3",
                always && "bg-chatcn-accent text-chatcn-accent-fg shadow-none",
              )}
            >
              <CheckIcon
                strokeWidth={2}
                className={cn(
                  "ease-chatcn-out transition-[opacity,scale] duration-150",
                  always ? "scale-100 opacity-100" : "scale-50 opacity-0",
                )}
              />
            </span>
            <span className="truncate">Always allow {toolLabel}</span>
          </button>
          <Button size="sm" onClick={() => setDenying(true)}>
            Deny
          </Button>
          <Button
            size="sm"
            variant="solid"
            onClick={() => onApprove?.(approval.id, always ? "always" : "once")}
          >
            Allow
          </Button>
        </div>
      )}
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * ToolCall
 * -----------------------------------------------------------------------------------------------*/

function ToolCall({
  name,
  title,
  detail,
  icon,
  state,
  input,
  inputText,
  output,
  outputCopyValue,
  meta: metaSlot,
  approvalTarget,
  statusLabel: statusLabelProp,
  errorText,
  approval,
  decision,
  onApprove,
  onDeny,
  onRetry,
  durationMs,
  startedAt,
  variant = "card",
  open: openProp,
  defaultOpen,
  onOpenChange,
  className,
  children,
  ...props
}: ToolCallProps) {
  const label = typeof title === "string" ? title : humanize(name)
  // Lowercase only the first letter, so "Edit file" reads "edit file" but "GitHub search" keeps its case.
  const target =
    approvalTarget ??
    (/^[A-Z][a-z]*(\s|$)/.test(label) ? label.charAt(0).toLowerCase() + label.slice(1) : label)
  const running = isRunning(state, decision)
  const auto = useAutoTiming(running, state === "output-available")
  const elapsed = useElapsed(startedAt ?? auto.startedAt, running)

  // Uncontrolled open state opens itself when the tool needs the user: approval or failure.
  const [openState, setOpenState] = React.useState(
    defaultOpen ?? (state === "approval-requested" || state === "output-error"),
  )
  const open = openProp ?? openState
  const prevState = React.useRef(state)
  React.useEffect(() => {
    if (prevState.current === state) return
    prevState.current = state
    if (openProp === undefined && (state === "approval-requested" || state === "output-error")) {
      setOpenState(true)
    }
  }, [state, openProp])

  const setOpen = (next: boolean) => {
    if (openProp === undefined) setOpenState(next)
    onOpenChange?.(next)
  }

  const streamingInput = state === "input-streaming"
  const shownInput = React.useMemo(() => {
    if (streamingInput && inputText !== undefined) return parsePartialJson(inputText).value
    return input
  }, [streamingInput, inputText, input])

  const hasInput = shownInput !== undefined && shownInput !== null
  const hasOutput = state === "output-available" && output !== undefined
  const statusText = statusLabel({
    state,
    decision,
    durationMs: durationMs ?? auto.durationMs,
    elapsed,
  })
  const shownDetail = detail ?? detailFrom(shownInput)
  const isCard = variant === "card"

  return (
    <Collapsible.Root
      open={open}
      onOpenChange={setOpen}
      data-slot="tool-call"
      data-tool-state={state}
      data-variant={variant}
      className={cn(
        "group/tool-call chatcn-text-14 text-chatcn-fg",
        isCard && "rounded-chatcn-lg bg-chatcn-card shadow-chatcn-sm overflow-hidden",
        className,
      )}
      {...props}
    >
      <Collapsible.Trigger
        className={cn(
          "flex w-full min-w-0 items-center gap-2.5 text-left outline-none select-none",
          "ease-chatcn-standard transition-colors duration-150",
          "focus-visible:outline-chatcn-accent focus-visible:outline-2 focus-visible:-outline-offset-2",
          isCard
            ? "hover:bg-chatcn-alpha-100 h-10 px-3.5 pointer-coarse:h-11"
            : "rounded-chatcn-md hover:bg-chatcn-fill-hover h-8 px-2 pointer-coarse:h-11",
        )}
      >
        <ToolCallStatus state={state} decision={decision} icon={icon ?? iconFor(name)} />
        <span className="flex min-w-0 flex-1 items-baseline gap-2">
          <span className="shrink-0 font-medium">{title ?? label}</span>
          {shownDetail ? (
            <span className="chatcn-text-12 text-chatcn-fg-subtle truncate font-mono">
              {shownDetail}
            </span>
          ) : null}
        </span>
        {metaSlot ? (
          <span className="chatcn-text-12 text-chatcn-fg-subtle shrink-0 tabular-nums">
            {metaSlot}
          </span>
        ) : null}
        <span
          aria-live="polite"
          className={cn(
            "chatcn-text-12 shrink-0 tabular-nums",
            state === "output-error" && statusLabelProp === undefined
              ? "text-chatcn-red"
              : "text-chatcn-fg-subtle",
          )}
        >
          {statusLabelProp ?? statusText}
        </span>
        <ChevronDownIcon
          aria-hidden="true"
          className={cn(
            "text-chatcn-fg-subtle ease-chatcn-out size-4 shrink-0 transition-transform duration-200",
            "group-data-[state=open]/tool-call:rotate-180 motion-reduce:transition-none",
          )}
        />
      </Collapsible.Trigger>

      <Collapsible.Content
        className={cn(
          "h-(--radix-collapsible-content-height) overflow-hidden",
          // Height is the one layout property we animate: Base UI measures it, and it stays
          // short. During streaming the panel is `auto`, so content grows without a transition.
          "ease-chatcn-out transition-[height,opacity] duration-200 motion-reduce:transition-none",
          "data-[state=closed]:h-0 data-[state=closed]:opacity-0 starting:h-0 starting:opacity-0 data-[state=closed]:animate-[chatcn-exit-hold_var(--tw-duration,150ms)]",
        )}
      >
        <div
          className={cn(
            "flex flex-col gap-3",
            isCard
              ? "px-3.5 pt-1 pb-3.5"
              : cn(
                  "border-chatcn-border ms-[15.5px] mt-1 border-s ps-[17.5px] pt-1",
                  // An approval below continues the same guide line without a break.
                  state === "approval-requested" && approval ? "mb-0" : "mb-2",
                ),
          )}
        >
          {children ?? (
            <>
              {hasInput ? (
                <ToolCallSection
                  label="Input"
                  action={
                    streamingInput ? null : (
                      <CopyButton
                        size="icon-xs"
                        value={() => stringify(shownInput)}
                        label="Copy input"
                      />
                    )
                  }
                >
                  <ToolCallCode streaming={streamingInput}>
                    {typeof shownInput === "string" ? (
                      shownInput
                    ) : (
                      <JsonValue value={shownInput} depth={0} />
                    )}
                  </ToolCallCode>
                </ToolCallSection>
              ) : streamingInput ? (
                <ToolCallSection label="Input">
                  <ToolCallCode streaming>{""}</ToolCallCode>
                </ToolCallSection>
              ) : null}

              {hasOutput ? (
                <ToolCallSection
                  label="Output"
                  action={
                    outputCopyValue !== undefined || !React.isValidElement(output) ? (
                      <CopyButton
                        size="icon-xs"
                        value={() => outputCopyValue ?? stringify(output)}
                        label="Copy output"
                      />
                    ) : null
                  }
                >
                  {React.isValidElement(output) ? (
                    output
                  ) : (
                    <ToolCallCode>
                      {typeof output === "string" ? output : <JsonValue value={output} depth={0} />}
                    </ToolCallCode>
                  )}
                </ToolCallSection>
              ) : null}

              {state === "output-error" ? (
                <ToolCallSection
                  label="Error"
                  action={
                    onRetry ? (
                      <Button size="xs" onClick={onRetry}>
                        <RotateCcwIcon />
                        Retry
                      </Button>
                    ) : null
                  }
                >
                  <ToolCallCode className="text-chatcn-fg">
                    {errorText ?? "The tool failed without a message."}
                  </ToolCallCode>
                </ToolCallSection>
              ) : null}

              {decision ? (
                <p className="chatcn-text-12 text-chatcn-fg-subtle">
                  {decision.approved
                    ? decision.scope === "always"
                      ? `Always allowed for ${target}`
                      : "Allowed once"
                    : decision.reason
                      ? `Denied: ${decision.reason}`
                      : "Denied"}
                </p>
              ) : null}
            </>
          )}
        </div>
      </Collapsible.Content>

      {state === "approval-requested" && approval ? (
        <ToolCallApproval
          inline={!isCard}
          approval={approval}
          toolLabel={target}
          onApprove={onApprove}
          onDeny={onDeny}
        />
      ) : null}
    </Collapsible.Root>
  )
}

/* -------------------------------------------------------------------------------------------------
 * From a message part
 * -----------------------------------------------------------------------------------------------*/

/**
 * Maps a tool part (chatcn contract or AI SDK `UIMessage` part) to `ToolCall` props.
 * `<ToolCall {...toolCallPropsFromPart(part)} onApprove={...} />`
 */
function toolCallPropsFromPart(
  part: ChatcnToolPart,
  options?: { stopped?: boolean },
): Pick<
  ToolCallProps,
  "name" | "state" | "input" | "inputText" | "output" | "errorText" | "approval" | "decision"
> &
  Partial<Pick<ToolCallProps, "title">> {
  const state = getToolDisplayState(part, options)
  const approval = part.approval
  return {
    name: getToolName(part),
    ...(part.title ? { title: part.title } : {}),
    state,
    input: part.input,
    inputText: typeof part.rawInput === "string" ? part.rawInput : undefined,
    output: part.output,
    errorText: part.errorText,
    approval:
      approval && state === "approval-requested"
        ? { id: approval.id, reason: approval.requestReason }
        : undefined,
    decision:
      approval && approval.approved !== undefined
        ? { approved: approval.approved, reason: approval.reason }
        : undefined,
  }
}

export type { ApprovalScope, ToolCallApprovalRequest, ToolCallProps }
export {
  StreamingCaret,
  ToolCall,
  ToolCallApproval,
  ToolCallCode,
  ToolCallSection,
  ToolCallStatus,
  toolCallPropsFromPart,
}
