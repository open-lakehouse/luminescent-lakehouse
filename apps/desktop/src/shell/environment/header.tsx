// What an environment shows in the editor: its run state and placement (a local slot
// or its Neon branch) on its tree row and tab, and in its hover card what its next
// Start binds (its Neon branch and its models, with their logos: each a detail that
// opens the Configure tab on it) and the gateway's URL. Its run action (Start, or
// Stop while its head runs) is an icon on its tree row and at the end of its
// sub-tabs; the rest (a revision switch when several run side by side, a way to its
// apps, Stop all) is in their ⋯ menus. Plain functions of the model, so the ⌘K
// commands list the same actions.

import type {
  CommitAction,
  EnvironmentDetail,
  EnvironmentStatus,
  HostAction,
} from "@open-lakehouse/env-editor";
import { Bot, Globe, KeyRound, LayoutGrid, Play, Square } from "lucide-react";
import type { ReactNode } from "react";
import { DatabricksIcon, NeonIcon } from "../brandIcons";
import { modelsLabel } from "../ModelsForm";
import { editorState, moduleStatus, runState } from "../run-state";
import type { ModelsInfo } from "../shell-host";
import { message } from "../WorkspaceBar";
import type { ConfigureSection } from "./ConfigureTab";
import type { EnvironmentModel } from "./model";
import { busyReason } from "./run";

/** What the card's details and the actions do. */
export interface EnvironmentHandlers {
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

/** The logo of where the models come from. */
function modelsIcon(models: ModelsInfo | undefined): ReactNode {
  if (!models?.chosen) return <Bot />;
  switch (models.choice.kind) {
    case "databricksProfile":
      return <DatabricksIcon />;
    case "neonAiGateway":
      return <NeonIcon />;
    case "keys":
      return <KeyRound />;
  }
}

/** What the next Start binds, as card details that open Configure: the Neon branch
 *  (the one it runs on, else the one picked for it), and the models. */
function configDetails(m: EnvironmentModel, h: EnvironmentHandlers): EnvironmentDetail[] {
  const out: EnvironmentDetail[] = [];
  if (m.onNeon || m.neon) {
    out.push({
      label: "Neon",
      value: m.neon?.label ?? "pick a branch",
      icon: <NeonIcon />,
      title: `The Neon branch ${m.name} runs on: Configure`,
      run: () => h.configure(m, "neon"),
    });
  }
  if (m.wantsModels) {
    const problem = m.shown?.modelsProblem;
    out.push({
      label: "Models",
      value: m.models?.chosen ? modelsLabel(m.models) : "choose",
      icon: modelsIcon(m.models),
      title: `${problem ? `${problem}. ` : ""}Where ${m.name}'s agents get models from: Configure`,
      run: () => h.configure(m, "models"),
    });
  }
  return out;
}

export function environmentStatus(m: EnvironmentModel, h: EnvironmentHandlers): EnvironmentStatus {
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
      icon: <Globe />,
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

export function environmentActions(m: EnvironmentModel, h: EnvironmentHandlers): HostAction[] {
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

/** The action drawn as an icon on `m`'s tree row and sub-tabs: Stop while its head
 *  runs (or a Stop is in flight), else Start. One of `environmentActions`, so the two
 *  agree; the editor leaves it out of the ⋯ menus. */
export function environmentRunAction(
  m: EnvironmentModel,
  actions: HostAction[],
): HostAction | undefined {
  const stopping = m.op === "stop" || (m.headRuns && m.op !== "start");
  return actions.find((a) => a.id === (stopping ? "stop" : "start"));
}

/** More ways to commit `m`'s draft: commit, then Start the new head (`head`, the
 *  revision the commit made). */
export function environmentCommitActions(
  m: EnvironmentModel,
  startHead: (m: EnvironmentModel, head: number) => void,
): CommitAction[] {
  const busy = m.op !== undefined;
  return [
    {
      id: "commit-start",
      label: "Commit and start",
      icon: <Play />,
      enabled: !busy,
      reason: busyReason(m.name, m.op),
      run: (revision) => startHead(m, revision.number),
    },
  ];
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
