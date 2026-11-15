// The query engine, faked for tests and the Playwright build: a fixed answer per
// query, framed as src-tauri frames its chunks (row count, then Arrow IPC).

import { tableFromArrays, tableToIPC } from "apache-arrow";
import type { QueryError } from "./bindings";
import type { QueryMessage, QueryTransport } from "./query";

/** What a faked query answers: its rows, chunk by chunk (one column each, by name), or
 *  the error it fails with. */
export type FakeAnswer = { chunks: Record<string, unknown[]>[] } | { error: QueryError };

/** Three people, in one chunk; any SQL mentioning `nope` names no table. */
export function fixtureAnswer(_instance: string | null, sql: string): FakeAnswer {
  if (/\bnope\b/i.test(sql)) {
    return { error: { kind: "invalid", message: "table 'nope' not found" } };
  }
  return {
    chunks: [{ id: [1, 2, 3], name: ["ada", "grace", "edsger"] }],
  };
}

/** One chunk as src-tauri sends it. */
export function frameChunk(columns: Record<string, unknown[]>): ArrayBuffer {
  // biome-ignore lint/suspicious/noExplicitAny: fixtures name their own columns
  const table = tableFromArrays(columns as any);
  const ipc = tableToIPC(table, "stream");
  const frame = new ArrayBuffer(8 + ipc.byteLength);
  new DataView(frame).setBigUint64(0, BigInt(table.numRows), true);
  new Uint8Array(frame, 8).set(ipc);
  return frame;
}

/** A transport answering each query with `answer`, its rows a tick later. */
export function fakeQueryTransport(
  answer: (instance: string | null, sql: string) => FakeAnswer = fixtureAnswer,
): QueryTransport {
  return {
    async run(_queryId, instance, request, onMessage: (m: QueryMessage) => void) {
      const result = answer(instance, request.sql);
      if ("error" in result && result.error.kind !== "failed") {
        return { status: "error", error: result.error };
      }
      setTimeout(() => {
        if ("error" in result) {
          onMessage({ error: result.error });
          return;
        }
        for (const chunk of result.chunks) onMessage(frameChunk(chunk));
        onMessage("end");
      }, 0);
      return { status: "ok" };
    },
    cancel() {},
  };
}
