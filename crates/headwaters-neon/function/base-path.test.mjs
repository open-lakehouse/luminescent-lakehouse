// Smoke test for HEADWATERS__UI__BASE_PATH=/lineage, as neon.ts deploys it:
// ingest and the read API move under the prefix, /health and the projection
// trigger stay at the root. The env is read when the handler starts, so this
// file runs in its own process; see `just test headwaters`.

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { test } from "node:test";

import fn from "./dist/index.mjs";

const BASE = "https://headwaters.example";
const PREFIX = "/lineage";
const namespace = `neon-base-path-${randomUUID()}`;

const call = (path, init) => fn.fetch(new Request(BASE + path, init));

test("health stays at the root", async () => {
  assert.equal(process.env.HEADWATERS__UI__BASE_PATH, PREFIX);
  assert.equal((await call("/health")).status, 200);
});

test("ingest and reads are served under the base path", async () => {
  const ingest = await call(`${PREFIX}/api/v1/lineage`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      eventType: "COMPLETE",
      eventTime: new Date().toISOString(),
      producer: "https://github.com/open-lakehouse/headwaters/neon-smoke",
      schemaURL: "https://openlineage.io/spec/2-0-2/OpenLineage.json#/definitions/RunEvent",
      run: { runId: randomUUID() },
      job: { namespace, name: "base_path_job" },
      inputs: [{ namespace, name: "in_table" }],
      outputs: [{ namespace, name: "out_table" }],
    }),
  });
  assert.equal(ingest.status, 202, await ingest.text());

  // Settled on ingest (the default): poll the ConnectRPC read API until it shows.
  let jobs = [];
  for (let i = 0; i < 40 && jobs.length === 0; i++) {
    await new Promise((r) => setTimeout(r, 250));
    const rpc = await call(`${PREFIX}/headwaters.read.v1.ReadService/ListJobs`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ namespace }),
    });
    assert.equal(rpc.status, 200, await rpc.clone().text());
    jobs = ((await rpc.json()).jobs ?? []).map((j) => j.name);
  }
  assert.deepEqual(jobs, ["base_path_job"]);

  // Nothing is left at the root.
  const root = await call("/api/v1/namespaces");
  assert.equal(root.status, 404);
});
