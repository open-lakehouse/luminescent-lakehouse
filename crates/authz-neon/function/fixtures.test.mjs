// The decision fixtures in policies/tests/*.json against the built PDP and the
// principals of seed.sql, and their coverage: every action of the composed
// schema has at least one allow and one deny. Run via `just test authz`.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

import { BASE, TOKENS, load, post } from "./support.mjs";

const { default: fn, actions } = await load();

const dir = new URL("./policies/tests/", import.meta.url);
const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
const seen = new Map();

for (const file of files) {
  const fixture = JSON.parse(readFileSync(new URL(file, dir), "utf8"));
  const token = TOKENS[fixture.caller];
  assert.ok(token, `${file}: no test secret for caller ${fixture.caller}`);
  for (const c of fixture.cases) {
    test(`${file}: ${c.name}`, async () => {
      const res = await post(fn, "/v1/authorize", c.request, token);
      assert.equal(res.status, 200, res.text);
      assert.deepEqual(res.json.errors, [], res.text);
      assert.equal(res.json.decision, c.decision, res.text);
      if (c.policies) assert.deepEqual(res.json.policies, c.policies, res.text);
      const key = `${c.request.action.type}::"${c.request.action.id}"`;
      seen.set(key, new Set([...(seen.get(key) ?? []), c.decision]));
    });
  }
}

test("every action has an allow and a deny fixture", async () => {
  await fn.fetch(new Request(`${BASE}/health`)); // starts the addon
  const missing = actions()
    .map(({ type, id }) => `${type}::"${id}"`)
    .flatMap((action) =>
      ["allow", "deny"].filter((d) => !seen.get(action)?.has(d)).map((d) => `${action}: no ${d}`),
    );
  assert.deepEqual(missing, []);
});
