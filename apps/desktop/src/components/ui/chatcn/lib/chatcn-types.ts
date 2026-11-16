// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
export type ToolState =
  | "input-streaming"
  | "input-available"
  | "approval-requested"
  | "approval-responded"
  | "output-available"
  | "output-error"
  | "output-denied"

/** Tool state as shown in the UI: adds `cancelled` for calls left unfinished by a stop. */
export type ToolDisplayState = ToolState | "cancelled"

/** Streaming state shared by text and reasoning parts. */
export type ChatcnTextState = "streaming" | "done"

/** Human approval attached to a tool call. */
export interface ChatcnApproval {
  id: string
  approved?: boolean
  /** Reason the user gave when responding (usually a denial). */
  reason?: string
  /** Reason the agent gave when asking. */
  requestReason?: string
}

/** A tool call: static (`tool-${name}`) or dynamic (`dynamic-tool` with `toolName`). */
export type ChatcnToolPart = {
  type: `tool-${string}` | "dynamic-tool"
  toolCallId: string
  /** Present on `dynamic-tool` parts. Use `getToolName` to read the name of any tool part. */
  toolName?: string
  state: ToolState
  title?: string
  /** Partial while `input-streaming`, complete afterwards. */
  input?: unknown
  /** Raw input text received so far: a string while `input-streaming` (AI SDK types it loosely). */
  rawInput?: unknown
  output?: unknown
  errorText?: string
  approval?: ChatcnApproval
  providerExecuted?: boolean
  /** True while an `output-available` result is still being refined. */
  preliminary?: boolean
}

/** Assistant or user prose. */
export type ChatcnTextPart = { type: "text"; text: string; state?: ChatcnTextState }

/** Model reasoning or thinking text. */
export type ChatcnReasoningPart = {
  type: "reasoning"
  text: string
  id?: string
  state?: ChatcnTextState
}

/** A web source cited by the response. */
export type ChatcnSourceUrlPart = {
  type: "source-url"
  sourceId: string
  url: string
  title?: string
}

/** A document source cited by the response. */
export type ChatcnSourceDocumentPart = {
  type: "source-document"
  sourceId: string
  mediaType: string
  title: string
  filename?: string
}

/** An attached or generated file (hosted URL or data URL). */
export type ChatcnFilePart = { type: "file"; mediaType: string; url: string; filename?: string }

/** App-defined structured data (`data-${name}`), reconciled by `id`. */
export type ChatcnDataPart<D = unknown> = { type: `data-${string}`; id?: string; data: D }

/** Boundary between agent steps. */
export type ChatcnStepStartPart = { type: "step-start" }

/** Every part type chatcn renders natively. */
export type ChatcnKnownPart =
  | ChatcnTextPart
  | ChatcnReasoningPart
  | ChatcnToolPart
  | ChatcnSourceUrlPart
  | ChatcnSourceDocumentPart
  | ChatcnFilePart
  | ChatcnDataPart
  | ChatcnStepStartPart

/** Any other part (for example AI SDK `custom` or `reasoning-file`): render a fallback, never crash. */
export type ChatcnUnknownPart = { type: string; [key: string]: unknown }

/** Any message part. Narrow with `isKnownPart` first, then switch on `part.type`. */
export type ChatcnPart = ChatcnKnownPart | ChatcnUnknownPart

/** A chat message, structurally compatible with AI SDK `UIMessage`. */
export interface ChatcnMessage<P extends ChatcnPart = ChatcnPart> {
  id: string
  role: "system" | "user" | "assistant"
  parts: P[]
  metadata?: unknown
}

/** How long an approval applies. */
export type ChatcnApprovalScope = "once" | "session" | "tool" | "always"

/** Callbacks blocks use to drive a chat runtime. Missing members hide the matching controls. */
export interface ChatcnActions {
  send?: (text: string, options?: { files?: ChatcnFilePart[] }) => void | Promise<void>
  stop?: () => void
  regenerate?: (messageId?: string) => void | Promise<void>
  approve?: (approvalId: string, options?: { scope?: ChatcnApprovalScope }) => void | Promise<void>
  deny?: (approvalId: string, reason?: string) => void | Promise<void>
  submitToolOutput?: (toolCallId: string, output: unknown) => void | Promise<void>
  editMessage?: (messageId: string, text: string) => void | Promise<void>
}

const KNOWN_TYPES = new Set([
  "text",
  "reasoning",
  "source-url",
  "source-document",
  "file",
  "step-start",
])

/** True for tool parts, static or dynamic. */
export function isToolPart(part: { type: string }): part is ChatcnToolPart {
  return part.type === "dynamic-tool" || part.type.startsWith("tool-")
}

/** True for `data-${name}` parts. */
export function isDataPart(part: { type: string }): part is ChatcnDataPart {
  return part.type.startsWith("data-")
}

/** True for every part chatcn renders natively; the rest go to the fallback. */
export function isKnownPart(part: ChatcnPart): part is ChatcnKnownPart {
  return KNOWN_TYPES.has(part.type) || isToolPart(part) || isDataPart(part)
}

/** Tool name for `tool-${name}` and `dynamic-tool` parts. */
export function getToolName(part: Pick<ChatcnToolPart, "type" | "toolName">): string {
  if (part.type === "dynamic-tool") return part.toolName ?? ""
  return part.type.slice("tool-".length)
}

/** True once a tool call will not change again. */
export function isTerminalToolState(state: ToolDisplayState): boolean {
  return (
    state === "output-available" ||
    state === "output-error" ||
    state === "output-denied" ||
    state === "cancelled"
  )
}

/** Display state for a tool part; unfinished calls in a stopped response become `cancelled`. */
export function getToolDisplayState(
  part: Pick<ChatcnToolPart, "state">,
  options?: { stopped?: boolean },
): ToolDisplayState {
  return options?.stopped && !isTerminalToolState(part.state) ? "cancelled" : part.state
}
