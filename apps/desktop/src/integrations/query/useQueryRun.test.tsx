import { afterEach, expect, test } from "bun:test";
import type { QueryChunk, QueryRunner } from "@open-lakehouse/query-runner";
import { act, cleanup, render, renderHook, screen, waitFor } from "@testing-library/react";
import { fakeQueryTransport, frameChunk } from "../../host/fake-query";
import { decodeChunk, hostQueryRunner } from "../../host/query";
import { QueryStatus } from "./QueryStatus";
import { type Schedule, useQueryRun } from "./useQueryRun";

afterEach(cleanup);

const now: Schedule = (callback) => callback();

test("runs into the store and counts the rows", async () => {
  const runner = hostQueryRunner(
    "inst-1",
    fakeQueryTransport(() => ({
      chunks: [{ id: [1, 2] }, { id: [3] }],
    })),
  );
  const { result } = renderHook(() => useQueryRun(runner, now));
  await act(() => result.current.run("SELECT id FROM t"));
  expect(result.current.running).toBe(false);
  expect(result.current.error === null).toBe(true);
  expect(result.current.rows).toBe(3);
  expect(result.current.store.rowCount).toBe(3);
  expect(result.current.elapsedMs).toBeGreaterThanOrEqual(0);
});

test("says why a query failed, and keeps no rows from before", async () => {
  let fail = false;
  const runner = hostQueryRunner(
    null,
    fakeQueryTransport(() =>
      fail
        ? { error: { kind: "invalid", message: "table 'nope' not found" } }
        : { chunks: [{ id: [1] }] },
    ),
  );
  const { result } = renderHook(() => useQueryRun(runner, now));
  await act(() => result.current.run("SELECT 1"));
  expect(result.current.rows).toBe(1);
  fail = true;
  await act(() => result.current.run("SELECT * FROM nope"));
  expect(result.current.error).toBe("table 'nope' not found");
  expect(result.current.store.rowCount).toBe(0);
});

/** A runner that yields one chunk, then waits until it is aborted. */
function hangingRunner(): { runner: QueryRunner; aborted: () => boolean } {
  let aborted = false;
  const runner: QueryRunner = (_req, { signal }) => ({
    async *[Symbol.asyncIterator](): AsyncGenerator<QueryChunk> {
      yield decodeChunk(frameChunk({ id: [1] }));
      await new Promise<void>((_, reject) => {
        signal.addEventListener("abort", () => {
          aborted = true;
          reject(signal.reason);
        });
      });
    },
  });
  return { runner, aborted: () => aborted };
}

test("cancel stops the run and keeps what came", async () => {
  const { runner, aborted } = hangingRunner();
  const { result } = renderHook(() => useQueryRun(runner, now));
  let done: Promise<void> = Promise.resolve();
  act(() => {
    done = result.current.run("SELECT * FROM big");
  });
  await waitFor(() => expect(result.current.rows).toBe(1));
  expect(result.current.running).toBe(true);
  act(() => result.current.cancel());
  await act(() => done);
  expect(aborted()).toBe(true);
  expect(result.current.running).toBe(false);
  // A cancel isn't an error.
  expect(result.current.error === null).toBe(true);
  expect(result.current.store.rowCount).toBe(1);
});

test("a new run replaces the one still going", async () => {
  const hanging = hangingRunner();
  let first = true;
  const runner: QueryRunner = (req, opts) => {
    if (first) {
      first = false;
      return hanging.runner(req, opts);
    }
    return hostQueryRunner(
      null,
      fakeQueryTransport(() => ({ chunks: [{ id: [7, 8] }] })),
    )(req, opts);
  };
  const { result } = renderHook(() => useQueryRun(runner, now));
  act(() => {
    void result.current.run("SELECT * FROM big");
  });
  await waitFor(() => expect(result.current.rows).toBe(1));
  await act(() => result.current.run("SELECT 7"));
  expect(hanging.aborted()).toBe(true);
  expect(result.current.rows).toBe(2);
  expect(result.current.running).toBe(false);
});

test("the status line", () => {
  const { rerender } = render(
    <QueryStatus running={false} rows={0} elapsedMs={null} failed={false} />,
  );
  expect(screen.queryByTestId("query-status") === null).toBe(true);
  rerender(<QueryStatus running={true} rows={0} elapsedMs={null} failed={false} />);
  expect(screen.getByTestId("query-status").textContent).toBe("running…");
  rerender(<QueryStatus running={false} rows={1} elapsedMs={12} failed={false} />);
  expect(screen.getByTestId("query-status").textContent).toBe("1 row · 12 ms");
  rerender(<QueryStatus running={false} rows={0} elapsedMs={5} failed={true} />);
  expect(screen.getByTestId("query-status").textContent).toBe("failed after 5 ms");
});
