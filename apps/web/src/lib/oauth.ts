// The authorization request a consent page is shown for. Neon Auth sends the
// request's parameters as the page's query (plus its signature, which the SDK
// passes back as `oauth_query`); this reads what the user is asked to approve.

export interface ConsentRequest {
  clientId: string;
  scopes: string[];
  resources: string[];
  /** Where the code goes: the requested redirect URI. */
  redirectUri: string;
  redirectHost: string;
  /** The code goes to an app on this computer, whoever the client claims to be. */
  loopback: boolean;
}

/** The request, or undefined when the query isn't one (no client or redirect URI). */
export function consentRequest(search: URLSearchParams): ConsentRequest | undefined {
  const clientId = search.get("client_id");
  const redirectUri = search.get("redirect_uri");
  if (!clientId || !redirectUri) return undefined;
  let url: URL;
  try {
    url = new URL(redirectUri);
  } catch {
    return undefined;
  }
  return {
    clientId,
    scopes: (search.get("scope") ?? "").split(" ").filter(Boolean),
    resources: search.getAll("resource"),
    redirectUri,
    redirectHost: url.host,
    loopback: isLoopback(url),
  };
}

/** `localhost`, `127.0.0.0/8` or `[::1]`. */
export function isLoopback(url: URL): boolean {
  const host = url.hostname;
  return host === "localhost" || host === "[::1]" || /^127(\.\d{1,3}){3}$/.test(host);
}
