// An environment's tab header and tree row: its run state and placement (a local slot
// or its Neon branch), a summary of what its next Start binds (its Neon branch and its
// models: each a detail that opens the Configure tab on it), the gateway's URL, and
// its run controls: Start and Stop, a revision switch when several run side by side,
// and a way to its apps. Plain functions of the model, so the ⌘K commands list the
// same actions.

import type { EnvironmentDetail, EnvironmentStatus, HostAction } from "@open-lakehouse/env-editor";
import { Bot, Cloud, LayoutGrid, Play, Square } from "lucide-react";
import { modelsLabel } from "../ModelsForm";
import { editorState, moduleStatus, runState } from "../run-state";
import { message } from "../WorkspaceBar";
import type { ConfigureSection } from "./ConfigureTab";
import type { EnvironmentModel } from "./model";
import { busyReason } from "./run";

/** What the header's details and actions do. */
export interface HeaderHandlers {
  /** Open the Configure tab on `section`. */
  configure: (m: EnvironmentModel, section: ConfigureSection) => void;
  start: (m: EnvironmentModel) => void;
  stop: (m: EnvironmentModel) => void;
  stopInstance: (m: EnvironmentModel, instance: string) => void;
  /** Show instance `instance` in the header. */
  show: (m: EnvironmentModel, instance: string) => void;
  /** The Apps view on `instance`, where the host has one. */
  openApps?: (instance: string) => void;
}

/** What the next Start binds, as header details that open Configure: the Neon branch
 *  (unless the placement already names it), and the models. */
function configDetails(m: EnvironmentModel, h: HeaderHandlers): EnvironmentDetail[] {
  const out: EnvironmentDetail[] = [];
  const placedOnBranch = !!m.shown?.branch && m.shown.slot === null;
  if ((m.onNeon || m.target) && !placedOnBranch) {
    out.push({
      label: "Neon",
      value: m.neon?.label ?? "pick a branch",
      icon: <Cloud />,
      title: `The Neon branch ${m.name} runs on: Configure`,
      run: () => h.configure(m, "neon"),
    });
  }
  if (m.wantsModels) {
    const problem = m.shown?.modelsProblem;
    out.push({
      label: "Models",
      value: m.models?.chosen ? modelsLabel(m.models) : "choose",
      icon: <Bot />,
      title: `${problem ? `${problem}. ` : ""}Where ${m.name}'s agents get models from: Configure`,
      run: () => h.configure(m, "models"),
    });
  }
  return out;
}

export function environmentStatus(m: EnvironmentModel, h: HeaderHandlers): EnvironmentStatus {
  const { env, op, shown, failure } = m;
  const details = configDetails(m, h);
  if (op === "start") {
    return {
      state: "starting",
      revision: env.head ?? undefined,
      detail: "pulling images on the first run, then waiting until every container is healthy",
      details,
    };
  }
  if (op === "stop") return { state: "stopping", revision: shown?.revision, details };
  if (!shown) {
    return failure
      ? { state: "failed", label: "start failed", detail: message(failure), details }
      : { state: "stopped", details };
  }
  const state = runState(shown);
  if (shown.gatewayUrl && shown.desired === "running") {
    details.push({
      label: "Gateway",
      value: shown.gatewayUrl,
      copy: true,
      title: "Its services, through the gateway (the Run panel lists them)",
    });
  }
  return {
    state: editorState(state),
    label: m.live.length > 1 ? `${state} (${m.live.length} revisions)` : state,
    revision: shown.revision,
    // On the tree row the branch stands in for a slot it lacks.
    placement:
      shown.slot !== null
        ? `local slot ${shown.slot}`
        : shown.branch
          ? `☁ ${shown.branch.name}`
          : undefined,
    details,
  };
}

export function environmentActions(m: EnvironmentModel, h: HeaderHandlers): HostAction[] {
  const { env, name, op, shown, running: runningRevs } = m;
  const busy = op !== undefined;
  const running = runningRevs.length > 0;
  const reason = busyReason(name, op);
  const out: HostAction[] = [];
  if (!m.headRuns || op === "start") {
    out.push({
      id: "start",
      label: op === "start" ? "Starting…" : running && env.head ? `Start #${env.head}` : "Start",
      icon: <Play />,
      variant: "primary",
      enabled: !busy && !!env.head,
      reason: !env.head ? "Commit a revision first (History)" : reason,
      run: () => h.start(m),
    });
  }
  // Its apps live in the Apps view, on the instance shown here.
  const openApps = h.openApps;
  if (openApps && shown && shown.desired === "running") {
    const up = runState(shown);
    const ready = up === "running" || up === "unhealthy";
    out.push({
      id: "open-apps",
      label: "Apps",
      icon: <LayoutGrid />,
      enabled: ready,
      reason: ready ? undefined : `${name} is ${up}`,
      run: () => openApps(shown.id),
    });
  }
  if (runningRevs.length > 1) {
    for (const i of runningRevs) {
      out.push({
        id: `show-r${i.revision}`,
        label: `#${i.revision}`,
        variant: i.id === shown?.id ? "primary" : "default",
        run: () => h.show(m, i.id),
      });
    }
  }
  if (m.live.length > 1 && shown) {
    out.push({
      id: "stop-shown",
      label: `Stop #${shown.revision}`,
      icon: <Square />,
      enabled: !busy,
      run: () => h.stopInstance(m, shown.id),
    });
  }
  if (m.live.length > 0 || op === "stop") {
    out.push({
      id: "stop",
      label: op === "stop" ? "Stopping…" : m.live.length > 1 ? "Stop all" : "Stop",
      icon: <Square />,
      enabled: !busy,
      reason,
      run: () => h.stop(m),
    });
  }
  return out;
}

/** A module's state on the canvas, from the shown instance. */
export function environmentNodeStatus(m: EnvironmentModel, moduleId: string) {
  const { shown } = m;
  if (!shown || shown.services.length === 0) return undefined;
  // A function on the branch: no container to ask, so it is as up as the instance.
  if (shown.branch?.functions.includes(moduleId)) {
    return {
      state: runState(shown) === "running" ? "running" : "unknown",
      label: "function",
      detail: `a Neon Function on ${shown.branch.name}`,
    } as const;
  }
  return moduleStatus(shown, moduleId);
}
