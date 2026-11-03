// End-to-end smoke test for the built function bundle (`dist/`), driven the
// way Neon drives it: fetch `Request` in, `Response` out. Needs DATABASE_URL on
// a Postgres whose `unitycatalog` database is migrated (the addon swaps the
// database in); run via `just test unitycatalog` (host) or
// `just test-arm64 unitycatalog` (linux-arm64 in Docker).

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { test } from "node:test";

// What neon.ts sets, with a throwaway KEK in place of the branch secret, plus a
// managed storage root so a catalog can be created without registering an
// external location first. Nothing is written to it.
process.env.UC_SERVER_KEK ??= Buffer.alloc(32, 7).toString("base64");
process.env.UC_SERVER_CONFIG_YAML ??= [
  "managed_storage_root: s3://smoke-bucket/managed",
  "ui:",
  "  serve: false",
  "auth:",
  "  mode: anonymous",
  "encryption:",
  "  active:",
  "    id: smoke",
  "    key: { env: UC_SERVER_KEK }",
].join("\n");

const { default: fn } = await import("./dist/index.mjs");

const BASE = "https://unitycatalog.example";
const API = "/api/2.1/unity-catalog";
const catalog = `smoke_${randomUUID().replaceAll("-", "")}`;

async function call(method, path, body) {
  const response = await fn.fetch(
    new Request(BASE + path, {
      method,
      headers: body === undefined ? {} : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    }),
  );
  const text = await response.text();
  const isJson = response.headers.get("content-type")?.includes("json");
  return { status: response.status, json: isJson && text ? JSON.parse(text) : undefined, text };
}

test("health", async () => {
  const res = await call("GET", "/health");
  assert.equal(res.status, 200);
  assert.equal(res.text, "OK");
});

test("create a catalog and a schema, then read them back", async () => {
  const created = await call("POST", `${API}/catalogs`, { name: catalog, comment: "smoke" });
  assert.equal(created.status, 200, created.text);
  assert.equal(created.json.name, catalog);

  const schema = await call("POST", `${API}/schemas`, { name: "s", catalog_name: catalog });
  assert.equal(schema.status, 200, schema.text);
  assert.equal(schema.json.full_name, `${catalog}.s`);

  const catalogs = await call("GET", `${API}/catalogs`);
  assert.equal(catalogs.status, 200, catalogs.text);
  assert.ok(
    (catalogs.json.catalogs ?? []).some((c) => c.name === catalog),
    `catalog ${catalog} not listed: ${catalogs.text}`,
  );

  const schemas = await call("GET", `${API}/schemas?catalog_name=${catalog}`);
  assert.equal(schemas.status, 200, schemas.text);
  assert.deepEqual(
    (schemas.json.schemas ?? []).map((s) => s.name).filter((n) => n !== "information_schema"),
    ["s"],
  );

  const deleted = await call("DELETE", `${API}/catalogs/${catalog}?force=true`);
  assert.equal(deleted.status, 200, deleted.text);
});

test("an unknown catalog is a 404", async () => {
  const res = await call("GET", `${API}/catalogs/missing_${randomUUID().replaceAll("-", "")}`);
  assert.equal(res.status, 404, res.text);
});
