// The PDP fails closed: with its Postgres unreachable, every decision is a
// deny with the reason, in time for the caller's 2 s budget.

import assert from "node:assert/strict";
import { test } from "node:test";

import { load, post, ucTable, user } from "./support.mjs";

// Nothing listens on port 1.
// No credentials: nothing gets far enough to use them.
process.env.DATABASE_URL = "postgres://127.0.0.1:1/lineage";
const { default: fn } = await load();

test("an unreachable database is a deny", async () => {
  // Start the PDP first: its first request loads and validates the policies,
  // which on a debug build can take most of the budget by itself. The budget
  // is for the decision; the database is only reached by it.
  const health = await fn.fetch(new Request("http://localhost/health"));
  assert.equal(health.status, 200);
  const started = Date.now();
  const res = await post(fn, "/v1/authorize", ucTable(user(1), "GetTable"));
  assert.equal(res.status, 200, res.text);
  assert.equal(res.json.decision, "deny");
  assert.match(res.json.errors.join(), /loading entities/);
  assert.ok(Date.now() - started < 2000, `took ${Date.now() - started} ms`);
});
