// The shell's own seam, next to `@open-lakehouse/host`'s: what the shell UI
// needs from the desktop beyond what service UIs get. The desktop provides it
// from src/host; tests provide a fake.

import type { Planner, WorkspaceStore } from "@open-lakehouse/env-editor";
import { createContext, type ReactNode, useContext } from "react";
import type {
  DoctorReport,
  InstanceStatus_Serialize as InstanceStatus,
  RunError,
  WorkspaceInfo,
  WorkspaceRun_Serialize as WorkspaceRun,
} from "../host/bindings";

export type {
  Daemon,
  DaemonFlavour,
  DoctorReport,
  EnvironmentRun_Serialize as EnvironmentRun,
  InstanceStatus_Serialize as InstanceStatus,
  ServiceRow,
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
    case "other":
      return new RunFailure(error.message);
  }
}

/** Running environments: trestle's instances, shared with `trestle up` / `down`. */
export interface RunHost {
  workspace(id: string): Promise<WorkspaceRun>;
  /** Start the environment's head revision; resolves once it is healthy. */
  start(environment: string): Promise<InstanceStatus>;
  stop(environment: string): Promise<void>;
  /** Make a running instance the current environment: the views reach its services. */
  use(instance: string): Promise<void>;
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
