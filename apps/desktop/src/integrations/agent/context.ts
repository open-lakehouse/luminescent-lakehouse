// What an MCP App tells the agent, back into the session. The agent never sees the
// app (it runs on the desktop's own MCP connection), so the app's messages go into the
// session as user messages:
//
// - `ui/message`: the app speaks for the user. Posted as it comes.
// - `ui/update-model-context`: what the agent should know next turn (the grant form's
//   outcome). Only the latest per app is kept. An app whose call is waiting on the
//   user (its result says `status: "awaiting_user"`, as `open_grant_form`'s does)
//   ended the agent's turn on purpose, so its context is posted on its own as soon as
//   the session is idle, as an `<mcp-app-context>` message. Any other context waits,
//   and goes in front of the user's next message.

/** One app: the tool call that rendered it. */
export interface AppRef {
  /** The MCP tool (`open_grant_form`). */
  tool: string;
  /** The agent's call id: the agent's prompt ties the outcome to its call by it. */
  callId: string;
  /** The call's result waits on the user (`structuredContent.status === "awaiting_user"`). */
  awaitingUser: boolean;
}

export interface ModelContextDeps {
  /** Post `text` to the session as a user message. */
  post(text: string): Promise<unknown>;
  /** Whether the session is idle (no turn under way). */
  idle(): boolean;
}

const escapeText = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escapeAttr = (s: string) => escapeText(s).replace(/"/g, "&quot;");

/** `<mcp-app-context tool=… call_id=…>text</mcp-app-context>`. The text is the app's,
 *  so it is escaped: it can't close the element and pass for the user's words. */
export function contextMessage(app: Pick<AppRef, "tool" | "callId">, text: string): string {
  return `<mcp-app-context tool="${escapeAttr(app.tool)}" call_id="${escapeAttr(app.callId)}">${escapeText(text)}</mcp-app-context>`;
}

const CONTEXT = /^\s*<mcp-app-context\b[^>]*>[\s\S]*?<\/mcp-app-context>\s*/;

/** A user message split into the contexts in front of it and what the user wrote. */
export function splitContext(text: string): { contexts: string[]; rest: string } {
  const contexts: string[] = [];
  let rest = text;
  for (let m = CONTEXT.exec(rest); m; m = CONTEXT.exec(rest)) {
    contexts.push(m[0].trim());
    rest = rest.slice(m[0].length);
  }
  return { contexts, rest };
}

/** The attributes and text of an `<mcp-app-context>` element, for its chip. */
export function parseContext(
  element: string,
): { tool?: string; callId?: string; text: string } | undefined {
  const m = /^<mcp-app-context\b([^>]*)>([\s\S]*)<\/mcp-app-context>$/.exec(element.trim());
  if (!m) return undefined;
  const attr = (name: string) => new RegExp(`${name}="([^"]*)"`).exec(m[1] ?? "")?.[1];
  const unescapeText = (s: string) =>
    s
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&amp;/g, "&");
  const tool = attr("tool");
  const callId = attr("call_id");
  return {
    ...(tool !== undefined ? { tool: unescapeText(tool) } : {}),
    ...(callId !== undefined ? { callId: unescapeText(callId) } : {}),
    text: unescapeText(m[2] ?? ""),
  };
}

/** The text of an app's content blocks (and its structured content, as JSON). */
export function contentText(params: {
  content?: unknown[];
  structuredContent?: Record<string, unknown>;
}): string {
  const text = (params.content ?? [])
    .map((c) =>
      typeof c === "object" && c && "text" in c && typeof c.text === "string" ? c.text : "",
    )
    .filter(Boolean)
    .join("\n");
  if (text) return text;
  return params.structuredContent ? JSON.stringify(params.structuredContent) : "";
}

export class ModelContext {
  private readonly pending = new Map<string, { app: AppRef; text: string }>();

  constructor(private readonly deps: ModelContextDeps) {}

  /** `ui/message`: posted now. */
  async message(text: string): Promise<void> {
    if (text.trim()) await this.deps.post(text);
  }

  /** `ui/update-model-context`: replaces the app's earlier context. */
  async update(app: AppRef, text: string): Promise<void> {
    if (!text.trim()) {
      this.pending.delete(app.callId);
      return;
    }
    this.pending.set(app.callId, { app, text });
    await this.flush();
  }

  /** The session went idle: post what waiting apps reported meanwhile. */
  async idle(): Promise<void> {
    await this.flush();
  }

  /** The user's `text`, after the contexts still pending (which it takes). */
  take(text: string): string {
    const contexts = [...this.pending.values()].map((p) => contextMessage(p.app, p.text));
    this.pending.clear();
    return contexts.length > 0 ? `${contexts.join("\n")}\n\n${text}` : text;
  }

  /** The contexts not yet sent, by call id (for the UI). */
  pendingCalls(): string[] {
    return [...this.pending.keys()];
  }

  private async flush(): Promise<void> {
    if (!this.deps.idle()) return;
    const ready = [...this.pending.values()].filter((p) => p.app.awaitingUser);
    if (ready.length === 0) return;
    for (const p of ready) this.pending.delete(p.app.callId);
    await this.deps.post(ready.map((p) => contextMessage(p.app, p.text)).join("\n"));
  }
}
