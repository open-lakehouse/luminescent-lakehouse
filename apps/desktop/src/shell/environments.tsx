// The workspace's environments as the editor shows them: each one's run state
// and placement (tree row, tab header: a local slot or its Neon branch, and the
// gateway's URL), the selectors of its Neon branch and its models (each shows the
// current choice and picks while it is stopped; Start binds them), Start and Stop,
// a revision switch when several run side by side, and a way to its apps (in the
// Apps view, apps.tsx). The
// bottom panel gets Run and Logs, for the open environment. What runs comes from
// trestle's instances (the ones `trestle up` / `ps` / `down` work on), refreshed
// on Docker's events, on commits, and every 30 s.
//
// The editor knows environments by its own ids; the run data, by the store's.
// Names are unique within a workspace, so the two meet on the name.

import type {
  Command,
  EnvironmentContributions,
  EnvironmentDetail,
  EnvironmentStatus,
  HostAction,
  PanelContribution,
  Selection,
} from "@open-lakehouse/env-editor";
import { useBottomPanel, useWorkspace } from "@open-lakehouse/env-editor";
import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, Cloud, LayoutGrid, Play, ScrollText, Square } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import {
  cliTarget,
  downCommand,
  healthCommand,
  logsCommand,
  restartCommand,
  upCommand,
} from "./cli";
import { LogsPanel } from "./LogsPanel";
import { ModelsDialog, missingQueryKey, modelsLabel, modelsQueryKey } from "./ModelsDialog";
import { NeonTargetDialog, targetLabel } from "./NeonTargetDialog";
import { RunPanel } from "./RunPanel";
import { editorState, liveInstances, moduleStatus, runState, shownInstance } from "./run-state";
import {
  type EnvironmentRun,
  type NeonTarget,
  useShellHost,
  type WorkspaceRun,
} from "./shell-host";
import { message } from "./WorkspaceBar";

/** How often the containers are asked about anyway: Docker's events refresh them as
 *  things change. */
export const RUN_REFRESH_MS = 30_000;
/** Events come in bursts (one per container): one refresh per burst. */
const EVENT_SETTLE_MS = 150;

export const runQueryKey = (workspaceId: string) => ["run", workspaceId] as const;

/** The workspace's environments and their instances, as last asked. Kept live by
 *  `useWorkspaceRun`, which the frame mounts once. */
export function useRunQuery(workspaceId: string) {
  const shell = useShellHost();
  return useQuery({
    queryKey: runQueryKey(workspaceId),
    queryFn: () => shell.run.workspace(workspaceId),
    refetchInterval: RUN_REFRESH_MS,
  });
}

/** The workspace's environments and their instances, live with Docker and the state. */
export function useWorkspaceRun(workspaceId: string) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const run = useRunQuery(workspaceId);
  // A commit (here or in the CLI) changes what Start runs: refresh at once.
  useEffect(
    () =>
      shell.workspaces.subscribe(() => void queryClient.invalidateQueries({ queryKey: ["run"] })),
    [shell, queryClient],
  );
  // A container started, stopped, died or changed health.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = shell.run.subscribe(() => {
      clearTimeout(timer);
      timer = setTimeout(
        () => void queryClient.invalidateQueries({ queryKey: ["run"] }),
        EVENT_SETTLE_MS,
      );
    });
    return () => {
      clearTimeout(timer);
      unsubscribe();
    };
  }, [shell, queryClient]);
  return run;
}

type Op = "start" | "stop";

/** The bottom panel's tabs this module contributes. */
export const RUN_PANEL = "run";
export const LOGS_PANEL = "logs";

export interface EnvironmentsView {
  environment: EnvironmentContributions;
  /** The bottom panel's Run and Logs. */
  panels: PanelContribution[];
  /** Start's branch dialog, while it asks. */
  dialog: ReactNode;
  /** The runtime commands for the ⌘K palette: each environment's header actions, and
   *  their `trestle` CLI equivalents to copy. */
  commands: Command[];
}

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

/** Put `text` on the clipboard, and say so. */
export function copyToClipboard(text: string) {
  void navigator.clipboard.writeText(text).then(
    () => toast.success("Copied to the clipboard", { description: text }),
    (e: unknown) => toast.error(`Copy: ${message(e)}`),
  );
}

/** The editor's environment contributions for workspace `workspaceId` (named
 *  `workspaceName`, as the CLI addresses it). `openApps` shows the Apps view on an
 *  instance. */
export function useEnvironmentContributions(
  workspaceId: string,
  workspaceName: string,
  openApps?: (instance: string) => void,
): EnvironmentsView {
  const shell = useShellHost();
  const host = useHost();
  const queryClient = useQueryClient();
  const { workspace } = useWorkspace();
  const bottomPanel = useBottomPanel();
  const run = useWorkspaceRun(workspaceId);
  useEnvironmentKey(); // re-render when the current environment changes
  const currentId = host.environment.current()?.id;

  const [pending, setPending] = useState<Record<string, Op>>({});
  const [failures, setFailures] = useState<Record<string, Error>>({});
  const [picked, setPicked] = useState<Record<string, string>>({});
  /** The environment (by name) whose Neon target is being picked, and whether a
   *  Start waits on it. */
  const [asking, setAsking] = useState<{ name: string; start: boolean } | null>(null);
  /** The environment (by name) whose Models are being chosen, and whether a Start
   *  waits on it. */
  const [askingModels, setAskingModels] = useState<{ name: string; start: boolean } | null>(null);
  const settings = useQuery({ queryKey: ["settings"], queryFn: () => shell.run.settings() });

  const data: WorkspaceRun | undefined = run.data;
  // What each environment's head revision needs of a Neon branch.
  const committed = (data?.environments ?? []).filter((e) => e.head != null);
  const needsQueries = useQueries({
    queries: committed.map((e) => ({
      queryKey: ["neon-needs", e.id, e.head],
      queryFn: () => shell.run.neonNeeds(e.id),
      staleTime: Number.POSITIVE_INFINITY,
    })),
  });
  const needsOf = new Map(committed.map((e, n) => [e.name, needsQueries[n]?.data] as const));
  /** Whether the environment's draft takes models: then it has a Models choice. */
  const wantsModels = (name: string) =>
    selectionWantsModels(workspace?.envs.find((e) => e.name === name)?.selection);
  const modelEnvs = (data?.environments ?? []).filter((e) => wantsModels(e.name));
  const modelsQueries = useQueries({
    queries: modelEnvs.map((e) => ({
      queryKey: [...modelsQueryKey(e.id), e.head],
      queryFn: () => shell.models.get(e.id),
    })),
  });
  const modelsOf = new Map(modelEnvs.map((e, n) => [e.name, modelsQueries[n]?.data] as const));
  /** Whether the environment's head revision or its draft runs anything on Neon. */
  const onNeon = (env: EnvironmentRun) =>
    !!needsOf.get(env.name)?.needed ||
    selectionOnNeon(workspace?.envs.find((e) => e.name === env.name)?.selection);

  const act = useCallback(
    (name: string, op: Op, work: () => Promise<unknown>) => {
      setPending((p) => ({ ...p, [name]: op }));
      setFailures(({ [name]: _, ...rest }) => rest);
      work()
        .catch((e: unknown) => {
          const error = e instanceof Error ? e : new Error(String(e));
          setFailures((f) => ({ ...f, [name]: error }));
          toast.error(`${op === "start" ? "Start" : "Stop"} ${name}: ${message(error)}`, {
            description: "The Run panel (⌘J) has the details.",
          });
        })
        .finally(() => {
          setPending(({ [name]: _, ...rest }) => rest);
          void queryClient.invalidateQueries({ queryKey: ["run"] });
          void queryClient.invalidateQueries({ queryKey: ["settings"] });
        });
    },
    [queryClient],
  );

  const start = (env: EnvironmentRun) => act(env.name, "start", () => shell.run.start(env.id));
  /** Start, asking for its Models first when it takes models and none were chosen, or
   *  when a secret its head revision requires is set nowhere. */
  const beginStart = (env: EnvironmentRun) => {
    if (!wantsModels(env.name)) {
      start(env);
      return;
    }
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
          setAskingModels({ name: env.name, start: true });
        } else {
          start(env);
        }
      },
      // Asking failed: Start says what is wrong.
      () => start(env),
    );
  };
  const targetOf = (env: EnvironmentRun): NeonTarget | undefined =>
    settings.data?.neonTargets[env.id];
  const projects = useQuery({
    queryKey: ["neon-projects"],
    queryFn: () => shell.neon.projects(),
    staleTime: 5 * 60_000,
    enabled: Object.values(settings.data?.neonTargets ?? {}).some((t) => t.project),
  });
  const labelOf = (target: NeonTarget) =>
    targetLabel(target, projects.data?.find((p) => p.id === target.project)?.name);

  const envById = (envId: string): EnvironmentRun | undefined => {
    const name = workspace?.envs.find((e) => e.id === envId)?.name;
    return name ? data?.environments.find((e) => e.name === name) : undefined;
  };

  const status = (envId: string): EnvironmentStatus | undefined => {
    const env = envById(envId);
    if (!env) return undefined;
    const op = pending[env.name];
    const shown = shownInstance(env, picked[env.name], currentId);
    if (op === "start") {
      return {
        state: "starting",
        revision: env.head ?? undefined,
        detail: "pulling images on the first run, then waiting until every container is healthy",
      };
    }
    if (op === "stop") return { state: "stopping", revision: shown?.revision };
    const failure = failures[env.name];
    // The Neon branch and the Models choice are the header's selectors (`actions`),
    // which show the current choice: no details repeat them.
    if (!shown) {
      return failure
        ? { state: "failed", label: "start failed", detail: message(failure) }
        : { state: "stopped" };
    }
    const state = runState(shown);
    const live = liveInstances(env).length;
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
      label: live > 1 ? `${state} (${live} revisions)` : state,
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
  };

  /** The Neon branch `env` runs on (its shown instance's), or would run on at the next
   *  Start (its target's), for the selector: `undefined` with neither. */
  const neonChoice = (env: EnvironmentRun): { label: string; branch?: string } | undefined => {
    const shown = shownInstance(env, picked[env.name], currentId);
    if (shown?.branch) return { label: shown.branch.name, branch: shown.branch.name };
    const target = targetOf(env);
    if (!target) return undefined;
    return {
      label: labelOf(target).replace(/^☁ /, ""),
      branch: target.branch.kind === "existing" ? target.branch.branch : undefined,
    };
  };

  const actions = (envId: string): HostAction[] => {
    const env = envById(envId);
    if (!env) return [];
    const name = env.name;
    const op = pending[name];
    const busy = op !== undefined;
    const live = liveInstances(env);
    const runningRevs = live.filter((i) => i.desired === "running");
    const headRuns = runningRevs.some((i) => i.revision === env.head);
    const shown = shownInstance(env, picked[name], currentId);
    const out: HostAction[] = [];
    const running = runningRevs.length > 0;
    const busyReason = busy ? `${name} is ${op === "start" ? "starting" : "stopping"}` : undefined;
    // The selectors first: each shows its current choice, and is picked here while
    // the environment is stopped. The Neon one appears as soon as the draft puts
    // something on Neon.
    const target = targetOf(env);
    if (onNeon(env) || target) {
      const choice = neonChoice(env);
      out.push({
        id: "neon-target",
        label: choice ? `${choice.label} ▾` : "Pick a Neon branch ▾",
        icon: <Cloud />,
        enabled: !running && !busy,
        reason: running ? `Stop ${name} to move it to another branch` : busyReason,
        run: () => setAsking({ name, start: false }),
      });
    }
    // Where its models come from: applies at the next Start.
    if (wantsModels(name)) {
      const models = modelsOf.get(name);
      const problem = shown?.modelsProblem;
      out.push({
        id: "models",
        label: models?.chosen ? `${modelsLabel(models)} ▾` : "Choose models ▾",
        icon: <Bot />,
        enabled: !running && !busy,
        reason: running
          ? `${problem ? `${problem}. ` : ""}Stop ${name} to change its models`
          : busyReason,
        run: () => setAskingModels({ name, start: false }),
      });
    }
    if (!headRuns || op === "start") {
      out.push({
        id: "start",
        label:
          op === "start"
            ? "Starting…"
            : runningRevs.length > 0 && env.head
              ? `Start #${env.head}`
              : "Start",
        icon: <Play />,
        variant: "primary",
        enabled: !busy && !!env.head,
        reason: !env.head
          ? "Commit a revision first (History)"
          : busy
            ? `${name} is ${op === "start" ? "starting" : "stopping"}`
            : undefined,
        // A revision on Neon without a target asks for one first, then its Models.
        run: () =>
          needsOf.get(name)?.needed && !targetOf(env)
            ? setAsking({ name, start: true })
            : beginStart(env),
      });
    }
    // Its apps live in the Apps view, on the instance shown here.
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
          run: () => setPicked((p) => ({ ...p, [name]: i.id })),
        });
      }
    }
    if (live.length > 1 && shown) {
      out.push({
        id: "stop-shown",
        label: `Stop #${shown.revision}`,
        icon: <Square />,
        enabled: !busy,
        run: () => act(name, "stop", () => shell.run.stopInstance(shown.id)),
      });
    }
    if (live.length > 0 || op === "stop") {
      out.push({
        id: "stop",
        label: op === "stop" ? "Stopping…" : live.length > 1 ? "Stop all" : "Stop",
        icon: <Square />,
        enabled: !busy,
        reason: busy ? `${name} is ${op === "start" ? "starting" : "stopping"}` : undefined,
        run: () => act(name, "stop", () => shell.run.stop(env.id)),
      });
    }
    return out;
  };

  const nodeStatus = (envId: string, moduleId: string) => {
    const env = envById(envId);
    const shown = env && shownInstance(env, picked[env.name], currentId);
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
  };

  /** One environment's runtime commands: its header's actions (the same ones, so the
   *  two can't disagree), a restart, and the CLI equivalents to copy. */
  const environmentCommands = (envId: string): Command[] => {
    const env = envById(envId);
    if (!env) return [];
    const name = env.name;
    const shown = shownInstance(env, picked[name], currentId);
    const busy = pending[name] !== undefined;
    const up = upCommand(cliTarget(workspaceName, name, env.head), targetOf(env)?.branch);
    const down = downCommand(cliTarget(workspaceName, name));
    const shownTarget = shown && cliTarget(workspaceName, name, shown.revision);
    const out: Command[] = [];
    for (const action of actions(envId)) {
      const base = {
        section: "Environment",
        keywords: [name],
        enabled: action.enabled,
        reason: action.reason,
        run: action.run,
      };
      if (action.id === "start") {
        const title = env.head && liveInstances(env).length > 0 ? `Start #${env.head}` : "Start";
        out.push({ ...base, id: `env.${envId}.start`, title: `${title} ${name}`, hint: up });
      } else if (action.id === "stop") {
        out.push({ ...base, id: `env.${envId}.stop`, title: `Stop ${name}`, hint: down });
      } else if (action.id === "stop-shown" && shownTarget) {
        const hint = downCommand(shownTarget);
        out.push({
          ...base,
          id: `env.${envId}.stop-shown`,
          title: `${action.label} of ${name}`,
          hint,
        });
      } else if (action.id === "neon-target") {
        out.push({ ...base, id: `env.${envId}.neon-target`, title: `Pick ${name}'s Neon branch…` });
      } else if (action.id === "models") {
        out.push({ ...base, id: `env.${envId}.models`, title: `Choose ${name}'s models…` });
      } else if (action.id.startsWith("open-")) {
        out.push({
          ...base,
          id: `env.${envId}.${action.id}`,
          title: `Open ${action.label} (${name})`,
        });
      }
    }
    const headRuns = liveInstances(env).some(
      (i) => i.desired === "running" && i.revision === env.head,
    );
    if (headRuns && env.head) {
      const hint = restartCommand(down, up);
      out.push({
        id: `env.${envId}.restart`,
        title: `Restart ${name}`,
        section: "Environment",
        keywords: [name, "stop", "start"],
        enabled: !busy,
        reason: busy ? `${name} is busy` : undefined,
        hint,
        run: () =>
          act(name, "start", async () => {
            await shell.run.stop(env.id);
            await shell.run.start(env.id);
          }),
      });
    }
    // The CLI's equivalents, whatever the header offers right now: `up` once there is
    // a revision, `down` while there is an instance, and logs and health for one with
    // containers.
    if (shown && shown.slot !== null) {
      out.push({
        id: `env.${envId}.logs`,
        title: `Show logs of ${name}`,
        section: "Environment",
        keywords: [name, "logs", "containers"],
        run: () => bottomPanel.open(LOGS_PANEL),
      });
    }
    const live = liveInstances(env).length > 0;
    const containers = shownTarget && shown.slot !== null ? shownTarget : undefined;
    const cli: [string, string | false | undefined][] = [
      ["start", !!env.head && up],
      ["stop", live && down],
      ["restart", headRuns && restartCommand(down, up)],
      ["logs", containers && logsCommand(containers)],
      ["health", containers && healthCommand(containers)],
    ];
    // The branch's name, which the Neon selector shows but can't copy.
    const branch = neonChoice(env)?.branch;
    if (branch) {
      out.push({
        id: `env.${envId}.copy-neon-branch`,
        title: `Copy ${name}'s Neon branch name`,
        hint: branch,
        section: "Environment",
        keywords: [name, "neon", "branch", "copy"],
        run: () => copyToClipboard(branch),
      });
    }
    for (const [verb, line] of cli) {
      if (!line) continue;
      out.push({
        id: `cli.${envId}.${verb}`,
        title: `Copy CLI: ${line}`,
        section: "CLI",
        keywords: [name, verb, "copy", "trestle"],
        run: () => copyToClipboard(line),
      });
    }
    return out;
  };
  const commands = (workspace?.envs ?? []).flatMap((e) => environmentCommands(e.id));

  const askingEnv = asking ? data?.environments.find((e) => e.name === asking.name) : undefined;
  const dialog = askingEnv ? (
    <NeonTargetDialog
      workspaceId={workspaceId}
      environment={askingEnv.name}
      needs={needsOf.get(askingEnv.name)}
      target={targetOf(askingEnv)}
      startAfter={!!asking?.start}
      onClose={() => setAsking(null)}
      onSave={(target) => {
        const then = asking?.start;
        setAsking(null);
        void shell.neon.setTarget(askingEnv.id, target).then(
          (next) => {
            queryClient.setQueryData(["settings"], next);
            if (then) beginStart(askingEnv);
          },
          (e: unknown) => toast.error(`${askingEnv.name}: ${message(e)}`),
        );
      }}
    />
  ) : null;
  const modelsEnv = askingModels
    ? data?.environments.find((e) => e.name === askingModels.name)
    : undefined;
  const modelsDialog = modelsEnv ? (
    <ModelsDialog
      environmentId={modelsEnv.id}
      environment={modelsEnv.name}
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

  return {
    dialog: (
      <>
        {dialog}
        {modelsDialog}
      </>
    ),
    commands,
    environment: { status, actions, nodeStatus },
    panels: [
      {
        id: RUN_PANEL,
        label: "Run",
        icon: <Play />,
        render: (envId) => {
          const env = envId ? envById(envId) : undefined;
          return (
            <RunPanel
              env={env}
              failure={env ? failures[env.name] : undefined}
              engineProblem={data?.engineProblem}
              openApps={openApps}
            />
          );
        },
      },
      {
        id: LOGS_PANEL,
        label: "Logs",
        icon: <ScrollText />,
        render: (envId) => {
          const env = envId ? envById(envId) : undefined;
          if (!env) return <PanelNote>Open an environment to follow its logs.</PanelNote>;
          const shown = shownInstance(env, picked[env.name], currentId);
          if (!shown || shown.slot === null) {
            return <PanelNote>{env.name} has no containers to follow.</PanelNote>;
          }
          return <LogsPanel key={shown.id} instance={shown} />;
        },
      },
    ],
  };
}

function PanelNote({ children }: { children: ReactNode }) {
  return <p className="p-3 text-xs text-muted-foreground">{children}</p>;
}
