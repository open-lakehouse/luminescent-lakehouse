// End-to-end tests of the built PDP (`dist/`): caller authentication, the
// namespace rule, the batch endpoint, failing closed, and Neon Auth changes
// counting from the next request. Needs DATABASE_URL on a Postgres migrated with
// `luminescent-migrate authz` and seeded with seed.sql: `just test authz`
// (host) or `just test-arm64 authz` (linux-arm64 in Docker).

import assert from "node:assert/strict";
import { execSync } from "node:child_process";
import { test } from "node:test";

import { BASE, TOKENS, load, post, ucTable, user } from "./support.mjs";

const { default: fn } = await load();

const ADMIN = user(1);
const PLAIN = user(2);
const FLIP = user(7);

test("health is open", async () => {
  const response = await fn.fetch(new Request(`${BASE}/health`));
  assert.equal(response.status, 200);
  assert.equal(await response.text(), "OK");
});

test("an admin may select, a user without a grant may not", async () => {
  const allowed = await post(fn, "/v1/authorize", ucTable(ADMIN, "select"));
  assert.equal(allowed.status, 200, allowed.text);
  assert.deepEqual(allowed.json, { decision: "allow", policies: ["admin"], errors: [] });

  const denied = await post(fn, "/v1/authorize", ucTable(PLAIN, "select"));
  assert.equal(denied.status, 200, denied.text);
  assert.deepEqual(denied.json, { decision: "deny", policies: [], errors: [] });
});

test("no secret or a wrong one is a 401", async () => {
  for (const path of ["/v1/authorize", "/v1/authorize/batch"]) {
    const body = path.endsWith("batch") ? { requests: [ucTable(ADMIN, "select")] } : ucTable(ADMIN, "select");
    for (const token of [undefined, "not-the-secret", `${TOKENS.UNITYCATALOG}x`]) {
      const res = await post(fn, path, body, token ?? null);
      assert.equal(res.status, 401, `${path} with ${token}: ${res.text}`);
    }
  }
});

test("a caller may only ask about its own namespace's actions", async () => {
  const foreign = { ...ucTable(ADMIN, "select"), action: { type: "Headwaters::Action", id: "read" } };
  const res = await post(fn, "/v1/authorize", foreign);
  assert.equal(res.status, 403, res.text);

  // headwaters' secret, asking a Unity Catalog action.
  const cross = await post(fn, "/v1/authorize", ucTable(ADMIN, "select"), TOKENS.HEADWATERS);
  assert.equal(cross.status, 403, cross.text);

  // One foreign request refuses the whole batch.
  const batch = await post(fn, "/v1/authorize/batch", { requests: [ucTable(ADMIN, "select"), foreign] });
  assert.equal(batch.status, 403, batch.text);
});

test("a batch is answered in order", async () => {
  const res = await post(fn, "/v1/authorize/batch", {
    requests: [ucTable(ADMIN, "select"), ucTable(PLAIN, "select"), ucTable(ADMIN, "external_use")],
  });
  assert.equal(res.status, 200, res.text);
  assert.deepEqual(
    res.json.responses.map((r) => r.decision),
    ["allow", "deny", "deny"],
  );
  const empty = await post(fn, "/v1/authorize/batch", { requests: [] });
  assert.deepEqual(empty.json, { responses: [] });
});

test("a body that isn't a request is a 400", async () => {
  assert.equal((await post(fn, "/v1/authorize", "{")).status, 400);
  assert.equal((await post(fn, "/v1/authorize", { principal: ADMIN })).status, 400);
});

test("an unknown principal is a deny with the reason", async () => {
  for (const principal of [
    user(9),
    { type: "Lakehouse::User", id: "not-a-uuid" },
    { type: "Lakehouse::ServiceAccount", id: "bbbbbbbb-0000-0000-0000-000000000009" },
    { type: "UnityCatalog::Table", id: "aaaaaaaa-0000-0000-0000-000000000001" },
  ]) {
    const res = await post(fn, "/v1/authorize", ucTable(principal, "select"));
    assert.equal(res.status, 200, res.text);
    assert.equal(res.json.decision, "deny", JSON.stringify(principal));
    assert.ok(res.json.errors.length > 0, res.text);
  }
});

test("a resource_entity for another resource is a deny", async () => {
  const request = ucTable(ADMIN, "select");
  request.resource_entity.uid = { type: "UnityCatalog::Table", id: "main.sales.other" };
  const res = await post(fn, "/v1/authorize", request);
  assert.equal(res.json.decision, "deny");
  assert.match(res.json.errors.join(), /resource_entity/);
});

// TEST_PSQL (`just test authz`) runs SQL on the test Postgres; the arm64
// container has no psql.
const psql = process.env.TEST_PSQL;
const sql = (statement) => execSync(psql, { input: statement, stdio: ["pipe", "inherit", "inherit"] });

test(
  "a Console admin change counts from the next request",
  { skip: psql ? false : "TEST_PSQL is not set" },
  async () => {
    const ask = async () => (await post(fn, "/v1/authorize", ucTable(FLIP, "manage"))).json.decision;
    try {
      assert.equal(await ask(), "deny");
      sql(`UPDATE neon_auth."user" SET role = 'admin' WHERE id = '${FLIP.id}'`);
      assert.equal(await ask(), "allow");
      sql(`UPDATE neon_auth."user" SET banned = true WHERE id = '${FLIP.id}'`);
      assert.equal(await ask(), "deny");
      sql(`UPDATE neon_auth."user" SET role = 'user', banned = false WHERE id = '${FLIP.id}'`);
      assert.equal(await ask(), "deny");
    } finally {
      sql(`UPDATE neon_auth."user" SET role = 'user', banned = false WHERE id = '${FLIP.id}'`);
    }
  },
);
