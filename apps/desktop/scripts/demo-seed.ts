#!/usr/bin/env bun
// Seed a Neon branch with demo data for the desktop's Catalog and Lineage views:
// a Unity Catalog catalog with schemas and tables, and OpenLineage runs in
// headwaters that connect those tables into a graph.
//
//   bun scripts/demo-seed.ts <branch> --project <id>   (just desktop-demo-seed <branch>)
//
// The project is `--project`, else LUMINESCENT_NEON_PROJECT (as the app reads it).
// The justfile supplies the default, the project neon.ts deploys to.
//
// It finds the branch's function URLs with `neon functions list` (signed in as
// the Neon CLI is) and talks to the functions directly; both are anonymous for
// now (docs/auth.md). Safe to run again: what exists is left alone, and the
// lineage runs are only posted when their jobs aren't there yet.

import { spawnSync } from "node:child_process";

const UC_API = "/api/2.1/unity-catalog";

const CATALOG = "demo";
/** The OpenLineage namespace of the jobs; the datasets are named for their UC tables. */
const JOB_NAMESPACE = "demo-pipelines";
const DATASET_NAMESPACE = "unity-catalog";
/** Where the "landing" files come from. */
const LANDING = "s3://luminescent-demo-landing";
const PRODUCER = "https://github.com/open-lakehouse/luminescent-lakehouse/apps/desktop/demo-seed";
const SCHEMA_URL = "https://openlineage.io/spec/2-0-2/OpenLineage.json#/definitions/RunEvent";
/**
 * Placeholder storage: nothing reads it, the catalog only records it. The server
 * wants a catalog's storage root and every external table inside a registered
 * external location, which needs a credential; neither is checked against S3.
 */
const STORAGE = "s3://luminescent-demo";
const TABLE_ROOT = `${STORAGE}/tables`;
const CREDENTIAL = "demo_storage";
const EXTERNAL_LOCATION = "demo_storage";
const DEMO_ROLE = "arn:aws:iam::000000000000:role/luminescent-demo";

type Column = { name: string; type: string; comment?: string };
type Table = { schema: string; name: string; comment: string; columns: Column[] };

const SCHEMAS = [
  { name: "raw", comment: "Landing tables, as the ingest jobs write them" },
  { name: "sales", comment: "Cleaned and modelled sales data" },
];

export const TABLES: Table[] = [
  {
    schema: "raw",
    name: "orders_raw",
    comment: "Orders as exported by the shop, one row per order line",
    columns: [
      { name: "order_id", type: "string" },
      { name: "line", type: "int" },
      { name: "customer_id", type: "string" },
      { name: "sku", type: "string" },
      { name: "quantity", type: "int" },
      { name: "unit_price", type: "double" },
      { name: "ordered_at", type: "timestamp" },
    ],
  },
  {
    schema: "raw",
    name: "customers_raw",
    comment: "Customer records from the CRM export",
    columns: [
      { name: "customer_id", type: "string" },
      { name: "name", type: "string" },
      { name: "email", type: "string" },
      { name: "country", type: "string" },
      { name: "signed_up_at", type: "timestamp" },
    ],
  },
  {
    schema: "sales",
    name: "orders",
    comment: "One row per order, deduplicated, with its total",
    columns: [
      { name: "order_id", type: "string" },
      { name: "customer_id", type: "string" },
      { name: "total", type: "double", comment: "Sum of quantity * unit_price" },
      { name: "ordered_at", type: "timestamp" },
    ],
  },
  {
    schema: "sales",
    name: "customers",
    comment: "Customers with a normalised country code",
    columns: [
      { name: "customer_id", type: "string" },
      { name: "name", type: "string" },
      { name: "country", type: "string", comment: "ISO 3166-1 alpha-2" },
    ],
  },
  {
    schema: "sales",
    name: "daily_revenue",
    comment: "Revenue per day and country",
    columns: [
      { name: "day", type: "date" },
      { name: "country", type: "string" },
      { name: "orders", type: "long" },
      { name: "revenue", type: "double" },
    ],
  },
];

/** The pipeline: each job reads `inputs` and writes `outputs` (table names, or landing paths). */
export const JOBS: { name: string; description: string; inputs: string[]; outputs: string[] }[] = [
  {
    name: "ingest_orders",
    description: "Load the shop's order export",
    inputs: [`${LANDING}/orders/`],
    outputs: ["raw.orders_raw"],
  },
  {
    name: "ingest_customers",
    description: "Load the CRM's customer export",
    inputs: [`${LANDING}/customers/`],
    outputs: ["raw.customers_raw"],
  },
  {
    name: "clean_orders",
    description: "Deduplicate order lines into orders",
    inputs: ["raw.orders_raw"],
    outputs: ["sales.orders"],
  },
  {
    name: "clean_customers",
    description: "Normalise customer countries",
    inputs: ["raw.customers_raw"],
    outputs: ["sales.customers"],
  },
  {
    name: "build_daily_revenue",
    description: "Aggregate orders per day and country",
    inputs: ["sales.orders", "sales.customers"],
    outputs: ["sales.daily_revenue"],
  },
];

// Spark/Delta type names → UC's type_name.
const TYPE_NAME: Record<string, string> = {
  string: "STRING",
  int: "INT",
  long: "LONG",
  double: "DOUBLE",
  date: "DATE",
  timestamp: "TIMESTAMP",
};

function usage(): never {
  console.error("usage: demo-seed.ts <branch> --project <id> (or LUMINESCENT_NEON_PROJECT)");
  process.exit(2);
}

function parseArgs(argv: string[]) {
  let branch: string | undefined;
  let project = process.env.LUMINESCENT_NEON_PROJECT || undefined;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--project") project = argv[++i] ?? usage();
    else if (arg?.startsWith("-")) usage();
    else if (!branch) branch = arg;
    else usage();
  }
  return { branch: branch ?? usage(), project: project ?? usage() };
}

/** The branch's functions, by slug, as `neon functions list` shows them. */
function functionUrls(project: string, branch: string): Map<string, string> {
  const out = spawnSync(
    "neon",
    ["functions", "list", "--project-id", project, "--branch", branch, "--output", "json"],
    { encoding: "utf8" },
  );
  if (out.error) throw new Error(`running neon: ${out.error.message}`);
  if (out.status !== 0) throw new Error(`neon functions list failed: ${out.stderr.trim()}`);
  const list = JSON.parse(out.stdout) as { slug: string; invocation_url: string }[];
  return new Map(list.map((f) => [f.slug, f.invocation_url.replace(/\/+$/, "")]));
}

async function call(method: string, url: string, body?: unknown): Promise<Response> {
  return fetch(url, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

async function fail(what: string, res: Response): Promise<never> {
  throw new Error(`${what}: ${res.status} ${(await res.text()).slice(0, 500)}`);
}

/** GET `item`; when it's missing (404), POST `body` to `collection`. */
async function ensure(
  label: string,
  item: string,
  collection: string,
  body: unknown,
): Promise<"created" | "exists"> {
  const got = await call("GET", item);
  if (got.ok) return "exists";
  if (got.status !== 404) return fail(`reading ${label}`, got);
  const created = await call("POST", collection, body);
  if (created.ok) return "created";
  // Raced with another run: fine.
  if (created.status === 409) return "exists";
  return fail(`creating ${label}`, created);
}

export function columnsOf(table: Table) {
  return table.columns.map((c, position) => ({
    name: c.name,
    type_name: TYPE_NAME[c.type] ?? c.type.toUpperCase(),
    type_text: c.type,
    type_json: JSON.stringify({ name: c.name, type: c.type, nullable: true, metadata: {} }),
    position,
    nullable: true,
    ...(c.comment ? { comment: c.comment } : {}),
  }));
}

async function seedCatalog(uc: string) {
  const api = `${uc}${UC_API}`;
  const log = (what: string, result: string) => console.log(`  ${result.padEnd(7)} ${what}`);
  log(
    `credential ${CREDENTIAL}`,
    await ensure(
      `credential ${CREDENTIAL}`,
      `${api}/credentials/${CREDENTIAL}`,
      `${api}/credentials`,
      {
        name: CREDENTIAL,
        purpose: "STORAGE",
        aws_iam_role: { role_arn: DEMO_ROLE },
        comment: "Placeholder for the demo's storage; nothing is read with it",
      },
    ),
  );
  log(
    `external location ${EXTERNAL_LOCATION}`,
    await ensure(
      `external location ${EXTERNAL_LOCATION}`,
      `${api}/external-locations/${EXTERNAL_LOCATION}`,
      `${api}/external-locations`,
      { name: EXTERNAL_LOCATION, url: STORAGE, credential_name: CREDENTIAL, read_only: true },
    ),
  );
  log(
    `catalog ${CATALOG}`,
    await ensure(`catalog ${CATALOG}`, `${api}/catalogs/${CATALOG}`, `${api}/catalogs`, {
      name: CATALOG,
      storage_root: `${STORAGE}/catalog`,
      comment: "Luminescent Lakehouse demo data (apps/desktop/scripts/demo-seed.ts)",
    }),
  );
  for (const schema of SCHEMAS) {
    const full = `${CATALOG}.${schema.name}`;
    log(
      `schema ${full}`,
      await ensure(`schema ${full}`, `${api}/schemas/${full}`, `${api}/schemas`, {
        name: schema.name,
        catalog_name: CATALOG,
        comment: schema.comment,
      }),
    );
  }
  for (const table of TABLES) {
    const full = `${CATALOG}.${table.schema}.${table.name}`;
    log(
      `table ${full}`,
      await ensure(`table ${full}`, `${api}/tables/${full}`, `${api}/tables`, {
        name: table.name,
        catalog_name: CATALOG,
        schema_name: table.schema,
        table_type: "EXTERNAL",
        data_source_format: "DELTA",
        storage_location: `${TABLE_ROOT}/${table.schema}/${table.name}`,
        comment: table.comment,
        columns: columnsOf(table),
      }),
    );
  }
}

/** A stable UUID for `name`, so a rerun posts the same run ids. */
export function uuidFor(name: string): string {
  const hex = new Bun.CryptoHasher("sha1").update(`luminescent-demo:${name}`).digest("hex");
  const variant = ((Number.parseInt(hex.slice(16, 18), 16) & 0x3f) | 0x80).toString(16);
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-5${hex.slice(13, 16)}-${variant}${hex.slice(18, 20)}-${hex.slice(20, 32)}`;
}

function dataset(ref: string, withSchema: boolean) {
  if (ref.startsWith("s3://")) {
    const url = new URL(ref);
    return { namespace: `s3://${url.host}`, name: url.pathname.replace(/^\/+/, "") };
  }
  const name = `${CATALOG}.${ref}`;
  const table = TABLES.find((t) => `${t.schema}.${t.name}` === ref);
  const facets =
    withSchema && table
      ? {
          schema: {
            _producer: PRODUCER,
            _schemaURL: "https://openlineage.io/spec/facets/1-1-1/SchemaDatasetFacet.json",
            fields: table.columns.map((c) => ({
              name: c.name,
              type: c.type,
              ...(c.comment ? { description: c.comment } : {}),
            })),
          },
        }
      : {};
  return { namespace: DATASET_NAMESPACE, name, facets };
}

/** Two runs per job (yesterday's and today's), each a START and a COMPLETE. */
export function lineageEvents(now: Date) {
  const events: unknown[] = [];
  const day = 24 * 3600 * 1000;
  for (const [runIndex, daysAgo] of [1, 0].entries()) {
    JOBS.forEach((job, jobIndex) => {
      const start = new Date(now.getTime() - daysAgo * day - (JOBS.length - jobIndex) * 600_000);
      const end = new Date(start.getTime() + 120_000 + jobIndex * 30_000);
      const runId = uuidFor(`${job.name}#${runIndex}`);
      const base = {
        producer: PRODUCER,
        schemaURL: SCHEMA_URL,
        run: { runId },
        job: {
          namespace: JOB_NAMESPACE,
          name: job.name,
          facets: {
            documentation: {
              _producer: PRODUCER,
              _schemaURL: "https://openlineage.io/spec/facets/1-0-1/DocumentationJobFacet.json",
              description: job.description,
            },
          },
        },
      };
      events.push({
        ...base,
        eventType: "START",
        eventTime: start.toISOString(),
        inputs: job.inputs.map((i) => dataset(i, false)),
        outputs: job.outputs.map((o) => dataset(o, false)),
      });
      events.push({
        ...base,
        eventType: "COMPLETE",
        eventTime: end.toISOString(),
        inputs: job.inputs.map((i) => dataset(i, true)),
        outputs: job.outputs.map((o) => dataset(o, true)),
      });
    });
  }
  return events;
}

async function listJobs(rpc: string): Promise<string[]> {
  const res = await call("POST", `${rpc}/ListJobs`, { namespace: JOB_NAMESPACE });
  if (!res.ok) return fail("listing jobs", res);
  return (((await res.json()) as { jobs?: { name: string }[] }).jobs ?? []).map((j) => j.name);
}

async function seedLineage(headwaters: string) {
  // The function serves headwaters at its root.
  const base = headwaters;
  const rpc = `${base}/headwaters.read.v1.ReadService`;
  const existing = await listJobs(rpc);
  if (JOBS.every((j) => existing.includes(j.name))) {
    console.log(`  exists  ${JOBS.length} jobs in ${JOB_NAMESPACE}`);
    return;
  }
  const events = lineageEvents(new Date());
  const res = await call("POST", `${base}/api/v1/lineage/batch`, events);
  if (!res.ok) return fail("posting the lineage events", res);
  const batch = (await res.json()) as {
    summary?: { failed?: number };
    failed_events?: { index: number; reason: string }[];
  };
  if (batch.summary?.failed) {
    const reasons = (batch.failed_events ?? []).map((f) => `#${f.index}: ${f.reason}`);
    throw new Error(`headwaters refused ${batch.summary.failed} event(s): ${reasons.join("; ")}`);
  }
  // Folded after ingest (settle-on-ingest); wait until the jobs read back.
  for (let i = 0; i < 40; i++) {
    const jobs = await listJobs(rpc);
    if (JOBS.every((j) => jobs.includes(j.name))) {
      console.log(`  created ${events.length} events: ${JOBS.length} jobs in ${JOB_NAMESPACE}`);
      return;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error("the lineage events were accepted but the jobs never read back");
}

async function main() {
  const { branch, project } = parseArgs(process.argv.slice(2));
  const urls = functionUrls(project, branch);
  const uc = urls.get("unitycatalog");
  const headwaters = urls.get("headwaters");
  if (!uc || !headwaters) {
    throw new Error(
      `branch ${branch} needs the unitycatalog and headwaters functions; it has: ${[...urls.keys()].join(", ") || "none"}`,
    );
  }
  console.log(`Unity Catalog ${uc}`);
  await seedCatalog(uc);
  console.log(`headwaters ${headwaters}`);
  await seedLineage(headwaters);
}

if (import.meta.main) {
  main().catch((e) => {
    console.error(`demo-seed: ${e instanceof Error ? e.message : e}`);
    process.exit(1);
  });
}
