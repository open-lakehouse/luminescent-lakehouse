// The shell's own seam, next to `@open-lakehouse/host`'s: what the shell UI
// needs from the desktop beyond what service UIs get. The desktop provides it
// from src/host; tests provide a fake.

import type { Planner, WorkspaceStore } from "@open-lakehouse/env-editor";
import { createContext, type ReactNode, useContext } from "react";
import type {
  DoctorReport,
  Imported_Serialize as Imported,
  InstanceChange,
  InstanceStatus_Serialize as InstanceStatus,
  LogMessage,
  NeonBranches,
  QuitPolicy,
  ReconcileReport,
  RunError,
  ServiceUi,
  Settings_Serialize as Settings,
  WorkspaceInfo,
  WorkspaceRun_Serialize as WorkspaceRun,
} from "../host/bindings";

export type {
  Daemon,
  DaemonFlavour,
  DoctorReport,
  EnvironmentRun_Serialize as EnvironmentRun,
  Imported_Serialize as Imported,
  InstanceChange,
  InstanceLabel,
  InstanceStatus_Serialize as InstanceStatus,
  LogEntry,
  LogMessage,
  NeonBranch,
  NeonBranches,
  ProjectSource,
  QuitPolicy,
  ReconcileReport,
  ServiceRow_Serialize as ServiceRow,
  ServiceUi,
  Settings_Serialize as Settings,
  Tool,
  WorkspaceInfo,
  WorkspaceRun_Serialize as WorkspaceRun,
} from "../host/bindings";

/** A failed start or stop: what to show, and what `docker compose` printed. */
export class RunFailure extends Error {
  readonly output?: string;
  constructor(message: string, output?: string) {
    super(message);
    this.name = "RunFailure";
    this.output = output;
  }
}

export function runFailure(error: RunError): RunFailure {
  switch (error.kind) {
    case "noRevision":
      return new RunFailure(
        `${error.env} has no revision yet: commit one (History in the editor), then start it.`,
      );
    case "unavailable":
      return new RunFailure(`Docker isn't available: ${error.message} (see Doctor).`);
    case "portsBusy":
      return new RunFailure(`Host ports are in use: ${error.ports.join(", ")}.`);
    case "compose":
      return new RunFailure(error.message, error.output);
    case "neon":
      return new RunFailure(`Neon: ${error.message} (see Doctor).`);
    case "other":
      return new RunFailure(error.message);
  }
}

/** Running environments: trestle's instances, shared with `trestle up` / `down`. */
export interface RunHost {
  workspace(id: string): Promise<WorkspaceRun>;
  /** Start the environment's head revision; resolves once it is healthy. */
  start(environment: string): Promise<InstanceStatus>;
  /** Stop every instance of the environment. */
  stop(environment: string): Promise<void>;
  /** Stop one instance (one of two revisions side by side). */
  stopInstance(instance: string): Promise<void>;
  /** Make a running instance the current environment: the views reach its services. */
  use(instance: string): Promise<void>;
  /**
   * Stream the instance's logs: the last `tail` lines per service, then (with
   * `follow`) new ones. Returns a function that stops the stream.
   */
  logs(
    instance: string,
    options: { tail: number | null; follow: boolean },
    onMessage: (message: LogMessage) => void,
  ): () => void;
  /** The instance's UIs that open in a window of their own (MLflow, Jaeger, …). */
  serviceUis(instance: string): Promise<ServiceUi[]>;
  /** Open (or focus) one of them, on the instance's own host name. */
  openServiceWindow(instance: string, module: string): Promise<void>;
  /** A container of any instance changed (Docker's events). Returns an unsubscribe. */
  subscribe(onChange: (change: InstanceChange) => void): () => void;
  /** What the launch's reconcile found; `null` until it ran. */
  reconciled(): Promise<ReconcileReport | null>;
  /** The launch's reconcile finished. Returns an unsubscribe. */
  onReconciled(onDone: () => void): () => void;
  /** The app's settings: the quit policy and the one-at-a-time workspaces. */
  settings(): Promise<Settings>;
  setQuit(quit: QuitPolicy): Promise<Settings>;
  setOneAtATime(workspace: string, on: boolean): Promise<Settings>;
}

/** Neon branches as environments: the workspace's project (`trestle ws set-neon`,
 *  else the default) and its branches' functions. */
export interface NeonHost {
  branches(workspace: string): Promise<NeonBranches>;
  /** Make the branch the current environment: the views reach its functions. */
  use(workspace: string, branch: string): Promise<void>;
}

/** The workspaces in trestle's shared state (what `trestle ws list` shows). */
export interface WorkspacesHost {
  /** Where the state lives, to show (`…/trestle/state.db`). */
  location(): Promise<string>;
  list(): Promise<WorkspaceInfo[]>;
  /** A new workspace, with one environment on the baseline selection. */
  create(name: string, firstEnv: string): Promise<WorkspaceInfo>;
  /** The editor's store for workspace `id`. */
  store(id: string): WorkspaceStore;
  /** The workspace's environments with their head revisions (store ids), for export. */
  environments(id: string): Promise<{ id: string; name: string; head: number | null }[]>;
  /** Ask for a directory with the native picker; `null` when cancelled. */
  pickDirectory(title: string): Promise<string | null>;
  /**
   * Import a directory: a workspace directory (`trestle-workspace.toml`) as a new
   * workspace, an environment directory (`env.toml`) as a new environment of `workspace`.
   */
  import(dir: string, workspace: string | null): Promise<Imported>;
  /** Export the head revisions to `<parent>/<workspace>`; resolves to that path. */
  exportWorkspace(id: string, parent: string): Promise<string>;
  /** Export a revision to `<parent>/<environment>-r<number>`; resolves to that path. */
  exportRevision(environment: string, revision: number, parent: string): Promise<string>;
  /** Any change to the state, from the app or the CLI. Returns an unsubscribe. */
  subscribe(onChange: () => void): () => void;
}

export interface ShellHost {
  /** Probe Docker, Compose, the daemon and the Neon CLI. */
  doctor(): Promise<DoctorReport>;
  /** The environment editor's planner. */
  planner: Planner;
  workspaces: WorkspacesHost;
  run: RunHost;
  neon: NeonHost;
}

const ShellHostContext = createContext<ShellHost | null>(null);

export function ShellHostProvider({ host, children }: { host: ShellHost; children: ReactNode }) {
  return <ShellHostContext.Provider value={host}>{children}</ShellHostContext.Provider>;
}

export function useShellHost(): ShellHost {
  const host = useContext(ShellHostContext);
  if (!host) throw new Error("useShellHost must be used within a <ShellHostProvider>");
  return host;
}

/** Whether a local environment could run (the Neon CLI is optional). */
export function isReady(report: DoctorReport): boolean {
  return (
    report.docker.status === "found" &&
    report.compose.status === "found" &&
    report.daemon.status === "up"
  );
}
