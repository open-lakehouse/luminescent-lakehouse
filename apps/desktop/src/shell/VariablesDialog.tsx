// An environment's variables, one tab per var set they can be in, as `trestle var set`
// sets them: the workspace's (every environment inherits them), the environment's (on a
// team's store, shared with the team), the next revision's (the draft's: a commit
// freezes them; the head revision's are shown, read-only), this machine's own (never
// shared), and the shown instance's overrides. Any value can be a secret, which goes to
// the keychain: the var sets, and this dialog, hold only its `keychain:<key>`
// reference. What the instance ends up with is the Environment panel's
// (EffectiveEnvPanel). The resolution order is the runtime's: workspace < environment
// < revision < this machine < sources (a Neon branch) < instance.

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@open-lakehouse/ui-kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { toast } from "sonner";
import { DialogBody } from "./choices";
import { Failure } from "./RunPanel";
import { useShellHost, type VarEntry, type VarsView, type VarTarget } from "./shell-host";
import { invalidateVars, scopeWords, VarField } from "./VarField";
import { message } from "./WorkspaceBar";

export const varsQueryKey = (environment: string, instance: string | null) =>
  ["vars", environment, instance] as const;

/** The environment's var sets, kept fresh as the store changes (the CLI's writes too). */
export function useVars(environmentId: string, instance: string | null) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  useEffect(
    () => shell.workspaces.subscribe(() => invalidateVars(queryClient)),
    [shell, queryClient],
  );
  return useQuery({
    queryKey: varsQueryKey(environmentId, instance),
    queryFn: () => shell.vars.get(environmentId, instance),
  });
}

type Tab = "workspace" | "environment" | "draft" | "local" | "instance";

/** The rows of one var set; `target` set, each has a remove button. */
function VarTable({
  entries,
  target,
  testId,
  empty,
}: {
  entries: VarEntry[];
  target?: VarTarget;
  testId: string;
  empty: string;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  if (entries.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid={`${testId}-empty`}>
        {empty}
      </p>
    );
  }
  const unset = (key: string) => {
    if (!target) return;
    void shell.vars.unset(target, key).then(
      () => {
        toast.success(`${key} unset on ${scopeWords(target)}`);
        invalidateVars(queryClient);
      },
      (e: unknown) => toast.error(`${key}: ${message(e)}`),
    );
  };
  return (
    <div className="max-h-56 divide-y overflow-auto rounded-md border" data-testid={testId}>
      {entries.map((e) => (
        <div
          key={e.key}
          className="flex items-center gap-2 px-2.5 py-1.5 text-xs"
          data-testid={`${testId}-row`}
          data-key={e.key}
          data-secret={e.secret}
        >
          <span className="w-44 shrink-0 truncate font-mono">{e.key}</span>
          <span
            className="min-w-0 flex-1 truncate font-mono text-muted-foreground"
            title={e.secret ? "A secret: the keychain holds its value" : e.value}
          >
            {e.secret ? `🔒 ${e.value}` : e.value || <em>empty</em>}
          </span>
          {target && (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-6 w-6 p-0"
              aria-label={`Unset ${e.key}`}
              onClick={() => unset(e.key)}
              data-testid={`${testId}-unset`}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      ))}
    </div>
  );
}

function Explain({ children }: { children: ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}

/** A team's store refuses a secret here, as the CLI does. */
const TEAM_SECRET =
  "On a team's store a secret here would reach colleagues whose keychains don't hold it: set it on the environment (kept on this machine)";

export function VariablesDialog({
  environmentId,
  environment,
  instance,
  onClose,
}: {
  /** The store's id. */
  environmentId: string;
  environment: string;
  /** The instance shown in the header, whose overrides the Instance tab edits. */
  instance: { id: string; revision: number } | null;
  onClose: () => void;
}) {
  const vars = useVars(environmentId, instance?.id ?? null);
  const [tab, setTab] = useState<Tab>("environment");
  const view: VarsView | undefined = vars.data;
  const env = (scope: "environment" | "draft" | "local"): VarTarget => ({
    scope,
    environment: environmentId,
  });
  const shared = !!view?.shared;
  const next = (view?.revision?.number ?? 0) + 1;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="variables-dialog" className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Variables of {environment}</DialogTitle>
          <DialogDescription>
            What its modules read, applied at the next Start. A secret's value goes to the keychain;
            only a reference to it is stored.
          </DialogDescription>
        </DialogHeader>
        <DialogBody>
          {vars.isError && <Failure error={vars.error} />}
          {view && (
            <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
              <TabsList>
                <TabsTrigger value="workspace" data-testid="vars-tab-workspace">
                  Workspace
                </TabsTrigger>
                <TabsTrigger value="environment" data-testid="vars-tab-environment">
                  Environment
                </TabsTrigger>
                <TabsTrigger value="draft" data-testid="vars-tab-draft">
                  Next revision
                </TabsTrigger>
                <TabsTrigger value="local" data-testid="vars-tab-local">
                  This machine
                </TabsTrigger>
                <TabsTrigger
                  value="instance"
                  disabled={!instance}
                  data-testid="vars-tab-instance"
                  title={instance ? undefined : "Start the environment to override its instance's"}
                >
                  Instance
                </TabsTrigger>
              </TabsList>

              <TabsContent value="workspace" className="space-y-2.5 pt-2">
                <Explain>
                  Every environment of the workspace inherits these; an environment's own win.
                  {shared ? " On this team's store, colleagues see them too." : ""}
                </Explain>
                <VarTable
                  entries={view.workspace}
                  target={{ scope: "workspace", workspace: view.workspaceId }}
                  testId="vars-workspace"
                  empty="No workspace variables."
                />
                <VarField
                  target={{ scope: "workspace", workspace: view.workspaceId }}
                  secretDisabled={shared ? TEAM_SECRET : undefined}
                  testId="vars-workspace-add"
                />
              </TabsContent>

              <TabsContent value="environment" className="space-y-2.5 pt-2">
                <Explain>
                  {environment}'s own, over the workspace's.{" "}
                  {shared
                    ? "This environment is on a team's store: these are shared with the team, so a secret set here goes to this machine's vars instead."
                    : "On a team's store these are shared with the team; this machine's vars never are."}
                </Explain>
                <VarTable
                  entries={view.environment}
                  target={env("environment")}
                  testId="vars-environment"
                  empty="No environment variables."
                />
                <VarField target={env("environment")} testId="vars-environment-add" />
              </TabsContent>

              <TabsContent value="draft" className="space-y-2.5 pt-2">
                <Explain>
                  The draft's: committing it freezes them into revision {next}. A committed
                  revision's never change.
                </Explain>
                <VarTable
                  entries={view.draft}
                  target={env("draft")}
                  testId="vars-draft"
                  empty="No variables in the draft."
                />
                <VarField
                  target={env("draft")}
                  secretDisabled={shared ? TEAM_SECRET : undefined}
                  testId="vars-draft-add"
                />
                {view.revision && (
                  <div className="space-y-1.5 pt-1">
                    <p className="text-xs font-medium text-muted-foreground">
                      Revision {view.revision.number} (the head), read-only
                    </p>
                    <VarTable
                      entries={view.revision.vars}
                      testId="vars-revision"
                      empty={`Revision ${view.revision.number} has no variables.`}
                    />
                  </div>
                )}
              </TabsContent>

              <TabsContent value="local" className="space-y-2.5 pt-2">
                <Explain>
                  Kept in this machine's state only, never shared, even when the environment is a
                  team's (<span className="font-mono">trestle var set --local</span>). They win over
                  the workspace's, the environment's and the revision's. The keys Start mints are
                  here.
                </Explain>
                <VarTable
                  entries={view.local}
                  target={env("local")}
                  testId="vars-local"
                  empty="No variables on this machine."
                />
                <VarField target={env("local")} testId="vars-local-add" />
              </TabsContent>

              <TabsContent value="instance" className="space-y-2.5 pt-2">
                {view.instance && (
                  <>
                    <Explain>
                      Overrides of the instance of revision {view.instance.revision}, over
                      everything else (a Neon branch's keys too), until it is removed. They apply
                      when it next starts.
                    </Explain>
                    <VarTable
                      entries={view.instance.vars}
                      target={{ scope: "instance", instance: view.instance.id }}
                      testId="vars-instance"
                      empty="No overrides."
                    />
                    <VarField
                      target={{ scope: "instance", instance: view.instance.id }}
                      testId="vars-instance-add"
                    />
                  </>
                )}
              </TabsContent>
            </Tabs>
          )}
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose} data-testid="variables-close">
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
