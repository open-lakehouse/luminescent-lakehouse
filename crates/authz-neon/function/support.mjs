// Shared by the authz tests: throwaway caller secrets and a client that drives
// the built handler (`dist/index.mjs`) the way Neon does, a fetch `Request` in
// and a `Response` out.

export const TOKENS = { UNITYCATALOG: "test-secret-unitycatalog", HEADWATERS: "test-secret-headwaters" };

export const BASE = "https://authz.example";

/** Import the handler after setting the callers' secrets (each test file runs in its own process). */
export async function load(dist = "./dist/index.mjs") {
  for (const [service, token] of Object.entries(TOKENS)) process.env[`AUTHZ_TOKEN_${service}`] ??= token;
  return import(dist);
}

/** POST `body` (JSON, or a raw string) to `path`, with the `token` as bearer when given. */
export async function post(fn, path, body, token = TOKENS.UNITYCATALOG) {
  const headers = { "content-type": "application/json" };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fn.fetch(
    new Request(BASE + path, {
      method: "POST",
      headers,
      body: typeof body === "string" ? body : JSON.stringify(body),
    }),
  );
  const text = await response.text();
  const isJson = response.headers.get("content-type")?.includes("json");
  return { status: response.status, json: isJson && text ? JSON.parse(text) : undefined, text };
}

/** The seed.sql user `aaaaaaaa-0000-0000-0000-<n, hex, 12 digits>`. */
export const user = (n) => ({ type: "Lakehouse::User", id: `aaaaaaaa-0000-0000-0000-${String(n).padStart(12, "0")}` });

/** The seed.sql service account `bbbbbbbb-…-<n>`. */
export const serviceAccount = (n) => ({
  type: "Lakehouse::ServiceAccount",
  id: `bbbbbbbb-0000-0000-0000-${String(n).padStart(12, "0")}`,
});

// Where each kind of Unity Catalog securable sits: its containers, outermost
// first, as the attribute names mangrove's schema gives them.
const CONTAINERS = {
  Catalog: [],
  Schema: [["Catalog", "catalog"]],
  Table: [["Catalog", "catalog"], ["Schema", "schema"]],
  Volume: [["Catalog", "catalog"], ["Schema", "schema"]],
  Function: [["Catalog", "catalog"], ["Schema", "schema"]],
  RegisteredModel: [["Catalog", "catalog"], ["Schema", "schema"]],
  Agent: [["Catalog", "catalog"], ["Schema", "schema"]],
  AgentSkill: [["Catalog", "catalog"], ["Schema", "schema"]],
  StagingTable: [["Catalog", "catalog"], ["Schema", "schema"]],
  ModelVersion: [["Catalog", "catalog"], ["Schema", "schema"], ["RegisteredModel", "registered_model"]],
  Column: [["Catalog", "catalog"], ["Schema", "schema"], ["Table", "table"]],
};

const ucRef = (kind, id) => ({ type: `UnityCatalog::${kind}`, id });

/** The entity of the securable `kind` named by `parts`, as olai-uc-server sends it: its containers as typed attributes, them and the metastore as parents. */
function ucEntity(kind, parts) {
  if (kind === "Metastore") return { uid: ucRef("Metastore", "metastore"), attrs: {}, parents: [] };
  const containers = CONTAINERS[kind] ?? [];
  const attrs = {};
  const parents = [];
  containers.forEach(([k, attr], i) => {
    const ref = ucRef(k, parts.slice(0, i + 1).join("."));
    attrs[attr] = { __entity: ref };
    parents.unshift(ref);
  });
  parents.push(ucRef("Metastore", "metastore"));
  return { uid: ucRef(kind, parts.join(".")), attrs, parents };
}

/**
 * A Unity Catalog request for `action` on the securable `kind` named `name`
 * (a full name; ignored for the metastore), as olai-uc-server asks it: the
 * securable and every ancestor as entities.
 */
export function uc(principal, action, kind = "Table", name = "main.sales.orders", context = {}) {
  const parts = kind === "Metastore" ? [] : name.split(".");
  const containers = CONTAINERS[kind] ?? [];
  const entities = [ucEntity(kind, parts)];
  containers.forEach(([k], i) => entities.push(ucEntity(k, parts.slice(0, i + 1))));
  if (kind !== "Metastore") entities.push(ucEntity("Metastore", []));
  return {
    principal,
    action: { type: "UnityCatalog::Action", id: action },
    resource: entities[0].uid,
    context: { auth_method: "jwt", scopes: [], ...context },
    entities,
  };
}

/** `uc` on the table `main.sales.orders` (or `table`). */
export const ucTable = (principal, action, table = "main.sales.orders", context = {}) =>
  uc(principal, action, "Table", table, context);
