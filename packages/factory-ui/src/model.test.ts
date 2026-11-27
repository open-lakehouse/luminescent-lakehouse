import { describe, expect, test } from "bun:test";
import { create } from "@bufbuild/protobuf";
import { EDGE, EnvironmentPlacesSchema, KIND, RunSchema } from "@open-lakehouse/factory-client";
import { builtinSchema, FakeFactory, seedDemo } from "@open-lakehouse/factory-client/testing";
import {
  childKinds,
  drawableEdgeKinds,
  latestRuns,
  ordinal,
  prLabel,
  pullRequests,
  queuedLabel,
  queuePlace,
  runBadge,
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

describe("a queued run's place in line", () => {
  const waiting = (ref: string, environment = "sandbox") =>
    create(RunSchema, {
      ref,
      node: `node-${ref}`,
      waiting: { environment, since: { seconds: 1_800_000_000n } },
    });
  // The line spans graphs: run-x is another graph's, ahead of ours.
  const places = [
    create(EnvironmentPlacesSchema, {
      environment: "sandbox",
      maxSessions: 2,
      held: [
        { run: "run-h1", root: "g1" },
        { run: "run-h2", root: "g2" },
      ],
      waiting: [
        { run: "run-x", root: "g2" },
        { run: "run-b", root: "g1" },
        { run: "run-c", root: "g1" },
      ],
    }),
  ];

  test("counts every graph's runs ahead of it", () => {
    expect(queuePlace(waiting("run-b"), places)).toEqual({
      environment: "sandbox",
      since: new Date(1_800_000_000_000),
      position: 2,
      waiting: 3,
      held: 2,
      max: 2,
    });
  });

  test("knows only the environment until the places load, or list the run", () => {
    expect(queuePlace(waiting("run-b"), undefined)?.position).toBeUndefined();
    expect(queuePlace(waiting("run-new"), places)).toEqual({
      environment: "sandbox",
      since: new Date(1_800_000_000_000),
    });
    expect(queuePlace(waiting("run-b", "other"), places)?.position).toBeUndefined();
  });

  test("a run that doesn't wait has no place", () => {
    expect(queuePlace(create(RunSchema, { ref: "run-a" }), places)).toBeUndefined();
    expect(
      runBadge(create(RunSchema, { ref: "run-a", stage: "implementing" }))?.queued,
    ).toBeUndefined();
  });

  test("the card says where and how far back", () => {
    const badge = runBadge(waiting("run-c"), places);
    expect(badge?.queued).toEqual({ environment: "sandbox", position: 3, of: 3 });
    expect(queuedLabel(badge?.queued ?? { environment: "" })).toBe("queued · sandbox · 3rd of 3");
    expect(queuedLabel({ environment: "sandbox" })).toBe("queued · sandbox");
    expect([1, 2, 3, 4, 11, 12, 13, 21, 22, 101].map(ordinal)).toEqual([
      "1st",
      "2nd",
      "3rd",
      "4th",
      "11th",
      "12th",
      "13th",
      "21st",
      "22nd",
      "101st",
    ]);
  });
});
