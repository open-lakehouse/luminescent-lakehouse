// The shell's own seam, next to `@open-lakehouse/host`'s: what the shell UI
// needs from the desktop beyond what service UIs get. The desktop provides it
// from src/host; tests provide a fake.

import { createContext, type ReactNode, useContext } from "react";
import type { DoctorReport } from "../host/bindings";

export type { Daemon, DaemonFlavour, DoctorReport, Tool } from "../host/bindings";

export interface ShellHost {
  /** Probe Docker, Compose, the daemon and the Neon CLI. */
  doctor(): Promise<DoctorReport>;
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
