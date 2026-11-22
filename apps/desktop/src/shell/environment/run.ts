// What runs: the workspace's environments and their instances, as trestle's
// instances report them (the ones `trestle up` / `ps` / `down` work on), refreshed on
// Docker's events, on commits, and every 30 s; and the Start and Stop in flight, with
// the last failure of each, for the header, the tree and the Run panel.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useShellHost } from "../shell-host";
import { message } from "../WorkspaceBar";

/** How often the containers are asked about anyway: Docker's events refresh them as
 *  things change. */
export const RUN_REFRESH_MS = 30_000;
/** Events come in bursts (one per container): one refresh per burst. */
const EVENT_SETTLE_MS = 150;

export const runQueryKey = (workspaceId: string) => ["run", workspaceId] as const;

/** The workspace's environments and their instances, as last asked. Kept live by
 *  `useWorkspaceRun`, which the frame mounts once. */
export function useRunQuery(workspaceId: string) {
  const shell = useShellHost();
  return useQuery({
    queryKey: runQueryKey(workspaceId),
    queryFn: () => shell.run.workspace(workspaceId),
    refetchInterval: RUN_REFRESH_MS,
  });
}

/** The workspace's environments and their instances, live with Docker and the state. */
export function useWorkspaceRun(workspaceId: string) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const run = useRunQuery(workspaceId);
  // A commit (here or in the CLI) changes what Start runs: refresh at once.
  useEffect(
    () =>
      shell.workspaces.subscribe(() => void queryClient.invalidateQueries({ queryKey: ["run"] })),
    [shell, queryClient],
  );
  // A container started, stopped, died or changed health.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = shell.run.subscribe(() => {
      clearTimeout(timer);
      timer = setTimeout(
        () => void queryClient.invalidateQueries({ queryKey: ["run"] }),
        EVENT_SETTLE_MS,
      );
    });
    return () => {
      clearTimeout(timer);
      unsubscribe();
    };
  }, [shell, queryClient]);
  return run;
}

export type Op = "start" | "stop";

/** The Starts and Stops in flight and the last failure, by environment name. */
export interface RunActions {
  pending: Record<string, Op>;
  failures: Record<string, Error>;
  /** Run `work` as `op` on environment `name`: pending meanwhile, a failure kept (and
   *  toasted), the run view refreshed after. */
  act: (name: string, op: Op, work: () => Promise<unknown>) => void;
}

export function useRunActions(): RunActions {
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<Record<string, Op>>({});
  const [failures, setFailures] = useState<Record<string, Error>>({});
  const act = useCallback(
    (name: string, op: Op, work: () => Promise<unknown>) => {
      setPending((p) => ({ ...p, [name]: op }));
      setFailures(({ [name]: _, ...rest }) => rest);
      work()
        .catch((e: unknown) => {
          const error = e instanceof Error ? e : new Error(String(e));
          setFailures((f) => ({ ...f, [name]: error }));
          toast.error(`${op === "start" ? "Start" : "Stop"} ${name}: ${message(error)}`, {
            description: "The Run panel (⌘J) has the details.",
          });
        })
        .finally(() => {
          setPending(({ [name]: _, ...rest }) => rest);
          void queryClient.invalidateQueries({ queryKey: ["run"] });
          void queryClient.invalidateQueries({ queryKey: ["settings"] });
        });
    },
    [queryClient],
  );
  return { pending, failures, act };
}

/** Why `name` can't be acted on while `op` runs, in the header's words. */
export function busyReason(name: string, op: Op | undefined): string | undefined {
  return op ? `${name} is ${op === "start" ? "starting" : "stopping"}` : undefined;
}
