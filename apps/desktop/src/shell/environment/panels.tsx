// The bottom panel's tabs for the open environment: Run (its revisions and
// containers), Logs (the shown instance's containers) and Environment (what the shown
// instance launches with).

import type { PanelContribution } from "@open-lakehouse/env-editor";
import { Play, ScrollText, Variable } from "lucide-react";
import type { ReactNode } from "react";
import { EffectiveEnvPanel } from "../EffectiveEnvPanel";
import { LogsPanel } from "../LogsPanel";
import { RunPanel } from "../RunPanel";
import type { Environments } from "./model";

/** The bottom panel's tabs this module contributes. */
export const RUN_PANEL = "run";
export const LOGS_PANEL = "logs";
export const ENV_PANEL = "effective-env";

export function environmentPanels(
  environments: Environments,
  openApps?: (instance: string) => void,
): PanelContribution[] {
  const of = (envId: string | undefined) => (envId ? environments.byEditorId(envId) : undefined);
  return [
    {
      id: RUN_PANEL,
      label: "Run",
      icon: <Play />,
      render: (envId) => {
        const m = of(envId);
        return (
          <RunPanel
            env={m?.env}
            failure={m?.failure}
            engineProblem={environments.data?.engineProblem}
            openApps={openApps}
          />
        );
      },
    },
    {
      id: LOGS_PANEL,
      label: "Logs",
      icon: <ScrollText />,
      render: (envId) => {
        const m = of(envId);
        if (!m) return <PanelNote>Open an environment to follow its logs.</PanelNote>;
        if (!m.shown || m.shown.slot === null) {
          return <PanelNote>{m.name} has no containers to follow.</PanelNote>;
        }
        return <LogsPanel key={m.shown.id} instance={m.shown} />;
      },
    },
    {
      id: ENV_PANEL,
      label: "Environment",
      icon: <Variable />,
      render: (envId) => {
        const m = of(envId);
        if (!m) return <PanelNote>Open an environment to see what it launches with.</PanelNote>;
        return <EffectiveEnvPanel key={m.shown?.id} environment={m.name} instance={m.shown} />;
      },
    },
  ];
}

function PanelNote({ children }: { children: ReactNode }) {
  return <p className="p-3 text-xs text-muted-foreground">{children}</p>;
}
