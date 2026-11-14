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

export const user = (n) => ({ type: "Lakehouse::User", id: `aaaaaaaa-0000-0000-0000-00000000000${n}` });

/** A Unity Catalog request for `action` on the table `main.sales.orders`, as olai-uc-server asks it. */
export function ucTable(principal, action, table = "main.sales.orders") {
  const [catalog, schema] = table.split(".");
  const resource = { type: "UnityCatalog::Table", id: table };
  return {
    principal,
    action: { type: "UnityCatalog::Action", id: action },
    resource,
    context: { auth_method: "jwt", scopes: [] },
    resource_entity: {
      uid: resource,
      attrs: {},
      parents: [
        { type: "UnityCatalog::Schema", id: `${catalog}.${schema}` },
        { type: "UnityCatalog::Catalog", id: catalog },
        { type: "UnityCatalog::Metastore", id: "metastore" },
      ],
    },
  };
}
