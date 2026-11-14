// What the function's tests share: local stand-ins for the branch's Neon Auth
// (the JWKS of a fresh Ed25519 key; the tests sign the tokens) and its `authz`
// function (a fixed policy each test file passes in), the environment Neon
// sets, the catalog's config document, and a client that drives the built
// handler the way Neon does: fetch `Request` in, `Response` out.
//
// The callers are fixtures/identity.sql's.

import { generateKeyPairSync, randomUUID, sign } from "node:crypto";
import { createServer } from "node:http";
import { after } from "node:test";

export const RESOURCE = "https://unitycatalog.luminescent-lakehouse.dev";
export const USER = "5e1f0000-0000-4000-8000-000000000001";
export const BANNED = "5e1f0000-0000-4000-8000-000000000002";
export const READER = "5e1f0000-0000-4000-8000-000000000003";
export const API_KEY = `olk_5e1f00000000a001_${"5e".repeat(32)}`;
export const API = "/api/2.1/unity-catalog";
const BASE = "https://unitycatalog.example";
const KID = "smoke";

/** A unique name for a catalog object. */
export const unique = (prefix = "smoke") => `${prefix}_${randomUUID().replaceAll("-", "")}`;

/** An HTTP server on loopback, closed after the file's tests; resolves to its origin. */
export async function serve(handler) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  after(() => {
    server.closeAllConnections();
    server.close();
  });
  return `http://127.0.0.1:${server.address().port}`;
}

export async function readJson(req) {
  let body = "";
  for await (const chunk of req) body += chunk;
  return body ? JSON.parse(body) : undefined;
}

/**
 * Start the branch's Neon Auth and authz stand-ins, and set the environment
 * Neon would. `decide({ principal, action, resource })` is the PDP's policy.
 * Returns the token helpers, the Auth URL, and `asked`: what the server asked
 * the PDP, with which secret.
 */
export async function standIns(decide) {
  const authzToken = randomUUID();
  const asked = [];
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const jwks = JSON.stringify({ keys: [{ ...publicKey.export({ format: "jwk" }), kid: KID, alg: "EdDSA", use: "sig" }] });

  const origin = await serve(async (req, res) => {
    if (req.method === "POST" && req.url.startsWith("/v1/authorize")) {
      const json = await readJson(req);
      const batch = req.url === "/v1/authorize/batch";
      const requests = batch ? json.requests : [json];
      for (const r of requests) asked.push({ secret: req.headers.authorization, ...r });
      const responses = requests.map((r) => {
        const allow = decide(r);
        return { decision: allow ? "allow" : "deny", policies: [], errors: [] };
      });
      res.writeHead(req.headers.authorization === `Bearer ${authzToken}` ? 200 : 401, { "content-type": "application/json" });
      res.end(JSON.stringify(batch ? { responses } : responses[0]));
      return;
    }
    res.writeHead(req.url === "/neondb/auth/.well-known/jwks.json" ? 200 : 404, { "content-type": "application/json" });
    res.end(jwks);
  });
  const authUrl = `${origin}/neondb/auth`;

  // What Neon sets for the function, with a throwaway KEK in place of the
  // branch secret. On Neon, function/index.ts derives AUTHZ_URL from the
  // request's host.
  process.env.NEON_AUTH_BASE_URL = authUrl;
  process.env.NEON_AUTH_JWKS_URL = `${authUrl}/.well-known/jwks.json`;
  process.env.UC_SERVER_KEK ??= Buffer.alloc(32, 7).toString("base64");
  process.env.AUTHZ_URL = origin;
  process.env.AUTHZ_TOKEN_UNITYCATALOG = authzToken;

  const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
  /** A JWT signed by the branch's key: `header` and `claims` override the defaults. */
  const jwt = (claims, header = {}) => {
    const now = Math.floor(Date.now() / 1000);
    const input = `${b64({ alg: "EdDSA", kid: KID, ...header })}.${b64({ sub: USER, iat: now, exp: now + 300, ...claims })}`;
    return `${input}.${sign(null, Buffer.from(input), privateKey).toString("base64url")}`;
  };
  /** An OAuth access token for `resource` (CLI, MCP, partner apps). */
  const accessToken = (claims = {}, resource = RESOURCE) =>
    jwt({ iss: authUrl, aud: [resource, `${authUrl}/oauth2/userinfo`], client_id: "smoke", ...claims }, { typ: "at+jwt" });
  /** The web app's session token: `iss` and `aud` are the Auth URL's origin. */
  const sessionToken = () => jwt({ iss: origin, aud: origin, email: "smoke@example.com" });

  return { origin, authUrl, authzToken, asked, accessToken, sessionToken };
}

/**
 * neon.ts's document (`unityCatalogConfig`), with `head` lines first (e.g. a
 * managed storage root) and `vending` lines (under `vending.neon`) last; and
 * no decision cache, so each test sees the stand-in's answer.
 */
export function configYaml({ head = [], vending = [] } = {}) {
  return [
    ...head,
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
    "  cache-ttl-secs: 0",
    "encryption:",
    "  active: { id: smoke, key: { env: UC_SERVER_KEK } }",
    ...(vending.length ? ["vending:", "  neon:", ...vending.map((l) => `    ${l}`)] : []),
  ].join("\n");
}

/** A client of the built handler `fn`, as `token` (default: `defaultToken()`). */
export function client(fn, defaultToken) {
  return async function call(method, path, { body, token = defaultToken() } = {}) {
    const headers = {};
    if (body !== undefined) headers["content-type"] = "application/json";
    if (token) headers.authorization = `Bearer ${token}`;
    const response = await fn.fetch(
      new Request(BASE + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }),
    );
    const text = await response.text();
    const isJson = response.headers.get("content-type")?.includes("json");
    return { status: response.status, headers: response.headers, json: isJson && text ? JSON.parse(text) : undefined, text };
  };
}
