import { expect, test } from "bun:test";
import { columnsOf, JOBS, lineageEvents, serviceToken, TABLES, uuidFor } from "./demo-seed";

test("run ids are stable UUIDs", () => {
  expect(uuidFor("a")).toBe(uuidFor("a"));
  expect(uuidFor("a")).not.toBe(uuidFor("b"));
  expect(uuidFor("a")).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
});

test("every job reads and writes known tables or landing paths, wiring one graph", () => {
  const tables = new Set(TABLES.map((t) => `${t.schema}.${t.name}`));
  const written = new Set(JOBS.flatMap((j) => j.outputs));
  for (const job of JOBS) {
    for (const ref of [...job.inputs, ...job.outputs]) {
      expect(ref.startsWith("s3://") || tables.has(ref)).toBe(true);
    }
    // Every table input is some other job's output.
    for (const input of job.inputs.filter((i) => tables.has(i))) {
      expect(written.has(input)).toBe(true);
    }
  }
  // And every table is written by a job.
  for (const table of tables) expect(written.has(table)).toBe(true);
});

test("two runs per job, each a START then a COMPLETE with the outputs' schema", () => {
  type Event = {
    eventType: string;
    eventTime: string;
    run: { runId: string };
    job: { name: string };
    outputs: { namespace: string; name: string; facets?: { schema?: { fields: unknown[] } } }[];
  };
  const events = lineageEvents(new Date("2026-11-03T12:00:00Z")) as Event[];
  expect(events).toHaveLength(JOBS.length * 4);
  const runs = new Map<string, Event[]>();
  for (const e of events) runs.set(e.run.runId, [...(runs.get(e.run.runId) ?? []), e]);
  expect(runs.size).toBe(JOBS.length * 2);
  for (const [, [start, complete]] of runs) {
    expect(start?.eventType).toBe("START");
    expect(complete?.eventType).toBe("COMPLETE");
    expect(Date.parse(complete?.eventTime ?? "")).toBeGreaterThan(
      Date.parse(start?.eventTime ?? ""),
    );
    const out = complete?.outputs[0];
    expect(out?.namespace).toBe("unity-catalog");
    expect(out?.name.startsWith("demo.")).toBe(true);
    expect(out?.facets?.schema?.fields.length).toBeGreaterThan(0);
  }
  // The same ids on a rerun.
  const again = lineageEvents(new Date()) as Event[];
  expect(again.map((e) => e.run.runId)).toEqual(events.map((e) => e.run.runId));
});

test("columns carry UC's type names and positions", () => {
  const orders = TABLES.find((t) => t.name === "orders");
  if (!orders) throw new Error("no orders");
  const columns = columnsOf(orders);
  expect(columns.map((c) => c.position)).toEqual([0, 1, 2, 3]);
  expect(columns.find((c) => c.name === "total")?.type_name).toBe("DOUBLE");
  expect(JSON.parse(columns[0]?.type_json ?? "{}").type).toBe("string");
});

test("the Unity Catalog token comes from LUMINESCENT_SERVICE_TOKENS, as the app reads it", () => {
  const spec = " headwaters=x , unity-catalog = olk_ab_cd ";
  expect(serviceToken(spec, "unity-catalog")).toBe("olk_ab_cd");
  expect(serviceToken(spec, "rustfs")).toBeUndefined();
  expect(serviceToken("unity-catalog=", "unity-catalog")).toBeUndefined();
  expect(serviceToken(undefined, "unity-catalog")).toBeUndefined();
});
