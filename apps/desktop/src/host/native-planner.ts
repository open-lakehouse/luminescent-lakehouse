// The environment editor's `Planner` on the native `olai-stack-topology`
// (shell-core's `planner`, over IPC): the same planner, at the same version, that
// the runtime runs environments with. It returns the same DTOs as the in-browser
// wasm planner.

import type { CatalogDto, Planner, PlanResult } from "@open-lakehouse/env-editor";
import { commands, type PlanError } from "./bindings";

type Bundle = Awaited<ReturnType<NonNullable<Planner["bundle"]>>>;

/** The commands the planner uses (all of `commands` in the app; a fake in tests). */
export type PlannerApi = Pick<typeof commands, "plannerCatalog" | "plannerPlan" | "plannerBundle">;

function unwrap<T>(result: { status: "ok"; data: T } | { status: "error"; error: PlanError }): T {
  if (result.status === "error") throw new Error(result.error.message);
  return result.data;
}

export function createNativePlanner(api: PlannerApi = commands): Planner {
  return {
    async catalog() {
      return (await api.plannerCatalog()) as CatalogDto;
    },
    async plan(selection, target, context) {
      return unwrap(
        await api.plannerPlan(selection, target ?? null, context ?? null),
      ) as PlanResult;
    },
    async bundle(selection, target, credentials, context) {
      return unwrap(
        await api.plannerBundle(selection, target ?? null, credentials, context ?? null),
      ) as Bundle;
    },
  };
}
