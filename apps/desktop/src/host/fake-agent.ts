// A scripted Omnigent for tests and the Playwright build: the session commands of
// `AgentHost`, in memory. A session answers each message with the script's reply, on
// the channel the way the shell's stream folds a turn: the user's message, `running`,
// the reply's messages, `idle`.

import type {
  AgentHost,
  AgentStatus,
  ChatMessage,
  ChatPart,
  SessionMessage,
} from "../shell/shell-host";

/** How the recorded grant turn names the tool (crates/omnigent-client/fixtures/grant-turn.sse):
 *  `<server>__<tool>`, the server as the agent's YAML names it. */
export const GRANT_TOOL_NAME = "uc__open_grant_form";
export const GRANT_INPUT = {
  securable_type: "table",
  full_name: "main.default.t",
  principal: "x",
  privileges: ["SELECT"],
};
export const GRANT_OUTPUT =
  "Opened a grant form for SELECT on table main.default.t to x. Nothing is granted until the user reviews the form and submits it; the outcome is reported back.";

/** The agent's answer to `text`: its messages, in order. */
export type FakeReply = (text: string, turn: number) => ChatMessage[];

const text = (t: string): ChatPart => ({ type: "text", text: t, state: "done" });

/** The demo: a grant request opens the form, a form outcome is confirmed, the rest is
 *  echoed. */
export const grantScript: FakeReply = (message, turn) => {
  const id = `a${turn}`;
  if (message.trimStart().startsWith("<mcp-app-context")) {
    return [
      {
        id,
        role: "assistant",
        parts: [text("Done: x now holds SELECT on main.default.t.")],
      },
    ];
  }
  if (/\bgrant\b/i.test(message)) {
    return [
      {
        id,
        role: "assistant",
        parts: [
          text("I'll open the grant form."),
          {
            type: "dynamic-tool",
            toolCallId: `call_${turn}`,
            toolName: GRANT_TOOL_NAME,
            state: "output-available",
            input: GRANT_INPUT,
            output: GRANT_OUTPUT,
            errorText: null,
            approval: null,
          },
          text("The form is open: review it and submit."),
        ],
      },
    ];
  }
  return [{ id, role: "assistant", parts: [text(`You said: ${message}`)] }];
};

/** The demo plan's sessions (factory-core's `demo::seed`, factory-client's `seedDemo`),
 *  as an Omnigent the factory ran them on would have them. */
export const FACTORY_SESSIONS: Record<string, ChatMessage[]> = {
  "sess-demo-1": [
    {
      id: "u0",
      role: "user",
      parts: [text("Implement the work graph's typed backend on olai-store.")],
    },
    { id: "a0", role: "assistant", parts: [text("Opened #101 and #102 as a stack.")] },
  ],
  "sess-demo-2": [
    { id: "u0", role: "user", parts: [text("Review the stack #101, #102.")] },
    {
      id: "a0",
      role: "assistant",
      parts: [text("Reviewed: one nit on the conformance suite's naming.")],
    },
  ],
};

interface FakeSession {
  id: string;
  status: string;
  messages: ChatMessage[];
  listeners: Set<(m: SessionMessage) => void>;
}

export interface FakeAgent extends AgentHost {
  /** Every message sent, in order. */
  sent: { sessionId: string; text: string }[];
  /** The agents `status` and `create` were asked for, in order. */
  agents: string[];
  /** The sessions interrupted, in order. */
  interrupts: string[];
  sessions: Map<string, FakeSession>;
  /** Send `message` on the session's channel, as the stream would. */
  emit(sessionId: string, message: SessionMessage): void;
  /** Make `status` answer this from now on. */
  setStatus(status: AgentStatus | Error): void;
}

export const READY_AGENT: AgentStatus = {
  reachable: true,
  agentId: "agent_lakehouse",
  hostOnline: true,
  error: null,
};

export function createFakeAgent(
  options: {
    status?: AgentStatus | Error;
    reply?: FakeReply;
    /** Between the reply's steps, in ms (0: the next tick). */
    delay?: number;
    /** Sessions there already (a factory run's), by id: their history. */
    sessions?: Record<string, ChatMessage[]>;
  } = {},
): FakeAgent {
  let status = options.status ?? READY_AGENT;
  const reply = options.reply ?? grantScript;
  const delay = options.delay ?? 0;
  const sessions = new Map<string, FakeSession>(
    Object.entries(options.sessions ?? {}).map(([id, messages]) => [
      id,
      { id, status: "idle", messages: structuredClone(messages), listeners: new Set() },
    ]),
  );
  let turns = 0;
  let created = 0;
  const later = (f: () => void) => setTimeout(f, delay);
  const session = (id: string) => {
    const s = sessions.get(id);
    if (!s) throw new Error(`no session ${id}`);
    return s;
  };
  const emit = (sessionId: string, message: SessionMessage) => {
    const s = session(sessionId);
    if (message.kind === "upsert") {
      const at = s.messages.findIndex((m) => m.id === message.message.id);
      if (at >= 0) s.messages[at] = message.message;
      else s.messages.push(message.message);
    }
    if (message.kind === "status") s.status = message.status;
    for (const l of [...s.listeners]) l(structuredClone(message));
  };
  const fake: FakeAgent = {
    sent: [],
    agents: [],
    interrupts: [],
    sessions,
    emit,
    setStatus: (next) => {
      status = next;
    },
    status: async (_instance, agent) => {
      fake.agents.push(agent);
      if (status instanceof Error) throw status;
      return status;
    },
    create: async (_instance, agent, title) => {
      fake.agents.push(agent);
      created += 1;
      const id = `sess_${created}`;
      sessions.set(id, { id, status: "idle", messages: [], listeners: new Set() });
      return { id, agentId: `agent_${agent}`, status: "idle", title: title ?? null };
    },
    subscribe: async (_instance, sessionId, onMessage) => {
      const s = session(sessionId);
      s.listeners.add(onMessage);
      const snapshot: SessionMessage = {
        kind: "snapshot",
        status: s.status,
        messages: structuredClone(s.messages),
        elicitations: [],
      };
      // As the shell's: the snapshot is in before the subscription resolves.
      onMessage(snapshot);
      return () => s.listeners.delete(onMessage);
    },
    send: async (_instance, sessionId, message) => {
      session(sessionId);
      fake.sent.push({ sessionId, text: message });
      turns += 1;
      const turn = turns;
      emit(sessionId, {
        kind: "upsert",
        message: { id: `u${turn}`, role: "user", parts: [text(message)] },
      });
      emit(sessionId, { kind: "status", status: "running" });
      later(() => {
        for (const m of reply(message, turn)) emit(sessionId, { kind: "upsert", message: m });
        later(() => emit(sessionId, { kind: "status", status: "idle" }));
      });
      return { queued: false, observed: true };
    },
    interrupt: async (_instance, sessionId) => {
      fake.interrupts.push(sessionId);
      emit(sessionId, { kind: "status", status: "idle" });
    },
  };
  return fake;
}
