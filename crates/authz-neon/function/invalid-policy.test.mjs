// A policy that doesn't validate against the schema (strict mode) keeps the
// PDP from starting: every call answers 503, which a caller counts as a deny.

import assert from "node:assert/strict";
import { appendFileSync, cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { pathToFileURL } from "node:url";

import { load, post, ucTable, user } from "./support.mjs";

const copy = mkdtempSync(join(tmpdir(), "authz-invalid-"));
after(() => rmSync(copy, { recursive: true, force: true }));
cpSync(new URL("./dist/", import.meta.url), copy, { recursive: true });
appendFileSync(
  join(copy, "policies", "zz-invalid.cedar"),
  'permit (principal, action == UnityCatalog::Action::"GetTable", resource) when { principal.no_such_attr };\n',
);
const { default: fn } = await load(pathToFileURL(join(copy, "index.mjs")).href);

test("an invalid policy is a 503", async () => {
  const res = await post(fn, "/v1/authorize", ucTable(user(1), "GetTable"));
  assert.equal(res.status, 503, res.text);
});
