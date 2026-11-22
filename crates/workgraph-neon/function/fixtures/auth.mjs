// A local stand-in for the branch's Neon Auth (the JWKS of a fresh Ed25519
// key, on loopback; the tests sign the tokens), and the callers of
// fixtures/identity.sql. Trimmed from crates/unitycatalog-neon's harness: the
// arm64 run mounts only this function's directory.

import { generateKeyPairSync, sign } from "node:crypto";
import { createServer } from "node:http";
import { after } from "node:test";

export const RESOURCE = "https://workgraph.luminescent-lakehouse.dev";
export const OPERATOR = "3c0f0000-0000-4000-8000-000000000001";
export const WATCHER = "3c0f0000-0000-4000-8000-000000000002";
export const BANNED = "3c0f0000-0000-4000-8000-000000000003";
const key = (n) => `olk_3c0f00000000a00${n}_${"3c".repeat(32)}`;
export const KEYS = {
  planner: key(1),
  reviewer: key(2),
  stranger: key(3),
  revoked: key(4),
  expired: key(5),
};
const KID = "smoke";

/**
 * Serve the JWKS; resolves to the env Neon sets for a branch with Neon Auth,
 * and the token signers.
 */
export async function neonAuth() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const jwks = JSON.stringify({ keys: [{ ...publicKey.export({ format: "jwk" }), kid: KID, alg: "EdDSA", use: "sig" }] });
  const server = createServer((req, res) => {
    res.writeHead(req.url === "/neondb/auth/.well-known/jwks.json" ? 200 : 404, { "content-type": "application/json" });
    res.end(jwks);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  after(() => {
    server.closeAllConnections();
    server.close();
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const authUrl = `${origin}/neondb/auth`;

  const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
  const jwt = (claims, header = {}) => {
    const now = Math.floor(Date.now() / 1000);
    const input = `${b64({ alg: "EdDSA", kid: KID, ...header })}.${b64({ sub: OPERATOR, iat: now, exp: now + 300, ...claims })}`;
    return `${input}.${sign(null, Buffer.from(input), privateKey).toString("base64url")}`;
  };
  /** An OAuth access token for `resource` (an MCP client signed in). */
  const accessToken = (claims = {}, resource = RESOURCE) =>
    jwt({ iss: authUrl, aud: [resource, `${authUrl}/oauth2/userinfo`], client_id: "smoke", ...claims }, { typ: "at+jwt" });
  /** The web app's session token: `iss` and `aud` are the Auth URL's origin. */
  const sessionToken = (claims = {}) => jwt({ iss: origin, aud: origin, ...claims });

  return {
    authUrl,
    env: {
      NEON_AUTH_BASE_URL: authUrl,
      NEON_AUTH_JWKS_URL: `${authUrl}/.well-known/jwks.json`,
      WORKGRAPH_RESOURCE: RESOURCE,
    },
    accessToken,
    sessionToken,
  };
}
