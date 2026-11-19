// The Apps view: the workspace's running instances, and the apps each serves
// (Catalog, Query and Lineage on its own services, and its UIs with a session of their
// own: MLflow, Jaeger, …), as a workbench view next to the editor. The editor is
// where an environment is configured and started. Its apps are a separate
// concern, so they don't live in its tabs. All the view needs of a running
// instance comes from the host, not the editor: the instances, their bindings
// (`run.use` makes one current, `EnvironmentData`) and their UIs
// (`run.serviceUis`).
//
// The selector picks one instance, which the data views make the current
// environment. `LakehouseHost` has one current environment, so apps of two
// instances side by side would need bindings per view.
//
// Above the instances are the desktop's own apps, which no instance serves: the
// Factory, on the shell's `factory` binding (in-process, or an external factory-server
// bound by hand). It opens its sessions on the selected instance's Omnigent.

import {
  type Command,
  EDITOR_VIEW,
  RunStateDot,
  useWorkbench,
  type WorkbenchViewContribution,
} from "@open-lakehouse/env-editor";
import { useEnvironmentKey, useHost, useService } from "@open-lakehouse/host/react";
import { Button } from "@open-lakehouse/ui-kit";
import { useQueries } from "@tanstack/react-query";
import {
  AppWindow,
  Bot,
  Database,
  Factory,
  LayoutGrid,
  SquareTerminal,
  Workflow,
} from "lucide-react";
import { lazy, type ReactNode, Suspense, useCallback, useState } from "react";
import type { SessionTarget } from "../integrations/factory/FactoryView";
import { UC_APPS } from "../integrations/mcp/connection";
import { FACTORY, HEADWATERS, OMNIGENT, UC_MCP, UNITY_CATALOG } from "../integrations/services";
import { EmbeddedServiceUi } from "./EmbeddedServiceUi";
import { EnvironmentData } from "./EnvironmentData";
import { useRunQuery } from "./environments";
import { editorState, placementLabel, type RunState, runState, servesModule } from "./run-state";
import {
  type EnvironmentRun,
  type InstanceStatus,
  type ServiceUi,
  useShellHost,
  type WorkspaceRun,
} from "./shell-host";
import { Notice } from "./WorkspaceBar";

// The service UIs load on first use: they carry most of the bundle.
const CatalogView = lazy(() =>
  import("../integrations/unity-catalog/CatalogView").then((m) => ({ default: m.CatalogView })),
);
const LineageView = lazy(() =>
  import("../integrations/lineage/LineageView").then((m) => ({ default: m.LineageView })),
);
const QueryView = lazy(() =>
  import("../integrations/query/QueryView").then((m) => ({ default: m.QueryView })),
);
// With the MCP SDK and the chat's components: none of it loads before the app opens.
const AgentView = lazy(() =>
  import("../integrations/agent/lakehouse").then((m) => ({ default: m.LakehouseAgentView })),
);

// With the control room's canvas and Monaco.
const FactoryView = lazy(() =>
  import("../integrations/factory/FactoryView").then((m) => ({ default: m.FactoryView })),
);

/** The desktop's own apps, which no instance serves. */
export type DesktopApp = "factory";

/** Where the Factory's "Open session" opens: `selected`, if it is up with Omnigent, with
 *  its MCP servers with Apps. */
export function sessionTarget(selected: RunningInstance | undefined): SessionTarget | undefined {
  if (!selected || !usable(selected.state) || !servesModule(selected.instance, OMNIGENT)) {
    return undefined;
  }
  return {
    instance: selected.instance.id,
    label: selected.label,
    servers: servesModule(selected.instance, UC_MCP) ? [UC_APPS] : [],
  };
}

/** The Apps view's id among the workbench's views. */
export const APPS_VIEW = "apps";

/** One app of a running instance. */
export type AppEntry =
  | {
      kind: "data";
      id: "catalog" | "query" | "lineage" | "agent";
      label: string;
      /** The services the instance must serve. The view's clients bind the first, which
       *  `service` names in its notices. */
      modules: readonly [string, ...string[]];
      service: string;
    }
  | { kind: "ui"; id: string; label: string; ui: ServiceUi };

/** An instance wanted running, with its environment and what it is doing. */
export interface RunningInstance {
  env: EnvironmentRun;
  instance: InstanceStatus;
  state: RunState;
  /** `dev #3` */
  label: string;
}

/** Whether an instance's apps can be used: up, if not every service is healthy. */
export const usable = (state: RunState) => state === "running" || state === "unhealthy";

/** The instances wanted running, by environment name, newest revision first. */
export function runningInstances(run: WorkspaceRun | undefined): RunningInstance[] {
  return [...(run?.environments ?? [])]
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((env) =>
      env.instances
        .filter((i) => i.desired === "running")
        .sort((a, b) => b.revision - a.revision)
        .map((instance) => ({
          env,
          instance,
          state: runState(instance),
          label: `${env.name} #${instance.revision}`,
        })),
    );
}

/** What `instance` offers: Catalog, Query, Lineage and Agent where it serves their
 *  modules, then its UIs with a session of their own. */
export function instanceApps(instance: InstanceStatus, uis: readonly ServiceUi[]): AppEntry[] {
  const data: AppEntry[] = [
    {
      kind: "data",
      id: "catalog",
      label: "Catalog",
      modules: [UNITY_CATALOG],
      service: "Unity Catalog",
    },
    // SQL in the host's engine, over the instance's Unity Catalog.
    {
      kind: "data",
      id: "query",
      label: "Query",
      modules: [UNITY_CATALOG],
      service: "Unity Catalog",
    },
    { kind: "data", id: "lineage", label: "Lineage", modules: [HEADWATERS], service: "Lineage" },
    // The agent on Omnigent, and its MCP Apps over the desktop's own uc-mcp connection.
    {
      kind: "data",
      id: "agent",
      label: "Agent",
      modules: [UC_MCP, OMNIGENT],
      service: "Unity Catalog MCP",
    },
  ];
  return [
    ...data.filter((a) => a.kind === "data" && a.modules.every((m) => servesModule(instance, m))),
    ...uis.map((ui): AppEntry => ({ kind: "ui", id: `ui-${ui.moduleId}`, label: ui.name, ui })),
  ];
}

/** The instance the view shows: the one picked, else the current environment's, else
 *  the first one up, else the first one at all. */
export function selectedInstance(
  running: readonly RunningInstance[],
  picked: string | undefined,
  currentId: string | undefined,
): RunningInstance | undefined {
  return (
    running.find((r) => r.instance.id === picked) ??
    running.find((r) => r.instance.id === currentId) ??
    running.find((r) => usable(r.state)) ??
    running[0]
  );
}

const APP_ICONS: Record<string, ReactNode> = {
  catalog: <Database />,
  query: <SquareTerminal />,
  lineage: <Workflow />,
  agent: <Bot />,
};
const appIcon = (app: AppEntry) => APP_ICONS[app.id] ?? <AppWindow />;

export interface AppsView {
  view: WorkbenchViewContribution;
  /** Show the Apps view, on `instance` (and its app `app`) if given. */
  open: (instance?: string, app?: string) => void;
  /** Show the Apps view on one of the desktop's own apps. */
  openDesktop: (app: DesktopApp) => void;
  /** ⌘K: the desktop's apps, and one row per app of each instance that is up. */
  commands: Command[];
}

/** The Apps workbench view for workspace `workspaceId`. Needs a `WorkbenchProvider`
 *  above it. */
export function useAppsView(workspaceId: string): AppsView {
  const shell = useShellHost();
  const host = useHost();
  const workbench = useWorkbench();
  // The environments keep it live (Docker's events, commits).
  const run = useRunQuery(workspaceId);
  useEnvironmentKey(); // re-render when the current environment changes
  const currentId = host.environment.current()?.id;
  const [picked, setPicked] = useState<{ instance?: string; app?: string; desktop?: DesktopApp }>(
    {},
  );
  const factory = useService(FACTORY) !== undefined;
  const desktopApps: DesktopApp[] = factory ? ["factory"] : [];
  const desktop =
    picked.desktop && desktopApps.includes(picked.desktop) ? picked.desktop : undefined;

  const running = runningInstances(run.data);
  // The UIs of each instance that is up (the same queries the environments make);
  // a revision's plan doesn't change, nor does its instance's slot.
  const up = running.filter((r) => r.state === "running");
  const uis = useQueries({
    queries: up.map((r) => ({
      queryKey: ["service-uis", r.instance.id],
      queryFn: () => shell.run.serviceUis(r.instance.id),
      staleTime: Number.POSITIVE_INFINITY,
    })),
  });
  const uisOf = new Map(up.map((r, n) => [r.instance.id, uis[n]?.data ?? []] as const));
  const appsOf = (r: RunningInstance) =>
    usable(r.state) ? instanceApps(r.instance, uisOf.get(r.instance.id) ?? []) : [];

  const selected = selectedInstance(running, picked.instance, currentId);
  const apps = selected ? appsOf(selected) : [];
  // The app picked carries over to another instance that has it.
  const app = apps.find((a) => a.id === picked.app) ?? apps[0];

  const { open: openView } = workbench;
  const open = useCallback(
    (instance?: string, appId?: string) => {
      if (instance) setPicked((p) => ({ instance, app: appId ?? p.app }));
      openView(APPS_VIEW);
    },
    [openView],
  );
  const openDesktop = useCallback(
    (app: DesktopApp) => {
      setPicked((p) => ({ ...p, desktop: app }));
      openView(APPS_VIEW);
    },
    [openView],
  );

  const commands: Command[] = [
    ...desktopApps.map((a) => ({
      id: `apps.desktop.${a}`,
      title: `Apps: ${DESKTOP_LABELS[a]}`,
      section: "Apps",
      keywords: [DESKTOP_LABELS[a], "app", "plan", "control room"],
      run: () => openDesktop(a),
    })),
    ...running.flatMap((r) =>
      appsOf(r).map((a) => ({
        id: `apps.${r.instance.id}.${a.id}`,
        title: `Apps: ${a.label} on ${r.label}`,
        section: "Apps",
        keywords: [r.env.name, a.label, "app"],
        run: () => open(r.instance.id, a.id),
      })),
    ),
  ];

  const view: WorkbenchViewContribution = {
    id: APPS_VIEW,
    label: "Apps",
    icon: <LayoutGrid />,
    sidebar: () => (
      <AppsSidebar
        desktopApps={desktopApps}
        desktop={desktop}
        running={running}
        selected={selected}
        apps={apps}
        app={desktop ? undefined : app}
        onDesktop={(d) => setPicked((p) => ({ ...p, desktop: d }))}
        onInstance={(instance) => setPicked((p) => ({ ...p, instance, desktop: undefined }))}
        onApp={(appId) =>
          setPicked((p) => ({ instance: selected?.instance.id ?? p.instance, app: appId }))
        }
      />
    ),
    render: () =>
      desktop ? (
        <div className="h-full" data-testid={`apps-main-${desktop}`}>
          <Suspense fallback={null}>
            <FactoryView sessions={sessionTarget(selected)} />
          </Suspense>
        </div>
      ) : (
        <AppsMain
          running={running}
          selected={selected}
          app={app}
          loading={run.isPending}
          onEditor={() => openView(EDITOR_VIEW)}
          onFactory={factory ? () => openDesktop("factory") : undefined}
        />
      ),
  };
  return { view, open, openDesktop, commands };
}

const DESKTOP_LABELS: Record<DesktopApp, string> = { factory: "Factory" };
const DESKTOP_ICONS: Record<DesktopApp, ReactNode> = { factory: <Factory /> };

const sectionTitle =
  "flex h-9 shrink-0 items-center px-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground";
const appButton = (current: boolean) =>
  `flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-sidebar-accent [&_svg]:h-3.5 [&_svg]:w-3.5 [&_svg]:shrink-0 ${
    current ? "bg-sidebar-accent font-medium" : ""
  }`;

function AppsSidebar({
  desktopApps,
  desktop,
  running,
  selected,
  apps,
  app,
  onDesktop,
  onInstance,
  onApp,
}: {
  desktopApps: readonly DesktopApp[];
  desktop: DesktopApp | undefined;
  onDesktop: (app: DesktopApp) => void;
  running: readonly RunningInstance[];
  selected: RunningInstance | undefined;
  apps: readonly AppEntry[];
  app: AppEntry | undefined;
  onInstance: (instance: string) => void;
  onApp: (app: string) => void;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="apps-sidebar">
      {desktopApps.length > 0 && (
        <>
          <div className={sectionTitle}>Desktop</div>
          <ul aria-label="Desktop apps" className="flex flex-col gap-0.5 px-1 pb-2">
            {desktopApps.map((a) => (
              <li key={a}>
                <button
                  type="button"
                  aria-current={a === desktop ? "page" : undefined}
                  data-testid={`apps-desktop-${a}`}
                  onClick={() => onDesktop(a)}
                  className={appButton(a === desktop)}
                >
                  {DESKTOP_ICONS[a]}
                  <span className="truncate">{DESKTOP_LABELS[a]}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      <div className={sectionTitle}>Running</div>
      <div className="min-h-0 flex-1 overflow-auto px-1 pb-2">
        {running.length === 0 ? (
          <p className="px-2 text-xs text-muted-foreground">Nothing is running.</p>
        ) : (
          <ul aria-label="Running instances" className="flex flex-col gap-0.5">
            {running.map((r) => {
              const isSelected = r.instance.id === selected?.instance.id;
              const placement = placementLabel(r.instance);
              return (
                <li key={r.instance.id}>
                  <button
                    type="button"
                    aria-current={isSelected ? "true" : undefined}
                    data-testid={`apps-instance-${r.instance.id}`}
                    title={r.instance.gatewayUrl ?? undefined}
                    onClick={() => onInstance(r.instance.id)}
                    className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm hover:bg-sidebar-accent ${
                      isSelected ? "bg-sidebar-accent font-medium" : ""
                    }`}
                  >
                    <RunStateDot status={{ state: editorState(r.state), label: r.state }} />
                    <span className="truncate">{r.label}</span>
                    {placement && (
                      <span className="ml-auto truncate text-xs text-muted-foreground">
                        {placement}
                      </span>
                    )}
                  </button>
                  {isSelected && apps.length > 0 && (
                    <ul aria-label={`Apps of ${r.label}`} className="ml-4 mt-0.5 flex flex-col">
                      {apps.map((a) => (
                        <li key={a.id}>
                          <button
                            type="button"
                            aria-current={a.id === app?.id ? "page" : undefined}
                            onClick={() => onApp(a.id)}
                            className={appButton(a.id === app?.id)}
                          >
                            {appIcon(a)}
                            <span className="truncate">{a.label}</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

function AppsMain({
  running,
  selected,
  app,
  loading,
  onEditor,
  onFactory,
}: {
  running: readonly RunningInstance[];
  selected: RunningInstance | undefined;
  app: AppEntry | undefined;
  loading: boolean;
  onEditor: () => void;
  /** The factory is bound: the empty view offers it. */
  onFactory?: () => void;
}) {
  if (loading) return <Notice>Loading…</Notice>;
  if (running.length === 0 || !selected) {
    return (
      <div
        className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
        data-testid="apps-empty"
      >
        <LayoutGrid className="size-8 text-muted-foreground" />
        <h2 className="text-lg font-semibold">Nothing is running</h2>
        <p className="max-w-md text-sm text-muted-foreground">
          Start an environment in the editor; its apps show here.
        </p>
        <div className="flex gap-2">
          <Button size="sm" variant="outline" onClick={onEditor}>
            Show the environments
          </Button>
          {onFactory && (
            <Button size="sm" variant="outline" onClick={onFactory}>
              <Factory /> Open the factory
            </Button>
          )}
        </div>
      </div>
    );
  }
  if (!usable(selected.state)) {
    return (
      <Notice>
        {selected.label} is {selected.state === "starting" ? "starting" : selected.state}…
      </Notice>
    );
  }
  if (!app) return <Notice>{selected.label} serves no apps.</Notice>;
  const id = selected.instance.id;
  return (
    <div className="h-full" data-testid={`apps-main-${app.id}`}>
      {app.kind === "data" ? (
        <EnvironmentData
          key={`${id}/${app.id}`}
          instance={id}
          label={selected.label}
          moduleId={app.modules[0]}
          name={app.service}
        >
          <Suspense fallback={null}>
            {app.id === "catalog" ? (
              <CatalogView />
            ) : app.id === "query" ? (
              <QueryView instance={id} label={selected.label} />
            ) : app.id === "agent" ? (
              <AgentView instance={id} label={selected.label} />
            ) : (
              <LineageView />
            )}
          </Suspense>
        </EnvironmentData>
      ) : (
        <EmbeddedServiceUi key={`${id}/${app.ui.moduleId}`} instance={id} ui={app.ui} />
      )}
    </div>
  );
}
