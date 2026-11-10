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
  void state.listenInstances((change) => void emit("instance-changed", { change }));
  // The app's settings, as src-tauri keeps them (settings.json).
  let settings = {
    quit: "keep",
    oneAtATime: [] as string[],
    lastInstance: null as string | null,
    neonTargets: {} as Record<
      string,
      { project: string | null; branch: { kind: string; branch?: string; ttlHours?: number } }
    >,
  };
  // The current environment, as src-tauri's run commands keep it.
  let current: unknown = null;
  let currentKey = "none";
  const announce = (next: unknown, key?: string) => {
    current = next;
    currentKey = key ?? "none";
    void emit("environment-changed", { key: currentKey, current: next });
  };
  let neonBranches = [
    { id: "br-production", name: "production", default: true, owned: false, expiresAt: null },
    { id: "br-staging", name: "staging", default: false, owned: false, expiresAt: null },
    {
      id: "br-scratch",
      name: "scratch",
      default: false,
      owned: true,
      expiresAt: "2026-11-14T00:00:00Z",
    },
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
          const env = s("environment");
          let oneAtATime = false;
          for (const id of settings.oneAtATime) {
            const snapshot = await state.stateWorkspace(id);
            if (snapshot.status === "ok" && snapshot.data.environments.some((e) => e.id === env)) {
              oneAtATime = true;
            }
          }
          const target = settings.neonTargets[env];
          const started = (await settle(
            state.runStart(env, oneAtATime, (target?.branch as never) ?? null),
          )) as { id: string; revision: number; branch: { name: string } | null };
          if (target?.branch.kind === "new" && started.branch) {
            settings = {
              ...settings,
              neonTargets: {
                ...settings.neonTargets,
                [env]: { ...target, branch: { kind: "existing", branch: started.branch.name } },
              },
            };
          }
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
          if ((current as { environmentId?: string } | null)?.environmentId === s("environment"))
            announce(null);
          return null;
        case "run_service_uis":
          return [
            {
              moduleId: "mlflow",
              name: "MLflow",
              url: "http://local-dev-r1.localhost:9080/mlflow/",
            },
          ];
        case "service_embeds_supported":
          return true;
        case "embed_service_ui":
          (window as { __LL_EMBEDS__?: unknown[] }).__LL_EMBEDS__ = [
            ...((window as { __LL_EMBEDS__?: unknown[] }).__LL_EMBEDS__ ?? []),
            { module: s("module"), bounds: a.bounds ?? null },
          ];
          return null;
        case "open_service_window":
          (window as { __LL_OPENED__?: string[] }).__LL_OPENED__ = [
            ...((window as { __LL_OPENED__?: string[] }).__LL_OPENED__ ?? []),
            s("module"),
          ];
          return null;
        case "instance_logs":
        case "instance_logs_stop":
          // Playwright doesn't stream logs: the viewer is covered by bun test.
          return null;
        case "run_reconciled":
          return { adopted: [], stale: [], orphans: [] };
        case "settings_get":
          return settings;
        case "settings_set_quit":
          settings = { ...settings, quit: s("quit") };
          return settings;
        case "settings_set_one_at_a_time": {
          const rest = settings.oneAtATime.filter((w) => w !== s("workspace"));
          settings = { ...settings, oneAtATime: a.on ? [...rest, s("workspace")] : rest };
          return settings;
        }
        case "run_stop_instance":
          await settle(state.runStopInstance(s("instance")));
          if ((current as { instanceId?: string } | null)?.instanceId === s("instance"))
            announce(null);
          return null;
        case "run_current":
          return { key: currentKey, current };
        case "neon_branches":
          return {
            projectId: "cool-poetry-28343550",
            source: "default",
            parent: "staging",
            branches: neonBranches,
          };
        case "neon_projects":
          return [
            { id: "cool-poetry-28343550", name: "luminescent-eu", org: "open-lakehouse" },
            { id: "quiet-lake-11", name: "luminescent-us", org: "open-lakehouse" },
          ];
        case "neon_target_set": {
          const { [s("environment")]: _, ...rest } = settings.neonTargets;
          settings = {
            ...settings,
            neonTargets: a.target ? { ...rest, [s("environment")]: a.target as never } : rest,
          };
          return settings;
        }
        case "neon_needs":
          return settle(state.neonNeeds(s("environment")));
        case "neon_create_branch": {
          const branch = {
            id: `br-mock-${neonBranches.length + 1}`,
            name: s("name"),
            default: false,
            owned: true,
            expiresAt: null,
          };
          neonBranches = [...neonBranches, branch];
          return branch;
        }
        case "neon_delete_branch": {
          const branch = neonBranches.find((b) => b.name === s("branch"));
          if (!branch?.owned) throw { kind: "neon", message: `${s("branch")} isn't owned` };
          neonBranches = neonBranches.filter((b) => b !== branch);
          return branch;
        }
        case "neon_reset_branch":
          return null;
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
