// Trusted Neon credential vending, end to end on a preview branch (#124's
// "done when"): `just vending-e2e preview/pr-<n>`, and preview.yml's
// `vending-e2e` job after each preview deploy.
//
// The callers are two service accounts made for the run with
// `luminescent-admin`, with keys that expire in a day; both are disabled
// afterwards. Storage is checked with Bun's own S3 client, with vended keys
// only; the branch's facts come from the Neon CLI. No npm dependency.
//
// Not covered: a Delta client reading through UC with no storage config.
// delta-rs 1.6.6 can't decode the server's tables (`created_at` as a string,
// no `updated_at`), and DuckDB's delta extension ignores a custom S3 endpoint
// (docs/auth.md, "Credential vending on Neon").

import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { S3Client } from "bun";
import { LAKEHOUSE_BUCKET, TENANT } from "../../../neon.ts";

const API = "/api/2.1/unity-catalog";
/** The minter garbage-collects at most every 10 min, after a mint; plus slack. */
const GC_GRACE_MS = 15 * 60_000;

const branch = process.env.E2E_NEON_BRANCH ?? "";
if (!/^preview\//.test(branch)) {
  throw new Error(`E2E_NEON_BRANCH='${branch}': the vending test only runs on preview/* branches`);
}
const projectId = process.env.NEON_PROJECT_ID ?? TENANT.neonProjectId;
const run = process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_RUN_ID}-${process.env.GITHUB_RUN_ATTEMPT ?? 1}` : randomBytes(4).toString("hex");
const catalog = `e2e_vend_${run.replaceAll("-", "_")}`;
const table = `uc://${catalog}.s.t`;
const user = `e2e-vend-user-${run}`;
const reader = `e2e-vend-reader-${run}`;

function neonRaw(args: string[]): string {
  return execFileSync("bunx", ["neon", ...args, "--project-id", projectId], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}

/** The JSON document in the CLI's output, which may carry notices around it. */
function neonJson<T>(args: string[]): T {
  const out = neonRaw([...args, "-o", "json"]);
  const start = out.search(/[[{]/);
  return JSON.parse(out.slice(start, Math.max(out.lastIndexOf("}"), out.lastIndexOf("]")) + 1)) as T;
}

let databaseUrl = "";
function admin(args: string[]): string {
  const env = { ...process.env, DATABASE_URL: databaseUrl, SQLX_OFFLINE: "true" };
  const [cmd, pre] = process.env.LUMINESCENT_ADMIN
    ? [process.env.LUMINESCENT_ADMIN, []]
    : ["cargo", ["run", "-q", "-p", "luminescent-migrate", "--bin", "luminescent-admin", "--"]];
  return execFileSync(cmd, [...pre, ...args], { encoding: "utf8", env, stdio: ["ignore", "pipe", "inherit"] }).trim();
}

interface Credential {
  token_id: string;
  name?: string;
  branch_id: string;
  created_at?: string;
  expires_at?: string;
  revoked_at?: string | null;
}

let base = "";
let branchId = "";
let storage: { s3_endpoint: string; region: string };
const keys: Record<string, string> = {};

async function call(method: string, path: string, { body, token }: { body?: unknown; token?: string | null } = {}) {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  const bearer = token === undefined ? keys.user : token;
  if (bearer) headers.authorization = `Bearer ${bearer}`;
  const res = await fetch(base + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  let json: any;
  try {
    json = JSON.parse(text);
  } catch {}
  return { status: res.status, json, text };
}

async function must(method: string, path: string, body?: unknown) {
  const res = await call(method, path, { body });
  if (res.status !== 200) throw new Error(`${method} ${path}: ${res.status} ${res.text}`);
  return res.json;
}

/** The access key ids this run was vended: Neon's `token_id`s. */
const vendedKeys = new Set<string>();
async function vend(operation: "READ" | "READ_WRITE", token?: string | null) {
  const res = await call("POST", `${API}/temporary-table-credentials`, { body: { table_id: tableId, operation }, token });
  if (res.status === 200) vendedKeys.add(res.json.aws_temp_credentials.access_key_id);
  return res;
}

const s3 = (creds: { access_key_id: string; secret_access_key: string; session_token?: string }) =>
  new S3Client({
    accessKeyId: creds.access_key_id,
    secretAccessKey: creds.secret_access_key,
    sessionToken: creds.session_token,
    endpoint: storage.s3_endpoint,
    region: storage.region,
    bucket: LAKEHOUSE_BUCKET,
  });

let tableId = "";

beforeAll(async () => {
  const info = neonJson<{ id?: string; branch?: { id: string } }>(["branches", "get", branch]);
  branchId = info.branch?.id ?? info.id ?? "";
  const fn = neonJson<{ invocation_url?: string; function?: { invocation_url: string } }>(["functions", "get", "unitycatalog", "--branch", branch]);
  base = (fn.invocation_url ?? fn.function?.invocation_url ?? "").replace(/\/+$/, "");
  storage = neonJson(["api", `/projects/${projectId}/branches/${branchId}/storage`]);
  databaseUrl = neonRaw(["connection-string", branch, "--database-name", "neondb"]).trim();
  if (!branchId || !base || !storage.s3_endpoint) throw new Error(`can't resolve ${branch}`);

  for (const [name, what] of [[user, "creates and writes"], [reader, "only reads"]]) {
    admin(["service-account", "create", name, "--if-missing", "--description", `vending e2e: ${what}`]);
    keys[name === user ? "user" : "reader"] = admin(["api-key", "issue", name, "--expires-in-days", "1"]);
  }
  admin(["grant", "add", `service-account:${user}`, "create", "UnityCatalog::Metastore", "metastore"]);

  // The run's table: external, under the branch's `lakehouse` location.
  await must("POST", `${API}/catalogs`, { name: catalog, storage_root: `s3://${LAKEHOUSE_BUCKET}/e2e/${catalog}` });
  await must("POST", `${API}/schemas`, { name: "s", catalog_name: catalog });
  const created = await must("POST", `${API}/tables`, {
    name: "t",
    catalog_name: catalog,
    schema_name: "s",
    table_type: "EXTERNAL",
    data_source_format: "DELTA",
    storage_location: `s3://${LAKEHOUSE_BUCKET}/e2e/${catalog}/s/t`,
  });
  tableId = created.table_id;
  for (const relation of ["select", "modify"]) {
    admin(["grant", "add", `service-account:${user}`, relation, "UnityCatalog::Catalog", catalog]);
  }
  admin(["grant", "add", `service-account:${user}`, "external_use", "UnityCatalog::Schema", `${catalog}.s`]);
  admin(["grant", "add", `service-account:${reader}`, "select", "UnityCatalog::Catalog", catalog]);

  // A grant applies within ~7 s: authz caches the entities for 2 s, the
  // catalog its decisions for 5 s.
  for (let i = 0; (await vend("READ")).status !== 200; i++) {
    if (i >= 15) throw new Error("the external_use grant didn't apply within 30 s");
    await Bun.sleep(2000);
  }
}, 300_000);

afterAll(async () => {
  if (keys.user) await call("DELETE", `${API}/catalogs/${catalog}?force=true`).catch(() => {});
  for (const name of [user, reader]) {
    try {
      admin(["service-account", "disable", name]);
    } catch {}
  }
}, 120_000);

describe("vending on a preview", () => {
  test("no token is a 401", async () => {
    const res = await vend("READ", null);
    expect(res.status).toBe(401);
  });

  test("read access without EXTERNAL USE is a 403", async () => {
    const table = await call("GET", `${API}/tables/${catalog}.s.t`, { token: keys.reader });
    expect(table.status).toBe(200);
    const res = await vend("READ", keys.reader);
    expect(res.status).toBe(403);
  });

  test("with the grant, a write key writes the table, and a read key reads its objects", async () => {
    const prefix = `e2e/${catalog}/s/t/`;
    const commit = `${JSON.stringify({ commitInfo: { operation: "CREATE TABLE", timestamp: Date.now() } })}\n`;
    const write = await vend("READ_WRITE");
    expect(write.status).toBe(200);
    await s3(write.json.aws_temp_credentials).write(`${prefix}_delta_log/00000000000000000000.json`, commit);

    const read = await vend("READ");
    expect(read.status).toBe(200);
    expect(read.json.url).toBe(`s3://${LAKEHOUSE_BUCKET}/${prefix.replace(/\/$/, "")}`);
    const client = s3(read.json.aws_temp_credentials);
    expect(await client.file(`${prefix}_delta_log/00000000000000000000.json`).text()).toBe(commit);
    const listed = await client.list({ prefix });
    expect((listed.contents ?? []).map((o) => o.key)).toContain(`${prefix}_delta_log/00000000000000000000.json`);
  });

  test("a read key can't write; a write key can", async () => {
    const key = `e2e/${catalog}/s/t/_e2e/${run}.txt`;
    const read = await vend("READ");
    const denied = await s3(read.json.aws_temp_credentials).write(key, "nope").then(
      () => "written",
      (err: { code?: string }) => err.code ?? String(err),
    );
    expect(denied).toBe("AccessDenied");

    const write = await vend("READ_WRITE");
    expect(write.status).toBe(200);
    const client = s3(write.json.aws_temp_credentials);
    await client.write(key, "ok");
    expect(await client.file(key).text()).toBe("ok");
    await client.delete(key);
  });

  test("the Delta API's credentials carry the branch's endpoint", async () => {
    const res = await call("GET", `${API}/delta/v1/catalogs/${catalog}/schemas/s/tables/t/credentials?operation=READ`);
    expect(res.status).toBe(200);
    const [cred] = res.json["storage-credentials"];
    expect(cred.config["s3.endpoint"]).toBe(storage.s3_endpoint);
    expect(cred.config["s3.region"]).toBe(storage.region);
    expect(cred.config["s3.path-style-access"]).toBe("true");
  });

  test("the preview mints on its own branch, and stale keys don't pile up", () => {
    const { credentials } = neonJson<{ credentials: Credential[] }>(["api", `/projects/${projectId}/branches/${branchId}/credentials`]);
    const vended = credentials.filter((c) => c.name?.startsWith("olai-vend-"));
    // A child branch's list includes its ancestors' credentials, with their
    // branch id: every key this run got must be the preview's own.
    expect(vendedKeys.size).toBeGreaterThan(0);
    const branchOf = new Map(credentials.map((c) => [c.token_id, c.branch_id]));
    expect([...vendedKeys].map((k) => [k.slice(0, 12), branchOf.get(k)])).toEqual(
      [...vendedKeys].map((k) => [k.slice(0, 12), branchId]),
    );

    const stale = vended.filter(
      (c) =>
        c.branch_id === branchId &&
        !c.revoked_at &&
        c.expires_at !== undefined &&
        Date.parse(c.expires_at) < Date.now() - GC_GRACE_MS,
    );
    expect(stale.map((c) => `${c.name} expired ${c.expires_at}`)).toEqual([]);
  });
});
