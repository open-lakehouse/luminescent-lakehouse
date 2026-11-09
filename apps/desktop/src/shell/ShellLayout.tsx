// The window: env-editor's workspace shell on the current workspace in trestle's
// shared state, as the one frame. The desktop contributes to it: a gear at the
// bottom of the activity bar that opens its settings over the workspace
// (settings.tsx), and for each environment its run state, Neon target, Start and
// Stop, and the sub-tabs on its own services (Catalog, Lineage, Logs;
// environments.tsx). The sidebar keeps the editor's own views.

import {
  type Command,
  ContributionsProvider,
  type EditorContributions,
  PlannerProvider,
  WorkspaceProvider,
  WorkspaceShell,
} from "@open-lakehouse/env-editor";
import { useTheme } from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { Settings } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { useEnvironmentContributions } from "./environments";
import { SettingsDialog, type SettingsSection } from "./settings";
import { useShellHost } from "./shell-host";
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
  const [settings, setSettings] = useState<SettingsSection | null>(null);
  const { environment, panel, dialog, commands } = useEnvironmentContributions(
    workspaceId,
    workspaceName,
  );
  const { theme, setTheme } = useTheme();
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia?.("(prefers-color-scheme: dark)").matches);
  const app: Command[] = [
    {
      id: "app.doctor",
      title: "Doctor",
      section: "App",
      keywords: ["docker", "neon", "toolchain", "path"],
      run: () => setSettings("doctor"),
    },
    {
      id: "app.neon",
      title: "Neon branches…",
      section: "App",
      keywords: ["reset", "delete", "branch"],
      run: () => setSettings("neon"),
    },
    {
      id: "app.workspaces",
      title: "Workspaces: switch, import or export…",
      section: "App",
      keywords: ["import", "export", "new"],
      run: () => setSettings("workspaces"),
    },
    {
      id: "app.theme",
      title: dark ? "Light theme" : "Dark theme",
      section: "App",
      keywords: ["theme", "appearance"],
      run: () => setTheme(dark ? "light" : "dark"),
    },
  ];
  const contributions: EditorContributions = {
    commands: [...commands, ...app],
    activityActions: [
      {
        id: "settings",
        label: "Settings",
        icon: <Settings />,
        align: "end",
        run: () => setSettings("general"),
      },
    ],
    environment,
    panel,
  };
  return (
    <ContributionsProvider contributions={contributions}>
      <div className="h-full" data-testid="workspace-view">
        <WorkspaceShell
          colorMode={colorMode}
          statusItems={
            <button
              type="button"
              className="text-muted-foreground hover:text-foreground"
              title="Switch, create, import or export workspaces"
              onClick={() => setSettings("workspaces")}
              data-testid="status-workspace"
            >
              {workspaceName}
            </button>
          }
        />
        {dialog}
        {settings && (
          <SettingsDialog
            section={settings}
            onSection={setSettings}
            onClose={() => setSettings(null)}
          />
        )}
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
    toast.warning(`At launch: ${parts.join(", ")}.`, {
      description: "Details in Settings, under General.",
    });
  }, [report.data]);
}
