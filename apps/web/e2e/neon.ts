// The end-to-end test's inputs and helpers: the preview branch's Neon Auth (via
// the Neon CLI), PKCE, and the access token's checks. What the branch should
// have (client, redirect URI, resource, web app origin) comes from the root
// neon.ts, the same `authSetup` that scripts/auth-setup.ts applies.

import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, randomBytes, verify } from "node:crypto";
import { authSetup, TENANT } from "../../../neon.ts";

/** Only preview branches: the test signs up (and deletes) a user on it. */
const PREVIEW_RE = /^preview\//;

export interface Target {
  branch: string;
  /** The branch's Neon Auth URL, `https://<endpoint>.neonauth…/<db>/auth`: the issuer. */
  authUrl: string;
  jwksUrl: string;
  clientId: string;
  redirectUri: string;
  /** The headwaters OAuth resource: the access token's `aud`. */
  resource: string;
  /** Where the branch's login and consent pages are. */
  pages: string;
  webOrigin: string;
}

export function branch(): string {
  const b = process.env.E2E_NEON_BRANCH;
  if (!b) throw new Error("set E2E_NEON_BRANCH to a preview branch, e.g. preview/pr-42");
  if (!PREVIEW_RE.test(b)) {
    throw new Error(`E2E_NEON_BRANCH='${b}': the end-to-end test only runs on preview/* branches`);
  }
  return b;
}

function neon(args: string[]): string {
  const project = process.env.NEON_PROJECT_ID ? ["--project-id", process.env.NEON_PROJECT_ID] : [];
  return execFileSync("bunx", ["neon", ...args, ...project], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}

export function resolveTarget(): Target {
  const b = branch();
  const status = JSON.parse(neon(["neon-auth", "status", "--branch", b, "-o", "json"])) as {
    base_url: string;
    jwks_url: string;
  };
  const setup = authSetup({ name: b, isDefault: false, authUrl: status.base_url });
  const clients = JSON.parse(
    neon(["neon-auth", "oauth-client", "list", "--branch", b, "-o", "json"]),
  ) as { client_id: string; name: string }[];
  const client = clients.find((c) => c.name === setup.cliClient.name);
  if (!client) throw new Error(`${b} has no OAuth client named '${setup.cliClient.name}'`);
  const redirectUri = setup.cliClient.redirectUris[0];
  const resource = setup.resources.headwaters;
  const loginPage = setup.oauthServer?.loginPage;
  if (!redirectUri || !resource || !loginPage)
    throw new Error("neon.ts: no CLI redirect URI, headwaters resource or login page");
  return {
    branch: b,
    authUrl: status.base_url.replace(/\/$/, ""),
    jwksUrl: status.jwks_url,
    clientId: client.client_id,
    redirectUri,
    resource,
    pages: loginPage.replace(/\/sign-in$/, ""),
    webOrigin: TENANT.webOrigin,
  };
}

export function deleteUser(b: string, id: string): void {
  neon(["neon-auth", "user", "delete", id, "--branch", b]);
}

const b64url = (buf: Buffer): string => buf.toString("base64url");

export function pkce(): { verifier: string; challenge: string; state: string } {
  const verifier = b64url(randomBytes(32));
  return {
    verifier,
    challenge: b64url(createHash("sha256").update(verifier).digest()),
    state: b64url(randomBytes(16)),
  };
}

export function authorizeUrl(
  t: Target,
  p: { challenge: string; state: string },
  extra: Record<string, string> = {},
): string {
  const q = new URLSearchParams({
    response_type: "code",
    client_id: t.clientId,
    redirect_uri: t.redirectUri,
    scope: "openid offline_access",
    state: p.state,
    code_challenge: p.challenge,
    code_challenge_method: "S256",
    resource: t.resource,
    ...extra,
  });
  return `${t.authUrl}/oauth2/authorize?${q}`;
}

export interface TokenResponse {
  access_token: string;
  refresh_token?: string;
  id_token?: string;
  token_type: string;
  expires_in: number;
}

/** POST the token endpoint; the status and the JSON body, whatever they are. */
export async function token(
  t: Target,
  params: Record<string, string>,
): Promise<{ status: number; body: Record<string, unknown> }> {
  const res = await fetch(`${t.authUrl}/oauth2/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ client_id: t.clientId, resource: t.resource, ...params }),
  });
  return { status: res.status, body: (await res.json()) as Record<string, unknown> };
}

export interface Jwt {
  header: { alg?: string; typ?: string; kid?: string };
  payload: { iss?: string; aud?: string | string[]; exp?: number; sub?: string };
}

/** Decode a JWT and check its signature against the branch's JWKS (Ed25519 only). */
export async function verifyJwt(t: Target, jwt: string): Promise<Jwt> {
  const [h, p, s] = jwt.split(".");
  if (!h || !p || !s) throw new Error("not a JWT");
  const header = JSON.parse(Buffer.from(h, "base64url").toString()) as Jwt["header"];
  const payload = JSON.parse(Buffer.from(p, "base64url").toString()) as Jwt["payload"];
  const { keys } = (await (await fetch(t.jwksUrl)).json()) as {
    keys: { kid?: string; kty: string; crv?: string; x?: string }[];
  };
  const jwk = keys.find((k) => k.kid === header.kid) ?? (keys.length === 1 ? keys[0] : undefined);
  if (!jwk) throw new Error(`no JWKS key for kid '${header.kid}'`);
  const key = createPublicKey({ key: jwk, format: "jwk" });
  if (!verify(null, Buffer.from(`${h}.${p}`), key, Buffer.from(s, "base64url"))) {
    throw new Error("bad JWT signature");
  }
  return { header, payload };
}
