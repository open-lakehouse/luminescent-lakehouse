// The shell's own seam, next to `@open-lakehouse/host`'s: what the shell UI
// needs from the desktop beyond what service UIs get. The desktop provides it
// from src/host; tests provide a fake.

import type { Planner, WorkspaceStore } from "@open-lakehouse/env-editor";
import { createContext, type ReactNode, useContext } from "react";
import type { DoctorReport, WorkspaceInfo } from "../host/bindings";

export type { Daemon, DaemonFlavour, DoctorReport, Tool, WorkspaceInfo } from "../host/bindings";

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
