// What an environment's instances are doing, from what the shared state wants and
// what Docker reports, and how that reads in the editor (its run states, a
// service's dot on the canvas). Shared by the environment contributions, the Apps
// view and the Run panel.

import type { RunState as EditorRunState, NodeStatus } from "@open-lakehouse/env-editor";
import type { EnvironmentRun, InstanceStatus, ServiceRow } from "./shell-host";

export type RunState = "stopped" | "starting" | "running" | "unhealthy" | "stopping";

/** A one-shot job (certificates, a migration) that ran to completion. */
export function isDone(s: ServiceRow): boolean {
  return s.state === "exited" && s.exitCode === 0;
}

/** Exited with an error, died, or failing its healthcheck. */
export function isFailing(s: ServiceRow): boolean {
  return (
    s.health === "unhealthy" || s.state === "dead" || (s.state === "exited" && s.exitCode !== 0)
  );
}

/** What an instance is doing, from what is wanted and what Docker reports. */
export function runState(instance: InstanceStatus | undefined): RunState {
  if (!instance) return "stopped";
  const services = instance.services;
  if (instance.desired === "stopped") return services.length > 0 ? "stopping" : "stopped";
  if (services.some(isFailing)) return "unhealthy";
  const serving = services.filter((s) => !isDone(s));
  if (serving.length > 0 && serving.every((s) => s.ready)) return "running";
  return "starting";
}

/** Instances that run, or still have containers, newest first. */
export function liveInstances(env: EnvironmentRun): InstanceStatus[] {
  return env.instances
    .filter((i) => i.desired === "running" || i.services.length > 0)
    .sort((a, b) => b.revision - a.revision);
}

/** The editor's word for a state. */
export function editorState(state: RunState): EditorRunState {
  return state === "unhealthy" ? "degraded" : state;
}

/** A service's state, from its container. */
export function serviceStatus(s: ServiceRow): NodeStatus {
  if (isDone(s)) return { state: "stopped", label: "done", detail: s.status };
  if (isFailing(s)) {
    const label = s.state === "exited" ? `exited (${s.exitCode})` : s.health;
    return { state: "failed", label, detail: s.status };
  }
  if (s.ready)
    return {
      state: "running",
      label: s.health === "healthy" ? "healthy" : "running",
      detail: s.status,
    };
  return { state: "starting", label: s.state, detail: s.status };
}

/**
 * The state of module `moduleId` in `instance`: its service's container (the plan
 * says which; a service isn't named after its module, Unity Catalog runs as
 * `unitycatalog`). A one-shot helper (`rustfs-init`) isn't its module's state.
 */
export function moduleStatus(instance: InstanceStatus, moduleId: string): NodeStatus | undefined {
  const rows = instance.services.filter((s) => s.module === moduleId);
  // A module with several services is as well as its worst one.
  const order: NodeStatus["state"][] = ["failed", "starting", "degraded", "running", "stopped"];
  const statuses = rows.map(serviceStatus);
  return statuses.sort((a, b) => order.indexOf(a.state) - order.indexOf(b.state))[0];
}

/** Whether `instance` runs module `moduleId` (a container of one of its services). */
export function serves(instance: InstanceStatus, moduleId: string): boolean {
  return instance.services.some((s) => s.module === moduleId);
}

/** The instance an environment's tab is about: the one picked in its revision switch,
 *  else the current environment's, else the newest one wanted running, else the newest
 *  with containers. */
export function shownInstance(
  env: EnvironmentRun,
  picked: string | undefined,
  currentId: string | undefined,
): InstanceStatus | undefined {
  const live = liveInstances(env);
  const running = live.filter((i) => i.desired === "running");
  return (
    running.find((i) => i.id === picked) ??
    running.find((i) => i.id === currentId) ??
    running[0] ??
    live[0]
  );
}

/** Where an instance runs: its local slot, and the Neon branch it is bound to. */
export function placementLabel(instance: InstanceStatus): string | undefined {
  const parts = [
    instance.branch && `☁ ${instance.branch.name}`,
    instance.slot !== null && `local slot ${instance.slot}`,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : undefined;
}

/** Whether `instance` serves module `moduleId`: a container of it, or a function on its
 *  branch. */
export function servesModule(instance: InstanceStatus, moduleId: string): boolean {
  return serves(instance, moduleId) || !!instance.branch?.functions.includes(moduleId);
}
