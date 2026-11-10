// The editor's bottom Run panel, for the open environment: what its last Start
// or Stop failed with (compose's output included), and each live revision's
// gateway (a link to its apps; the header copies the URL) and containers, as
// Docker reports them.

import { Badge } from "@open-lakehouse/ui-kit";
import { liveInstances, runState } from "./run-state";
import {
  type EnvironmentRun,
  type InstanceStatus,
  RunFailure,
  type ServiceRow,
} from "./shell-host";
import { message } from "./WorkspaceBar";

export function RunPanel({
  env,
  failure,
  engineProblem,
  openApps,
}: {
  env: EnvironmentRun | undefined;
  failure: Error | undefined;
  engineProblem: string | null | undefined;
  /** Shows the Apps view on an instance. */
  openApps?: (instance: string) => void;
}) {
  if (!env) {
    return <p className="p-3 text-xs text-muted-foreground">Open an environment to see its run.</p>;
  }
  const live = liveInstances(env);
  return (
    <div className="space-y-3 p-3" data-testid={`run-panel-${env.name}`}>
      {engineProblem && (
        <p className="text-xs text-destructive" role="alert">
          Docker isn't available, so the containers are unknown: {engineProblem}
        </p>
      )}
      {failure && <Failure error={failure} />}
      {live.length === 0 && !failure && (
        <p className="text-xs text-muted-foreground">
          {env.name} isn't running.{" "}
          {env.head ? `Start runs revision #${env.head}.` : "Commit a revision first (History)."}
        </p>
      )}
      {live.map((instance) => (
        <div
          key={instance.id}
          className="space-y-2"
          data-testid={`run-${env.name}-r${instance.revision}`}
        >
          <div className="flex items-center gap-3 text-xs">
            <span className="font-medium">#{instance.revision}</span>
            <span data-testid={`run-${env.name}-r${instance.revision}-state`}>
              {runState(instance)}
            </span>
            <span className="font-mono text-muted-foreground">{instance.project}</span>
            {instance.gatewayUrl && instance.desired === "running" && (
              <GatewayLink
                url={instance.gatewayUrl}
                onOpen={openApps && (() => openApps(instance.id))}
              />
            )}
          </div>
          <Services instance={instance} />
        </div>
      ))}
    </div>
  );
}

/** The gateway's URL, as a link to the instance's apps (the Apps view); a plain
 *  URL when there is no view to open. */
export function GatewayLink({ url, onOpen }: { url: string; onOpen?: () => void }) {
  if (!onOpen) {
    return (
      <span className="font-mono text-xs" data-testid="gateway-url">
        {url}
      </span>
    );
  }
  return (
    <button
      type="button"
      className="font-mono text-xs text-primary underline-offset-2 hover:underline"
      title="Open its apps"
      onClick={onOpen}
      data-testid="gateway-url"
    >
      {url}
    </button>
  );
}

export function Failure({ error }: { error: Error }) {
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
