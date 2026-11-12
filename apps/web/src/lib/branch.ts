// One deployment serves every Neon branch, of any Neon project in any region.
// The path names the branch's Neon Auth server, and the pages talk to it:
//
//   /a/<auth-host>/<db>/…
//   e.g. /a/ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/
//
// That is the branch's Auth URL (`https://<auth-host>/<db>/auth`) without the
// scheme. The build needs no configuration, so any static host can serve it
// (docs/web.md, "Self-hosting").

/**
 * A branch's Neon Auth host: `<endpoint-id>.neonauth.<…>.neon.tech`. Only
 * Neon's, so the pages can't be pointed at someone else's server.
 */
export const AUTH_HOST_RE = /^ep-[a-z0-9-]+\.neonauth(\.[a-z0-9-]+)+\.neon\.tech$/;

/** The database in the Auth URL's path, e.g. `neondb`. */
export const DB_RE = /^[A-Za-z0-9_-]{1,63}$/;

/** The branch the pages are for. */
export interface Branch {
  /** Its Neon Auth URL: the SDK's base URL, and the tokens' issuer. */
  authUrl: string;
  /** The path the pages live under, for links between them. */
  base: string;
  /** What the pages call the branch: its Auth host. */
  label: string;
}

/** `/a/<host>/<db>/…`: the branch, or undefined when the path names no Neon Auth server. */
export function branchFromAuthHost(
  host: string | undefined,
  db: string | undefined,
): Branch | undefined {
  if (host === undefined || db === undefined) return undefined;
  if (!AUTH_HOST_RE.test(host) || !DB_RE.test(db)) return undefined;
  return { authUrl: `https://${host}/${db}/auth`, base: `/a/${host}/${db}`, label: host };
}
