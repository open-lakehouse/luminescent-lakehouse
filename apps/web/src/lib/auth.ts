// Neon Auth for the branch named by the path. Each branch has its own Auth
// server (and session cookie, on that server's origin), so there is one client
// per Auth URL, created on first use.
//
// The Better Auth vanilla client: sign-in is `signIn.social({ provider })`,
// sign-out `signOut()`, and `useSession` is the session nanostore. Its OAuth
// provider plugin backs the login and consent pages: on any POST it sends the
// page's signed authorization query as `oauth_query`, and the client follows
// the `{ redirect, url }` the server answers with. The SDK's types leave out
// that plugin's `oauth2.*` methods, so the pages call its endpoints through
// `$fetch`, which runs the same fetch plugins.

import { BetterAuthVanillaAdapter } from "@neon/neon-js";
import { createAuthClient } from "@neon/neon-js/auth";
import { useSyncExternalStore } from "react";

type AuthClient = ReturnType<typeof makeClient>;

function makeClient(authUrl: string) {
  return createAuthClient(authUrl, { adapter: BetterAuthVanillaAdapter() });
}

const clients = new Map<string, AuthClient>();

function client(authUrl: string): AuthClient {
  let c = clients.get(authUrl);
  if (!c) {
    c = makeClient(authUrl);
    clients.set(authUrl, c);
  }
  return c;
}

/**
 * The one-time query param Neon Auth appends to the OAuth callback URL. Leave it
 * out of the next callback, or the SDK would re-send a spent verifier.
 */
const SESSION_VERIFIER_PARAM = "neon_auth_session_verifier";

/**
 * Start GitHub sign-in on the branch's Auth server, returning to this page. On
 * the login page, the return URL keeps the signed authorization query, so the
 * page can resume the authorization afterwards.
 */
export async function signIn(authUrl: string): Promise<void> {
  const callback = new URL(window.location.href);
  callback.searchParams.delete(SESSION_VERIFIER_PARAM);
  await client(authUrl).signIn.social({ provider: "github", callbackURL: callback.href });
}

export async function signOut(authUrl: string): Promise<void> {
  await client(authUrl).signOut();
}

export type Session = ReturnType<AuthClient["useSession"]["get"]>;

/** The branch's session store as React state: `{ data, isPending, error }`. */
export function useSession(authUrl: string): Session {
  const store = client(authUrl).useSession;
  return useSyncExternalStore(
    (onChange) => store.listen(onChange),
    () => store.get(),
  );
}

/** A failed call: the HTTP status, and the server's message. */
export interface AuthFailure {
  status: number;
  message: string;
}

function failure(error: { status: number; message?: string } | null): AuthFailure | undefined {
  return error ? { status: error.status, message: error.message ?? "" } : undefined;
}

/**
 * Resume the authorization this login page was sent for, as the signed-in
 * user. On success the SDK navigates on (to consent, or back to the client).
 */
export async function continueAuthorization(authUrl: string): Promise<AuthFailure | undefined> {
  const { error } = await client(authUrl).$fetch("/oauth2/continue", {
    method: "POST",
    body: { selected: true },
  });
  return failure(error);
}

/** Answer the consent page; either way the SDK navigates back to the client. */
export async function consent(authUrl: string, accept: boolean): Promise<AuthFailure | undefined> {
  const { error } = await client(authUrl).$fetch("/oauth2/consent", {
    method: "POST",
    body: { accept },
  });
  return failure(error);
}

/** The client's self-description. Only the name is shown; it proves nothing. */
export interface PublicClient {
  client_id: string;
  client_name?: string;
  client_uri?: string;
}

export async function publicClient(authUrl: string, clientId: string): Promise<PublicClient> {
  const { data, error } = await client(authUrl).$fetch<PublicClient>("/oauth2/public-client", {
    method: "GET",
    query: { client_id: clientId },
  });
  if (error) throw new Error(error.message ?? `HTTP ${error.status}`);
  return data;
}
