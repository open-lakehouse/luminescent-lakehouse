import { describe, expect, test } from "bun:test";
import { ChangeKind, createFactoryClient, KIND, Phase, TaskSchema } from "./index.js";
import { packPayload, unpackPayload } from "./kinds.js";
import { createFakeFactory, createFakeFactoryFetch } from "./testing.js";
import { createDefaultTransport } from "./transport.js";

describe("the fake factory, through the client", () => {
  test("serves the demo plan with a history and drift", async () => {
    const { client } = createFakeFactory({ seed: "demo" });
    const { graphs } = await client.listGraphs({});
    expect(graphs).toHaveLength(1);
    const root = graphs[0].root?.ref ?? "";
    const state = await client.getGraph({ root });
    expect(state.approvedRevision).not.toBe("");
    expect(state.drifted.length).toBeGreaterThan(0);
    const { revisions } = await client.listRevisions({ root });
    expect(revisions.length).toBeGreaterThan(5);
    expect(revisions[0].parentRootHash).toBe(revisions[1].rootHash);
    const diff = await client.diffRevisions({ root, base: state.approvedRevision, compact: true });
    const kinds = new Set(diff.entries.map((e) => e.change));
    expect(kinds.has(ChangeKind.ADDED)).toBe(true);
    expect(kinds.has(ChangeKind.MODIFIED)).toBe(true);
  });

  test("records a revision per write and derives readiness", async () => {
    const { client } = createFakeFactory();
    const root =
      (await client.createNode({ kind: KIND.plan, spec: { title: "P" } })).node?.ref ?? "";
    const a = (
      await client.createNode({
        parent: root,
        kind: KIND.task,
        spec: { title: "A", payload: packPayload(TaskSchema, { acceptance: "works" }) },
      })
    ).node;
    expect(unpackPayload(a?.spec?.payload)).toMatchObject({ acceptance: "works" });
    const before = await client.getGraph({ root });
    await client.approveRevision({ root, rootHash: before.revision });
    const after = await client.getGraph({ root });
    const node = after.graph?.nodes.find((n) => n.ref === a?.ref);
    expect(node?.status?.phase).toBe(Phase.APPROVED);
    expect(node?.status?.conditions.find((c) => c.type === "Ready")?.status).toBe(true);
    await expect(client.moveNode({ ref: root, newParent: a?.ref ?? "" })).rejects.toThrow();
  });

  test("a client without a host or transport is the platform default", () => {
    expect(createFactoryClient()).toBeDefined();
  });
});

describe("the fake factory, over fetch", () => {
  const base = "http://factory.invalid";
  const asFetch = (fetch: (r: Request) => Promise<Response>) =>
    ((input: RequestInfo | URL, init?: RequestInit) =>
      fetch(new Request(input, init))) as typeof globalThis.fetch;

  test("answers the client's binary Connect calls", async () => {
    const { fetch } = createFakeFactoryFetch({ seed: "demo" });
    const client = createFactoryClient(createDefaultTransport(base, asFetch(fetch)));
    const { graphs } = await client.listGraphs({});
    expect(graphs.map((g) => g.root?.spec?.title)).toEqual(["Typed work graph on olai-store"]);
  });

  test("404s outside the service", async () => {
    const { fetch } = createFakeFactoryFetch({ seed: "demo" });
    expect((await fetch(new Request(`${base}/nope`, { method: "POST" }))).status).toBe(404);
  });
});
