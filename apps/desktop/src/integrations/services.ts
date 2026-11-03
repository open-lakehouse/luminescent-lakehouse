// The services the shell has views for, by trestle module id, and where their
// APIs live under a binding's root.

export const UNITY_CATALOG = "unity-catalog";
export const HEADWATERS = "headwaters";

/** Unity Catalog's REST API root. */
export const UC_API_ROOT = "/api/2.1/unity-catalog";
/** Where headwaters serves its read API (trestle's `HEADWATERS_BASE_PATH`). */
export const HEADWATERS_BASE_PATH = "/lineage";
