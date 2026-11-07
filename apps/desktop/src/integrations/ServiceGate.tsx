// Renders a service's view only when the current environment binds the
// service; otherwise says what doesn't serve it.

import { useService } from "@open-lakehouse/host/react";
import { Unplug } from "lucide-react";
import type { ReactNode } from "react";

export function ServiceGate({
  moduleId,
  name,
  environment,
  children,
}: {
  moduleId: string;
  name: string;
  /** The environment the view is under, e.g. `dev #3`. */
  environment?: string;
  children: ReactNode;
}) {
  const binding = useService(moduleId);
  if (binding) return <>{children}</>;
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
      data-testid={`unbound-${moduleId}`}
    >
      <Unplug className="size-8 text-muted-foreground" />
      <h2 className="text-lg font-semibold">{name} isn't bound</h2>
      <p className="max-w-md text-sm text-muted-foreground">
        {environment ?? "The current environment"} doesn't serve <code>{moduleId}</code>. Add the
        module and start a revision with it, or bind it by hand:{" "}
        <code>LUMINESCENT_SERVICES={moduleId}=&lt;gateway or function URL&gt;</code>
      </p>
    </div>
  );
}
