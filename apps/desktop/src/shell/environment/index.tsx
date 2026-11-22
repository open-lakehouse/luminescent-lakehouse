// The desktop's contributions to the editor for the workspace's environments: each
// one's run state and placement (tree row, tab header), its header's summary and run
// controls, its Configure tab, its services' states on the canvas, the bottom panel's
// Run, Logs and Environment, and its commands for ⌘K. Composed here from the model
// (model.ts: each environment resolved once), the run (run.ts), the header
// (header.tsx), the Configure tab (ConfigureTab.tsx), the commands (commands.ts) and
// the panels (panels.tsx). This file holds what ties them together: the section each
// Configure tab shows, and Start, which opens it on what it lacks (a Neon branch, its
// models) and goes on once that is saved.

import type {
  Command,
  EnvironmentContributions,
  EnvironmentTab,
  PanelContribution,
} from "@open-lakehouse/env-editor";
import { useBottomPanel, useEditor } from "@open-lakehouse/env-editor";
import { useQueryClient } from "@tanstack/react-query";
import { SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { missingQueryKey, modelsQueryKey } from "../ModelsForm";
import { type NeonTarget, useShellHost } from "../shell-host";
import { message } from "../WorkspaceBar";
import { CONFIGURE_TAB, type ConfigureSection, ConfigureTab } from "./ConfigureTab";
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

export { CONFIGURE_TAB, type ConfigureSection } from "./ConfigureTab";
export { selectionOnNeon, selectionWantsModels } from "./model";
export { ENV_PANEL, LOGS_PANEL, RUN_PANEL } from "./panels";
export { RUN_REFRESH_MS, runQueryKey, useRunQuery, useWorkspaceRun } from "./run";

export interface EnvironmentsView {
  environment: EnvironmentContributions;
  /** The bottom panel's Run, Logs and Environment. */
  panels: PanelContribution[];
  /** The commands for the ⌘K palette: each environment's header actions, its
   *  Configure sections, and their `trestle` CLI equivalents to copy. */
  commands: Command[];
}

/** A Start waiting on a Configure section of an environment (by name). */
type Waiting = { name: string; section: ConfigureSection } | null;

/** The editor's environment contributions for workspace `workspaceId` (named
 *  `workspaceName`, as the CLI addresses it). `openApps` shows the Apps view on an
 *  instance. Needs env-editor's `EditorProvider` and `BottomPanelProvider` above it. */
export function useEnvironmentContributions(
  workspaceId: string,
  workspaceName: string,
  openApps?: (instance: string) => void,
): EnvironmentsView {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const bottomPanel = useBottomPanel();
  const editor = useEditor();
  const run = useRunActions();
  const environments = useEnvironments(workspaceId, run);
  const { act } = run;

  /** The section each environment's Configure tab shows, by name. */
  const [sections, setSections] = useState<Record<string, ConfigureSection>>({});
  const [waiting, setWaiting] = useState<Waiting>(null);

  /** Show `m`'s Configure tab, on `section` if given; `start`: a Start waits on it. */
  const configure = (m: EnvironmentModel, section?: ConfigureSection, start = false) => {
    if (section) setSections((s) => ({ ...s, [m.name]: section }));
    setWaiting(start && section ? { name: m.name, section } : null);
    editor.openEnvironment(m.editorId, { subTab: CONFIGURE_TAB });
  };

  const start = (m: EnvironmentModel) => {
    setWaiting(null);
    act(m.name, "start", () => shell.run.start(m.env.id));
  };
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
          configure(m, "models", true);
        } else {
          start(m);
        }
      },
      // Asking failed: Start says what is wrong.
      () => start(m),
    );
  };

  const saveTarget = (m: EnvironmentModel, target: NeonTarget) => {
    const then = waiting?.name === m.name && waiting.section === "neon";
    void shell.neon.setTarget(m.env.id, target).then(
      (next) => {
        queryClient.setQueryData(["settings"], next);
        if (then) beginStart({ ...m, target });
        else toast.success(`${m.name}'s Neon branch is saved`);
      },
      (e: unknown) => toast.error(`${m.name}: ${message(e)}`),
    );
  };
  const modelsSaved = (m: EnvironmentModel) => {
    void queryClient.invalidateQueries({ queryKey: ["models"] });
    if (waiting?.name === m.name && waiting.section === "models") {
      start(m);
    } else {
      toast.success(`${m.name}'s models are saved`);
    }
  };

  const handlers: HeaderHandlers = {
    configure: (m, section) => configure(m, section),
    // A revision on Neon without a target asks for one first, then its Models.
    start: (m) => (m.needs?.needed && !m.target ? configure(m, "neon", true) : beginStart(m)),
    stop: (m) => act(m.name, "stop", () => shell.run.stop(m.env.id)),
    stopInstance: (m, instance) => act(m.name, "stop", () => shell.run.stopInstance(instance)),
    show: (m, instance) => environments.pick(m.name, instance),
    openApps,
  };

  const tabs = (envId: string): EnvironmentTab[] => {
    const m = environments.byEditorId(envId);
    if (!m) return [];
    const waits = waiting?.name === m.name;
    return [
      {
        id: CONFIGURE_TAB,
        label: "Configure",
        icon: <SlidersHorizontal />,
        enabled: true,
        render: () => (
          <ConfigureTab
            m={m}
            workspaceId={workspaceId}
            section={waits ? waiting?.section : sections[m.name]}
            onSection={(section) => {
              setSections((s) => ({ ...s, [m.name]: section }));
              if (waiting?.name === m.name && waiting.section !== section) setWaiting(null);
            }}
            waiting={waits}
            onCancelWaiting={() => setWaiting(null)}
            onSaveTarget={(target) => saveTarget(m, target)}
            onModelsSaved={() => modelsSaved(m)}
          />
        ),
      },
    ];
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
        configure: (m, section) => configure(m, section),
      },
      { logs: LOGS_PANEL, effectiveEnv: ENV_PANEL },
    ),
  );

  return {
    commands,
    environment: {
      status: (envId) => {
        const m = environments.byEditorId(envId);
        return m && environmentStatus(m, handlers);
      },
      actions: (envId) => {
        const m = environments.byEditorId(envId);
        return m ? environmentActions(m, handlers) : [];
      },
      tabs,
      nodeStatus: (envId, moduleId) => {
        const m = environments.byEditorId(envId);
        return m && environmentNodeStatus(m, moduleId);
      },
    },
    panels: environmentPanels(environments, openApps),
  };
}
