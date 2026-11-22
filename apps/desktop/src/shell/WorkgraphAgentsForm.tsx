// The Configure tab's "Work graph agents" (shell-core's `workgraph_agents`): the roles of
// the environment's branch's `workgraph` function, one egress route per role chosen, and
// every egress route of the environment with whether its key is set.
//
// A role is what the function enforces, so the choices come from its catalog (`GET
// /roles`), never from a list here. Turning a role on makes a service account for this
// environment on the branch and puts its key straight into the keychain. Its key can be
// rotated (the running instance's secret server is recreated, the egress gateway keeps
// running) or replaced by one made elsewhere, pasted once and never shown again. There is
// no way here to give a route the apps token: only a service account's `olk_` key is taken.

import {
  Badge,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
  Switch,
} from "@open-lakehouse/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { DialogBody } from "./choices";
import type { AgentsApplied, RouteView } from "./shell-host";
import { useShellHost } from "./shell-host";
import { invalidateVars } from "./VarField";
import { message } from "./WorkspaceBar";

export const agentsQueryKey = (environment: string) => ["workgraph-agents", environment];

/** The routes' upstream, in a few words. */
export function upstreamWords(route: RouteView): string {
  return route.function ? `${route.function} function` : (route.upstream ?? "?");
}

export function WorkgraphAgentsForm({
  environmentId,
  locked,
}: {
  environmentId: string;
  /** Why the roles can't change now (the environment runs): keys still can. */
  locked?: string;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const agents = useQuery({
    queryKey: agentsQueryKey(environmentId),
    queryFn: () => shell.workgraphAgents.get(environmentId),
  });
  const [chosen, setChosen] = useState<string[] | null>(null);
  const [pasting, setPasting] = useState<string | null>(null);
  const view = agents.data;
  const current = view?.identities.map((i) => i.role) ?? [];
  const roles = chosen ?? current;

  const done = (what: string) => (applied: AgentsApplied) => {
    queryClient.setQueryData(agentsQueryKey(environmentId), applied.view);
    invalidateVars(queryClient);
    void queryClient.invalidateQueries({ queryKey: ["workspace-run"] });
    setChosen(null);
    const recreated = applied.recreated.length > 0 ? "; the running instance has it now" : "";
    const commit = applied.routes?.needsCommit ? "; commit the draft to start with the routes" : "";
    toast.success(`${what}${recreated}${commit}`);
  };
  const failed = (e: unknown) => toast.error(message(e));
  const save = useMutation({
    mutationFn: () =>
      roles.length > 0
        ? shell.workgraphAgents.enable(environmentId, roles)
        : shell.workgraphAgents.disable(environmentId),
    onSuccess: done("Work graph agents saved"),
    onError: failed,
  });
  const rotate = useMutation({
    mutationFn: (role: string) => shell.workgraphAgents.rotate(environmentId, role),
    onSuccess: (applied, role) => done(`The ${role} agent has a new key`)(applied),
    onError: failed,
  });

  if (agents.isPending) return <p className="text-xs text-muted-foreground">Loading…</p>;
  if (agents.isError) return <p className="text-xs text-destructive">{message(agents.error)}</p>;
  if (!view) return null;

  const changed = JSON.stringify([...roles].sort()) !== JSON.stringify([...current].sort());
  const blocked = !view.omnigent
    ? "The agents reach the work graph through Omnigent: add Omnigent to the environment first."
    : (view.branchError ?? view.catalogError ?? undefined);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate();
  };

  return (
    <div className="space-y-5" data-testid="workgraph-agents">
      <form onSubmit={submit} className="space-y-3">
        {blocked && (
          <p className="text-xs text-muted-foreground" data-testid="workgraph-agents-blocked">
            {blocked}
          </p>
        )}
        {view.catalog && (
          <fieldset disabled={!!locked || !!blocked} className="space-y-2">
            <legend className="text-xs text-muted-foreground">
              The roles the {view.branch ?? "branch"} branch's work graph grants. Each one on gets a
              service account of this environment, its key in the keychain, and an egress route its
              sessions select by name.
            </legend>
            {view.catalog.roles.map((role) => (
              <div key={role.name} className="flex items-start gap-3">
                <Switch
                  id={`agent-role-${role.name}`}
                  checked={roles.includes(role.name)}
                  onCheckedChange={(on) =>
                    setChosen(on ? [...roles, role.name] : roles.filter((r) => r !== role.name))
                  }
                  data-testid={`agent-role-${role.name}`}
                />
                <Label htmlFor={`agent-role-${role.name}`} className="space-y-0.5">
                  <span className="block font-medium">{role.name}</span>
                  <span className="block font-mono text-xs text-muted-foreground">
                    {role.tools.join(", ")}
                  </span>
                </Label>
              </div>
            ))}
          </fieldset>
        )}
        {view.drift.length > 0 && (
          <ul className="space-y-1 text-xs text-amber-600" data-testid="workgraph-agents-drift">
            {view.drift.map((d) => (
              <li key={d}>{d}: save to follow the function.</li>
            ))}
          </ul>
        )}
        <div className="flex justify-end">
          <Button
            type="submit"
            disabled={
              !!locked || !!blocked || save.isPending || (!changed && view.drift.length === 0)
            }
            title={locked}
            data-testid="workgraph-agents-save"
          >
            Save
          </Button>
        </div>
      </form>

      <section className="space-y-2" aria-label="Egress routes">
        <h3 className="text-xs font-semibold">Egress routes</h3>
        {view.routes.length === 0 ? (
          <p className="text-xs text-muted-foreground" data-testid="egress-routes-none">
            None: nothing in the environment reaches an outside service through the egress gateway.
          </p>
        ) : (
          <table className="w-full text-xs" data-testid="egress-routes">
            <thead className="text-left text-muted-foreground">
              <tr>
                <th className="py-1 font-normal">Route</th>
                <th className="py-1 font-normal">Upstream</th>
                <th className="py-1 font-normal">Consumers</th>
                <th className="py-1 font-normal">Key</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {view.routes.map((route) => (
                <tr
                  key={route.name}
                  className="border-t"
                  data-testid={`egress-route-${route.name}`}
                >
                  <td className="py-1.5 font-mono" title={route.tools.join(", ")}>
                    {route.name}
                  </td>
                  <td className="py-1.5">
                    {upstreamWords(route)}
                    <span className="text-muted-foreground"> {route.path}</span>
                  </td>
                  <td className="py-1.5">{route.consumers.join(", ")}</td>
                  <td className="py-1.5">
                    <Badge
                      variant={route.keySet ? "success" : "destructive"}
                      title={route.secret}
                      data-testid={`egress-route-${route.name}-key`}
                    >
                      {route.keySet ? "set" : "missing"}
                    </Badge>
                  </td>
                  <td className="space-x-1 py-1.5 text-right">
                    {route.role && view.identities.some((i) => i.role === route.role) && (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={rotate.isPending}
                        onClick={() => route.role && rotate.mutate(route.role)}
                        data-testid={`egress-route-${route.name}-rotate`}
                      >
                        Rotate
                      </Button>
                    )}
                    {route.role && (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() => setPasting(route.role)}
                        data-testid={`egress-route-${route.name}-paste`}
                      >
                        {route.keySet ? "Replace key" : "Set key"}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <PasteKeyDialog
        environmentId={environmentId}
        role={pasting}
        onClose={() => setPasting(null)}
        onDone={(role, applied) => {
          setPasting(null);
          done(`The ${role} agent's key is set`)(applied);
        }}
      />
    </div>
  );
}

/** Paste a service account's key, once: it goes to the keychain and is never shown.
 *  Open while `role` is set; it stays mounted, as closing a dialog by unmounting it
 *  mid-transition trips the test DOM. */
function PasteKeyDialog({
  environmentId,
  role,
  onClose,
  onDone,
}: {
  environmentId: string;
  role: string | null;
  onClose: () => void;
  onDone: (role: string, applied: AgentsApplied) => void;
}) {
  const shell = useShellHost();
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const set = useMutation({
    mutationFn: (role: string) => shell.workgraphAgents.setKey(environmentId, role, value),
    onSuccess: (applied, role) => {
      setValue("");
      setError(null);
      onDone(role, applied);
    },
    onError: (e) => {
      setValue("");
      setError(message(e));
    },
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (role && value.trim()) set.mutate(role);
  };
  const close = () => {
    setValue("");
    setError(null);
    onClose();
  };
  return (
    <Dialog open={role !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent data-testid="paste-key">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>The {role} agent's key</DialogTitle>
            <DialogDescription>
              A service account's API key (olk_…), as `just admin &lt;branch&gt; api-key issue`
              shows it once. It goes to the keychain and is never shown again. Never the apps token:
              everything behind the route would get the right to approve plans.
            </DialogDescription>
          </DialogHeader>
          <DialogBody>
            <Input
              aria-label={`The ${role} agent's key`}
              type="password"
              autoComplete="off"
              className="font-mono text-xs"
              placeholder="olk_…"
              value={value}
              onChange={(e) => {
                setValue(e.target.value);
                setError(null);
              }}
              data-testid="paste-key-value"
            />
            {error && (
              <p
                className="pt-2 text-xs text-destructive"
                role="alert"
                data-testid="paste-key-error"
              >
                {error}
              </p>
            )}
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={!value.trim() || set.isPending}
              data-testid="paste-key-save"
            >
              Set key
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
