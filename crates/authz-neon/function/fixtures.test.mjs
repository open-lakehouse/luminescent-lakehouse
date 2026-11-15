// The decision fixtures against the built PDP and the principals of seed.sql:
// policies/tests/*.json and *.cases.mjs, and a sweep of every Unity Catalog
// action of the composed schema for an admin, a user without a grant and a
// banned admin. Then their coverage: every action has at least one allow and
// one deny. Run via `just test authz`.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

import { BASE, TOKENS, load, post, serviceAccount, uc, user } from "./support.mjs";

const { default: fn, actions } = await load();

const dir = new URL("./policies/tests/", import.meta.url);
const fixtures = [];
for (const file of readdirSync(dir).sort()) {
  if (file.endsWith(".json")) fixtures.push([file, JSON.parse(readFileSync(new URL(file, dir), "utf8"))]);
  if (file.endsWith(".cases.mjs")) fixtures.push([file, await import(new URL(file, dir).href)]);
}

// The resource types each Unity Catalog action applies to, from the staged
// schema.
const typesOf = new Map();
const staged = new URL("./dist/policies/", import.meta.url);
for (const file of readdirSync(staged).filter((f) => f.endsWith(".cedarschema"))) {
  const text = readFileSync(new URL(file, staged), "utf8");
  if (!/namespace UnityCatalog\b/.test(text)) continue;
  for (const [, action, resources] of text.matchAll(/action "(\w+)" appliesTo \{[^}]*?resource: \[([^\]]*)\]/g)) {
    const types = resources
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    typesOf.set(action, types.map((t) => t.replace(/^UnityCatalog::/, "")));
  }
}

// A securable of each kind, under main.sales where it sits in a schema.
const NAMES = {
  Metastore: "",
  Catalog: "main",
  Schema: "main.sales",
  Table: "main.sales.orders",
  Volume: "main.sales.files",
  Function: "main.sales.score",
  RegisteredModel: "main.sales.forecast",
  ModelVersion: "main.sales.forecast.1",
  Agent: "main.sales.helper",
  AgentSkill: "main.sales.lookup",
  StagingTable: "main.sales.staged",
  Column: "main.sales.orders.id",
  ExternalLocation: "landing",
  Credential: "landing-cred",
  Share: "sales",
  Recipient: "partner",
  Provider: "upstream",
  TagPolicy: "pii",
  PolicyInfo: "mask",
};

// Vends take an EXTERNAL USE grant, admin or not: who holds the one each needs.
const VENDS = {
  GenerateTemporaryTableCredentials: user(11),
  GenerateTemporaryVolumeCredentials: user(11),
  GenerateTemporaryModelVersionCredentials: user(11),
  GenerateTemporaryStagingTableCredentials: user(11),
  CreateStagingTable: user(11),
  GenerateTemporaryPathCredentials: serviceAccount(1),
};
// Lists of the metastore's securables: anyone may ask.
const OPEN = new Set([
  "ListCatalogs",
  "ListCredentials",
  "ListExternalLocations",
  "ListShares",
  "ListRecipients",
  "ListProviders",
  "ListTagPolicies",
]);

// The kind a sweep asks an action on: the one it applies to, a table for the
// actions on (nearly) every securable, and an external location for a path,
// which is asked on the location covering it.
const KIND_OF = { GenerateTemporaryPathCredentials: "ExternalLocation" };
const kindOf = (action, types) => KIND_OF[action] ?? (types.length > 3 ? "Table" : types[0]);

const sweep = [...typesOf].flatMap(([action, types]) => {
  const kind = kindOf(action, types);
  const context = { credential_operation: "READ", is_creator: true };
  const ask = (principal) => uc(principal, action, kind, NAMES[kind], context);
  const cases = [
    VENDS[action]
      ? { name: `${action}: an admin needs EXTERNAL USE`, request: ask(user(1)), decision: "deny" }
      : {
          name: `${action}: an admin`,
          request: ask(user(1)),
          decision: "allow",
          policies: OPEN.has(action) ? undefined : ["admin"],
        },
    { name: `${action}: a banned admin`, request: ask(user(5)), decision: "deny" },
    OPEN.has(action)
      ? { name: `${action}: anyone`, request: ask(user(2)), decision: "allow" }
      : { name: `${action}: a user without a grant`, request: ask(user(2)), decision: "deny" },
  ];
  if (VENDS[action]) cases.push({ name: `${action}: with the grant`, request: ask(VENDS[action]), decision: "allow" });
  return cases;
});
// A path no external location covers is asked on the metastore: nobody gets it.
if (typesOf.get("GenerateTemporaryPathCredentials")?.includes("Metastore")) {
  for (const [who, principal] of [
    ["an admin", user(1)],
    ["a grantee", serviceAccount(1)],
  ]) {
    sweep.push({
      name: `GenerateTemporaryPathCredentials: ${who}, for a path no location covers`,
      request: uc(principal, "GenerateTemporaryPathCredentials", "Metastore", "", { credential_operation: "READ" }),
      decision: "deny",
    });
  }
}
fixtures.push(["sweep", { caller: "UNITYCATALOG", cases: sweep }]);

const seen = new Map();
for (const [file, fixture] of fixtures) {
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

test("the sweep found the Unity Catalog actions", () => {
  assert.ok(typesOf.size > 50, `only ${typesOf.size} actions in dist/policies`);
  for (const action of ["ReadData", "WriteData", "CommitTable"]) assert.ok(typesOf.has(action), action);
});

test("every action has an allow and a deny fixture", async () => {
  await fn.fetch(new Request(`${BASE}/health`)); // starts the addon
  const missing = actions()
    .map(({ type, id }) => `${type}::"${id}"`)
    .flatMap((action) =>
      ["allow", "deny"].filter((d) => !seen.get(action)?.has(d)).map((d) => `${action}: no ${d}`),
    );
  assert.deepEqual(missing, []);
});
