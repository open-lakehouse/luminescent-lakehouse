// scripts/storage-seed.ts's decisions, against a fake catalog: `bun test ./scripts`.

import { describe, expect, test } from "bun:test";
import { credentialStep, locationStep, neonArn, type Step, seed } from "./storage-seed.ts";

const ARN = neonArn("cool-poetry-28343550", "br-preview-1");

/** A catalog holding `objects` by path, answering like UC's REST API. */
function fakeCatalog(objects: Record<string, unknown>) {
  const calls: Step[] = [];
  const call = async (step: Step) => {
    calls.push(step);
    if (step.method === "GET") {
      const found = objects[step.path];
      return found
        ? { status: 200, json: found, text: JSON.stringify(found) }
        : { status: 404, json: { error_code: "NOT_FOUND" }, text: "not found" };
    }
    return { status: 200, json: step.body, text: "" };
  };
  return { call, calls };
}

describe("neonArn", () => {
  test("names the project and the branch", () => {
    expect(ARN).toBe("arn:neon:storage::cool-poetry-28343550:branch/br-preview-1");
  });
});

describe("seed", () => {
  test("registers the credential and the location on a fresh branch", async () => {
    const { call, calls } = fakeCatalog({});
    await seed(call, ARN);
    const writes = calls.filter((c) => c.method !== "GET");
    expect(writes).toEqual([
      credentialStep(false, ARN),
      locationStep(undefined) as Step,
    ]);
    expect(writes[0]?.body).toMatchObject({ name: "neon", purpose: "STORAGE", aws_iam_role: { role_arn: ARN } });
    expect(writes[1]?.body).toMatchObject({ name: "lakehouse", url: "s3://lakehouse", credential_name: "neon" });
  });

  test("repoints an inherited credential at this branch, and leaves a right location alone", async () => {
    const { call, calls } = fakeCatalog({
      "/api/2.1/unity-catalog/credentials/neon": { name: "neon", purpose: "STORAGE" },
      "/api/2.1/unity-catalog/external-locations/lakehouse": { name: "lakehouse", url: "s3://lakehouse/", credential_name: "neon" },
    });
    await seed(call, ARN);
    expect(calls.filter((c) => c.method !== "GET")).toEqual([
      { method: "PATCH", path: "/api/2.1/unity-catalog/credentials/neon", body: { aws_iam_role: { role_arn: ARN } } },
    ]);
  });

  test("points a changed location back at the bucket", () => {
    expect(locationStep({ url: "s3://other", credential_name: "neon" })).toEqual({
      method: "PATCH",
      path: "/api/2.1/unity-catalog/external-locations/lakehouse",
      body: { url: "s3://lakehouse", credential_name: "neon" },
    });
  });

  test("fails on anything but found or not found, and on a refused write", async () => {
    const denied = async () => ({ status: 403, json: {}, text: "PERMISSION_DENIED" });
    await expect(seed(denied, ARN)).rejects.toThrow("403");
    const { call } = fakeCatalog({});
    const refusing = async (s: Step) => (s.method === "GET" ? call(s) : { status: 400, json: {}, text: "INVALID" });
    await expect(seed(refusing, ARN)).rejects.toThrow("POST /api/2.1/unity-catalog/credentials: 400");
  });
});
