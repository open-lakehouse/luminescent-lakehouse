// Run: the current workspace's environments as running instances. Start runs an
// environment's head revision (`docker compose up`, waiting for health), Stop
// takes it down; the containers' state and health come from Docker every 2 s.
// The same instances `trestle up` / `trestle ps` / `trestle down` work on.
// Below them, the workspace's Neon branches: Use binds a branch's deployed
// functions instead, so the views show that branch's data.

import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { Badge, Button, Card, CardContent, cn } from "@open-lakehouse/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, Cloud, Copy, Loader2, Play, Square } from "lucide-react";
import { useEffect, useState } from "react";
import {
  type EnvironmentRun,
  type InstanceStatus,
  type ProjectSource,
  RunFailure,
  type ServiceRow,
  useShellHost,
} from "./shell-host";
import { message, Notice, useCurrentWorkspace, WorkspaceBar } from "./WorkspaceBar";

/** How often the containers are asked about while the view is open. */
export const RUN_REFRESH_MS = 2000;

export type RunState = "stopped" | "starting" | "running" | "unhealthy" | "stopping";

/** A one-shot job (certificates, a migration) that ran to completion. */
export function isDone(s: ServiceRow): boolean {
  return s.state === "exited" && s.exitCode === 0;
}

/** Exited with an error, died, or failing its healthcheck. */
export function isFailing(s: ServiceRow): boolean {
  return (
    s.health === "unhealthy" || s.state === "dead" || (s.state === "exited" && s.exitCode !== 0)
  );
}

/** What an instance is doing, from what is wanted and what Docker reports. */
export function runState(instance: InstanceStatus | undefined): RunState {
  if (!instance) return "stopped";
  const services = instance.services;
  if (instance.desired === "stopped") return services.length > 0 ? "stopping" : "stopped";
  if (services.some(isFailing)) return "unhealthy";
  const serving = services.filter((s) => !isDone(s));
  if (serving.length > 0 && serving.every((s) => s.ready)) return "running";
  return "starting";
}

const STATE_BADGE: Record<RunState, "default" | "primary" | "success" | "destructive" | "outline"> =
  {
    stopped: "outline",
    starting: "primary",
    running: "success",
    unhealthy: "destructive",
    stopping: "primary",
  };

/** The instance the environment's row is about: the one wanted running, else the
 *  newest. */
export function shownInstance(env: EnvironmentRun): InstanceStatus | undefined {
  return env.instances.find((i) => i.desired === "running") ?? env.instances[0];
}

export function RunView() {
  const shell = useShellHost();
  const workspace = useCurrentWorkspace();
  const currentId = workspace.current?.id;
  const queryClient = useQueryClient();
  const run = useQuery({
    queryKey: ["run", currentId],
    queryFn: () => shell.run.workspace(currentId as string),
    enabled: !!currentId,
    refetchInterval: RUN_REFRESH_MS,
  });
  // A commit (here or in the CLI) changes what Start runs: refresh at once.
  useEffect(
    () =>
      shell.workspaces.subscribe(() => void queryClient.invalidateQueries({ queryKey: ["run"] })),
    [shell, queryClient],
  );

  let body: React.ReactNode;
  if (workspace.problem) {
    body = <Notice tone="error">{workspace.problem}</Notice>;
  } else if (run.isError) {
    body = <Notice tone="error">{run.error.message}</Notice>;
  } else if (!run.data) {
    body = <Notice>Loading…</Notice>;
  } else {
    body = (
      <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
        {run.data.engineProblem && (
          <p
            className="rounded-md border border-destructive/40 p-3 text-sm text-destructive"
            role="alert"
          >
            Docker isn't available, so the containers are unknown: {run.data.engineProblem}
          </p>
        )}
        {run.data.environments.map((env) => (
          <EnvironmentCard key={env.id} env={env} workspaceId={currentId as string} />
        ))}
        <NeonCard workspaceId={currentId as string} workspace={workspace.current?.name ?? ""} />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col" data-testid="run-view">
      <WorkspaceBar workspace={workspace} />
      <div className="min-h-0 flex-1 overflow-auto">{body}</div>
    </div>
  );
}

function EnvironmentCard({ env, workspaceId }: { env: EnvironmentRun; workspaceId: string }) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["run", workspaceId] });
  const start = useMutation({
    mutationFn: () => shell.run.start(env.id),
    onSettled: refresh,
  });
  const stop = useMutation({
    mutationFn: () => shell.run.stop(env.id),
    onSettled: refresh,
  });
  const use = useMutation({ mutationFn: (instance: string) => shell.run.use(instance) });
  // The current environment is a running instance: its services are what the
  // Catalog and Lineage views reach.
  useEnvironmentKey(); // re-render when it changes
  const currentId = useHost().environment.current()?.id;
  const instance = shownInstance(env);
  const state: RunState = start.isPending
    ? "starting"
    : stop.isPending
      ? "stopping"
      : runState(instance);
  const busy = start.isPending || stop.isPending;
  const failure = start.error ?? stop.error ?? use.error;
  const wantedRunning = instance?.desired === "running";
  const isCurrent = !!instance && instance.id === currentId;

  return (
    <Card data-testid={`run-${env.name}`}>
      <CardContent className="space-y-3 p-4">
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="flex items-center gap-1 text-left"
            onClick={() => setOpen(!open)}
            aria-expanded={open}
            aria-label={`${env.name} containers`}
          >
            <ChevronRight className={cn("size-4 transition-transform", open && "rotate-90")} />
            <span className="font-medium">{env.name}</span>
          </button>
          <span className="text-xs text-muted-foreground">
            {env.head ? `revision #${instance?.revision ?? env.head}` : "no revision yet"}
          </span>
          <Badge variant={STATE_BADGE[state]} data-testid={`run-${env.name}-state`}>
            {state}
          </Badge>
          {isCurrent && (
            <Badge
              variant="primary"
              title="The views use its services"
              data-testid={`run-${env.name}-current`}
            >
              current
            </Badge>
          )}
          <div className="flex-1" />
          {instance?.gatewayUrl && wantedRunning && <GatewayUrl url={instance.gatewayUrl} />}
          {instance && wantedRunning && !isCurrent && !busy && (
            <Button
              size="sm"
              variant="ghost"
              disabled={use.isPending}
              onClick={() => use.mutate(instance.id)}
              title="Use its services in the views"
              data-testid={`run-${env.name}-use`}
            >
              Use
            </Button>
          )}
          {wantedRunning || stop.isPending ? (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => stop.mutate()}
              data-testid={`run-${env.name}-stop`}
            >
              {stop.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Square className="size-4" />
              )}
              Stop
            </Button>
          ) : (
            <Button
              size="sm"
              disabled={busy || !env.head}
              onClick={() => start.mutate()}
              title={env.head ? `Run revision #${env.head}` : "Commit a revision first"}
              data-testid={`run-${env.name}-start`}
            >
              {start.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Play className="size-4" />
              )}
              Start
            </Button>
          )}
        </div>
        {start.isPending && (
          <p className="text-xs text-muted-foreground">
            Starting revision #{env.head}: pulling images on the first run, then waiting until every
            container is healthy.
          </p>
        )}
        {failure && <Failure error={failure} />}
        {open && instance && <Services instance={instance} />}
        {open && !instance && <p className="text-xs text-muted-foreground">Not started yet.</p>}
      </CardContent>
    </Card>
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
function NeonCard({ workspaceId, workspace }: { workspaceId: string; workspace: string }) {
  const shell = useShellHost();
  const branches = useQuery({
    queryKey: ["neon", workspaceId],
    queryFn: () => shell.neon.branches(workspaceId),
    staleTime: 60_000,
  });
  const use = useMutation({
    mutationFn: (branch: string) => shell.neon.use(workspaceId, branch),
  });
  useEnvironmentKey(); // re-render when it changes
  const currentId = useHost().environment.current()?.id;

  return (
    <Card data-testid="neon-card">
      <CardContent className="space-y-3 p-4">
        <div className="flex items-center gap-2">
          <Cloud className="size-4" />
          <span className="font-medium">Neon</span>
          {branches.data && (
            <span className="text-xs text-muted-foreground">
              <span className="font-mono">{branches.data.projectId}</span> (
              {projectHint(branches.data.source, workspace)})
            </span>
          )}
        </div>
        {branches.isPending && <p className="text-xs text-muted-foreground">Loading branches…</p>}
        {branches.isError && <Failure error={branches.error} />}
        {branches.data && branches.data.branches.length === 0 && (
          <p className="text-xs text-muted-foreground">The project has no branches.</p>
        )}
        {branches.data?.branches.map((b) => {
          const isCurrent = b.id === currentId;
          const pending = use.isPending && use.variables === b.name;
          return (
            <div key={b.id} className="flex items-center gap-3" data-testid={`neon-${b.name}`}>
              <span className="text-sm">
                Neon: <span className="font-mono">{b.name}</span>
              </span>
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
      </CardContent>
    </Card>
  );
}

function GatewayUrl({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <span className="flex items-center gap-1 font-mono text-xs" data-testid="gateway-url">
      {url}
      <Button
        variant="ghost"
        size="sm"
        className="h-6 px-1"
        aria-label="Copy the gateway URL"
        onClick={() =>
          void navigator.clipboard.writeText(url).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          })
        }
      >
        <Copy className="size-3" />
      </Button>
      {copied && <span className="text-muted-foreground">copied</span>}
    </span>
  );
}

function Failure({ error }: { error: Error }) {
  const output = error instanceof RunFailure ? error.output : undefined;
  return (
    <div className="space-y-1 rounded-md border border-destructive/40 p-3" role="alert">
      <p className="text-sm text-destructive">{message(error)}</p>
      {output && (
        <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-xs text-muted-foreground">
          {output.trim()}
        </pre>
      )}
    </div>
  );
}

function Services({ instance }: { instance: InstanceStatus }) {
  if (instance.services.length === 0) {
    return <p className="text-xs text-muted-foreground">No containers ({instance.project}).</p>;
  }
  return (
    <table className="w-full text-xs" data-testid="run-services">
      <thead className="text-left text-muted-foreground">
        <tr>
          <th className="py-1 font-normal">Service</th>
          <th className="py-1 font-normal">State</th>
          <th className="py-1 font-normal">Health</th>
          <th className="py-1 font-normal">Ports</th>
        </tr>
      </thead>
      <tbody>
        {instance.services.map((s: ServiceRow) => (
          <tr key={s.service} className="border-t">
            <td className="py-1 font-mono">{s.service}</td>
            <td className="py-1">
              {s.state}
              {s.state === "exited" && ` (${s.exitCode === 0 ? "done" : `code ${s.exitCode}`})`}
            </td>
            <td className="py-1">
              <Badge
                variant={
                  s.health === "healthy"
                    ? "success"
                    : s.health === "unhealthy"
                      ? "destructive"
                      : "default"
                }
              >
                {s.health}
              </Badge>
            </td>
            <td className="py-1 font-mono">{s.ports.join(" ")}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
