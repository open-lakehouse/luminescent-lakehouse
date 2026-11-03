// The workspace editor: env-editor's `WorkspaceShell`, on an in-memory store
// for now (#17 backs it with trestle's shared state store, so the app and the
// CLI see the same workspaces) and the fixture planner (#17 swaps in the
// native planner behind IPC).

import {
  createMemoryWorkspaceStore,
  PlannerProvider,
  WorkspaceProvider,
  WorkspaceShell,
  type WorkspaceStore,
} from "@open-lakehouse/env-editor";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { useTheme } from "@open-lakehouse/ui-kit";
import { type ReactNode, useMemo } from "react";

export interface WorkspaceViewProps {
  /** Defaults to a fresh in-memory store that keeps revisions. */
  store?: WorkspaceStore;
  statusItems?: ReactNode;
}

export function WorkspaceView({ store, statusItems }: WorkspaceViewProps) {
  const { theme } = useTheme();
  const planner = useMemo(() => createWasmPlanner(), []);
  const workspaceStore = useMemo(
    () => store ?? createMemoryWorkspaceStore(null, { revisions: true }),
    [store],
  );
  return (
    <div className="h-full" data-testid="workspace-view">
      <PlannerProvider planner={planner}>
        <WorkspaceProvider store={workspaceStore}>
          <WorkspaceShell colorMode={theme} statusItems={statusItems} />
        </WorkspaceProvider>
      </PlannerProvider>
    </div>
  );
}
