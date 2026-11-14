// End-to-end smoke test for the built function bundle (`dist/`), driven the
// way Neon drives it: fetch `Request` in, `Response` out. Needs DATABASE_URL on
// a Postgres whose `unitycatalog` database is migrated (the addon swaps the
// database in), with `authz` migrated and fixtures/identity.sql applied in the
// database itself (neondb's stand-in); run via `just test unitycatalog` (host)
// or `just test-arm64 unitycatalog` (linux-arm64 in Docker).
//
// Callers authenticate as on a branch, with a local stand-in for its Neon
// Auth: this file serves the JWKS of a fresh Ed25519 key and signs the tokens.
// It stands in for the branch's `authz` function too, with a fixed policy
// (`decide` below): the real policies are tested in crates/authz-neon.

import assert from "node:assert/strict";
import { generateKeyPairSync, randomUUID, sign } from "node:crypto";
import { createServer } from "node:http";
import { after, test } from "node:test";

const RESOURCE = "https://unitycatalog.luminescent-lakehouse.dev";
const USER = "5e1f0000-0000-4000-8000-000000000001";
const BANNED = "5e1f0000-0000-4000-8000-000000000002";
const API_KEY = `olk_5e1f00000000a001_${"5e".repeat(32)}`;

const AUTHZ_TOKEN = randomUUID();

// The stand-in PDP: users may do anything but receive storage credentials;
// service accounts may only list catalogs and see those named `visible_*`.
function decide({ principal, action, resource }) {
  const allow =
    principal.type === "Lakehouse::User"
      ? !action.id.startsWith("GenerateTemporary") && action.id !== "CreateStagingTable"
      : action.id === "ListCatalogs" ||
        (action.id === "GetCatalog" && resource.type === "UnityCatalog::Catalog" && resource.id.startsWith("visible_"));
  return { decision: allow ? "allow" : "deny", policies: [], errors: [] };
}
/** What the server asked the PDP, and with which secret. */
const asked = [];

// The branch's Neon Auth (its JWKS, at an Auth URL on loopback) and its authz
// function.
const KID = "smoke";
const { privateKey, publicKey } = generateKeyPairSync("ed25519");
const jwks = JSON.stringify({ keys: [{ ...publicKey.export({ format: "jwk" }), kid: KID, alg: "EdDSA", use: "sig" }] });
const auth = createServer(async (req, res) => {
  if (req.method === "POST" && req.url.startsWith("/v1/authorize")) {
    let body = "";
    for await (const chunk of req) body += chunk;
    const json = JSON.parse(body);
    const requests = req.url === "/v1/authorize/batch" ? json.requests : [json];
    for (const r of requests) asked.push({ secret: req.headers.authorization, ...r });
    const responses = requests.map(decide);
    res.writeHead(req.headers.authorization === `Bearer ${AUTHZ_TOKEN}` ? 200 : 401, { "content-type": "application/json" });
    res.end(JSON.stringify(req.url === "/v1/authorize/batch" ? { responses } : responses[0]));
    return;
  }
  res.writeHead(req.url === "/neondb/auth/.well-known/jwks.json" ? 200 : 404, { "content-type": "application/json" });
  res.end(jwks);
});
await new Promise((resolve) => auth.listen(0, "127.0.0.1", resolve));
after(() => {
  auth.closeAllConnections();
  auth.close();
});
const ORIGIN = `http://127.0.0.1:${auth.address().port}`;
const AUTH_URL = `${ORIGIN}/neondb/auth`;

// What Neon sets for the function, with a throwaway KEK in place of the branch
// secret.
process.env.NEON_AUTH_BASE_URL = AUTH_URL;
process.env.NEON_AUTH_JWKS_URL = `${AUTH_URL}/.well-known/jwks.json`;
process.env.UC_SERVER_KEK ??= Buffer.alloc(32, 7).toString("base64");
// On Neon, function/index.ts derives AUTHZ_URL from the request's host.
process.env.AUTHZ_URL = ORIGIN;
process.env.AUTHZ_TOKEN_UNITYCATALOG = AUTHZ_TOKEN;
// neon.ts's document, plus a managed storage root so a catalog can be created
// without registering an external location first. Nothing is written to it.
process.env.UC_SERVER_CONFIG_YAML ??= [
  "managed_storage_root: s3://smoke-bucket/managed",
  "ui: { serve: false }",
  "auth:",
  "  mode: olai-auth",
  "  olai:",
  "    issuer: { env: NEON_AUTH_BASE_URL }",
  "    jwks-url: { env: NEON_AUTH_JWKS_URL }",
  `    audiences: ["${RESOURCE}"]`,
  "    accept-session-tokens: true",
  "    database-url: { env: DATABASE_URL }",
  `    resource: "${RESOURCE}"`,
  "policy:",
  "  kind: authorizer",
  "  url: { env: AUTHZ_URL }",
  "  token: { env: AUTHZ_TOKEN_UNITYCATALOG }",
  // No decision cache: each test sees the stand-in's answer.
  "  cache-ttl-secs: 0",
  "encryption:",
  "  active: { id: smoke, key: { env: UC_SERVER_KEK } }",
].join("\n");

const { default: fn, authzUrl } = await import("./dist/index.mjs");

const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");

/** A JWT signed by the branch's key: `header` and `claims` override the defaults. */
function jwt(claims, header = {}) {
  const now = Math.floor(Date.now() / 1000);
  const input = `${b64({ alg: "EdDSA", kid: KID, ...header })}.${b64({ sub: USER, iat: now, exp: now + 300, ...claims })}`;
  return `${input}.${sign(null, Buffer.from(input), privateKey).toString("base64url")}`;
}

/** An OAuth access token for `resource` (CLI, MCP, partner apps). */
const accessToken = (claims = {}, resource = RESOURCE) =>
  jwt({ iss: AUTH_URL, aud: [resource, `${AUTH_URL}/oauth2/userinfo`], client_id: "smoke", ...claims }, { typ: "at+jwt" });
/** The web app's session token: `iss` and `aud` are the Auth URL's origin. */
const sessionToken = () => jwt({ iss: ORIGIN, aud: ORIGIN, email: "smoke@example.com" });

const BASE = "https://unitycatalog.example";
const API = "/api/2.1/unity-catalog";
const catalog = `smoke_${randomUUID().replaceAll("-", "")}`;

async function call(method, path, { body, token = accessToken() } = {}) {
  const headers = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fn.fetch(
    new Request(BASE + path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
  );
  const text = await response.text();
  const isJson = response.headers.get("content-type")?.includes("json");
  return {
    status: response.status,
    headers: response.headers,
    json: isJson && text ? JSON.parse(text) : undefined,
    text,
  };
}

test("health needs no token", async () => {
  const res = await call("GET", "/health", { token: null });
  assert.equal(res.status, 200);
  assert.equal(res.text, "OK");
});

test("the protected-resource metadata needs no token and names the resource", async () => {
  const res = await call("GET", "/.well-known/oauth-protected-resource", { token: null });
  assert.equal(res.status, 200, res.text);
  assert.equal(res.json.resource, RESOURCE);
  assert.deepEqual(res.json.authorization_servers, [AUTH_URL]);
});

test("no token is a 401 with a Bearer challenge", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: null });
  assert.equal(res.status, 401, res.text);
  assert.match(res.headers.get("www-authenticate") ?? "", /^Bearer/i);
});

test("an OAuth access token for the resource is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: accessToken() });
  assert.equal(res.status, 200, res.text);
});

test("a web session token is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: sessionToken() });
  assert.equal(res.status, 200, res.text);
});

test("a service account's API key is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: API_KEY });
  assert.equal(res.status, 200, res.text);
});

test("tokens that aren't for this function are 401s", async (t) => {
  const cases = {
    "another resource": accessToken({}, "https://headwaters.luminescent-lakehouse.dev"),
    "another issuer": accessToken({ iss: `${ORIGIN}/other/auth` }),
    "an unknown user": accessToken({ sub: randomUUID() }),
    expired: accessToken({ exp: Math.floor(Date.now() / 1000) - 3600 }),
    "an unknown API key": `olk_5e1f00000000a001_${"00".repeat(32)}`,
    garbage: "not-a-token",
  };
  for (const [name, token] of Object.entries(cases)) {
    await t.test(name, async () => {
      const res = await call("GET", `${API}/catalogs`, { token });
      assert.equal(res.status, 401, res.text);
    });
  }
});

test("a banned user is a 403", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: accessToken({ sub: BANNED }) });
  assert.equal(res.status, 403, res.text);
});

test("create a catalog and a schema, then read them back", async () => {
  const created = await call("POST", `${API}/catalogs`, { body: { name: catalog, comment: "smoke" } });
  assert.equal(created.status, 200, created.text);
  assert.equal(created.json.name, catalog);

  const schema = await call("POST", `${API}/schemas`, { body: { name: "s", catalog_name: catalog } });
  assert.equal(schema.status, 200, schema.text);
  assert.equal(schema.json.full_name, `${catalog}.s`);

  const catalogs = await call("GET", `${API}/catalogs`);
  assert.equal(catalogs.status, 200, catalogs.text);
  assert.ok(
    (catalogs.json.catalogs ?? []).some((c) => c.name === catalog),
    `catalog ${catalog} not listed: ${catalogs.text}`,
  );

  const schemas = await call("GET", `${API}/schemas?catalog_name=${catalog}`);
  assert.equal(schemas.status, 200, schemas.text);
  assert.deepEqual(
    (schemas.json.schemas ?? []).map((s) => s.name).filter((n) => n !== "information_schema"),
    ["s"],
  );

  const deleted = await call("DELETE", `${API}/catalogs/${catalog}?force=true`);
  assert.equal(deleted.status, 200, deleted.text);
});

test("an unknown catalog is a 404", async () => {
  const res = await call("GET", `${API}/catalogs/missing_${randomUUID().replaceAll("-", "")}`);
  assert.equal(res.status, 404, res.text);
});

test("the authz function decides, asked with the caller secret", async () => {
  asked.length = 0;
  const name = `smoke_${randomUUID().replaceAll("-", "")}`;
  const created = await call("POST", `${API}/catalogs`, { body: { name } });
  assert.equal(created.status, 200, created.text);
  const ask = asked.find((a) => a.action.id === "CreateCatalog");
  assert.ok(ask, JSON.stringify(asked));
  assert.equal(ask.secret, `Bearer ${AUTHZ_TOKEN}`);
  assert.deepEqual(ask.principal, { type: "Lakehouse::User", id: USER });
  // The new catalog itself, sent with the metastore: CREATE_CATALOG there covers it.
  assert.deepEqual(ask.resource, { type: "UnityCatalog::Catalog", id: name });
  const entity = ask.entities.find((e) => e.uid.id === name);
  assert.deepEqual(entity?.parents, [{ type: "UnityCatalog::Metastore", id: "metastore" }], JSON.stringify(ask));
  await call("DELETE", `${API}/catalogs/${name}?force=true`);
});

test("a deny is a 403, and lists show only what may be read", async () => {
  const hidden = `smoke_${randomUUID().replaceAll("-", "")}`;
  const visible = `visible_${randomUUID().replaceAll("-", "")}`;
  for (const name of [hidden, visible]) {
    const res = await call("POST", `${API}/catalogs`, { body: { name } });
    assert.equal(res.status, 200, res.text);
  }
  try {
    const create = await call("POST", `${API}/catalogs`, { body: { name: `${hidden}_x` }, token: API_KEY });
    assert.equal(create.status, 403, create.text);
    const get = await call("GET", `${API}/catalogs/${hidden}`, { token: API_KEY });
    assert.equal(get.status, 403, get.text);
    const list = await call("GET", `${API}/catalogs`, { token: API_KEY });
    assert.equal(list.status, 200, list.text);
    const names = (list.json.catalogs ?? []).map((c) => c.name);
    assert.ok(names.includes(visible), list.text);
    assert.ok(!names.includes(hidden), list.text);
  } finally {
    for (const name of [hidden, visible]) await call("DELETE", `${API}/catalogs/${name}?force=true`);
  }
});

test("the authz function's URL follows from this function's host", () => {
  assert.equal(
    authzUrl("br-cool-darkness-a1b2c3-unitycatalog.compute.c-3.us-east-2.aws.neon.tech"),
    "https://br-cool-darkness-a1b2c3-authz.compute.c-3.us-east-2.aws.neon.tech",
  );
  assert.equal(authzUrl("unitycatalog.example"), undefined);
  assert.equal(authzUrl("br-x-headwaters.compute.c-3.us-east-2.aws.neon.tech"), undefined);
});
