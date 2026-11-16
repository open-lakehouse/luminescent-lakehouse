// A session as the Agent app holds it: the conversation from the session's channel
// (`session_subscribe`), and which tool calls it saw happen. A call that first shows up
// in a snapshot is history (the conversation as it was when the app opened it, or
// after a reconnect); one that first shows up in an upsert happened while the app
// watched. Only those open their MCP App by themselves: reopening the app never runs
// anything again.

import type { ChatMessage, SessionMessage } from "../../shell/shell-host";

export interface SessionState {
  /** `idle`, `launching`, `running`, `waiting`, `failed`; `null` before the snapshot. */
  status: string | null;
  messages: ChatMessage[];
  /** The tool calls seen first in an upsert. */
  live: ReadonlySet<string>;
  /** The stream dropped and is reconnecting. */
  reconnecting?: { attempt: number; error?: string | null };
  /** A snapshot has arrived. */
  loaded: boolean;
}

export const EMPTY_SESSION: SessionState = {
  status: null,
  messages: [],
  live: new Set(),
  loaded: false,
};

const toolCalls = (message: ChatMessage) =>
  message.parts.flatMap((p) => (p.type === "dynamic-tool" ? [p.toolCallId] : []));

export function reduceSession(state: SessionState, message: SessionMessage): SessionState {
  switch (message.kind) {
    case "snapshot":
      return {
        status: message.status ?? state.status,
        messages: message.messages,
        live: state.live,
        loaded: true,
      };
    case "upsert": {
      const seen = new Set(state.messages.flatMap(toolCalls));
      const fresh = toolCalls(message.message).filter((id) => !seen.has(id));
      const at = state.messages.findIndex((m) => m.id === message.message.id);
      const messages =
        at >= 0
          ? state.messages.map((m, i) => (i === at ? message.message : m))
          : [...state.messages, message.message];
      // Before the snapshot, the stream's events are already part of it.
      const live =
        fresh.length > 0 && state.loaded ? new Set([...state.live, ...fresh]) : state.live;
      return { ...state, messages, live };
    }
    case "status":
      return { ...state, status: message.status };
    case "reconnecting":
      return { ...state, reconnecting: { attempt: message.attempt, error: message.error } };
    case "elicitations":
      return state;
  }
}

/** Whether a turn is under way. */
export const busy = (status: string | null) =>
  status === "running" || status === "launching" || status === "waiting";
