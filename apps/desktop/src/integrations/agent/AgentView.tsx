// An agent's session on an instance's Omnigent, over the shell's session commands:
// the Agent app's chat with the `lakehouse` agent (lakehouse.tsx), or one session the
// factory started (its control room's "Open session"). Omnigent runs the session; this
// renders it, with chatcn's components. A tool call that has an MCP App (correlate.ts)
// renders as the app, on the desktop's own MCP connection to the server it came from
// (`servers`, AgentAppPart), and what the app reports goes back into the session
// (context.ts).
//
// Without `session`, the chat's session stays the instance's while the shell runs (and
// over a reload, in sessionStorage); "New chat" starts another. With it, the view
// follows that one session.

import { Button } from "@open-lakehouse/ui-kit";
import { Bot, MessageSquarePlus, Puzzle, RotateCw } from "lucide-react";
import {
  type ComponentType,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import { Conversation, ConversationContent } from "@/components/ui/chatcn/conversation";
import type { ChatcnToolPart } from "@/components/ui/chatcn/lib/chatcn-types";
import { Markdown } from "@/components/ui/chatcn/markdown";
import { Message, MessageContent } from "@/components/ui/chatcn/message";
import {
  PromptInput,
  PromptInputActions,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputToolbar,
} from "@/components/ui/chatcn/prompt-input";
import { ToolCall, toolCallPropsFromPart } from "@/components/ui/chatcn/tool-call";
import {
  type AgentStatus,
  type ChatMessage,
  type ChatPart,
  type SessionMessage,
  useShellHost,
} from "../../shell/shell-host";
import { message as errorMessage, Notice } from "../../shell/WorkspaceBar";
import {
  closeMcpConnections,
  type McpAppServer,
  type McpConnection,
  mcpConnection,
} from "../mcp/connection";
import type { McpAppProps } from "../mcp/McpApp";
import { AgentAppPart, type AppCache, type ToolPart } from "./AgentAppPart";
import { ModelContext, parseContext, splitContext } from "./context";
import { type ConnectedServer, matchApp, parseToolName } from "./correlate";
import { busy, EMPTY_SESSION, reduceSession } from "./session";

const sessionKey = (instance: string, agent: string) =>
  agent === "lakehouse" ? `agent-session:${instance}` : `agent-session:${instance}:${agent}`;

/** Opens the instance's connection to an MCP server (tests pass a fake). */
export type OpenMcp = (environment: string, server: McpAppServer) => Promise<McpConnection>;

export interface AgentViewProps {
  instance: string;
  /** `dev #3` */
  label: string;
  /** The agent's name on Omnigent: what `agent_status` looks for and a new chat starts. */
  agent: string;
  /** The MCP servers whose tools may render as apps, by the name the agent knows each by. */
  servers: readonly McpAppServer[];
  /** Follow this session only: no new chats, and the agent needn't be installed. */
  session?: string;
  /** The header's title. */
  heading?: string;
  /** Says how to install the agent, when Omnigent doesn't have it. */
  notInstalled?: React.ReactNode;
  /** What an empty chat suggests asking. */
  placeholder?: React.ReactNode;
  mcp?: OpenMcp;
  /** Renders an MCP App (tests pass a stand-in for the sandboxed frame). */
  App?: ComponentType<McpAppProps>;
}

export function AgentView({
  instance,
  label,
  agent,
  servers,
  session,
  heading = "Agent",
  notInstalled,
  placeholder,
  mcp = mcpConnection,
  App,
}: AgentViewProps) {
  const shell = useShellHost();
  const [status, setStatus] = useState<AgentStatus | { failed: string }>();
  const [check, setCheck] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `check` is the retry.
  useEffect(() => {
    let stale = false;
    setStatus(undefined);
    shell.agent.status(instance, agent).then(
      (s) => !stale && setStatus(s),
      (e: unknown) => !stale && setStatus({ failed: errorMessage(e) }),
    );
    return () => {
      stale = true;
    };
  }, [shell, instance, agent, check]);

  // One MCP connection: this instance's. Switching instances closes the others'.
  useEffect(() => {
    void closeMcpConnections(instance);
  }, [instance]);

  const retry = (
    <Button size="sm" variant="outline" onClick={() => setCheck((n) => n + 1)}>
      <RotateCw /> Check again
    </Button>
  );
  if (!status) return <Notice>Asking {label}'s Omnigent for the agent…</Notice>;
  const chat = (
    <AgentChat
      instance={instance}
      label={label}
      agent={agent}
      servers={servers}
      session={session}
      heading={heading}
      placeholder={placeholder}
      mcp={mcp}
      App={App}
    />
  );
  if ("failed" in status || !status.reachable) {
    const why = "failed" in status ? status.failed : status.error;
    return (
      <AgentNotice title="Omnigent isn't answering" action={retry} testId="agent-unreachable">
        {label}'s Omnigent couldn't be reached{why ? `: ${why}` : "."}
      </AgentNotice>
    );
  }
  // An existing session reads without its agent installed here, or a host online.
  if (session) return chat;
  if (!status.agentId) {
    return (
      <AgentNotice title="The agent isn't installed" action={retry} testId="agent-not-installed">
        {label}'s Omnigent has no <code>{agent}</code> agent.{notInstalled ? " " : ""}
        {notInstalled}
      </AgentNotice>
    );
  }
  if (!status.hostOnline) {
    return (
      <AgentNotice title="No host is online" action={retry} testId="agent-no-host">
        No Omnigent host is connected to {label}'s Omnigent to run the agent. The environment's{" "}
        <code>omnigent-host</code> container connects to the server through the gateway; check that
        it runs, and its logs if the gateway refused it (Omnigent's auth must be off locally).
      </AgentNotice>
    );
  }
  return chat;
}

function AgentNotice({
  title,
  action,
  testId,
  children,
}: {
  title: string;
  action?: React.ReactNode;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
      data-testid={testId}
    >
      <Bot className="size-8 text-muted-foreground" />
      <h2 className="text-lg font-semibold">{title}</h2>
      <p className="max-w-md text-sm text-muted-foreground">{children}</p>
      {action}
    </div>
  );
}

function AgentChat({
  instance,
  label,
  agent: agentName,
  servers,
  session: fixed,
  heading,
  placeholder,
  mcp,
  App,
}: Required<Pick<AgentViewProps, "agent" | "servers" | "heading" | "mcp">> &
  Pick<AgentViewProps, "instance" | "label" | "session" | "placeholder" | "App">) {
  const shell = useShellHost();
  const agent = shell.agent;
  const key = sessionKey(instance, agentName);
  const [sessionId, setSessionId] = useState<string | null>(
    () => fixed ?? sessionStorage.getItem(key) ?? null,
  );
  const [session, dispatch] = useReducer(reduceSession, EMPTY_SESSION);
  const [problem, setProblem] = useState<string>();
  const [conns, setConns] = useState<ReadonlyMap<string, ConnectedServer>>(new Map());
  const cache = useRef<AppCache>(new Map());
  const statusRef = useRef(session.status);
  statusRef.current = session.status;
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;

  // Each server's tools, to tell an app's call from a plain one. A server that doesn't
  // answer has no apps: its calls are plain cards.
  const serverKey = servers.map((s) => `${s.name}=${s.module}${s.url}`).join("\n");
  // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on the servers' identity.
  useEffect(() => {
    let stale = false;
    setConns(new Map());
    for (const server of servers) {
      mcp(instance, server)
        .then(async (c) => ({ conn: c, tools: await c.tools() }))
        .then(
          (loaded) => !stale && setConns((prev) => new Map(prev).set(server.name, loaded)),
          () => {},
        );
    }
    return () => {
      stale = true;
    };
  }, [mcp, instance, serverKey]);

  const context = useMemo(
    () =>
      new ModelContext({
        post: async (text) => {
          const id = sessionRef.current;
          if (id) await agent.send(instance, id, text);
        },
        idle: () => !busy(statusRef.current),
      }),
    [agent, instance],
  );

  // Follow a session, from a snapshot: one at a time, the latest asked for.
  const subscription = useRef<{ id: string; stop: () => void }>(undefined);
  const follow = useCallback(
    async (id: string) => {
      subscription.current?.stop();
      const current = { id, stop: () => {} };
      subscription.current = current;
      dispatch({ kind: "snapshot", status: null, messages: [], elicitations: [] });
      const unsubscribe = await agent.subscribe(instance, id, (m: SessionMessage) => {
        if (subscription.current === current) dispatch(m);
      });
      if (subscription.current === current) current.stop = unsubscribe;
      else unsubscribe();
    },
    [agent, instance],
  );
  useEffect(() => {
    const restored = sessionRef.current;
    if (restored) {
      follow(restored).catch((e: unknown) => {
        setProblem(errorMessage(e));
        // A chat's session the instance's Omnigent no longer has (another revision):
        // start over. A given session stays what the view shows.
        if (fixed) return;
        sessionStorage.removeItem(key);
        setSessionId(null);
      });
    }
    return () => {
      subscription.current?.stop();
      subscription.current = undefined;
    };
  }, [follow, fixed, key]);

  // A waiting app's outcome goes in once the turn is over.
  useEffect(() => {
    if (session.loaded && !busy(session.status)) void context.idle();
  }, [context, session.loaded, session.status]);

  const start = useCallback(async () => {
    const created = await agent.create(instance, agentName, null);
    sessionStorage.setItem(key, created.id);
    cache.current = new Map();
    sessionRef.current = created.id;
    setSessionId(created.id);
    // Subscribed before the first message: the stream has no replay.
    await follow(created.id);
    return created.id;
  }, [agent, instance, agentName, key, follow]);

  const newChat = () => {
    setProblem(undefined);
    start().catch((e: unknown) => setProblem(errorMessage(e)));
  };

  const send = async (text: string) => {
    if (!text.trim()) return;
    setProblem(undefined);
    try {
      const id = sessionRef.current ?? (await start());
      await agent.send(instance, id, context.take(text));
    } catch (e) {
      setProblem(errorMessage(e));
    }
  };

  const running = busy(session.status);
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="agent-view">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3 text-sm">
        <Bot className="size-4 text-muted-foreground" />
        <span className="font-medium">{heading}</span>
        <span className="text-muted-foreground">on {label}</span>
        {session.reconnecting && (
          <span className="text-xs text-muted-foreground" data-testid="agent-reconnecting">
            Reconnecting…
          </span>
        )}
        {fixed ? null : (
          <Button size="sm" variant="ghost" className="ml-auto" onClick={newChat}>
            <MessageSquarePlus /> New chat
          </Button>
        )}
      </div>
      {problem && (
        <p className="border-b px-3 py-1.5 text-sm text-destructive" role="alert">
          {problem}
        </p>
      )}
      <Conversation className="min-h-0 flex-1">
        <ConversationContent label="Conversation with the agent" className="mx-auto max-w-3xl">
          {session.messages.length === 0 ? (
            <p className="py-8 text-center text-sm text-muted-foreground">
              {fixed ? "Nothing in this session yet." : (placeholder ?? "Ask the agent.")}
            </p>
          ) : (
            session.messages.map((m, i) => (
              <ChatMessageView
                key={m.id}
                message={m}
                latest={i === session.messages.length - 1}
                streaming={running && i === session.messages.length - 1}
                renderTool={(part) => {
                  const call = matchApp(part.toolName, conns);
                  if (!call) return <PlainTool part={part} />;
                  return (
                    <AgentAppPart
                      key={part.toolCallId}
                      part={part}
                      call={call}
                      conn={call.conn}
                      cache={cache.current}
                      live={session.live.has(part.toolCallId)}
                      context={context}
                      openExternal={(url) => shell.openExternal(url)}
                      App={App}
                    />
                  );
                }}
              />
            ))
          )}
        </ConversationContent>
      </Conversation>
      <div className="mx-auto w-full max-w-3xl shrink-0 p-3">
        <PromptInput
          status={running ? "streaming" : "idle"}
          onSubmit={({ text }) => void send(text)}
          onStop={() => {
            if (sessionId) {
              agent
                .interrupt(instance, sessionId)
                .catch((e: unknown) => setProblem(errorMessage(e)));
            }
          }}
        >
          <PromptInputTextarea placeholder="Ask the agent…" aria-label="Message the agent" />
          <PromptInputToolbar>
            <PromptInputActions>
              <PromptInputSubmit />
            </PromptInputActions>
          </PromptInputToolbar>
        </PromptInput>
      </div>
    </div>
  );
}

function PlainTool({ part }: { part: ToolPart }) {
  return (
    <ToolCall
      {...toolCallPropsFromPart(part as unknown as ChatcnToolPart)}
      name={parseToolName(part.toolName).tool}
      variant="inline"
    />
  );
}

function ChatMessageView({
  message,
  latest,
  streaming,
  renderTool,
}: {
  message: ChatMessage;
  latest: boolean;
  streaming: boolean;
  renderTool: (part: ToolPart) => React.ReactNode;
}) {
  if (message.role === "user") {
    const text = message.parts.map((p) => (p.type === "text" ? p.text : "")).join("");
    const { contexts, rest } = splitContext(text);
    return (
      <div className="flex flex-col items-end gap-1" data-testid="agent-user-message">
        {contexts.map((c) => (
          <ContextChip key={c} element={c} />
        ))}
        {rest.trim() && (
          <Message from="user" latest={latest}>
            <MessageContent>{rest}</MessageContent>
          </Message>
        )}
      </div>
    );
  }
  return (
    <Message
      from={message.role === "system" ? "system" : "assistant"}
      latest={latest}
      state={streaming ? "streaming" : "done"}
    >
      <div className="flex flex-col gap-2">
        {message.parts.map((part, i) => (
          <PartView
            key={part.type === "dynamic-tool" ? part.toolCallId : `${message.id}-${i}`}
            part={part}
            renderTool={renderTool}
          />
        ))}
      </div>
    </Message>
  );
}

function PartView({
  part,
  renderTool,
}: {
  part: ChatPart;
  renderTool: (part: ToolPart) => React.ReactNode;
}) {
  switch (part.type) {
    case "text":
      return (
        <MessageContent>
          <Markdown streaming={part.state === "streaming"}>{part.text}</Markdown>
        </MessageContent>
      );
    case "reasoning":
      return (
        <details className="text-sm text-muted-foreground">
          <summary className="cursor-pointer select-none">Thinking</summary>
          <p className="whitespace-pre-wrap">{part.text}</p>
        </details>
      );
    case "dynamic-tool":
      return <>{renderTool(part)}</>;
  }
}

/** An app's report to the agent, as a chip: the user didn't write it. */
function ContextChip({ element }: { element: string }) {
  const parsed = parseContext(element);
  return (
    <div
      className="flex max-w-[80%] items-start gap-1.5 rounded-full border px-3 py-1 text-xs text-muted-foreground"
      data-testid="agent-context-chip"
      title={parsed?.text}
    >
      <Puzzle className="mt-0.5 size-3 shrink-0" />
      <span className="truncate">
        {parsed?.tool ? `${parsed.tool}: ` : ""}
        {parsed?.text ?? element}
      </span>
    </div>
  );
}
