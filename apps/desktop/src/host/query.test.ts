import { describe, expect, test } from "bun:test";
import type { QueryChunk, QueryRequest } from "@open-lakehouse/query-runner";
import type { QueryError } from "./bindings";
import {
  decodeChunk,
  hostQueryRunner,
  QueryFailedError,
  type QueryMessage,
  type QueryTransport,
} from "./query";

/** A chunk as src-tauri frames it: the row count, then the Arrow IPC bytes. */
function frame(numRows: number, ipc: number[]): ArrayBuffer {
  const buf = new ArrayBuffer(8 + ipc.length);
  new DataView(buf).setBigUint64(0, BigInt(numRows), true);
  new Uint8Array(buf, 8).set(ipc);
  return buf;
}

interface Run {
  queryId: number;
  instance: string | null;
  request: QueryRequest;
  send: (message: QueryMessage) => void;
}

/** A transport that records its runs and cancels; `plan` decides how each starts. */
function fakeTransport(plan: (run: Run) => QueryError | null = () => null) {
  const runs: Run[] = [];
  const cancelled: number[] = [];
  const transport: QueryTransport = {
    async run(queryId, instance, request, onMessage) {
      const run = { queryId, instance, request, send: onMessage };
      runs.push(run);
      const error = plan(run);
      return error ? { status: "error", error } : { status: "ok" };
    },
    cancel(queryId) {
      cancelled.push(queryId);
    },
  };
  return { transport, runs, cancelled };
}

async function collect(iterable: AsyncIterable<QueryChunk>): Promise<QueryChunk[]> {
  const out: QueryChunk[] = [];
  for await (const chunk of iterable) out.push(chunk);
  return out;
}

const never = () => new AbortController().signal;

describe("decodeChunk", () => {
  test("splits the row count from the Arrow IPC bytes", () => {
    const chunk = decodeChunk(frame(42, [0xff, 0xff, 0xff, 0xff, 1]));
    expect(chunk.numRows).toBe(42);
    expect([...chunk.arrowIpc]).toEqual([0xff, 0xff, 0xff, 0xff, 1]);
  });
});

describe("hostQueryRunner", () => {
  test("streams the chunks of the named instance until the end", async () => {
    const { transport, runs, cancelled } = fakeTransport((run) => {
      setTimeout(() => {
        run.send(frame(2, [1]));
        run.send(frame(1, [2]));
        run.send("end");
      }, 0);
      return null;
    });
    const chunks = await collect(
      hostQueryRunner("inst-1", transport)({ sql: "SELECT 1", limit: 5 }, { signal: never() }),
    );
    expect(chunks.map((c) => c.numRows)).toEqual([2, 1]);
    expect(runs[0]?.instance).toBe("inst-1");
    expect(runs[0]?.request).toEqual({ sql: "SELECT 1", limit: 5 });
    // It ended by itself: nothing to cancel.
    expect(cancelled).toEqual([]);
  });

  test("refuses a query that doesn't plan", async () => {
    const { transport } = fakeTransport(() => ({ kind: "invalid", message: "syntax error" }));
    const run = collect(hostQueryRunner(null, transport)({ sql: "SELEC 1" }, { signal: never() }));
    await expect(run).rejects.toBeInstanceOf(QueryFailedError);
    await expect(run).rejects.toThrow("syntax error");
  });

  test("fails with the error the run ended on", async () => {
    const { transport } = fakeTransport((run) => {
      setTimeout(() => {
        run.send(frame(1, [1]));
        run.send({ error: { kind: "forbidden", message: "no EXTERNAL USE" } });
      }, 0);
      return null;
    });
    const seen: QueryChunk[] = [];
    let error: unknown;
    try {
      for await (const chunk of hostQueryRunner(null, transport)(
        { sql: "SELECT * FROM t" },
        { signal: never() },
      )) {
        seen.push(chunk);
      }
    } catch (e) {
      error = e;
    }
    expect(seen.length).toBe(1);
    expect(error).toBeInstanceOf(QueryFailedError);
    expect((error as QueryFailedError).error.kind).toBe("forbidden");
    expect((error as Error).message).toContain("EXTERNAL_USE_SCHEMA");
  });

  test("an abort stops the run in the host", async () => {
    const { transport, runs, cancelled } = fakeTransport((run) => {
      setTimeout(() => run.send(frame(1, [1])), 0);
      return null;
    });
    const controller = new AbortController();
    const chunks: QueryChunk[] = [];
    const run = (async () => {
      for await (const chunk of hostQueryRunner(null, transport)(
        { sql: "SELECT * FROM big" },
        { signal: controller.signal },
      )) {
        chunks.push(chunk);
        controller.abort(new DOMException("stopped", "AbortError"));
      }
    })();
    await expect(run).rejects.toThrow("stopped");
    expect(chunks.length).toBe(1);
    expect(cancelled).toEqual([runs[0]?.queryId as number]);
  });

  test("letting go of the rows stops the run too", async () => {
    const { transport, runs, cancelled } = fakeTransport((run) => {
      setTimeout(() => {
        run.send(frame(1, [1]));
        run.send(frame(1, [2]));
      }, 0);
      return null;
    });
    for await (const _ of hostQueryRunner(null, transport)(
      { sql: "SELECT 1" },
      { signal: never() },
    )) {
      break;
    }
    expect(cancelled).toEqual([runs[0]?.queryId as number]);
  });

  test("an aborted signal runs nothing", async () => {
    const { transport, runs } = fakeTransport();
    const controller = new AbortController();
    controller.abort(new DOMException("gone", "AbortError"));
    const run = collect(
      hostQueryRunner(null, transport)({ sql: "SELECT 1" }, { signal: controller.signal }),
    );
    await expect(run).rejects.toThrow("gone");
    expect(runs.length).toBe(0);
  });
});
