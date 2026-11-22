// The bottom panel's Environment view: what the shown instance launches with, from the
// host's vantage (`trestle env show --effective`). Each variable shows its value (a
// secret's mask, and its `keychain:<key>` reference when a var set holds it, never its
// value), the layer that set it and the lower ones it shadows. The layers are the
// runtime's, lowest first: catalog defaults, revision manifest, placement, client env,
// workspace vars, environment vars, revision vars, local vars, each source (a Neon
// branch, shown as the branch; a Databricks profile), instance overrides. It is
// shell-core's `Runner::effective_env`, the runtime's `Instances::effective_env`: the
// resolver the launch goes through, so the two can't differ. Resolving asks the
// instance's sources as a launch does (the Neon CLI for a branch), so it runs when the
// view opens, after a variable changes, and on Refresh.

import { Button, Input } from "@open-lakehouse/ui-kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { copyToClipboard } from "./environments";
import { Failure } from "./RunPanel";
import { type EffectiveEntry, type InstanceStatus, useShellHost } from "./shell-host";

export const effectiveQueryKey = (instance: string) => ["effective-env", instance] as const;

function Row({ entry }: { entry: EffectiveEntry }) {
  return (
    <tr
      className="border-b last:border-0 align-top"
      data-testid="effective-row"
      data-key={entry.key}
      data-layer={entry.layer.name}
      data-secret={entry.secret}
    >
      <td className="py-1 pr-3 font-mono">{entry.key}</td>
      <td
        className="max-w-[28rem] truncate py-1 pr-3 font-mono text-muted-foreground"
        title={entry.secret ? (entry.reference ?? "a secret") : entry.value}
      >
        {entry.secret ? (
          <>
            {entry.value}
            {entry.reference && <span className="ml-2 text-[11px]">🔒 {entry.reference}</span>}
          </>
        ) : (
          entry.value || <em>empty</em>
        )}
      </td>
      <td className="py-1 pr-3 whitespace-nowrap" title={entry.layer.name}>
        {entry.layer.label}
      </td>
      <td className="py-1 text-muted-foreground" data-testid="effective-shadowed">
        {entry.shadowed.length > 0 && (
          <span title={entry.shadowed.map((l) => l.name).join(", ")}>
            over {entry.shadowed.map((l) => l.label).join(", ")}
          </span>
        )}
      </td>
    </tr>
  );
}

export function EffectiveEnvPanel({
  environment,
  instance,
}: {
  environment: string;
  /** The instance the header shows, if the environment has one. */
  instance: InstanceStatus | undefined;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState("");
  const id = instance?.id;
  const view = useQuery({
    queryKey: effectiveQueryKey(id ?? ""),
    queryFn: () => shell.vars.effective(id ?? ""),
    enabled: !!id,
    // It asks the instance's sources: not on a timer.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
  // A variable changed (here or in the CLI), or the instance restarted.
  useEffect(
    () =>
      shell.workspaces.subscribe(
        () => void queryClient.invalidateQueries({ queryKey: ["effective-env"] }),
      ),
    [shell, queryClient],
  );
  useEffect(
    () =>
      shell.run.subscribe(
        () => void queryClient.invalidateQueries({ queryKey: ["effective-env"] }),
      ),
    [shell, queryClient],
  );

  if (!instance) {
    return (
      <p className="p-3 text-xs text-muted-foreground" data-testid="effective-none">
        {environment} has no instance yet: start it to see what it launches with.
      </p>
    );
  }
  const needle = filter.trim().toLowerCase();
  const entries = (view.data?.entries ?? []).filter(
    (e) =>
      !needle ||
      e.key.toLowerCase().includes(needle) ||
      e.layer.label.toLowerCase().includes(needle),
  );
  return (
    <div className="space-y-2 p-3 text-xs" data-testid="effective-panel">
      <div className="flex items-center gap-2">
        <span className="font-medium">#{instance.revision}</span>
        <span className="text-muted-foreground">from the host, secrets masked</span>
        <Input
          aria-label="Filter"
          placeholder="Filter by name or layer"
          className="ml-auto h-7 w-56 text-xs"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          data-testid="effective-filter"
        />
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-xs"
          disabled={view.isFetching}
          onClick={() => void view.refetch()}
          data-testid="effective-refresh"
        >
          <RefreshCw className="mr-1 h-3.5 w-3.5" />
          Refresh
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-xs"
          disabled={!view.data}
          onClick={() => view.data && copyToClipboard(view.data.dotenv)}
          data-testid="effective-copy"
        >
          <Copy className="mr-1 h-3.5 w-3.5" />
          Copy as masked dotenv
        </Button>
      </div>
      {view.isPending && <p className="text-muted-foreground">Resolving its environment…</p>}
      {view.isError && <Failure error={view.error} />}
      {view.data && (
        <table className="w-full border-collapse" data-testid="effective-table">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="py-1 pr-3 font-medium">Variable</th>
              <th className="py-1 pr-3 font-medium">Value</th>
              <th className="py-1 pr-3 font-medium">Layer</th>
              <th className="py-1 font-medium">Shadows</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <Row key={e.key} entry={e} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
