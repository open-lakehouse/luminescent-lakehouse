import { describe, expect, test } from "bun:test";
import fixture from "../../../apps/desktop/e2e/fixtures/open_plan_review.json";
import { decodeSnapshot, parseReview, snapshotClient } from "../src/snapshot.js";

// workgraph-mcp's `open_plan_review` on the demo plan, as its tests write it.
const review = parseReview(fixture.structuredContent);

describe("the plan review's snapshot", () => {
  test("parses and decodes", () => {
    if (!review) throw new Error("the fixture is a plan review");
    expect(review.status).toBe("awaiting_user");
    const s = decodeSnapshot(review.snapshot);
    expect(s.graph.revision).toBe(review.revision);
    expect(s.graph.approvedRevision).toBe(review.approved_revision ?? "");
    expect(s.target.revision?.rootHash).toBe(review.revision);
    expect(s.base?.revision?.rootHash).toBe(review.approved_revision ?? "");
    expect(s.diff.entries.length).toBeGreaterThan(0);
    expect(s.schema.schema?.nodeKinds.length).toBeGreaterThan(0);
  });

  test("rejects what isn't one", () => {
    expect(parseReview(undefined)).toBeUndefined();
    expect(parseReview({ root: "r", revision: "x" })).toBeUndefined();
    expect(parseReview({ root: "r", revision: "x", snapshot: { schema: 1 } })).toBeUndefined();
  });

  test("answers factory-ui's RPCs from the snapshot alone", async () => {
    if (!review) throw new Error("the fixture is a plan review");
    const s = decodeSnapshot(review.snapshot);
    const client = snapshotClient(review, s);
    const base = review.approved_revision ?? "";
    expect((await client.getSchema({})).schema?.nodeKinds.length).toBeGreaterThan(0);
    expect((await client.getGraph({ root: review.root })).revision).toBe(review.revision);
    const target = await client.getRevision({ root: review.root, rootHash: review.revision });
    expect(target.tree?.key).toBeDefined();
    expect(
      (await client.getRevision({ root: review.root, rootHash: base })).revision?.rootHash,
    ).toBe(base);
    const compact = await client.diffRevisions({
      root: review.root,
      base,
      target: review.revision,
      compact: true,
    });
    const full = await client.diffRevisions({
      root: review.root,
      base,
      target: review.revision,
      compact: false,
    });
    expect(compact.entries.length).toBe(s.diff.entries.length);
    expect(full.entries.length).toBe(s.diffFull.entries.length);

    // Anything else isn't in the review: no network to fall back on.
    await expect(client.getRevision({ root: review.root, rootHash: "nope" })).rejects.toThrow(
      /isn't part of the plan review/,
    );
    await expect(client.getGraph({ root: "another" })).rejects.toThrow(/isn't part/);
    await expect(client.listGraphs({})).rejects.toThrow();
  });
});
