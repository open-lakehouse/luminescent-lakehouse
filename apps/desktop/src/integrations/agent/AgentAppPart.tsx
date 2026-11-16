// A tool call of the agent's that has an MCP App (correlate.ts), rendered as the app.
// Omnigent gave the agent only text, so the app gets its full result from the desktop's
// own connection: the tool is read-only, and is run again with the agent's arguments.
// The app's own calls (`apply_grants`) go through that connection too, and what it
// tells the agent goes back into the session (context.ts).
//
// A call from the session's history doesn't run again by itself: it shows collapsed,
// and opens on "Open form again".

import type { CallToolResult } from "@modelcontextprotocol/client";
import { Button } from "@open-lakehouse/ui-kit";
import { type ComponentType, lazy, Suspense, useEffect, useState } from "react";
import type { ChatcnToolPart } from "@/components/ui/chatcn/lib/chatcn-types";
import { ToolCall, toolCallPropsFromPart } from "@/components/ui/chatcn/tool-call";
import type { ChatPart } from "../../shell/shell-host";
import type { McpAppResource, McpConnection } from "../mcp/connection";
import type { McpAppProps } from "../mcp/McpApp";
import type { AppRef } from "./context";
import { contentText, type ModelContext } from "./context";
import type { AppCall } from "./correlate";

// The sandboxed frame and the MCP Apps bridge load with the first app.
const McpApp = lazy(() => import("../mcp/McpApp").then((m) => ({ default: m.McpApp })));

export type ToolPart = Extract<ChatPart, { type: "dynamic-tool" }>;

/** An app's View and the result it renders. */
export interface LoadedApp {
  resource: McpAppResource;
  result: CallToolResult;
}

/** One load per call while the view is open: a re-render, or the conversation
 *  scrolling the part out and back, doesn't run the tool again. */
export type AppCache = Map<string, Promise<LoadedApp>>;

export function loadApp(
  cache: AppCache,
  conn: McpConnection,
  part: ToolPart,
  call: AppCall,
): Promise<LoadedApp> {
  let loaded = cache.get(part.toolCallId);
  if (!loaded) {
    const args =
      part.input && typeof part.input === "object" && !Array.isArray(part.input)
        ? (part.input as Record<string, unknown>)
        : {};
    loaded = Promise.all([
      conn.callTool(call.tool.name, args),
      conn.readApp(call.resourceUri),
    ]).then(([result, resource]) => ({ result, resource }));
    cache.set(part.toolCallId, loaded);
    loaded.catch(() => cache.delete(part.toolCallId));
  }
  return loaded;
}

/** Whether a result waits on the user (`open_grant_form`'s `status: "awaiting_user"`). */
export const awaitsUser = (result: CallToolResult) =>
  (result.structuredContent as { status?: unknown } | undefined)?.status === "awaiting_user";

export function AgentAppPart({
  part,
  call,
  conn,
  cache,
  live,
  context,
  openExternal,
  App = McpApp,
}: {
  part: ToolPart;
  call: AppCall;
  conn: McpConnection;
  cache: AppCache;
  /** The call happened while the view watched: open the app at once. */
  live: boolean;
  context: ModelContext;
  openExternal: (url: string) => Promise<void>;
  /** Renders the app (tests pass a stand-in for the sandboxed frame). */
  App?: ComponentType<McpAppProps>;
}) {
  const [open, setOpen] = useState(live);
  const [loaded, setLoaded] = useState<LoadedApp>();
  const [error, setError] = useState<string>();
  const [attempt, setAttempt] = useState(0);
  // The agent's call has to have finished: until then the server has nothing to show.
  const done = part.state === "output-available";

  // biome-ignore lint/correctness/useExhaustiveDependencies: `attempt` is the retry.
  useEffect(() => {
    if (!open || !done) return;
    let stale = false;
    setError(undefined);
    loadApp(cache, conn, part, call).then(
      (app) => !stale && setLoaded(app),
      (e: unknown) => !stale && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      stale = true;
    };
  }, [open, done, cache, conn, part.toolCallId, call, attempt]);

  const card = (
    <ToolCall
      {...toolCallPropsFromPart(part as unknown as ChatcnToolPart)}
      name={call.tool.name}
      variant="inline"
    />
  );
  if (!done) return card;
  if (!open) {
    return (
      <div className="flex flex-col gap-1" data-testid={`agent-app-${part.toolCallId}`}>
        {card}
        <div>
          <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
            Open form again
          </Button>
        </div>
      </div>
    );
  }
  const app: AppRef = {
    tool: call.tool.name,
    callId: part.toolCallId,
    awaitingUser: loaded ? awaitsUser(loaded.result) : false,
  };
  return (
    <div data-testid={`agent-app-${part.toolCallId}`}>
      <Suspense fallback={card}>
        <App
          resource={loaded?.resource}
          toolName={call.tool.name}
          toolInput={part.input as Record<string, unknown> | undefined}
          toolResult={loaded?.result}
          error={error}
          onRetry={() => setAttempt((n) => n + 1)}
          onCallTool={(p) => conn.callTool(p.name, p.arguments, { from: "app" })}
          onOpenLink={openExternal}
          onMessage={(text) => context.message(text)}
          onUpdateModelContext={(params) => void context.update(app, contentText(params))}
        />
      </Suspense>
    </div>
  );
}
