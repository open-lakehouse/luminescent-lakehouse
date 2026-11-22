// Neon Function entry point: a thin fetch handler over workgraph-mcp's
// Node-API addon (`../src/lib.rs`). Every request is forwarded, whole, into the
// in-process axum router. A tool call's SSE response streams back chunk by
// chunk, so rmcp's keep-alives reach the client while a slow tool runs; any
// other response comes back whole.
//
// The addon decides everything: POST only on /mcp (405 otherwise, before the
// token check), the bearer token, then the MCP transport, statelessly.
//
// Kept to erasable TypeScript (no enums/namespaces/parameter properties) so the
// build can emit `index.mjs` with Node's built-in type stripping — no bundler,
// no npm dependencies. See `just build workgraph`.

import { createRequire } from "node:module";

interface Header {
  name: string;
  value: string;
}

interface NativeRequest {
  method: string;
  url: string;
  headers: Header[];
  body: Buffer;
}

interface NativeResponse {
  status: number;
  headers: Header[];
  body?: Buffer | null;
  stream?: number | null;
}

interface Addon {
  start(): Promise<void>;
  handle(request: NativeRequest): Promise<NativeResponse>;
  nextChunk(stream: number): Promise<Buffer | null>;
  cancelStream(stream: number): void;
  seedDemo(): Promise<string>;
  shutdown(): Promise<void>;
}

const addon = createRequire(import.meta.url)("./native/workgraph.node") as Addon;

// Statuses the fetch `Response` constructor rejects a body for.
const NULL_BODY_STATUSES = new Set([101, 204, 205, 304]);

// Start lazily on the first request and share the promise across concurrent
// ones. A failed start (e.g. pending migrations) is retried on the next request.
let started: Promise<void> | undefined;
function ready(): Promise<void> {
  started ??= addon.start().catch((err: unknown) => {
    started = undefined;
    throw err;
  });
  return started;
}

// Neon sends SIGINT before evicting an isolate and kills it 5 s later.
process.once("SIGINT", () => {
  addon.shutdown().finally(() => process.exit(0));
});

// A streamed body, pulled from the addon as the client reads it. Cancelling
// (the client went away) drops it there, which ends the tool call's stream.
export function streamBody(stream: number, native: Pick<Addon, "nextChunk" | "cancelStream">): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      try {
        const chunk = await native.nextChunk(stream);
        if (chunk) controller.enqueue(new Uint8Array(chunk));
        else controller.close();
      } catch (err) {
        controller.error(err);
      }
    },
    cancel() {
      native.cancelStream(stream);
    },
  });
}

// The tests' way to a plan: the demo, seeded through the addon. No request
// reaches it.
export async function seedDemo(): Promise<string> {
  await ready();
  return addon.seedDemo();
}

export default {
  async fetch(request: Request): Promise<Response> {
    try {
      await ready();
    } catch (err) {
      console.error("workgraph failed to start:", err);
      return new Response("workgraph failed to start; see function logs\n", { status: 503 });
    }

    const url = new URL(request.url);
    const response = await addon.handle({
      method: request.method,
      url: url.pathname + url.search,
      headers: [...request.headers].map(([name, value]) => ({ name, value })),
      body: Buffer.from(await request.arrayBuffer()),
    });

    const empty = NULL_BODY_STATUSES.has(response.status) || request.method === "HEAD";
    let body: BodyInit | null = null;
    if (response.stream != null) {
      if (empty) addon.cancelStream(response.stream);
      else body = streamBody(response.stream, addon);
    } else if (!empty) {
      body = response.body ?? null;
    }
    return new Response(body, {
      status: response.status,
      headers: response.headers.map(({ name, value }): [string, string] => [name, value]),
    });
  },
};
