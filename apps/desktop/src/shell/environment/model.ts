// Each environment as the desktop's contributions to the editor see it, resolved once
// per render: its run data, the instance the editor shows, the Start or Stop in
// flight, and its configuration (whether it runs on Neon and the branch picked for it,
// whether it takes models and which). The card, the actions, the commands, the panels and the
// configuration all read this one model, so they can't disagree.
//
// The editor knows environments by its own ids; the run data, by the store's. Names
// are unique within a workspace, so the two meet on the name, here and nowhere else.

import type { Selection } from "@open-lakehouse/env-editor";
import { useWorkspace } from "@open-lakehouse/env-editor";
import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { modelsQueryKey } from "../ModelsForm";
import { targetLabel } from "../NeonTargetForm";
import { liveInstances, shownInstance } from "../run-state";
import {
  type EnvironmentRun,
  type InstanceStatus,
  type ModelsInfo,
  type NeonNeeds,
  type NeonTarget,
  type Settings,
  useShellHost,
  type WorkspaceRun,
} from "../shell-host";
import { type Op, type RunActions, useWorkspaceRun } from "./run";

/** Whether `selection` puts anything on a Neon branch: a module run as a Neon
 *  Function, or the branch's Postgres or object storage. Read off the draft, so the
 *  environment's Neon target can be picked while it is edited, before a revision of
 *  it needs one. */
export function selectionOnNeon(selection: Selection | undefined): boolean {
  if (!selection) return false;
  return (
    selection.modules.includes("neon-storage") ||
    Object.values(selection.hosted ?? {}).some(
      (h) =>
        h.vendor === "neon" ||
        h.vendor === "neon-function" ||
        h.cloud === "neon" ||
        h.connection.kind === "function",
    )
  );
}

/** Modules that take models: the choice (ModelsDialog) applies to them. */
const MODEL_MODULES = ["mlflow", "omnigent"];

/** Whether the draft runs something that takes models (MLflow or Omnigent). */
export function selectionWantsModels(selection: Selection | undefined): boolean {
  return !!selection?.modules.some((m) => MODEL_MODULES.includes(m));
}

/** One environment, resolved. */
export interface EnvironmentModel {
  /** The editor's id. */
  editorId: string;
  /** Its run data; `env.id` is the store's id. */
  env: EnvironmentRun;
  name: string;
  /** The instance the card and actions show: the one picked, the current one, else the newest. */
  shown: InstanceStatus | undefined;
  /** Its instances with containers or wanted running, the newest first. */
  live: InstanceStatus[];
  /** Those wanted running. */
  running: InstanceStatus[];
  /** Whether its head revision runs. */
  headRuns: boolean;
  /** The Start or Stop in flight. */
  op: Op | undefined;
  /** The last Start or Stop that failed. */
  failure: Error | undefined;
  /** What its head revision needs of a Neon branch. */
  needs: NeonNeeds | undefined;
  /** The Neon branch picked for its next Start. */
  target: NeonTarget | undefined;
  /** Whether its head revision or its draft runs anything on Neon. */
  onNeon: boolean;
  /** The Neon branch it runs on (its shown instance's), or would at the next Start (its
   *  target's): the selector's label, and the branch's name once it has one. */
  neon: { label: string; branch?: string } | undefined;
  /** Whether its draft takes models (MLflow or Omnigent): then it has a Models choice. */
  wantsModels: boolean;
  /** Its Models choice, when it takes models. */
  models: ModelsInfo | undefined;
}

export interface Environments {
  /** The run data, as last asked. */
  data: WorkspaceRun | undefined;
  /** The app's settings (the Neon targets). */
  settings: Settings | undefined;
  /** Every environment of the run data, by its name. */
  byName: Map<string, EnvironmentModel>;
  /** The same, in the workspace's order (the tree's). */
  all: EnvironmentModel[];
  /** The environment the editor calls `envId`. */
  byEditorId: (envId: string) => EnvironmentModel | undefined;
  /** Show instance `instance` of environment `name` in its card and actions. */
  pick: (name: string, instance: string) => void;
}

/** The workspace's environments, resolved for the contributions. `actions` holds the
 *  Starts and Stops in flight. */
export function useEnvironments(workspaceId: string, actions: RunActions): Environments {
  const shell = useShellHost();
  const host = useHost();
  const { workspace } = useWorkspace();
  const run = useWorkspaceRun(workspaceId);
  useEnvironmentKey(); // re-render when the current environment changes
  const currentId = host.environment.current()?.id;
  const [picked, setPicked] = useState<Record<string, string>>({});
  const settings = useQuery({ queryKey: ["settings"], queryFn: () => shell.run.settings() });

  const data = run.data;
  const environments = data?.environments ?? [];
  const selectionOf = (name: string) => workspace?.envs.find((e) => e.name === name)?.selection;
  // What each environment's head revision needs of a Neon branch.
  const committed = environments.filter((e) => e.head != null);
  const needsQueries = useQueries({
    queries: committed.map((e) => ({
      queryKey: ["neon-needs", e.id, e.head],
      queryFn: () => shell.run.neonNeeds(e.id),
      staleTime: Number.POSITIVE_INFINITY,
    })),
  });
  const needsOf = new Map(committed.map((e, n) => [e.name, needsQueries[n]?.data] as const));
  const modelEnvs = environments.filter((e) => selectionWantsModels(selectionOf(e.name)));
  const modelsQueries = useQueries({
    queries: modelEnvs.map((e) => ({
      queryKey: [...modelsQueryKey(e.id), e.head],
      queryFn: () => shell.models.get(e.id),
    })),
  });
  const modelsOf = new Map(modelEnvs.map((e, n) => [e.name, modelsQueries[n]?.data] as const));
  const targets = settings.data?.neonTargets ?? {};
  const projects = useQuery({
    queryKey: ["neon-projects"],
    queryFn: () => shell.neon.projects(),
    staleTime: 5 * 60_000,
    enabled: Object.values(targets).some((t) => t.project),
  });
  const labelOf = (target: NeonTarget) =>
    targetLabel(target, projects.data?.find((p) => p.id === target.project)?.name);

  const byName = new Map<string, EnvironmentModel>();
  for (const env of environments) {
    const editorId = workspace?.envs.find((e) => e.name === env.name)?.id;
    if (!editorId) continue;
    const live = liveInstances(env);
    const running = live.filter((i) => i.desired === "running");
    const shown = shownInstance(env, picked[env.name], currentId);
    const target = targets[env.id];
    const selection = selectionOf(env.name);
    const needs = needsOf.get(env.name);
    const neon = shown?.branch
      ? { label: shown.branch.name, branch: shown.branch.name }
      : target
        ? {
            label: labelOf(target).replace(/^☁ /, ""),
            branch: target.branch.kind === "existing" ? target.branch.branch : undefined,
          }
        : undefined;
    const wantsModels = selectionWantsModels(selection);
    byName.set(env.name, {
      editorId,
      env,
      name: env.name,
      shown,
      live,
      running,
      headRuns: running.some((i) => i.revision === env.head),
      op: actions.pending[env.name],
      failure: actions.failures[env.name],
      needs,
      target,
      onNeon: !!needs?.needed || selectionOnNeon(selection),
      neon,
      wantsModels,
      models: wantsModels ? modelsOf.get(env.name) : undefined,
    });
  }
  const byEditorId = (envId: string) => {
    const name = workspace?.envs.find((e) => e.id === envId)?.name;
    return name ? byName.get(name) : undefined;
  };
  const all = (workspace?.envs ?? []).flatMap((e) => byName.get(e.name) ?? []);
  const pick = (name: string, instance: string) => setPicked((p) => ({ ...p, [name]: instance }));
  return { data, settings: settings.data, byName, all, byEditorId, pick };
}
