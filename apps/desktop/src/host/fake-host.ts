// A `ShellHost` for tests and the Playwright build: the real workspace-store
// adapter on the in-memory fake state, the fixture planner, a fixed doctor.

import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { READY_REPORT } from "../shell/fixtures";
import { type DoctorReport, runFailure, type ShellHost } from "../shell/shell-host";
import type { Json, RunError, StateError } from "./bindings";
import { createFakeState, type FakeState } from "./fake-state";
import { createStateWorkspaceStore, stateErrorMessage, subscribeWith } from "./workspace-store";

function unwrap<T>(result: { status: "ok"; data: T } | { status: "error"; error: StateError }): T {
  if (result.status === "error") throw new Error(stateErrorMessage(result.error));
  return result.data;
}

function unwrapRun<T>(result: { status: "ok"; data: T } | { status: "error"; error: RunError }): T {
  if (result.status === "error") throw runFailure(result.error);
  return result.data;
}

export interface FakeShellHost extends ShellHost {
  /** The state behind it, to write to as the CLI would. */
  state: FakeState;
}

export function createFakeShellHost(
  options: { doctor?: () => Promise<DoctorReport>; state?: FakeState } = {},
): FakeShellHost {
  const state = options.state ?? createFakeState(FIXTURE_CATALOG.default_selection as Json);
  return {
    state,
    doctor: options.doctor ?? (async () => READY_REPORT),
    planner: createWasmPlanner(),
    workspaces: {
      location: async () => unwrap(await state.stateLocation()),
      list: async () => unwrap(await state.stateWorkspaces()),
      create: async (name, firstEnv) =>
        unwrap(await state.stateCreateWorkspace(name, false, firstEnv)),
      store: (id) => createStateWorkspaceStore(id, state, state.listen),
      subscribe: (onChange) => subscribeWith(state.listen, onChange),
    },
    run: {
      workspace: async (id) => unwrapRun(await state.runWorkspace(id)),
      start: async (environment) => unwrapRun(await state.runStart(environment)),
      stop: async (environment) => {
        unwrapRun(await state.runStop(environment));
      },
    },
  };
}
