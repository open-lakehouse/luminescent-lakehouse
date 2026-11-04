// The services the shell has views for, by trestle module id, and where their
// APIs live under a binding's root.

export const UNITY_CATALOG = "unity-catalog";
export const HEADWATERS = "headwaters";

/** Unity Catalog's REST API root. */
export const UC_API_ROOT = "/api/2.1/unity-catalog";
/** headwaters' read API (ConnectRPC) under its binding's root. The root is where the
 *  service serves itself: a Neon Function's URL, or `<gateway>/lineage/` on a local
 *  instance, where it runs under trestle's base path. */
export const HEADWATERS_READ_SERVICE = "/headwaters.read.v1.ReadService";
