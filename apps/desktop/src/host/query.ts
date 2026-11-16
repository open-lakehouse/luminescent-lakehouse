// The desktop's `QueryRunner` (@open-lakehouse/query-runner): SQL runs in the
// host, in hydrofoil's engine against an instance's Unity Catalog
// (src-tauri/src/query.rs). The rows stream back over a Tauri `Channel`, one
// chunk per record batch: its row count (8 bytes, little endian), then its Arrow
// IPC stream.

import type { QueryChunk, QueryRequest, QueryRunner } from "@open-lakehouse/query-runner";
import { Channel } from "@tauri-apps/api/core";
import { commands, type QueryError } from "./bindings";

/**
 * One message on a query's channel: a chunk as an `ArrayBuffer`, the end as
 * `"end"`, a failure mid-run as `{ error }` (see `QueryMessage` in
 * src-tauri/src/query.rs).
 */
export type QueryMessage = ArrayBuffer | "end" | { error: QueryError };

/** A query the host refused or that failed while it ran. */
export class QueryFailedError extends Error {
  readonly error: QueryError;
  constructor(error: QueryError) {
    super(queryErrorMessage(error));
    this.name = "QueryFailedError";
    this.error = error;
  }
}

/** What to tell the user about `error`. */
export function queryErrorMessage(error: QueryError): string {
  switch (error.kind) {
    case "noEnvironment":
      return "No environment is running. Start one to query it.";
    case "noCatalog":
      return "This environment has no Unity Catalog to query.";
    case "unavailable":
      return "This build of the app has no query engine. Build it with the `query` feature (`just desktop-dev query`).";
    case "unauthenticated":
      return `Unity Catalog refused the app's credential (LUMINESCENT_SERVICE_TOKENS): ${error.message}`;
    case "forbidden":
      return `Not allowed. Querying needs SELECT, and EXTERNAL_USE_SCHEMA on the schema to read its data: ${error.message}`;
    case "invalid":
    case "failed":
      return error.message;
  }
}

/** A chunk off the channel, as the query runner hands it on. */
export function decodeChunk(frame: ArrayBuffer): QueryChunk {
  const numRows = Number(new DataView(frame, 0, 8).getBigUint64(0, true));
  return { arrowIpc: new Uint8Array(frame, 8), numRows };
}

/** What a runner needs of the host: start a query, stop it. */
export interface QueryTransport {
  /** Resolves once the query planned (or didn't); the rows follow on `onMessage`. */
  run(
    queryId: number,
    instance: string | null,
    request: QueryRequest,
    onMessage: (message: QueryMessage) => void,
  ): Promise<{ status: "ok" } | { status: "error"; error: QueryError }>;
  cancel(queryId: number): void;
}

/** The transport over src-tauri's `query_run` / `query_cancel`. */
export const tauriQueryTransport: QueryTransport = {
  async run(queryId, instance, request, onMessage) {
    const channel = new Channel<QueryMessage>();
    channel.onmessage = onMessage;
    const result = await commands.queryRun(
      queryId,
      instance,
      {
        sql: request.sql,
        limit: request.limit ?? null,
        catalog: request.catalog ?? null,
        schema: request.schema ?? null,
      },
      // The bindings type the channel's messages as `unknown` (see QueryMessage).
      channel as Channel<unknown>,
    );
    return result.status === "ok" ? { status: "ok" } : result;
  },
  cancel(queryId) {
    // Best effort: the run may have ended already.
    void commands.queryCancel(queryId).catch(() => {});
  },
};

let nextQueryId = 1;

/**
 * A runner against `instance`'s Unity Catalog, or the current environment's
 * when `instance` is `null` (what the Catalog app's previews use: the Apps
 * view's selected instance is the current one).
 */
export function hostQueryRunner(
  instance: string | null,
  transport: QueryTransport = tauriQueryTransport,
): QueryRunner {
  return (request, { signal }) => ({
    async *[Symbol.asyncIterator](): AsyncGenerator<QueryChunk> {
      signal.throwIfAborted();
      const queryId = nextQueryId++;
      const pending: QueryChunk[] = [];
      let ended = false;
      let failure: unknown;
      let wake: (() => void) | undefined;
      const notify = () => {
        wake?.();
        wake = undefined;
      };
      const onAbort = () => {
        failure ??= signal.reason;
        notify();
      };
      signal.addEventListener("abort", onAbort);
      try {
        const started = await transport.run(queryId, instance, request, (message) => {
          if (ended || failure !== undefined) return;
          if (message instanceof ArrayBuffer) pending.push(decodeChunk(message));
          else if (message === "end") ended = true;
          else failure = new QueryFailedError(message.error);
          notify();
        });
        if (started.status === "error") throw new QueryFailedError(started.error);
        while (true) {
          const chunk = pending.shift();
          if (chunk) {
            yield chunk;
            continue;
          }
          if (failure !== undefined) throw failure;
          if (ended) return;
          await new Promise<void>((resolve) => {
            wake = resolve;
          });
        }
      } finally {
        signal.removeEventListener("abort", onAbort);
        // Stopped early (aborted, failed, or the consumer let go): stop the run.
        if (!ended) transport.cancel(queryId);
      }
    },
  });
}
