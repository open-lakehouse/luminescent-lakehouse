import { describe, expect, test } from "bun:test";
import { create, type DescMessage, type MessageShape } from "@bufbuild/protobuf";
import { type Any, anyUnpack } from "@bufbuild/protobuf/wkt";
import {
  KIND,
  NodeSchema,
  PlanSchema,
  packPayload,
  ReconcileNoteSchema,
  TaskSchema,
} from "@open-lakehouse/factory-client";
import { draftOf, payloadOf } from "./NodeInspector.js";

function unpack<D extends DescMessage>(any: Any | undefined, schema: D): MessageShape<D> {
  const m = any ? anyUnpack(any, schema) : undefined;
  if (!m) throw new Error(`no ${schema.typeName} payload`);
  return m;
}

describe("saving a node's payload", () => {
  test("a task keeps its repo, profiles and extensions", () => {
    const node = create(NodeSchema, {
      ref: "n",
      kind: KIND.task,
      spec: {
        title: "T",
        payload: packPayload(TaskSchema, {
          acceptance: "old",
          repo: "o/r",
          profile: "executor",
          extensions: [packPayload(ReconcileNoteSchema, { text: "an extension" })],
        }),
      },
    });
    const draft = { ...draftOf(node), acceptance: "new", touchedPaths: "a\nb" };
    const saved = unpack(payloadOf(KIND.task, draft, node), TaskSchema);
    expect(saved.acceptance).toBe("new");
    expect(saved.touchedPaths).toEqual(["a", "b"]);
    expect(saved.repo).toBe("o/r");
    expect(saved.profile).toBe("executor");
    expect(saved.extensions.map((e) => e.typeUrl)).toEqual([
      "type.googleapis.com/factory.workgraph.v1.ReconcileNote",
    ]);
  });

  test("a plan keeps its repos and planner profile", () => {
    const node = create(NodeSchema, {
      ref: "p",
      kind: KIND.plan,
      spec: {
        title: "P",
        payload: packPayload(PlanSchema, {
          goal: "g",
          repos: [{ slug: "o/r" }],
          plannerProfile: "planner",
        }),
      },
    });
    const saved = unpack(payloadOf(KIND.plan, { ...draftOf(node), goal: "g2" }, node), PlanSchema);
    expect(saved.goal).toBe("g2");
    expect(saved.repos.map((r) => r.slug)).toEqual(["o/r"]);
    expect(saved.plannerProfile).toBe("planner");
  });
});
