// The window: a rail of shell views next to the active one. The view remounts
// when the environment key changes, and the query cache is cleared, so nothing
// from the previous environment survives a switch.

import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { cn, useTheme } from "@open-lakehouse/ui-kit";
import { useQueryClient } from "@tanstack/react-query";
import { Boxes, Database, Moon, Play, Stethoscope, Sun, Workflow } from "lucide-react";
import { type ComponentType, lazy, type ReactNode, Suspense, useEffect, useState } from "react";
import { ServiceGate } from "../integrations/ServiceGate";
import { HEADWATERS, UNITY_CATALOG } from "../integrations/services";
import { DoctorView } from "./DoctorView";
import { RunView } from "./RunView";
import { CurrentWorkspaceProvider } from "./WorkspaceBar";
import { WorkspaceView } from "./WorkspaceView";

// The service UIs load on first use: they carry most of the bundle.
const CatalogView = lazy(() =>
  import("../integrations/unity-catalog/CatalogView").then((m) => ({ default: m.CatalogView })),
);
const LineageView = lazy(() =>
  import("../integrations/lineage/LineageView").then((m) => ({ default: m.LineageView })),
);

export type ShellViewId = "workspace" | "run" | "catalog" | "lineage" | "doctor";

const VIEWS: { id: ShellViewId; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "workspace", label: "Workspace", icon: Boxes },
  { id: "run", label: "Run", icon: Play },
  { id: "catalog", label: "Catalog", icon: Database },
  { id: "lineage", label: "Lineage", icon: Workflow },
  { id: "doctor", label: "Doctor", icon: Stethoscope },
];

function ActiveView({ view }: { view: ShellViewId }) {
  switch (view) {
    case "workspace":
      return <WorkspaceView />;
    case "run":
      return <RunView />;
    case "doctor":
      return <DoctorView />;
    case "catalog":
      return (
        <ServiceGate moduleId={UNITY_CATALOG} name="Unity Catalog">
          <Suspense fallback={null}>
            <CatalogView />
          </Suspense>
        </ServiceGate>
      );
    case "lineage":
      return (
        <ServiceGate moduleId={HEADWATERS} name="Lineage">
          <Suspense fallback={null}>
            <LineageView />
          </Suspense>
        </ServiceGate>
      );
  }
}

/**
 * What is under it belongs to one environment: it remounts on the environment's key,
 * and the query cache is cleared first, so nothing a view fetched from the previous
 * environment (a cached result, or a response still in flight) shows in the next.
 */
export function EnvironmentBoundary({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  const host = useHost();
  const environmentKey = useEnvironmentKey();
  const queryClient = useQueryClient();
  // Clear in the scope's own notification, which runs before React renders the
  // remounted view: an effect would run after it and wipe the new
  // environment's queries too.
  useEffect(() => host.environment.subscribe(() => queryClient.clear()), [host, queryClient]);
  return (
    <main key={environmentKey} className={className}>
      {children}
    </main>
  );
}

export function ShellLayout({ initialView = "workspace" }: { initialView?: ShellViewId }) {
  const [view, setView] = useState<ShellViewId>(initialView);

  return (
    <div className="flex h-full" data-testid="shell">
      <nav
        aria-label="Shell views"
        className="flex w-12 shrink-0 flex-col items-center gap-1 border-r bg-activitybar py-2"
      >
        {VIEWS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            title={label}
            aria-label={label}
            aria-current={view === id ? "page" : undefined}
            data-testid={`nav-${id}`}
            onClick={() => setView(id)}
            className={cn(
              "flex size-9 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground",
              view === id && "bg-accent text-foreground",
            )}
          >
            <Icon className="size-5" />
          </button>
        ))}
        <div className="flex-1" />
        <ThemeToggle />
      </nav>
      <EnvironmentBoundary className="min-w-0 flex-1">
        <CurrentWorkspaceProvider>
          <ActiveView view={view} />
        </CurrentWorkspaceProvider>
      </EnvironmentBoundary>
    </div>
  );
}

function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  return (
    <button
      type="button"
      title={dark ? "Light theme" : "Dark theme"}
      aria-label={dark ? "Light theme" : "Dark theme"}
      onClick={() => setTheme(dark ? "light" : "dark")}
      className="flex size-9 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
    >
      {dark ? <Sun className="size-5" /> : <Moon className="size-5" />}
    </button>
  );
}
