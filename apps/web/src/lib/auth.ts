// Neon Auth for the branch named by the path. Each branch has its own Auth
// server (and session cookie, on that server's origin), so there is one client
// per endpoint, created on first use.
//
// The Better Auth vanilla client: sign-in is `signIn.social({ provider })`,
// sign-out `signOut()`, and `useSession` is the session nanostore.

import { BetterAuthVanillaAdapter } from "@neondatabase/neon-js";
import { createAuthClient } from "@neondatabase/neon-js/auth";
import { useSyncExternalStore } from "react";
import { authUrl } from "./endpoint";

type AuthClient = ReturnType<typeof makeClient>;

function makeClient(endpoint: string) {
  return createAuthClient(authUrl(endpoint), { adapter: BetterAuthVanillaAdapter() });
}

const clients = new Map<string, AuthClient>();

function client(endpoint: string): AuthClient {
  let c = clients.get(endpoint);
  if (!c) {
    c = makeClient(endpoint);
    clients.set(endpoint, c);
  }
  return c;
}

/**
 * The one-time query param Neon Auth appends to the OAuth callback URL. Leave it
 * out of the next callback, or the SDK would re-send a spent verifier.
 */
const SESSION_VERIFIER_PARAM = "neon_auth_session_verifier";

/** Start GitHub sign-in on the branch's Auth server, returning to this page. */
export async function signIn(endpoint: string): Promise<void> {
  const callback = new URL(window.location.href);
  callback.searchParams.delete(SESSION_VERIFIER_PARAM);
  await client(endpoint).signIn.social({ provider: "github", callbackURL: callback.href });
}

export async function signOut(endpoint: string): Promise<void> {
  await client(endpoint).signOut();
}

export type Session = ReturnType<AuthClient["useSession"]["get"]>;

/** The branch's session store as React state: `{ data, isPending, error }`. */
export function useSession(endpoint: string): Session {
  const store = client(endpoint).useSession;
  return useSyncExternalStore(
    (onChange) => store.listen(onChange),
    () => store.get(),
  );
}
