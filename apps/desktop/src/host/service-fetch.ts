// The `fetch` of a desktop `ServiceBinding`: the request goes to Rust as a
// module id and a path (never a URL), and the response streams back over a
// Tauri `Channel`, so large or long-lived bodies (query results, SSE) arrive as
// they are produced.

import { ServiceUnavailableError } from "@open-lakehouse/host";
import { Channel } from "@tauri-apps/api/core";
import { commands, type ProxyError } from "./bindings";

/**
 * One message on the body channel: raw bytes arrive as an `ArrayBuffer`, the
 * end as `"end"`, a failure mid-body as `{ error }` (see `BodyMessage` in
 * src-tauri/src/commands.rs).
 */
export type BodyMessage = ArrayBuffer | "end" | { error: ProxyError };

/** Relative paths resolve against this; only the path and query are sent. */
const PLACEHOLDER_ORIGIN = "http://service.invalid";

/** Statuses whose `Response` may not have a body. */
const NULL_BODY = new Set([101, 103, 204, 205, 304]);

let nextRequestId = 1;

/** The error `fetch` would raise for `error`. */
export function toFetchError(moduleId: string, error: ProxyError): Error {
  switch (error.kind) {
    case "unbound":
      return new ServiceUnavailableError(moduleId, "the current environment doesn't bind it");
    case "invalidRequest":
      return new TypeError(`${moduleId}: ${error.message}`);
    case "upstream":
      return new TypeError(`${moduleId}: ${error.message}`);
  }
}

/** A `fetch` for `moduleId` through the Rust proxy. */
export function serviceFetch(moduleId: string): typeof globalThis.fetch {
  const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = new Request(
      typeof input === "string" ? new URL(input, PLACEHOLDER_ORIGIN) : input,
      init,
    );
    const signal = request.signal;
    signal.throwIfAborted();

    const url = new URL(request.url);
    const bytes = new Uint8Array(await request.arrayBuffer());
    const id = nextRequestId++;

    // Best effort: the body may already have ended on the Rust side.
    const cancel = () => void commands.serviceFetchCancel(id).catch(() => {});
    let finished = false;
    // Until the head is in, Rust has nothing to cancel.
    let headIn = false;
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const body = new ReadableStream<Uint8Array>({
      start(c) {
        controller = c;
      },
      cancel() {
        finish();
      },
    });
    const finish = (error?: unknown) => {
      if (finished) return;
      finished = true;
      signal.removeEventListener("abort", onAbort);
      if (headIn) cancel();
      if (error !== undefined) controller.error(error);
    };
    const onAbort = () => finish(signal.reason);
    signal.addEventListener("abort", onAbort);

    const channel = new Channel<BodyMessage>();
    channel.onmessage = (message) => {
      if (finished) return;
      if (message instanceof ArrayBuffer) {
        controller.enqueue(new Uint8Array(message));
      } else if (message === "end") {
        finished = true;
        signal.removeEventListener("abort", onAbort);
        controller.close();
      } else {
        finish(toFetchError(moduleId, message.error));
      }
    };

    const sent = commands.serviceFetch(
      id,
      {
        moduleId,
        method: request.method,
        path: url.pathname + url.search,
        headers: [...request.headers.entries()],
        body: bytes.byteLength > 0 ? Array.from(bytes) : null,
      },
      // The bindings type the channel's messages as `unknown` (see BodyMessage).
      channel as Channel<unknown>,
    );
    // Rust only registers the body for cancelling once the head is in, so an
    // abort before then cancels when the head arrives.
    sent.then(
      (result) => {
        if (result.status !== "ok") return;
        headIn = true;
        if (signal.aborted) cancel();
      },
      () => {},
    );
    const aborted = new Promise<never>((_, reject) => {
      if (signal.aborted) reject(signal.reason);
      signal.addEventListener("abort", () => reject(signal.reason), { once: true });
    });
    aborted.catch(() => {}); // raced below; a later abort is the body's business
    const result = await Promise.race([sent, aborted]);
    if (result.status === "error") {
      finish();
      throw toFetchError(moduleId, result.error);
    }
    const head = result.data;
    const nullBody = NULL_BODY.has(head.status) || request.method === "HEAD";
    if (nullBody) finish();
    return new Response(nullBody ? null : body, {
      status: head.status,
      statusText: head.statusText,
      headers: head.headers,
    });
  };
  // `typeof fetch` carries runtime extras (Bun's `preconnect`) a binding doesn't need.
  return fetch as typeof globalThis.fetch;
}
