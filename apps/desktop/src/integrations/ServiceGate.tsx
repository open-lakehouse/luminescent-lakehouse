// Renders a service's view only when the current environment binds the
// service; otherwise says how to bind it.

import { useService } from "@open-lakehouse/host/react";
import { Unplug } from "lucide-react";
import type { ReactNode } from "react";

export function ServiceGate({
  moduleId,
  name,
  children,
}: {
  moduleId: string;
  name: string;
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
        The current environment doesn't run <code>{moduleId}</code>. Start an environment with it,
        or bind it by hand:{" "}
        <code>LUMINESCENT_SERVICES={moduleId}=&lt;gateway or function URL&gt;</code>
      </p>
    </div>
  );
}
