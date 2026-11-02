// One deployment serves every Neon branch: the branch's compute endpoint id is
// the first path segment (/b/<endpoint-id>/…), and its Neon Auth URL is rebuilt
// from it. Only the parts every branch of the project shares are configured.

/** A Neon compute endpoint id, e.g. `ep-cool-darkness-a1b2c3d4`. */
export const ENDPOINT_RE = /^ep-[a-z0-9-]+$/;

export function isEndpoint(value: string | undefined): value is string {
  return value !== undefined && ENDPOINT_RE.test(value);
}

type AuthEnv = Pick<ImportMetaEnv, "VITE_NEON_AUTH_HOST_SUFFIX" | "VITE_NEON_AUTH_DB">;

/** The branch's Neon Auth URL: `https://<endpoint>.<host suffix>/<db>/auth`. */
export function authUrl(endpoint: string, env: AuthEnv = import.meta.env): string {
  if (!isEndpoint(endpoint)) throw new Error(`not a Neon endpoint id: ${endpoint}`);
  const suffix = env.VITE_NEON_AUTH_HOST_SUFFIX;
  const db = env.VITE_NEON_AUTH_DB;
  if (!suffix || !db) {
    throw new Error(
      "VITE_NEON_AUTH_HOST_SUFFIX and VITE_NEON_AUTH_DB must be set (see apps/web/.env.example)",
    );
  }
  return `https://${endpoint}.${suffix}/${db}/auth`;
}
