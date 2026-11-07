// A service UI under the environment it belongs to (an environment tab's
// Catalog or Lineage). Showing it makes that environment's instance the current
// one, so the views reach its services; until the switch is done, it shows no
// service at all, so another environment's data can't render under this one.

import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { createQueryClient } from "../app";
import { ServiceGate } from "../integrations/ServiceGate";
import { useShellHost } from "./shell-host";
import { message, Notice } from "./WorkspaceBar";

/**
 * What is under it belongs to one environment: a query cache of its own per
 * environment key, and a remount on the key. A result cached for the previous
 * environment, or a response still in flight from it, lands in that
 * environment's cache, which nothing renders any more.
 */
export function EnvironmentBoundary({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  const environmentKey = useEnvironmentKey();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new cache per environment key is the point.
  const queryClient = useMemo(() => createQueryClient(), [environmentKey]);
  useEffect(() => () => queryClient.clear(), [queryClient]);
  return (
    <QueryClientProvider client={queryClient}>
      <div key={environmentKey} className={className}>
        {children}
      </div>
    </QueryClientProvider>
  );
}

/** Instance `instance` (labelled `label`, e.g. `dev #3`) as the current environment,
 *  then `moduleId`'s view (`name`) on its services. */
export function EnvironmentData({
  instance,
  label,
  moduleId,
  name,
  children,
}: {
  instance: string;
  label: string;
  moduleId: string;
  name: string;
  children: ReactNode;
}) {
  const shell = useShellHost();
  const host = useHost();
  useEnvironmentKey(); // re-render when it changes
  const isCurrent = host.environment.current()?.id === instance;
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    if (isCurrent) return;
    setProblem(null);
    shell.run.use(instance).catch((e: unknown) => setProblem(message(e)));
  }, [shell, instance, isCurrent]);

  if (problem) return <Notice tone="error">{problem}</Notice>;
  if (!isCurrent) return <Notice>Switching the views to {label}…</Notice>;
  return (
    <EnvironmentBoundary className="h-full">
      <ServiceGate moduleId={moduleId} name={name} environment={label}>
        {children}
      </ServiceGate>
    </EnvironmentBoundary>
  );
}
