// The desktop's contributions to the editor for the workspace's environments: each
// one's run state and placement (tree row, tab header), its header's actions, its
// services' states on the canvas, the bottom panel's Run, Logs and Environment, and the
// runtime commands for ⌘K. Composed here from the model (model.ts: each environment
// resolved once), the run (run.ts), the header (header.tsx), the commands
// (commands.ts) and the panels (panels.tsx); this file holds what ties them together:
// Start's questions (a Neon branch, models) and the dialogs that ask them.

import type {
  Command,
  EnvironmentContributions,
  PanelContribution,
} from "@open-lakehouse/env-editor";
import { useBottomPanel } from "@open-lakehouse/env-editor";
import { useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { ModelsDialog, missingQueryKey, modelsQueryKey } from "../ModelsDialog";
import { NeonTargetDialog } from "../NeonTargetDialog";
import { useShellHost } from "../shell-host";
import { VariablesDialog } from "../VariablesDialog";
import { message } from "../WorkspaceBar";
import { environmentCommands } from "./commands";
import {
  environmentActions,
  environmentNodeStatus,
  environmentStatus,
  type HeaderHandlers,
} from "./header";
import type { EnvironmentModel } from "./model";
import { useEnvironments } from "./model";
import { ENV_PANEL, environmentPanels, LOGS_PANEL } from "./panels";
import { useRunActions } from "./run";

export { selectionOnNeon, selectionWantsModels } from "./model";
export { ENV_PANEL, LOGS_PANEL, RUN_PANEL } from "./panels";
export { RUN_REFRESH_MS, runQueryKey, useRunQuery, useWorkspaceRun } from "./run";

export interface EnvironmentsView {
  environment: EnvironmentContributions;
  /** The bottom panel's Run, Logs and Environment. */
  panels: PanelContribution[];
  /** Start's branch dialog, while it asks. */
  dialog: ReactNode;
  /** The runtime commands for the ⌘K palette: each environment's header actions, and
   *  their `trestle` CLI equivalents to copy. */
  commands: Command[];
}

/** A question for an environment (by name), and whether a Start waits on it. */
type Asking = { name: string; start: boolean } | null;

/** The editor's environment contributions for workspace `workspaceId` (named
 *  `workspaceName`, as the CLI addresses it). `openApps` shows the Apps view on an
 *  instance. */
export function useEnvironmentContributions(
  workspaceId: string,
  workspaceName: string,
  openApps?: (instance: string) => void,
): EnvironmentsView {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const bottomPanel = useBottomPanel();
  const run = useRunActions();
  const environments = useEnvironments(workspaceId, run);
  const { act } = run;

  /** The environment whose Neon target is being picked. */
  const [asking, setAsking] = useState<Asking>(null);
  /** The environment whose Models are being chosen. */
  const [askingModels, setAskingModels] = useState<Asking>(null);
  /** The environment (by name) whose variables are open. */
  const [editingVars, setEditingVars] = useState<string | null>(null);

  const start = (m: EnvironmentModel) => act(m.name, "start", () => shell.run.start(m.env.id));
  /** Start, asking for its Models first when it takes models and none were chosen, or
   *  when a secret its head revision requires is set nowhere. */
  const beginStart = (m: EnvironmentModel) => {
    if (!m.wantsModels) {
      start(m);
      return;
    }
    const { env } = m;
    void Promise.all([
      queryClient.fetchQuery({
        queryKey: [...modelsQueryKey(env.id), env.head],
        queryFn: () => shell.models.get(env.id),
      }),
      queryClient.fetchQuery({
        queryKey: missingQueryKey(env.id),
        queryFn: () => shell.models.missingSecrets(env.id),
        staleTime: 0,
      }),
    ]).then(
      ([info, missing]) => {
        if (info.wantsModels && (!info.chosen || missing.required.length > 0)) {
          setAskingModels({ name: m.name, start: true });
        } else {
          start(m);
        }
      },
      // Asking failed: Start says what is wrong.
      () => start(m),
    );
  };

  const handlers: HeaderHandlers = {
    pickNeon: (m) => setAsking({ name: m.name, start: false }),
    chooseModels: (m) => setAskingModels({ name: m.name, start: false }),
    editVariables: (m) => setEditingVars(m.name),
    // A revision on Neon without a target asks for one first, then its Models.
    start: (m) =>
      m.needs?.needed && !m.target ? setAsking({ name: m.name, start: true }) : beginStart(m),
    stop: (m) => act(m.name, "stop", () => shell.run.stop(m.env.id)),
    stopInstance: (m, instance) => act(m.name, "stop", () => shell.run.stopInstance(instance)),
    show: (m, instance) => environments.pick(m.name, instance),
    openApps,
  };
  const actionsOf = (envId: string) => {
    const m = environments.byEditorId(envId);
    return m ? environmentActions(m, handlers) : [];
  };

  const commands = environments.all.flatMap((m) =>
    environmentCommands(
      m,
      workspaceName,
      environmentActions(m, handlers),
      {
        restart: (m) =>
          act(m.name, "start", async () => {
            await shell.run.stop(m.env.id);
            await shell.run.start(m.env.id);
          }),
        openPanel: (panel) => bottomPanel.open(panel),
      },
      { logs: LOGS_PANEL, effectiveEnv: ENV_PANEL },
    ),
  );

  const neonEnv = asking ? environments.byName.get(asking.name) : undefined;
  const neonDialog = neonEnv ? (
    <NeonTargetDialog
      workspaceId={workspaceId}
      environment={neonEnv.name}
      needs={neonEnv.needs}
      target={neonEnv.target}
      startAfter={!!asking?.start}
      onClose={() => setAsking(null)}
      onSave={(target) => {
        const then = asking?.start;
        setAsking(null);
        void shell.neon.setTarget(neonEnv.env.id, target).then(
          (next) => {
            queryClient.setQueryData(["settings"], next);
            if (then) beginStart(neonEnv);
          },
          (e: unknown) => toast.error(`${neonEnv.name}: ${message(e)}`),
        );
      }}
    />
  ) : null;
  const modelsEnv = askingModels ? environments.byName.get(askingModels.name) : undefined;
  const modelsDialog = modelsEnv ? (
    <ModelsDialog
      environmentId={modelsEnv.env.id}
      environment={modelsEnv.name}
      neonBranch={modelsEnv.neon?.label ?? null}
      startAfter={!!askingModels?.start}
      onClose={() => setAskingModels(null)}
      onSaved={() => {
        const then = askingModels?.start;
        setAskingModels(null);
        void queryClient.invalidateQueries({ queryKey: ["models"] });
        if (then) start(modelsEnv);
      }}
    />
  ) : null;
  const varsEnv = editingVars ? environments.byName.get(editingVars) : undefined;
  const varsDialog = varsEnv ? (
    <VariablesDialog
      environmentId={varsEnv.env.id}
      environment={varsEnv.name}
      instance={varsEnv.shown ? { id: varsEnv.shown.id, revision: varsEnv.shown.revision } : null}
      onClose={() => setEditingVars(null)}
    />
  ) : null;

  return {
    dialog: (
      <>
        {neonDialog}
        {modelsDialog}
        {varsDialog}
      </>
    ),
    commands,
    environment: {
      status: (envId) => {
        const m = environments.byEditorId(envId);
        return m && environmentStatus(m);
      },
      actions: actionsOf,
      nodeStatus: (envId, moduleId) => {
        const m = environments.byEditorId(envId);
        return m && environmentNodeStatus(m, moduleId);
      },
    },
    panels: environmentPanels(environments, openApps),
  };
}
