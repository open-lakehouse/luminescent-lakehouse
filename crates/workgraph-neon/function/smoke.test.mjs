// End-to-end smoke test for the built function bundle (`dist/`): two
// isolates (fixtures/isolate.mjs, a process each, sharing nothing but the
// database) serving MCP the way Neon would. Needs DATABASE_URL on a Postgres
// whose `workgraph` database is migrated (the addon swaps the database in),
// with `authz` migrated and fixtures/identity.sql applied in the database
// itself (neondb's stand-in); run via `just test workgraph` (host) or
// `just test-arm64 workgraph` (linux-arm64 in Docker).
//
// Callers authenticate as on a branch: service accounts' API keys (as a
// gateway route would carry them), Neon Auth users' OAuth and session tokens
// (a stand-in Neon Auth, fixtures/auth.mjs), and the desktop's interim apps
// token. What each may do is FACTORY_MCP_PRINCIPALS', over the named roles of
// FACTORY_MCP_ROLES, for a principal or for every member of a group.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import { after, before, test } from "node:test";
import { BANNED, KEYS, neonAuth, OPERATOR, WATCHER } from "./fixtures/auth.mjs";

const APPS = `wg_desk_${randomBytes(16).toString("hex")}`;
const PLANNER_POLICY = "@read,add_item,add_edge,annotate,propose,submit";
const { env: AUTH_ENV, accessToken, sessionToken } = await neonAuth();
const ENV = {
  ...process.env,
  ...AUTH_ENV,
  FACTORY_MCP_ROLES: [`planner=${PLANNER_POLICY}`, "reviewer=@read,annotate"].join(";"),
  FACTORY_MCP_PRINCIPALS: [
    "service-account:wg-planner=planner",
    "service-account:wg-reviewer=reviewer",
    "group:wg-planners=planner",
    `user:${OPERATOR}=operator:*`,
    `user:${WATCHER}=watcher:*`,
    `user:${BANNED}=operator:*`,
  ].join(";"),
  FACTORY_MCP_APP_PRINCIPALS: `user:${OPERATOR}`,
  FACTORY_MCP_APP_TOKENS: `${APPS}=*`,
  // A stale secret from before principals: ignored.
  FACTORY_MCP_TOKENS: "planner:wg_stale=*",
};
const PLANNER = KEYS.planner;
const REVIEWER = KEYS.reviewer;

const UI = { "io.modelcontextprotocol/ui": { mimeTypes: ["text/html;profile=mcp-app"] } };
const APP_ONLY = ["approve_revision", "return_plan"];

/** Start an isolate; resolves to `{ url, root, child }` once it serves. */
async function isolate(seed) {
  const child = spawn(process.execPath, ["fixtures/isolate.mjs"], {
    env: { ...ENV, SEED: seed ? "1" : "0" },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const exited = new Promise((_, reject) => child.once("exit", (code) => reject(new Error(`isolate exited: ${code}`))));
  const lines = createInterface({ input: child.stdout });
  const ready = new Promise((resolve) => {
    lines.on("line", (line) => {
      if (line.startsWith("isolate ")) resolve(JSON.parse(line.slice("isolate ".length)));
      else process.stderr.write(`${line}\n`);
    });
  });
  const { port, root } = await Promise.race([ready, exited]);
  return { url: `http://127.0.0.1:${port}/mcp`, base: `http://127.0.0.1:${port}`, root, child };
}

let A;
let B;
before(async () => {
  [A, B] = await Promise.all([isolate(true), isolate(false)]);
});
after(() => {
  A?.child.kill();
  B?.child.kill();
});

let nextId = 1;

/**
 * An MCP client of one protocol version, with no session: a 2025-11-25 one
 * (`initialize` first, then plain requests) declares its capabilities in
 * `initialize` only, unless `perRequest`; a 2026-07-28 one sends them in
 * every request's `_meta`.
 */
function client(url, bearer, { version = "2026-07-28", capabilities = {}, perRequest = false } = {}) {
  const modern = version === "2026-07-28";
  async function raw(method, params = {}) {
    const meta = modern
      ? {
          "io.modelcontextprotocol/protocolVersion": version,
          "io.modelcontextprotocol/clientInfo": { name: "smoke", version: "0" },
          "io.modelcontextprotocol/clientCapabilities": capabilities,
        }
      : perRequest
        ? { "io.modelcontextprotocol/clientCapabilities": capabilities }
        : undefined;
    const headers = {
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
      "mcp-protocol-version": version,
    };
    if (bearer) headers.authorization = `Bearer ${bearer}`;
    if (modern) headers["mcp-method"] = method;
    if (modern && params.name) headers["mcp-name"] = params.name;
    if (modern && params.uri) headers["mcp-name"] = params.uri;
    const id = nextId++;
    const res = await fetch(url, {
      method: "POST",
      headers,
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params: meta ? { ...params, _meta: meta } : params }),
    });
    const text = await res.text();
    const type = res.headers.get("content-type") ?? "";
    const messages = type.startsWith("text/event-stream")
      ? text
          .split("\n")
          .filter((l) => l.startsWith("data: "))
          .map((l) => JSON.parse(l.slice(6)))
      : type.startsWith("application/json")
        ? [JSON.parse(text)]
        : [];
    return { res, message: messages.find((m) => m.id === id) };
  }
  async function rpc(method, params) {
    const { res, message } = await raw(method, params);
    assert.equal(res.status, 200, JSON.stringify(message));
    assert.equal(res.headers.get("mcp-session-id"), null, "no sessions");
    return message;
  }
  return {
    raw,
    async init() {
      if (modern) return;
      const message = await rpc("initialize", {
        protocolVersion: version,
        capabilities,
        clientInfo: { name: "smoke", version: "0" },
      });
      assert.equal(message.result.protocolVersion, version);
    },
    async tools() {
      const message = await rpc("tools/list");
      return message.result.tools.map((t) => t.name);
    },
    /** A tool's result, or `{ error }` for a JSON-RPC error. */
    async call(name, args = {}) {
      const message = await rpc("tools/call", { name, arguments: args });
      return message.error ? { error: message.error } : message.result;
    },
    async read(uri) {
      const message = await rpc("resources/read", { uri });
      return message.error ? { error: message.error } : message.result;
    },
  };
}

async function connected(url, bearer, options) {
  const c = client(url, bearer, options);
  await c.init();
  return c;
}

test("only POST, and only with a valid credential", async () => {
  for (const method of ["GET", "DELETE", "PUT"]) {
    const res = await fetch(A.url, { method });
    assert.equal(res.status, 405, method);
    assert.equal(res.headers.get("allow"), "POST");
  }
  const cases = {
    none: undefined,
    garbage: "nope",
    "the stale static token": "wg_stale",
    "a token for another resource": accessToken({}, "https://unitycatalog.luminescent-lakehouse.dev"),
    "an expired token": accessToken({ exp: Math.floor(Date.now() / 1000) - 3600 }),
    "an unknown user": accessToken({ sub: randomUUID() }),
    "a revoked key": KEYS.revoked,
    "an expired key": KEYS.expired,
    "an unknown key": `olk_3c0f00000000a0ff_${"3c".repeat(32)}`,
  };
  for (const [name, bearer] of Object.entries(cases)) {
    const { res } = await client(A.url, bearer).raw("tools/list");
    assert.equal(res.status, 401, name);
    assert.match(res.headers.get("www-authenticate") ?? "", /^Bearer/, name);
  }
  const health = await fetch(new URL("/health", A.url));
  assert.equal(health.status, 200);
});

test("the protected-resource metadata needs no token and names the resource", async () => {
  const res = await fetch(new URL("/.well-known/oauth-protected-resource", A.url));
  assert.equal(res.status, 200);
  const doc = await res.json();
  assert.equal(doc.resource, AUTH_ENV.WORKGRAPH_RESOURCE);
  assert.deepEqual(doc.authorization_servers, [AUTH_ENV.NEON_AUTH_BASE_URL]);
});

test("a known caller without an entry, or banned, is a 403", async () => {
  for (const [name, bearer] of Object.entries({
    "an unknown service account": KEYS.stranger,
    "a banned user": accessToken({ sub: BANNED }),
  })) {
    for (const isolate of [A, B]) {
      const { res } = await client(isolate.url, bearer).raw("tools/list");
      assert.equal(res.status, 403, name);
    }
  }
});

test("two isolates serve one plan from the database", async () => {
  const a = await connected(A.url, PLANNER);
  const b = await connected(B.url, PLANNER);

  // B never saw the seed: it reads the plan A wrote.
  const plan = (await b.call("plan_read", { root: A.root })).structuredContent;
  assert.ok(plan.items.length > 1, JSON.stringify(plan));
  const item = plan.items.find((i) => i.ref !== A.root);

  // A proposal raised on B is on the item when A reads it.
  const proposed = await b.call("propose", { node: item.ref, summary: "split it in two" });
  assert.ok(!proposed.isError, JSON.stringify(proposed));
  assert.equal(proposed.structuredContent.status, "proposed");
  const read = (await a.call("item_read", { ref: item.ref })).structuredContent;
  const kinds = JSON.stringify(read.attention ?? read);
  assert.match(kinds, /split it in two/);

  // And an item added on A is in B's outline.
  const added = await a.call("add_item", { parent: A.root, kind: "Task", title: "added on A" });
  assert.ok(!added.isError, JSON.stringify(added));
  const again = (await b.call("plan_read", { root: A.root })).structuredContent;
  assert.ok(again.items.some((i) => i.title === "added on A"));

  // Both isolates map a caller the same way.
  assert.deepEqual((await a.tools()).sort(), (await b.tools()).sort());
});

test("writes are attributed to the principal", async () => {
  const reviewer = await connected(A.url, REVIEWER);
  const plan = (await reviewer.call("plan_read", { root: A.root })).structuredContent;
  const item = plan.items.find((i) => i.ref !== A.root);
  const note = await reviewer.call("annotate", {
    node: item.ref,
    kind: "learning",
    payload: { lesson: "Keys are per service account.", evidence: ["smoke"] },
  });
  assert.ok(!note.isError, JSON.stringify(note));
  const read = await (await connected(B.url, PLANNER)).call("item_read", { ref: item.ref });
  const text = read.content.map((c) => c.text ?? "").join("\n");
  assert.match(text, /by service-account:wg-reviewer/);
});

test("an API key reaches only its role's tools", async () => {
  const planner = await (await connected(A.url, PLANNER)).tools();
  for (const name of ["plan_read", "item_read", "add_item", "propose", "submit"]) assert.ok(planner.includes(name), name);
  assert.ok(!planner.includes("report_change"));

  const reviewer = await connected(B.url, REVIEWER);
  const names = await reviewer.tools();
  assert.ok(names.includes("plan_read") && names.includes("annotate"));
  assert.ok(!names.includes("add_item") && !names.includes("propose"));
  const refused = await reviewer.call("add_item", { parent: A.root, kind: "Task", title: "nope" });
  assert.match(refused.error?.message ?? "", /unknown tool/);
});

test("a group's member gets the group's role, as itself", async () => {
  const member = await connected(A.url, KEYS.member);
  const tools = await member.tools();
  assert.ok(tools.includes("add_item") && tools.includes("submit"));
  assert.ok(!tools.includes("report_change"));
  const added = await member.call("add_item", { parent: A.root, kind: "Task", title: "By group" });
  assert.ok(!added.isError, JSON.stringify(added));
  const ref = added.structuredContent?.item?.ref;
  assert.ok(ref, JSON.stringify(added.structuredContent));
  const note = await member.call("annotate", {
    node: ref,
    kind: "learning",
    payload: { lesson: "Groups grant roles.", evidence: ["smoke"] },
  });
  assert.ok(!note.isError, JSON.stringify(note));
  const read = await (await connected(B.url, PLANNER)).call("item_read", { ref });
  assert.match(read.content.map((c) => c.text ?? "").join("\n"), /by service-account:wg-member/);
});

test("the role catalog needs no token and lists each role's tools", async () => {
  const res = await fetch(`${A.base}/roles`);
  assert.equal(res.status, 200);
  const { roles } = await res.json();
  assert.deepEqual(
    roles.map((r) => r.name),
    ["planner", "reviewer"],
  );
  const reviewer = roles.find((r) => r.name === "reviewer");
  assert.equal(reviewer.policy, "@read,annotate");
  assert.ok(reviewer.tools.includes("plan_read") && reviewer.tools.includes("annotate"));
  assert.ok(!reviewer.tools.includes("add_item"));
  for (const r of roles) for (const t of APP_ONLY) assert.ok(!r.tools.includes(t), `${r.name}: ${t}`);
  // It matches what the server enforces.
  const listed = await (await connected(B.url, REVIEWER)).tools();
  assert.deepEqual([...listed].sort(), [...reviewer.tools].sort());
});

test("users sign in with an OAuth token or a session token", async () => {
  for (const bearer of [accessToken({ sub: WATCHER }), sessionToken({ sub: WATCHER })]) {
    const c = await connected(A.url, bearer);
    assert.ok((await c.tools()).includes("plan_read"));
  }
});

test("app-only tools and views need an app principal or the apps token", async () => {
  // An agent's key gets none, even when its client declares MCP Apps.
  const agent = await connected(A.url, PLANNER, { capabilities: { extensions: UI } });
  const names = await agent.tools();
  for (const name of APP_ONLY) assert.ok(!names.includes(name), name);
  assert.match((await agent.call("approve_revision", { root: A.root, revision: "x" })).error?.message ?? "", /unknown tool/);
  assert.ok((await agent.read("ui://factory/plan")).error);

  // Nor does a user without an apps entry, whose policy is `*`.
  const watcher = await connected(A.url, accessToken({ sub: WATCHER }), { capabilities: { extensions: UI } });
  const seen = await watcher.tools();
  assert.ok(seen.includes("add_item"));
  for (const name of APP_ONLY) assert.ok(!seen.includes(name), name);
  assert.ok((await watcher.read("ui://factory/plan")).error);

  // The app principal, and the interim apps token, with the extension
  // declared, get both.
  for (const bearer of [accessToken({ sub: OPERATOR }), APPS]) {
    const desk = await connected(B.url, bearer, { capabilities: { extensions: UI } });
    const all = await desk.tools();
    for (const name of APP_ONLY) assert.ok(all.includes(name), name);
    const view = await desk.read("ui://factory/plan");
    assert.match(view.contents[0].text, /<html|<!doctype/i);
  }

  // Without the declaration, not even the app principal.
  const plain = await connected(B.url, accessToken({ sub: OPERATOR }));
  for (const name of APP_ONLY) assert.ok(!(await plain.tools()).includes(name), name);
});

test("a 2025-11-25 client and a 2026-07-28 client both work", async () => {
  for (const version of ["2025-11-25", "2026-07-28"]) {
    const c = await connected(A.url, PLANNER, { version });
    assert.ok((await c.tools()).includes("plan_read"), version);
    const plan = await c.call("plan_read", { root: A.root });
    assert.ok(!plan.isError && plan.structuredContent.items.length > 1, version);
  }

  // Statelessly, a 2025-11-25 client's `initialize` capabilities don't carry
  // over to its next request; declared in the request's own `_meta`, they do.
  const legacy = await connected(B.url, APPS, { version: "2025-11-25", capabilities: { extensions: UI } });
  assert.ok(!(await legacy.tools()).includes("approve_revision"));
  const perRequest = await connected(B.url, APPS, {
    version: "2025-11-25",
    capabilities: { extensions: UI },
    perRequest: true,
  });
  assert.ok((await perRequest.tools()).includes("approve_revision"));
});
