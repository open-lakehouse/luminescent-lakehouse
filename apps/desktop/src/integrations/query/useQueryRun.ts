// One query at a time through a `QueryRunner`, into an Arrow result store for
// arrow-grid's `DataGrid`: a new run aborts the one before, chunks append as they
// come, and the grid re-renders at most once a frame.

import { ArrowResultStore } from "@open-lakehouse/arrow-grid";
import type { QueryRunner } from "@open-lakehouse/query-runner";
import { useCallback, useEffect, useRef, useState } from "react";

export interface QueryRunState {
  store: ArrowResultStore;
  /** Bumped on each change to the store, for the grid. */
  version: number;
  running: boolean;
  error: string | null;
  /** Rows so far, and how long the run took (or has taken). */
  rows: number;
  elapsedMs: number | null;
  run(sql: string): Promise<void>;
  cancel(): void;
}

/** A frame scheduler; tests pass one that runs at once. */
export type Schedule = (callback: () => void) => void;

const nextFrame: Schedule = (callback) => {
  requestAnimationFrame(() => callback());
};

export function useQueryRun(runner: QueryRunner, schedule: Schedule = nextFrame): QueryRunState {
  const storeRef = useRef(new ArrowResultStore());
  const abortRef = useRef<AbortController | null>(null);
  const [version, setVersion] = useState(0);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rows, setRows] = useState(0);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);

  // Stop a run that outlives the view.
  useEffect(() => () => abortRef.current?.abort(), []);

  const run = useCallback(
    async (sql: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      const store = storeRef.current;
      store.reset();
      setError(null);
      setRows(0);
      setElapsedMs(null);
      setRunning(true);
      setVersion((v) => v + 1);
      const started = performance.now();
      let count = 0;
      let pending = false;
      const flush = () => {
        pending = false;
        if (controller.signal.aborted) return;
        setRows(count);
        setVersion((v) => v + 1);
      };
      try {
        for await (const chunk of runner({ sql }, { signal: controller.signal })) {
          store.append(chunk.arrowIpc);
          count += chunk.numRows;
          if (!pending) {
            pending = true;
            schedule(flush);
          }
        }
      } catch (e) {
        if (!controller.signal.aborted) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
          setRows(count);
          setVersion((v) => v + 1);
          setElapsedMs(Math.round(performance.now() - started));
          setRunning(false);
        }
      }
    },
    [runner, schedule],
  );

  const cancel = useCallback(() => abortRef.current?.abort(), []);

  return {
    store: storeRef.current,
    version,
    running,
    error,
    rows,
    elapsedMs,
    run,
    cancel,
  };
}
