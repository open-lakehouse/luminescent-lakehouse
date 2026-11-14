// End-to-end smoke test for the built function bundle (`dist/`), driven the
// way Neon drives it: fetch `Request` in, `Response` out. Needs DATABASE_URL on
// a Postgres whose `unitycatalog` database is migrated (the addon swaps the
// database in), with `authz` migrated and fixtures/identity.sql applied in the
// database itself (neondb's stand-in); run via `just test unitycatalog` (host)
// or `just test-arm64 unitycatalog` (linux-arm64 in Docker).
//
// Callers authenticate as on a branch, with local stand-ins for its Neon Auth
// and its `authz` function (fixtures/harness.mjs), the PDP with a fixed policy
// (`decide` below): the real policies are tested in crates/authz-neon.
// Vending with it on is vending.test.mjs, in a process of its own.

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { test } from "node:test";
import { API, API_KEY, BANNED, client, configYaml, RESOURCE, standIns, USER, unique } from "./fixtures/harness.mjs";

// The stand-in PDP: users may do anything but EXTERNAL USE, which they hold
// only in catalogs named `vend_*` (to reach the vend itself); service accounts
// may only read catalogs named `visible_*`, and (to list them) the metastore.
function decide({ principal, action, resource }) {
  const read = (type, id) => action.id === "read" && resource.type === `UnityCatalog::${type}` && id(resource.id);
  if (principal.type === "Lakehouse::User") return action.id !== "external_use" || resource.id.startsWith("vend_");
  return read("Metastore", (id) => id === "metastore") || read("Catalog", (id) => id.startsWith("visible_"));
}
const { origin: ORIGIN, authUrl: AUTH_URL, authzToken: AUTHZ_TOKEN, asked, accessToken, sessionToken } =
  await standIns(decide);

// neon.ts's document with vending off, as on production, plus a managed
// storage root so a catalog can be created without registering an external
// location first. Nothing is written to it.
process.env.UC_SERVER_CONFIG_YAML ??= configYaml({ head: ["managed_storage_root: s3://smoke-bucket/managed"] });

const { default: fn, authzUrl } = await import("./dist/index.mjs");
const call = client(fn, () => accessToken());
const catalog = unique();

test("health needs no token", async () => {
  const res = await call("GET", "/health", { token: null });
  assert.equal(res.status, 200);
  assert.equal(res.text, "OK");
});

test("the protected-resource metadata needs no token and names the resource", async () => {
  const res = await call("GET", "/.well-known/oauth-protected-resource", { token: null });
  assert.equal(res.status, 200, res.text);
  assert.equal(res.json.resource, RESOURCE);
  assert.deepEqual(res.json.authorization_servers, [AUTH_URL]);
});

test("no token is a 401 with a Bearer challenge", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: null });
  assert.equal(res.status, 401, res.text);
  assert.match(res.headers.get("www-authenticate") ?? "", /^Bearer/i);
});

test("an OAuth access token for the resource is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: accessToken() });
  assert.equal(res.status, 200, res.text);
});

test("a web session token is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: sessionToken() });
  assert.equal(res.status, 200, res.text);
});

test("a service account's API key is accepted", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: API_KEY });
  assert.equal(res.status, 200, res.text);
});

test("tokens that aren't for this function are 401s", async (t) => {
  const cases = {
    "another resource": accessToken({}, "https://headwaters.luminescent-lakehouse.dev"),
    "another issuer": accessToken({ iss: `${ORIGIN}/other/auth` }),
    "an unknown user": accessToken({ sub: randomUUID() }),
    expired: accessToken({ exp: Math.floor(Date.now() / 1000) - 3600 }),
    "an unknown API key": `olk_5e1f00000000a001_${"00".repeat(32)}`,
    garbage: "not-a-token",
  };
  for (const [name, token] of Object.entries(cases)) {
    await t.test(name, async () => {
      const res = await call("GET", `${API}/catalogs`, { token });
      assert.equal(res.status, 401, res.text);
    });
  }
});

test("a banned user is a 403", async () => {
  const res = await call("GET", `${API}/catalogs`, { token: accessToken({ sub: BANNED }) });
  assert.equal(res.status, 403, res.text);
});

test("create a catalog and a schema, then read them back", async () => {
  const created = await call("POST", `${API}/catalogs`, { body: { name: catalog, comment: "smoke" } });
  assert.equal(created.status, 200, created.text);
  assert.equal(created.json.name, catalog);

  const schema = await call("POST", `${API}/schemas`, { body: { name: "s", catalog_name: catalog } });
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

test("the authz function decides, asked with the caller secret", async () => {
  asked.length = 0;
  const name = unique();
  const created = await call("POST", `${API}/catalogs`, { body: { name } });
  assert.equal(created.status, 200, created.text);
  const ask = asked.find((a) => a.action.id === "create");
  assert.ok(ask, JSON.stringify(asked));
  assert.equal(ask.secret, `Bearer ${AUTHZ_TOKEN}`);
  assert.deepEqual(ask.principal, { type: "Lakehouse::User", id: USER });
  // The new catalog itself, under the metastore: a `create` grant there covers it.
  assert.deepEqual(ask.resource, { type: "UnityCatalog::Catalog", id: name });
  assert.deepEqual(ask.resource_entity.parents, [{ type: "UnityCatalog::Metastore", id: "metastore" }]);
  await call("DELETE", `${API}/catalogs/${name}?force=true`);
});

test("a deny is a 403, and lists show only what may be read", async () => {
  const hidden = unique();
  const visible = unique("visible");
  for (const name of [hidden, visible]) {
    const res = await call("POST", `${API}/catalogs`, { body: { name } });
    assert.equal(res.status, 200, res.text);
  }
  try {
    const create = await call("POST", `${API}/catalogs`, { body: { name: `${hidden}_x` }, token: API_KEY });
    assert.equal(create.status, 403, create.text);
    const get = await call("GET", `${API}/catalogs/${hidden}`, { token: API_KEY });
    assert.equal(get.status, 403, get.text);
    const list = await call("GET", `${API}/catalogs`, { token: API_KEY });
    assert.equal(list.status, 200, list.text);
    const names = (list.json.catalogs ?? []).map((c) => c.name);
    assert.ok(names.includes(visible), list.text);
    assert.ok(!names.includes(hidden), list.text);
  } finally {
    for (const name of [hidden, visible]) await call("DELETE", `${API}/catalogs/${name}?force=true`);
  }
});

test("the authz function's URL follows from this function's host", () => {
  assert.equal(
    authzUrl("br-cool-darkness-a1b2c3-unitycatalog.compute.c-3.us-east-2.aws.neon.tech"),
    "https://br-cool-darkness-a1b2c3-authz.compute.c-3.us-east-2.aws.neon.tech",
  );
  assert.equal(authzUrl("unitycatalog.example"), undefined);
  assert.equal(authzUrl("br-x-headwaters.compute.c-3.us-east-2.aws.neon.tech"), undefined);
});

test("with vending off, a Neon credential registers but doesn't vend", async () => {
  const name = unique("vend");
  const credential = unique("neon");
  const location = unique("lake");
  const url = `s3://${location.replaceAll("_", "-")}`;
  const setup = [
    ["POST", `${API}/credentials`, { name: credential, purpose: "STORAGE", aws_iam_role: { role_arn: "arn:neon:storage::proj-smoke:branch/br-smoke" } }],
    ["POST", `${API}/external-locations`, { name: location, url, credential_name: credential }],
    ["POST", `${API}/catalogs`, { name, storage_root: `${url}/${name}` }],
    ["POST", `${API}/schemas`, { name: "s", catalog_name: name }],
  ];
  try {
    for (const [method, path, body] of setup) {
      const res = await call(method, path, { body });
      assert.equal(res.status, 200, `${path}: ${res.text}`);
    }
    const table = await call("POST", `${API}/tables`, {
      body: { name: "t", catalog_name: name, schema_name: "s", table_type: "EXTERNAL", data_source_format: "DELTA", storage_location: `${url}/${name}/s/t` },
    });
    assert.equal(table.status, 200, table.text);
    const vend = await call("POST", `${API}/temporary-table-credentials`, { body: { table_id: table.json.table_id, operation: "READ" } });
    assert.equal(vend.status, 400, vend.text);
    assert.match(vend.text, /FAILED_PRECONDITION/);
    assert.match(vend.text, /vending\.neon\.enabled/);
  } finally {
    await call("DELETE", `${API}/catalogs/${name}?force=true`);
    await call("DELETE", `${API}/external-locations/${location}?force=true`);
    await call("DELETE", `${API}/credentials/${credential}?force=true`);
  }
});
