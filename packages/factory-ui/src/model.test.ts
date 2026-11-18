import { describe, expect, test } from "bun:test";
import { create } from "@bufbuild/protobuf";
import { EDGE, KIND, RunSchema } from "@open-lakehouse/factory-client";
import { builtinSchema, FakeFactory, seedDemo } from "@open-lakehouse/factory-client/testing";
import {
  childKinds,
  drawableEdgeKinds,
  latestRuns,
  prLabel,
  pullRequests,
  sectionAt,
  sectionText,
  sessions,
} from "./model.js";

describe("model", () => {
  const schema = builtinSchema();

  test("edge drawing offers only the kinds the schema allows between two kinds", () => {
    const names = (a: string, b: string) => drawableEdgeKinds(schema, a, b).map((k) => k.name);
    expect(names(KIND.task, KIND.task)).toEqual([
      EDGE.blockedBy,
      EDGE.relatesTo,
      EDGE.discoveredFrom,
    ]);
    expect(names(KIND.plan, KIND.task)).toEqual([EDGE.relatesTo]);
    expect(names(KIND.task, KIND.task)).not.toContain(EDGE.childOf);
  });

  test("child kinds come from the parent's kind", () => {
    expect(childKinds(schema, KIND.decision)).toEqual([]);
    expect(childKinds(schema, KIND.plan)).toContain(KIND.task);
  });

  test("sections cut a rendered revision by node", () => {
    const text = "# Plan: P\n\n## Task: A\nacceptance: x\n";
    const sections = [
      { key: "p", startLine: 1, endLine: 1 },
      { key: "a", startLine: 3, endLine: 4 },
    ];
    expect(sectionText(text, sections, "a")).toBe("## Task: A\nacceptance: x\n");
    expect(sectionAt(sections, 4)).toBe("a");
    expect(sectionAt(sections, 2)).toBeUndefined();
  });

  test("gates sit under plans and block like tasks", () => {
    expect(childKinds(schema, KIND.plan)).toContain(KIND.gate);
    expect(childKinds(schema, KIND.gate)).toEqual([]);
    expect(drawableEdgeKinds(schema, KIND.task, KIND.gate).map((k) => k.name)).toContain(
      EDGE.blockedBy,
    );
  });

  test("a node's latest run is its highest attempt", () => {
    const runs = [
      create(RunSchema, { ref: "r1", node: "a", attempt: 1 }),
      create(RunSchema, { ref: "r2", node: "a", attempt: 2 }),
      create(RunSchema, { ref: "r3", node: "b", attempt: 1 }),
    ];
    const latest = latestRuns(runs);
    expect(latest.get("a")?.ref).toBe("r2");
    expect(latest.get("b")?.ref).toBe("r3");
  });

  test("a run's PRs come bottom layer first, with their session steps", () => {
    const fake = new FakeFactory();
    const root = seedDemo(fake);
    const run = latestRuns(fake.graphOf(root).runs).values().next().value;
    expect(pullRequests(run).map(prLabel)).toEqual(["#101 1/2", "#102 2/2"]);
    expect(sessions(run).map((s) => [s.name, s.session.spec?.harness])).toEqual([
      ["implement", "claude-sdk"],
      ["review", "codex"],
    ]);
  });
});
