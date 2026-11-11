// One deployment serves every Neon branch, of any Neon project in any region.
// The path names the branch's Neon Auth server, and the pages talk to it:
//
//   /a/<auth-host>/<db>/…
//   e.g. /a/ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/
//
// That is the branch's Auth URL (`https://<auth-host>/<db>/auth`) without the
// scheme. The build needs no configuration, so any static host can serve it
// (docs/web.md, "Self-hosting").
//
// The older `/b/<endpoint-id>/…` form rebuilds the Auth URL from two build-time
// settings shared by one project's branches (VITE_NEON_AUTH_HOST_SUFFIX,
// VITE_NEON_AUTH_DB). It only works where those are set, and stays until every
// branch's OAuth server points at `/a/` pages.

/** A Neon compute endpoint id, e.g. `ep-cool-darkness-a1b2c3d4`. */
export const ENDPOINT_RE = /^ep-[a-z0-9-]+$/;

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

export function isEndpoint(value: string | undefined): value is string {
  return value !== undefined && ENDPOINT_RE.test(value);
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

type AuthEnv = Pick<ImportMetaEnv, "VITE_NEON_AUTH_HOST_SUFFIX" | "VITE_NEON_AUTH_DB">;

/**
 * `/b/<endpoint-id>/…`: the branch, from the build's settings. Undefined for a
 * malformed endpoint id, or a build without them (any self-hosted one).
 */
export function branchFromEndpoint(
  endpoint: string | undefined,
  env: AuthEnv = import.meta.env,
): Branch | undefined {
  if (!isEndpoint(endpoint)) return undefined;
  const suffix = env.VITE_NEON_AUTH_HOST_SUFFIX;
  const db = env.VITE_NEON_AUTH_DB;
  if (!suffix || !db) return undefined;
  const host = `${endpoint}.${suffix}`;
  if (!AUTH_HOST_RE.test(host) || !DB_RE.test(db)) return undefined;
  return { authUrl: `https://${host}/${db}/auth`, base: `/b/${endpoint}`, label: endpoint };
}
