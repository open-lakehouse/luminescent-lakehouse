// The IPC a Playwright run (`vite build --mode e2e`) answers in place of Rust:
// the UI is exercised in a plain browser, the commands return fixtures (the
// shared state is the in-memory fake, the planner the fixture one). main.ts
// imports this only in that mode, so production builds don't contain it.

import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { READY_REPORT } from "../shell/fixtures";
import type { Json, ProxyError } from "./bindings";
import { createFakeState, type FakeState } from "./fake-state";

type Args = Record<string, unknown>;

/** `{ status: "error" }` results become rejections, as Rust's `Err` does. */
async function settle(result: Promise<{ status: string; data?: unknown; error?: unknown }>) {
  const r = await result;
  if (r.status === "error") throw r.error;
  return r.data;
}

export function installIpcMock() {
  const state = createFakeState(FIXTURE_CATALOG.default_selection as Json);
  // Playwright writes to it as the trestle CLI would (e2e builds only).
  (window as { __LL_FAKE_STATE__?: FakeState }).__LL_FAKE_STATE__ = state;
  const planner = createWasmPlanner();
  void state.listen(() => void emit("state-changed", null));
  // The current environment, as src-tauri's run commands keep it.
  let current: unknown = null;
  let currentKey = "none";
  const announce = (next: unknown, key?: string) => {
    current = next;
    currentKey = key ?? "none";
    void emit("environment-changed", { key: currentKey, current: next });
  };
  const neonBranches = [
    { id: "br-production", name: "production", default: true },
    { id: "br-staging", name: "staging", default: false },
  ];

  mockIPC(
    async (cmd, payload) => {
      const a = (payload ?? {}) as Args;
      const s = (k: string) => a[k] as string;
      switch (cmd) {
        case "doctor":
          return READY_REPORT;
        case "list_services":
          return [];
        case "service_fetch": {
          const { request } = a as { request: { moduleId: string } };
          const error: ProxyError = { kind: "unbound", moduleId: request.moduleId };
          throw error;
        }
        case "service_fetch_cancel":
          return null;
        case "state_location":
          return settle(state.stateLocation());
        case "state_workspaces":
          return settle(state.stateWorkspaces());
        case "state_create_workspace":
          return settle(state.stateCreateWorkspace(s("name"), a.mesh as boolean, s("firstEnv")));
        case "state_workspace":
          return settle(state.stateWorkspace(s("id")));
        case "state_create_environment":
          return settle(
            state.stateCreateEnvironment(s("workspace"), s("name"), (a.selection ?? null) as Json),
          );
        case "state_delete_environment":
          return settle(state.stateDeleteEnvironment(s("id")));
        case "state_set_selection":
          return settle(state.stateSetSelection(s("id"), a.selection as Json));
        case "state_revisions":
          return settle(state.stateRevisions(s("id")));
        case "state_revision":
          return settle(state.stateRevision(s("id"), a.number as number));
        case "state_commit":
          return settle(state.stateCommit(s("id")));
        case "state_import":
          return settle(
            state.stateImport(
              s("dir"),
              (a.workspace as string) ?? null,
              (a.name as string) ?? null,
            ),
          );
        case "state_export_workspace":
          return settle(state.stateExportWorkspace(s("id"), s("parent")));
        case "state_export_revision":
          return settle(state.stateExportRevision(s("id"), a.number as number, s("parent")));
        case "pick_directory":
          // Playwright picks this folder; an import picks the export made into it.
          return (window as { __LL_PICK__?: string }).__LL_PICK__ ?? "/fake/exports";
        case "run_workspace":
          return settle(state.runWorkspace(s("workspace")));
        case "run_start": {
          const started = (await settle(state.runStart(s("environment")))) as {
            id: string;
            revision: number;
          };
          announce(
            {
              kind: "instance",
              instanceId: started.id,
              workspaceId: "",
              workspace: "local",
              environmentId: s("environment"),
              environment: "dev",
              revision: started.revision,
            },
            `instance:${started.id}`,
          );
          return started;
        }
        case "run_stop":
          await settle(state.runStop(s("environment")));
          // As in Rust: a stop only clears an instance, not a Neon branch.
          if ((current as { kind?: string } | null)?.kind === "instance") announce(null);
          return null;
        case "run_current":
          return { key: currentKey, current };
        case "neon_branches":
          return { projectId: "cool-poetry-28343550", source: "default", branches: neonBranches };
        case "neon_use": {
          const branch = neonBranches.find((b) => b.name === s("branch"));
          if (!branch) throw { kind: "neon", message: `no branch ${s("branch")}` };
          announce(
            {
              kind: "neon",
              projectId: "cool-poetry-28343550",
              branchId: branch.id,
              branch: branch.name,
              workspaceId: s("workspace"),
              workspace: "local",
            },
            `neon:${branch.id}`,
          );
          return current;
        }
        case "planner_catalog":
          return planner.catalog();
        case "planner_plan":
          return planner.plan(a.selection as never, (a.target as string) ?? undefined);
        default:
          throw new Error(`ipc-mock: unexpected command ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
}
