// The window: a rail of shell views next to the active one. The view remounts
// when the environment key changes, and the query cache is cleared, so nothing
// from the previous environment survives a switch.

import { useEnvironmentKey, useHost } from "@open-lakehouse/host/react";
import { cn, useTheme } from "@open-lakehouse/ui-kit";
import { useQueryClient } from "@tanstack/react-query";
import { Boxes, Moon, Stethoscope, Sun } from "lucide-react";
import { type ComponentType, useEffect, useState } from "react";
import { DoctorView } from "./DoctorView";
import { WorkspaceView } from "./WorkspaceView";

export type ShellViewId = "workspace" | "doctor";

const VIEWS: { id: ShellViewId; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "workspace", label: "Workspace", icon: Boxes },
  { id: "doctor", label: "Doctor", icon: Stethoscope },
];

export function ShellLayout({ initialView = "workspace" }: { initialView?: ShellViewId }) {
  const [view, setView] = useState<ShellViewId>(initialView);
  const host = useHost();
  const environmentKey = useEnvironmentKey();
  const queryClient = useQueryClient();

  // Clear in the scope's own notification, which runs before React renders the
  // remounted view: an effect would run after it and wipe the new
  // environment's queries too.
  useEffect(() => host.environment.subscribe(() => queryClient.clear()), [host, queryClient]);

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
      <main key={environmentKey} className="min-w-0 flex-1">
        {view === "workspace" ? <WorkspaceView /> : <DoctorView />}
      </main>
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
