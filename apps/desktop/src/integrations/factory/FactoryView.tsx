// The Factory app: the agentic factory's control room (`@open-lakehouse/factory-ui`) on
// the shell's `factory` binding, the in-process reference factory or an external
// factory-server (docs/desktop.md, "Factory"). The factory isn't an environment's: the
// control room sees one fixed environment, so making an instance current (to open a
// session on it) doesn't remount it.
//
// "Open session" shows a run's Omnigent session in the Agent view, on the instance's
// Omnigent (the Apps view's selected instance), with the instance's MCP servers with
// Apps. The control room stays mounted underneath, so its selection survives.

import type { OmnigentSession } from "@open-lakehouse/factory-client";
import { FactoryControlRoom } from "@open-lakehouse/factory-ui";
import type { LakehouseHost } from "@open-lakehouse/host";
import { HostProvider, useHost } from "@open-lakehouse/host/react";
import { Button, useTheme } from "@open-lakehouse/ui-kit";
import { QueryClientProvider } from "@tanstack/react-query";
import { ArrowLeft, Bot } from "lucide-react";
import { type ComponentType, useMemo, useState } from "react";
import { createQueryClient } from "../../app";
import { EnvironmentData } from "../../shell/EnvironmentData";
import { AgentView, type OpenMcp } from "../agent/AgentView";
import type { McpAppServer } from "../mcp/connection";
import type { McpAppProps } from "../mcp/McpApp";
import { ServiceGate } from "../ServiceGate";
import { FACTORY, OMNIGENT } from "../services";

/** Where the control room's sessions open: an instance with Omnigent. */
export interface SessionTarget {
  instance: string;
  /** `dev #3` */
  label: string;
  /** Its MCP servers with Apps (uc, where it serves uc-mcp). */
  servers: readonly McpAppServer[];
}

const FACTORY_ENVIRONMENT = { id: "factory", name: "factory" };

/** `host`, seen from the control room: its services, under one environment that never
 *  switches. */
export function factoryHost(host: LakehouseHost): LakehouseHost {
  return {
    ...host,
    environment: {
      current: () => FACTORY_ENVIRONMENT,
      key: FACTORY_ENVIRONMENT.id,
      subscribe: () => () => {},
    },
  };
}

export function FactoryView({
  sessions,
  mcp,
  App,
}: {
  /** Where "Open session" opens; without it, it says to start one. */
  sessions?: SessionTarget;
  mcp?: OpenMcp;
  App?: ComponentType<McpAppProps>;
}) {
  const host = useHost();
  const scoped = useMemo(() => factoryHost(host), [host]);
  const queryClient = useMemo(() => createQueryClient(), []);
  // React Flow's ColorMode takes `system` too.
  const { theme } = useTheme();
  const [open, setOpen] = useState<OmnigentSession>();

  return (
    <ServiceGate moduleId={FACTORY} name="The factory">
      <div className="flex h-full min-h-0 flex-col" data-testid="factory-view">
        <div className={open ? "hidden" : "min-h-0 flex-1"}>
          <HostProvider host={scoped}>
            <QueryClientProvider client={queryClient}>
              <FactoryControlRoom colorMode={theme} onOpenSession={setOpen} />
            </QueryClientProvider>
          </HostProvider>
        </div>
        {open ? (
          <SessionPane
            session={open}
            target={sessions}
            onBack={() => setOpen(undefined)}
            mcp={mcp}
            App={App}
          />
        ) : null}
      </div>
    </ServiceGate>
  );
}

function SessionPane({
  session,
  target,
  onBack,
  mcp,
  App,
}: {
  session: OmnigentSession;
  target: SessionTarget | undefined;
  onBack: () => void;
  mcp?: OpenMcp;
  App?: ComponentType<McpAppProps>;
}) {
  const agent = session.spec?.agentName || session.spec?.profile || "";
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="factory-session">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-2 text-sm">
        <Button size="sm" variant="ghost" onClick={onBack}>
          <ArrowLeft /> Control room
        </Button>
        <code className="truncate text-xs text-muted-foreground">{session.sessionId}</code>
      </div>
      <div className="min-h-0 flex-1">
        {target ? (
          <EnvironmentData
            key={`${target.instance}/${session.sessionId}`}
            instance={target.instance}
            label={target.label}
            moduleId={OMNIGENT}
            name="Omnigent"
          >
            <AgentView
              instance={target.instance}
              label={target.label}
              agent={agent}
              session={session.sessionId}
              heading={agent || "Session"}
              servers={target.servers}
              mcp={mcp}
              App={App}
            />
          </EnvironmentData>
        ) : (
          <div
            className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
            data-testid="factory-no-omnigent"
          >
            <Bot className="size-8 text-muted-foreground" />
            <h2 className="text-lg font-semibold">No Omnigent to open it on</h2>
            <p className="max-w-md text-sm text-muted-foreground">
              Start an environment with Omnigent (and pick it under Running) to open the factory's
              sessions. The session opens on that environment's Omnigent, where the factory's
              reconciler ran it.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
