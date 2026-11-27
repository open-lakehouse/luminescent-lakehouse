import { afterEach, describe, expect, test } from "bun:test";
import { create } from "@bufbuild/protobuf";
import { RunSchema, RunState } from "@open-lakehouse/factory-client";
import { cleanup, waitFor } from "@testing-library/react";
import { backoffDelay, useRuns, useWatchGraph } from "./queries.js";
import { renderWithFactory } from "./test-utils.js";

afterEach(cleanup);

/** The watch on `root` and its runs, as the control room holds them. */
function Probe({ root }: { root: string }) {
  const status = useWatchGraph(root, { backoff: { initialMs: 5, maxMs: 20 } });
  const runs = useRuns(root);
  return (
    <div>
      <span data-testid="status">{status}</span>
      <span data-testid="runs">{(runs.data ?? []).map((r) => r.ref).join(",")}</span>
    </div>
  );
}

function setup() {
  let root = "";
  const r = renderWithFactory(
    () => <Probe root={root} />,
    "demo",
    (h) => {
      root = [...h.fake.graphs.keys()][0] as string;
    },
  );
  return { ...r, root };
}

/** A run the fake stores without telling any watcher: a change made while
 * the stream was down. */
function missedRun(root: string) {
  return create(RunSchema, {
    ref: "run-missed",
    node: root,
    attempt: 1,
    state: RunState.QUEUED,
  });
}

describe("useWatchGraph", () => {
  test("reconnects after the stream fails, and refetches what it missed", async () => {
    const r = setup();
    await waitFor(() => expect(r.fake.watchCount).toBe(1));
    await waitFor(() => expect(r.getByTestId("runs").textContent).not.toBe(""));
    r.fake.putRun(missedRun(r.root));
    expect(r.getByTestId("runs").textContent).not.toContain("run-missed");

    r.fake.dropWatches(new Error("proxy restarted"));
    await waitFor(() => expect(r.fake.watchCount).toBe(2));
    await waitFor(() => expect(r.getByTestId("runs").textContent).toContain("run-missed"));
    await waitFor(() => expect(r.getByTestId("status").textContent).toBe("live"));
  });

  test("reconnects after the stream ends cleanly", async () => {
    const r = setup();
    await waitFor(() => expect(r.fake.watchCount).toBe(1));
    r.fake.dropWatches();
    await waitFor(() => expect(r.fake.watchCount).toBe(2));
    r.fake.dropWatches();
    await waitFor(() => expect(r.fake.watchCount).toBe(3));
    expect(r.getByTestId("status").textContent).toBe("live");
  });

  test("says it's reconnecting while the server is away", async () => {
    const r = setup();
    await waitFor(() => expect(r.fake.watchCount).toBe(1));
    // Every reopened stream fails before its first message.
    const graphs = new Map(r.fake.graphs);
    r.fake.graphs.clear();
    r.fake.dropWatches(new Error("gone"));
    await waitFor(() => expect(r.getByTestId("status").textContent).toBe("reconnecting"));
    const failed = r.fake.watchCount;
    await waitFor(() => expect(r.fake.watchCount).toBeGreaterThan(failed + 1));
    for (const [k, v] of graphs) r.fake.graphs.set(k, v);
    await waitFor(() => expect(r.getByTestId("status").textContent).toBe("live"));
  });
});

describe("backoffDelay", () => {
  test("doubles from the initial wait up to the cap, less up to a fifth of jitter", () => {
    const none = () => 0;
    expect([0, 1, 2, 3, 10].map((a) => backoffDelay(a, {}, none))).toEqual([
      500, 1000, 2000, 4000, 30_000,
    ]);
    expect(backoffDelay(0, { initialMs: 100 }, () => 1)).toBe(80);
    expect(backoffDelay(6, { initialMs: 100, maxMs: 1000 }, none)).toBe(1000);
  });
});
