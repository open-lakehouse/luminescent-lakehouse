// The workspace editor: env-editor's `WorkspaceShell` on trestle's shared state
// (the store the `trestle` CLI uses) and the native planner, for the current
// workspace (see WorkspaceBar).

import { PlannerProvider, WorkspaceProvider, WorkspaceShell } from "@open-lakehouse/env-editor";
import { useTheme } from "@open-lakehouse/ui-kit";
import { useMemo } from "react";
import { useShellHost } from "./shell-host";
import { Notice, useCurrentWorkspace, WorkspaceBar } from "./WorkspaceBar";

export function WorkspaceView() {
  const shell = useShellHost();
  const { theme } = useTheme();
  const workspace = useCurrentWorkspace();
  const currentId = workspace.current?.id;
  const store = useMemo(
    () => (currentId ? shell.workspaces.store(currentId) : null),
    [shell, currentId],
  );

  let body: React.ReactNode;
  if (workspace.problem) {
    body = <Notice tone="error">{workspace.problem}</Notice>;
  } else if (!store || !currentId) {
    body = <Notice>Loading workspaces…</Notice>;
  } else {
    body = (
      <PlannerProvider planner={shell.planner}>
        <WorkspaceProvider key={currentId} store={store}>
          <WorkspaceShell colorMode={theme} />
        </WorkspaceProvider>
      </PlannerProvider>
    );
  }

  return (
    <div className="flex h-full flex-col" data-testid="workspace-view">
      <WorkspaceBar workspace={workspace} />
      <div className="min-h-0 flex-1">{body}</div>
    </div>
  );
}
