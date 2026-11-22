// An environment's tab header and tree row: its run state and placement (a local slot
// or its Neon branch, and the gateway's URL), and its actions: the selectors of its
// Neon branch and its models (each shows the current choice and picks while it is
// stopped; Start binds them), its variables, Start and Stop, a revision switch when
// several run side by side, and a way to its apps. Plain functions of the model, so
// the ⌘K commands list the same actions.

import type { EnvironmentDetail, EnvironmentStatus, HostAction } from "@open-lakehouse/env-editor";
import { Bot, Braces, Cloud, LayoutGrid, Play, Square } from "lucide-react";
import { modelsLabel } from "../ModelsDialog";
import { editorState, moduleStatus, runState } from "../run-state";
import { message } from "../WorkspaceBar";
import type { EnvironmentModel } from "./model";
import { busyReason } from "./run";

/** What the header's actions do. */
export interface HeaderHandlers {
  pickNeon: (m: EnvironmentModel) => void;
  chooseModels: (m: EnvironmentModel) => void;
  editVariables: (m: EnvironmentModel) => void;
  start: (m: EnvironmentModel) => void;
  stop: (m: EnvironmentModel) => void;
  stopInstance: (m: EnvironmentModel, instance: string) => void;
  /** Show instance `instance` in the header. */
  show: (m: EnvironmentModel, instance: string) => void;
  /** The Apps view on `instance`, where the host has one. */
  openApps?: (instance: string) => void;
}

export function environmentStatus(m: EnvironmentModel): EnvironmentStatus {
  const { env, op, shown, failure } = m;
  if (op === "start") {
    return {
      state: "starting",
      revision: env.head ?? undefined,
      detail: "pulling images on the first run, then waiting until every container is healthy",
    };
  }
  if (op === "stop") return { state: "stopping", revision: shown?.revision };
  // The Neon branch and the Models choice are the header's selectors (`actions`),
  // which show the current choice: no details repeat them.
  if (!shown) {
    return failure
      ? { state: "failed", label: "start failed", detail: message(failure) }
      : { state: "stopped" };
  }
  const state = runState(shown);
  const details: EnvironmentDetail[] = [];
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
  // The selectors first: each shows its current choice, and is picked here while
  // the environment is stopped. The Neon one appears as soon as the draft puts
  // something on Neon.
  if (m.onNeon || m.target) {
    out.push({
      id: "neon-target",
      label: m.neon ? `${m.neon.label} ▾` : "Pick a Neon branch ▾",
      icon: <Cloud />,
      enabled: !running && !busy,
      reason: running ? `Stop ${name} to move it to another branch` : reason,
      run: () => h.pickNeon(m),
    });
  }
  // Where its models come from: applies at the next Start.
  if (m.wantsModels) {
    const problem = shown?.modelsProblem;
    out.push({
      id: "models",
      label: m.models?.chosen ? `${modelsLabel(m.models)} ▾` : "Choose models ▾",
      icon: <Bot />,
      enabled: !running && !busy,
      reason: running
        ? `${problem ? `${problem}. ` : ""}Stop ${name} to change its models`
        : reason,
      run: () => h.chooseModels(m),
    });
  }
  // Its variables: editable while it runs too, applied at the next Start.
  out.push({
    id: "variables",
    label: "Variables",
    icon: <Braces />,
    run: () => h.editVariables(m),
  });
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
