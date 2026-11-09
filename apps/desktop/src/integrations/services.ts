// The services the shell has views for, by the key it addresses them by, and where
// their APIs live under a binding's root. A service is keyed by the API it serves when
// other implementations serve it too (Unity Catalog: `unity-catalog-oss` and
// `unity-catalog-rs` both serve `unity-catalog`), else by its trestle module id.

export const UNITY_CATALOG = "unity-catalog";
/** The Unity Catalog implementation that can run as a Neon Function (slug
 *  `unitycatalog`): what a selection hosts there. */
export const UNITY_CATALOG_RS = "unity-catalog-rs";
export const HEADWATERS = "headwaters";

/** Unity Catalog's REST API root. */
export const UC_API_ROOT = "/api/2.1/unity-catalog";
/** headwaters' read API (ConnectRPC) under its binding's root. The root is where the
 *  service serves itself: a Neon Function's URL, or `<gateway>/lineage/` on a local
 *  instance, where it runs under trestle's base path. */
export const HEADWATERS_READ_SERVICE = "/headwaters.read.v1.ReadService";
