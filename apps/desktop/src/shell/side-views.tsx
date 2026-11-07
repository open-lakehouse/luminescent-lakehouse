// The desktop's views in the editor's activity bar, after the editor's own
// (Environments, History, Modules, Workspace settings): the workspaces in
// trestle's shared state, the Neon project's branches, the doctor, and the
// app's own settings at the bottom.

import type { SideViewContribution } from "@open-lakehouse/env-editor";
import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import {
  Badge,
  Button,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  useTheme,
} from "@open-lakehouse/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Boxes, Cloud, Loader2, Moon, SlidersHorizontal, Stethoscope, Sun, X } from "lucide-react";
import { useEffect, useState } from "react";
import { DoctorView } from "./DoctorView";
import { Failure } from "./RunPanel";
import { type ProjectSource, useShellHost } from "./shell-host";
import { Transfer } from "./Transfer";
import {
  type CurrentWorkspace,
  message,
  NewWorkspace,
  useCurrentWorkspace,
  WORKSPACES_QUERY_KEY,
} from "./WorkspaceBar";

export function useSideViews(): SideViewContribution[] {
  return [
    { id: "workspaces", label: "Workspaces", icon: <Boxes />, render: () => <WorkspacesPanel /> },
    { id: "neon", label: "Neon", icon: <Cloud />, render: () => <NeonPanel /> },
    { id: "doctor", label: "Doctor", icon: <Stethoscope />, render: () => <DoctorPanel /> },
    {
      id: "app-settings",
      label: "App settings",
      icon: <SlidersHorizontal />,
      align: "end",
      render: () => <AppSettingsPanel />,
    },
  ];
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3 border-b p-3 text-sm last:border-b-0">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** Which workspace the editor shows, new ones, import and export, and where the
 *  shared state lives. */
export function WorkspacesPanel() {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const workspace: CurrentWorkspace = useCurrentWorkspace();
  const location = useQuery({
    queryKey: ["state-location"],
    queryFn: () => shell.workspaces.location(),
  });
  const { current, workspaces, select } = workspace;
  const reload = () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY });
  return (
    <div data-testid="workspaces-view">
      <Section title="Workspace">
        <div className="flex items-center gap-1">
          <Select value={current?.id ?? ""} onValueChange={select} disabled={!workspaces?.length}>
            <SelectTrigger
              className="h-7 flex-1"
              aria-label="Workspace"
              data-testid="workspace-picker"
            >
              <SelectValue placeholder="…" />
            </SelectTrigger>
            <SelectContent>
              {workspaces?.map((ws) => (
                <SelectItem key={ws.id} value={ws.id}>
                  {ws.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <NewWorkspace
            onCreated={(ws) => {
              select(ws.id);
              reload();
            }}
          />
        </div>
        <div className="flex flex-wrap items-center gap-1">
          <Transfer
            workspace={current}
            onImported={(id) => {
              select(id);
              reload();
            }}
          />
        </div>
      </Section>
      {location.data && (
        <Section title="Shared state">
          <p
            className="break-all font-mono text-xs text-muted-foreground"
            title="Shared with the trestle CLI"
            data-testid="state-location"
          >
            {location.data}
          </p>
          <p className="text-xs text-muted-foreground">
            The same store `trestle ws list` reads: what the app writes, the CLI sees, and the other
            way round.
          </p>
        </Section>
      )}
    </div>
  );
}

export function DoctorPanel() {
  return (
    <div className="h-full overflow-auto" data-testid="doctor-panel">
      <DoctorView />
    </div>
  );
}

/** Where the listed project came from, and how to change it. */
function projectHint(source: ProjectSource, workspace: string): string {
  switch (source) {
    case "workspace":
      return "the workspace's project";
    case "env":
      return "from LUMINESCENT_NEON_PROJECT";
    case "default":
      return `the default; set the workspace's with \`trestle ws set-neon ${workspace} --project <id>\``;
  }
}

/** The workspace's Neon branches. Use makes one the current environment: the views
 *  reach its deployed functions (Unity Catalog, headwaters). */
export function NeonPanel() {
  const shell = useShellHost();
  const workspace = useCurrentWorkspace().current;
  const workspaceId = workspace?.id ?? "";
  const branches = useQuery({
    queryKey: ["neon", workspaceId],
    queryFn: () => shell.neon.branches(workspaceId),
    enabled: !!workspaceId,
    staleTime: 60_000,
  });
  const use = useMutation({
    mutationFn: (branch: string) => shell.neon.use(workspaceId, branch),
  });
  useEnvironmentKey(); // re-render when it changes
  const currentId = useHost().environment.current()?.id;

  return (
    <div data-testid="neon-card">
      <Section title="Neon project">
        {branches.data && (
          <p className="text-xs text-muted-foreground">
            <span className="font-mono">{branches.data.projectId}</span> (
            {projectHint(branches.data.source, workspace?.name ?? "")})
          </p>
        )}
        {branches.isPending && <p className="text-xs text-muted-foreground">Loading branches…</p>}
        {branches.isError && <Failure error={branches.error} />}
        {branches.data && branches.data.branches.length === 0 && (
          <p className="text-xs text-muted-foreground">The project has no branches.</p>
        )}
        {branches.data?.branches.map((b) => {
          const isCurrent = b.id === currentId;
          const pending = use.isPending && use.variables === b.name;
          return (
            <div key={b.id} className="flex items-center gap-2" data-testid={`neon-${b.name}`}>
              <span className="font-mono text-sm">{b.name}</span>
              {b.default && <Badge variant="outline">default</Badge>}
              {isCurrent && (
                <Badge
                  variant="primary"
                  title="The views use its functions"
                  data-testid={`neon-${b.name}-current`}
                >
                  current
                </Badge>
              )}
              <div className="flex-1" />
              {!isCurrent && (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={use.isPending}
                  onClick={() => use.mutate(b.name)}
                  title="Use its functions in the views"
                  data-testid={`neon-${b.name}-use`}
                >
                  {pending && <Loader2 className="size-4 animate-spin" />}
                  Use
                </Button>
              )}
            </div>
          );
        })}
        {use.error && <Failure error={use.error} />}
      </Section>
    </div>
  );
}

/** Side by side or one at a time for this workspace, what quitting does, the theme,
 *  and what the launch's reconcile found. */
export function AppSettingsPanel() {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const workspace = useCurrentWorkspace().current;
  const settings = useQuery({ queryKey: ["settings"], queryFn: () => shell.run.settings() });
  const save = useMutation({
    mutationFn: (change: () => ReturnType<typeof shell.run.settings>) => change(),
    onSuccess: (next) => queryClient.setQueryData(["settings"], next),
  });
  const oneAtATime = !!workspace && !!settings.data?.oneAtATime.includes(workspace.id);
  return (
    <div data-testid="app-settings">
      <Section title="Running">
        {settings.data && (
          <div className="space-y-3" data-testid="run-settings">
            <div className="flex items-center gap-2">
              <Switch
                id="one-at-a-time"
                checked={oneAtATime}
                disabled={!workspace}
                onCheckedChange={(on) =>
                  workspace && save.mutate(() => shell.run.setOneAtATime(workspace.id, on))
                }
                data-testid="one-at-a-time"
              />
              <Label
                htmlFor="one-at-a-time"
                title="Start stops the workspace's other instances first"
              >
                One at a time{workspace ? ` (${workspace.name})` : ""}
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="stop-on-quit"
                checked={settings.data.quit === "stop"}
                onCheckedChange={(on) => save.mutate(() => shell.run.setQuit(on ? "stop" : "keep"))}
                data-testid="stop-on-quit"
              />
              <Label
                htmlFor="stop-on-quit"
                title="Stop the instances started from the app when it quits (they keep running otherwise)"
              >
                Stop on quit
              </Label>
            </div>
          </div>
        )}
        {save.error && (
          <p className="text-xs text-destructive" role="alert">
            {message(save.error)}
          </p>
        )}
      </Section>
      <Section title="Appearance">
        <ThemeToggle withLabel />
      </Section>
      <Reconciled />
    </div>
  );
}

export function ThemeToggle({ withLabel = false }: { withLabel?: boolean }) {
  const { theme, setTheme } = useTheme();
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  const label = dark ? "Light theme" : "Dark theme";
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={() => setTheme(dark ? "light" : "dark")}
      className="inline-flex items-center gap-2 rounded-md px-1 text-muted-foreground hover:text-foreground"
    >
      {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
      {withLabel && <span className="text-sm">{label}</span>}
    </button>
  );
}

const RECONCILED_DISMISSED = "luminescent.reconciled-dismissed";

/** What the launch found: instances that were gone, and projects nobody knows. Shown
 *  until dismissed, once per launch. */
export function Reconciled() {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const [dismissed, setDismissed] = useState(
    () => sessionStorage.getItem(RECONCILED_DISMISSED) === "1",
  );
  const report = useQuery({ queryKey: ["reconciled"], queryFn: () => shell.run.reconciled() });
  useEffect(
    () =>
      shell.run.onReconciled(
        () => void queryClient.invalidateQueries({ queryKey: ["reconciled"] }),
      ),
    [shell, queryClient],
  );
  const r = report.data;
  if (dismissed || !r || (r.stale.length === 0 && r.orphans.length === 0)) return null;
  return (
    <Section title="At launch">
      <div className="flex items-start gap-2" data-testid="reconciled">
        <div className="flex-1 space-y-1 text-xs">
          {r.stale.length > 0 && (
            <p>
              Not running any more, so marked stopped:{" "}
              <span className="font-mono">{r.stale.map((i) => i.label).join(", ")}</span>.
            </p>
          )}
          {r.orphans.length > 0 && (
            <p>
              Running in Docker but unknown to the shared state (left alone; `docker compose -p
              &lt;project&gt; down` removes one):{" "}
              <span className="font-mono">{r.orphans.join(", ")}</span>.
            </p>
          )}
          {r.adopted.length > 0 && (
            <p className="text-muted-foreground">
              Still running:{" "}
              <span className="font-mono">{r.adopted.map((i) => i.label).join(", ")}</span>.
            </p>
          )}
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-1"
          aria-label="Dismiss"
          onClick={() => {
            sessionStorage.setItem(RECONCILED_DISMISSED, "1");
            setDismissed(true);
          }}
        >
          <X className="size-3" />
        </Button>
      </div>
    </Section>
  );
}
