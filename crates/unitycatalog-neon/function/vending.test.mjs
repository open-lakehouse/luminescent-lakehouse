// Trusted Neon credential vending, end to end through the built bundle
// (`dist/`), with vending on as neon.ts turns it on for staging and previews,
// against a fake Neon control plane. Its own file, so its own process: the
// addon reads its config once. Run like smoke.test.mjs (`just test
// unitycatalog`, `just test-arm64 unitycatalog`).
//
// The fake answers what trestle's minter asks (olai-http's Neon client):
// the branch's storage hints, then creating, listing and revoking the
// branch's credentials, all with the function's NEON_API_KEY as Bearer.

import assert from "node:assert/strict";
import { after, test } from "node:test";
import { API, client, configYaml, READER, readJson, serve, standIns, USER, unique } from "./fixtures/harness.mjs";

const PROJECT = "proj-smoke-12345678";
const BRANCH = "br-smoke-a1b2c3d4";
const NEON_KEY = "napi_smoke";
const ENDPOINT = "https://br-smoke-a1b2c3d4.storage.c-7.eu-central-1.aws.neon.tech";

// The stand-in PDP: users may do anything; EXTERNAL USE only USER holds.
const { accessToken } = await standIns(
  ({ principal, action }) =>
    principal.type === "Lakehouse::User" && (action.id !== "external_use" || principal.id === USER),
);

/** What the minter asked the control plane. */
const minted = [];
const listed = [];
const neon = await serve(async (req, res) => {
  const branchPath = `/projects/${PROJECT}/branches/${BRANCH}`;
  const send = (status, body) => {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  };
  if (req.headers.authorization !== `Bearer ${NEON_KEY}`) return send(401, { code: "", message: "bad key" });
  if (req.method === "GET" && req.url === `${branchPath}/storage`) {
    return send(200, { enabled: true, s3_endpoint: ENDPOINT, region: "eu-central-1", force_path_style: true });
  }
  if (req.method === "POST" && req.url === `${branchPath}/credentials`) {
    const body = await readJson(req);
    minted.push(body);
    return send(201, { token_id: `nak_smoke${minted.length}`, s3_secret_access_key: `secret-${minted.length}`, expires_at: body.expires_at });
  }
  if (req.method === "GET" && req.url === `${branchPath}/credentials`) {
    listed.push(Date.now());
    return send(200, { credentials: [] });
  }
  if (req.method === "DELETE" && req.url.startsWith(`${branchPath}/credentials/`)) return send(204, {});
  return send(404, { code: "", message: `no ${req.method} ${req.url}` });
});

process.env.NEON_API_KEY = NEON_KEY;
process.env.NEON_BRANCH_ID = BRANCH;
// neon.ts's document for a preview: the managed root on the bucket, vending
// on, the project from neon.ts, the branch from NEON_BRANCH_ID.
process.env.UC_SERVER_CONFIG_YAML = configYaml({
  head: ["managed_storage_root: s3://lakehouse/managed", "managed_storage_root_credential: neon"],
  vending: [
    "enabled: true",
    `project-id: "${PROJECT}"`,
    "api-key: { env: NEON_API_KEY }",
    "branch-id: { env: NEON_BRANCH_ID }",
    "ttl-secs: 900",
    `api-url: "${neon}/"`,
  ],
});

const { default: fn } = await import("./dist/index.mjs");
const call = client(fn, () => accessToken());
const must = async (method, path, body) => {
  const res = await call(method, path, { body });
  assert.equal(res.status, 200, `${method} ${path}: ${res.text}`);
  return res.json;
};

// As scripts/storage-seed.ts registers the branch's storage (the ids come
// from `vending.neon`), under names of this run's: the test database stays.
const catalog = unique("vend");
const credential = unique("neon");
const location = unique("lake");
const bucket = location.replaceAll("_", "-");
await must("POST", `${API}/credentials`, { name: credential, purpose: "STORAGE", aws_iam_role: { role_arn: "arn:neon:storage::" } });
await must("POST", `${API}/external-locations`, { name: location, url: `s3://${bucket}`, credential_name: credential });
after(async () => {
  await call("DELETE", `${API}/catalogs/${catalog}?force=true`);
  await call("DELETE", `${API}/external-locations/${location}?force=true`);
  await call("DELETE", `${API}/credentials/${credential}?force=true`);
});
await must("POST", `${API}/catalogs`, { name: catalog, storage_root: `s3://${bucket}/${catalog}` });
await must("POST", `${API}/schemas`, { name: "s", catalog_name: catalog });
const table = await must("POST", `${API}/tables`, {
  name: "t",
  catalog_name: catalog,
  schema_name: "s",
  table_type: "EXTERNAL",
  data_source_format: "DELTA",
  storage_location: `s3://${bucket}/${catalog}/s/t`,
});

const vend = (operation, token) =>
  call("POST", `${API}/temporary-table-credentials`, { body: { table_id: table.table_id, operation }, token });

test("an empty Neon ARN is completed from vending.neon, and withheld like a secret", async () => {
  const stored = await must("GET", `${API}/credentials/${credential}`);
  assert.equal(stored.aws_iam_role, undefined, JSON.stringify(stored));
  // The fake answers only on this project's branch, so a mint is the proof
  // that the ARN names them: the vends below.
  const res = await vend("READ");
  assert.equal(res.status, 200, res.text);
});

test("a read vend mints one read-only branch key, cached across vends", async () => {
  const first = await vend("READ");
  assert.equal(first.status, 200, first.text);
  const creds = first.json.aws_temp_credentials;
  assert.match(creds.access_key_id, /^nak_smoke\d+$/);
  assert.match(creds.secret_access_key, /^secret-\d+$/);
  assert.equal(first.json.url, `s3://${bucket}/${catalog}/s/t`);
  // Expiring within the TTL (900 s), in epoch milliseconds.
  const left = first.json.expiration_time - Date.now();
  assert.ok(left > 0 && left <= 900_000 + 5_000, `expires in ${left} ms`);

  assert.deepEqual(minted.map((m) => m.scopes), [["storage:read"]], "one read key for every read vend so far");
  assert.match(minted.at(-1).name, /^olai-vend-/);
  assert.equal(minted.at(-1).principal_type, "user");

  const second = await vend("READ");
  assert.equal(second.status, 200, second.text);
  assert.equal(second.json.aws_temp_credentials.access_key_id, creds.access_key_id, "cached to half its TTL");
  assert.equal(minted.length, 1);
});

test("a write vend mints a read+write key", async () => {
  const res = await vend("READ_WRITE");
  assert.equal(res.status, 200, res.text);
  assert.deepEqual(minted.at(-1).scopes, ["storage:read", "storage:write"]);
});

test("a caller without EXTERNAL USE is refused, and nothing is minted", async () => {
  const before = minted.length;
  const res = await vend("READ", accessToken({ sub: READER }));
  assert.equal(res.status, 403, res.text);
  assert.equal(minted.length, before);
});

test("no token is a 401", async () => {
  const res = await vend("READ", null);
  assert.equal(res.status, 401, res.text);
});

test("the Delta API's credentials carry the branch's endpoint", async () => {
  const res = await call(
    "GET",
    `${API}/delta/v1/catalogs/${catalog}/schemas/s/tables/t/credentials?operation=READ`,
  );
  assert.equal(res.status, 200, res.text);
  const [cred] = res.json["storage-credentials"];
  assert.equal(cred.prefix, `s3://${bucket}/${catalog}/s/t`);
  assert.match(cred.config["s3.access-key-id"], /^nak_smoke\d+$/);
  assert.equal(cred.config["s3.endpoint"], ENDPOINT);
  assert.equal(cred.config["s3.region"], "eu-central-1");
  assert.equal(cred.config["s3.path-style-access"], "true");
});

test("the minter garbage-collects after a mint that created a key", () => {
  assert.ok(listed.length >= 1, "listed the branch's credentials");
});
