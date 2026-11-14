// Neon Function entry point: a thin fetch handler over the Unity Catalog
// server's Node-API addon (`../src/lib.rs`). Every request is forwarded, whole,
// into the in-process axum router; there is no background work to schedule.
//
// Kept to erasable TypeScript (no enums/namespaces/parameter properties) so the
// build can emit `index.mjs` with Node's built-in type stripping — no bundler,
// no npm dependencies. See `just build unitycatalog`.

import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { rootCertificates } from "node:tls";

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
  body: Buffer;
}

interface Addon {
  start(): Promise<void>;
  handle(request: NativeRequest): Promise<NativeResponse>;
  shutdown(): Promise<void>;
}

const addon = createRequire(import.meta.url)("./native/unitycatalog.node") as Addon;

// Statuses the fetch `Response` constructor rejects a body for.
const NULL_BODY_STATUSES = new Set([101, 204, 205, 304]);

// The addon verifies callers' tokens against the branch's JWKS over HTTPS, and
// its TLS client (rustls) trusts the system's CA certificates only: it fails
// to start without any, and slim images (node:24-bookworm-slim) have none. So
// unless the environment names its own, hand it the roots Node ships with
// (Mozilla's), through the variable rustls reads them from.
function trustNodeRoots(): void {
  if (process.env.SSL_CERT_FILE || process.env.SSL_CERT_DIR) return;
  const file = join(tmpdir(), "unitycatalog-ca-certificates.pem");
  try {
    writeFileSync(file, `${rootCertificates.join("\n")}\n`);
    process.env.SSL_CERT_FILE = file;
  } catch (err) {
    console.error("could not write Node's CA certificates; using the system's:", err);
  }
}

// The branch's `authz` function, the policy decision point every request is
// checked with (neon.ts). Neon names a function's URL after the branch and the
// slug (`https://<branch id>-<slug>.compute.<cell>.…`) and injects no other
// function's, so it is this function's own host with the slug swapped.
// AUTHZ_URL, when set, wins (a custom domain, a PDP elsewhere).
export function authzUrl(host: string): string | undefined {
  const match = /^([a-z0-9-]+)-unitycatalog(\.compute\.[a-z0-9.-]+)$/.exec(host.toLowerCase());
  return match ? `https://${match[1]}-authz${match[2]}` : undefined;
}

function findAuthz(request: Request): void {
  if (process.env.AUTHZ_URL) return;
  const url = authzUrl(new URL(request.url).host);
  if (url) process.env.AUTHZ_URL = url;
  else console.error(`set AUTHZ_URL: no authz function URL follows from ${new URL(request.url).host}`);
}

// Start lazily on the first request and share the promise across concurrent
// ones. A failed start (e.g. pending migrations) is retried on the next request.
let started: Promise<void> | undefined;
function ready(request: Request): Promise<void> {
  if (!started) {
    trustNodeRoots();
    findAuthz(request);
  }
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

export default {
  async fetch(request: Request): Promise<Response> {
    try {
      await ready(request);
    } catch (err) {
      console.error("unity catalog failed to start:", err);
      return new Response("unity catalog failed to start; see function logs\n", { status: 503 });
    }

    const url = new URL(request.url);
    const response = await addon.handle({
      method: request.method,
      url: url.pathname + url.search,
      headers: [...request.headers].map(([name, value]) => ({ name, value })),
      body: Buffer.from(await request.arrayBuffer()),
    });

    const body =
      NULL_BODY_STATUSES.has(response.status) || request.method === "HEAD" ? null : response.body;
    return new Response(body, {
      status: response.status,
      headers: response.headers.map(({ name, value }): [string, string] => [name, value]),
    });
  },
};
