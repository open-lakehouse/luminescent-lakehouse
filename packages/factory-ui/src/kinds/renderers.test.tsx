import { afterEach, describe, expect, test } from "bun:test";
import { create } from "@bufbuild/protobuf";
import type { Any } from "@bufbuild/protobuf/wkt";
import {
  type Gate,
  GateSchema,
  KIND,
  type Node,
  NodeSpecSchema,
  PrMergedSchema,
  packPayload,
  payloadRegistry,
  schemaRegistry,
  type Task,
  TaskSchema,
  unpackPayload,
} from "@open-lakehouse/factory-client";
import { exampleExtension, FakeFactory } from "@open-lakehouse/factory-client/testing";
import { cleanup, render } from "@testing-library/react";
import { GenericPayload } from "./GenericPayload.js";
import { cardChips, summarize } from "./renderers.js";

afterEach(cleanup);

/** A fake with the example extension registered, a plan, and one task and
 * one gate that carry its payloads. */
function fixture(fake: FakeFactory) {
  const ext = exampleExtension();
  ext.register(fake);
  const root = fake.createNode("", KIND.plan, create(NodeSpecSchema, { title: "P" })).ref;
  const task = fake.createNode(
    root,
    KIND.task,
    create(NodeSpecSchema, {
      title: "T",
      payload: packPayload(TaskSchema, {
        acceptance: "a",
        extensions: [ext.pack("remember the milk", 3)],
      }),
    }),
  );
  const gate = fake.createNode(
    root,
    KIND.gate,
    create(NodeSpecSchema, {
      title: "G",
      payload: packPayload(GateSchema, { condition: ext.pack("noted", 1) }),
    }),
  );
  const merged = fake.createNode(
    root,
    KIND.gate,
    create(NodeSpecSchema, {
      title: "M",
      payload: packPayload(GateSchema, {
        condition: packPayload(PrMergedSchema, { task: task.ref }),
      }),
    }),
  );
  return { root, task, gate, merged, registry: schemaRegistry(fake.descriptors()) };
}

describe("payloads from GetSchema's descriptors", () => {
  test("chips name extensions and unknown conditions, and known ones by renderer", () => {
    const fake = new FakeFactory();
    const { task, gate, merged, registry } = fixture(fake);
    const ctx = {
      schema: fake.schema,
      registry,
      titleOf: (ref: string) => (ref === task.ref ? "T" : undefined),
    };
    expect(cardChips(task, ctx)).toEqual(["Note"]);
    expect(cardChips(gate, ctx)).toEqual(["Noted"]);
    expect(cardChips(merged, ctx)).toEqual(["Merged: T"]);
  });

  test("an extension renders its fields from the descriptors", () => {
    const fake = new FakeFactory();
    const { task, registry } = fixture(fake);
    const t = unpackPayload(task.spec?.payload, registry) as Task;
    const r = render(
      <GenericPayload
        payload={t.extensions[0] as Any}
        registry={registry}
        schema={fake.schema}
        place="extension"
      />,
    );
    const ext = r.getByRole("region", { name: "Note" });
    expect(ext.textContent).toContain("remember the milk");
    expect(ext.textContent).toContain("count");
    expect(ext.textContent).toContain("3");
  });

  test("an unknown gate condition renders generically, a known one by its renderer", () => {
    const fake = new FakeFactory();
    const { gate, merged, registry } = fixture(fake);
    const ctx = { schema: fake.schema, registry };
    const condition = (n: Node) => (unpackPayload(n.spec?.payload, registry) as Gate).condition;
    expect(summarize(condition(gate), ctx, "condition")).toBe("Noted");
    expect(summarize(condition(merged), ctx, "condition")).toStartWith("Merged: ");
    const r = render(
      <GenericPayload
        payload={condition(gate) as Any}
        registry={registry}
        schema={fake.schema}
        place="condition"
      />,
    );
    expect(r.getByRole("region", { name: "Noted" }).textContent).toContain("noted");
  });

  test("without descriptors, an extension is kept but not decoded", () => {
    const fake = new FakeFactory();
    const { task } = fixture(fake);
    const t = unpackPayload(task.spec?.payload) as Task;
    const r = render(
      <GenericPayload payload={t.extensions[0] as Any} registry={payloadRegistry} />,
    );
    expect(r.getByTestId("undecodable-payload").textContent).toContain("example.v1.Note");
  });
});
