// The window: env-editor's workspace shell on the current workspace in trestle's
// shared state, as the one frame. The desktop contributes to it: its own views
// in the activity bar (workspaces, Neon, doctor, app settings), and for each
// environment its run state, Start and Stop, and the sub-tabs on its own
// services (Catalog, Lineage, Logs). See environments.tsx and side-views.tsx.

import {
  ContributionsProvider,
  type EditorContributions,
  PlannerProvider,
  WorkspaceProvider,
  WorkspaceShell,
} from "@open-lakehouse/env-editor";
import { useTheme } from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef } from "react";
import { toast } from "sonner";
import { useEnvironmentContributions } from "./environments";
import { useShellHost } from "./shell-host";
import { useSideViews } from "./side-views";
import { CurrentWorkspaceProvider, Notice, useCurrentWorkspace } from "./WorkspaceBar";

export function ShellLayout() {
  return (
    <div className="h-full" data-testid="shell">
      <CurrentWorkspaceProvider>
        <WorkspaceFrame />
      </CurrentWorkspaceProvider>
    </div>
  );
}

function WorkspaceFrame() {
  const shell = useShellHost();
  const { theme } = useTheme();
  const workspace = useCurrentWorkspace();
  const currentId = workspace.current?.id;
  const store = useMemo(
    () => (currentId ? shell.workspaces.store(currentId) : null),
    [shell, currentId],
  );
  useReconcileNotice();

  if (workspace.problem) return <Notice tone="error">{workspace.problem}</Notice>;
  if (!store || !currentId) return <Notice>Loading workspaces…</Notice>;
  return (
    <PlannerProvider planner={shell.planner}>
      <WorkspaceProvider key={currentId} store={store}>
        <DesktopShell
          workspaceId={currentId}
          workspaceName={workspace.current?.name ?? ""}
          colorMode={theme}
        />
      </WorkspaceProvider>
    </PlannerProvider>
  );
}

function DesktopShell({
  workspaceId,
  workspaceName,
  colorMode,
}: {
  workspaceId: string;
  workspaceName: string;
  colorMode: "light" | "dark" | "system";
}) {
  const sideViews = useSideViews();
  const { environment, panel, dialog } = useEnvironmentContributions(workspaceId);
  const contributions: EditorContributions = { sideViews, environment, panel };
  return (
    <ContributionsProvider contributions={contributions}>
      <div className="h-full" data-testid="workspace-view">
        <WorkspaceShell
          colorMode={colorMode}
          statusItems={
            <span className="text-muted-foreground" title="The workspace (Workspaces view)">
              {workspaceName}
            </span>
          }
        />
        {dialog}
      </div>
    </ContributionsProvider>
  );
}

/** Once per launch, when the reconcile found instances gone or projects nobody knows:
 *  a notice pointing at App settings, which has the details. */
function useReconcileNotice() {
  const shell = useShellHost();
  const report = useQuery({ queryKey: ["reconciled"], queryFn: () => shell.run.reconciled() });
  const shown = useRef(false);
  useEffect(() => {
    const r = report.data;
    if (shown.current || !r || (r.stale.length === 0 && r.orphans.length === 0)) return;
    shown.current = true;
    const parts = [
      r.stale.length > 0 && `${r.stale.length} instance(s) not running any more`,
      r.orphans.length > 0 && `${r.orphans.length} unknown compose project(s)`,
    ].filter(Boolean);
    toast.warning(`At launch: ${parts.join(", ")}.`, { description: "Details in App settings." });
  }, [report.data]);
}
