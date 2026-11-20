// Where an environment runs on Neon: a branch of a project, picked in its header
// and bound by Start (`trestle up --neon-branch <branch|new> [--ttl]`). The project
// is the workspace's unless the environment picks its own, so a dev and a prod
// environment can run side by side on different projects. A new branch is made at
// the next Start, or right here under a name of your own (`CreateBranch`), from the
// workspace's parent (`trestle ws set-neon --parent`) or the project's default
// branch, and owned by trestle.

import {
  Badge,
  Button,
  cn,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
} from "@open-lakehouse/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { ChoiceList, ChoiceRow, DialogBody, fieldClass, Section } from "./choices";
import { Failure } from "./RunPanel";
import { type NeonBranch, type NeonNeeds, type NeonTarget, useShellHost } from "./shell-host";

/** How long a new branch lives. */
export const TTLS = [
  { hours: 12, label: "12 hours" },
  { hours: 24, label: "1 day" },
  { hours: 168, label: "7 days" },
] as const;

const NEW = "\0new";
const WORKSPACE = "\0workspace";

/** The target in a few words, for the header and the tree: `☁ demo`, `☁ new branch`,
 *  with the project's name when it is the environment's own. */
export function targetLabel(target: NeonTarget, projectName?: string): string {
  const branch = target.branch.kind === "existing" ? target.branch.branch : "new branch";
  return target.project ? `☁ ${projectName ?? target.project} / ${branch}` : `☁ ${branch}`;
}

export function NeonTargetDialog({
  workspaceId,
  environment,
  needs,
  target,
  startAfter,
  onSave,
  onClose,
}: {
  workspaceId: string;
  environment: string;
  needs: NeonNeeds | undefined;
  /** The environment's target so far. */
  target: NeonTarget | undefined;
  /** Opened from Start: the button saves and starts. */
  startAfter: boolean;
  onSave: (target: NeonTarget) => void;
  onClose: () => void;
}) {
  const shell = useShellHost();
  const projectField = useId();
  const [project, setProject] = useState<string>(target?.project ?? WORKSPACE);
  const projectId = project === WORKSPACE ? null : project;
  const projects = useQuery({
    queryKey: ["neon-projects"],
    queryFn: () => shell.neon.projects(),
    staleTime: 5 * 60_000,
  });
  const branches = useQuery({
    queryKey: ["neon", workspaceId, projectId],
    queryFn: () => shell.neon.branches(workspaceId, projectId),
    staleTime: 60_000,
  });
  const initial =
    target && (target.project ?? null) === projectId && target.branch.kind === "existing"
      ? target.branch.branch
      : NEW;
  const [picked, setPicked] = useState<string | null>(null);
  const [ttl, setTtl] = useState<number>(
    target?.branch.kind === "new" ? (target.branch.ttlHours ?? 168) : 168,
  );
  const names = branches.data?.branches.map((b) => b.name) ?? [];
  const choice = picked ?? (initial === NEW || names.includes(initial) ? initial : NEW);
  const parent =
    (projectId ? null : branches.data?.parent) ??
    branches.data?.branches.find((b) => b.default)?.name ??
    "the default branch";
  const workspaceProject = projects.data?.find(
    (p) => p.id === (projectId ? undefined : branches.data?.projectId),
  );

  const submit = (e: FormEvent) => {
    e.preventDefault();
    onSave({
      project: projectId,
      branch:
        choice === NEW ? { kind: "new", ttlHours: ttl } : { kind: "existing", branch: choice },
    });
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="neon-target" className="max-w-lg">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>Neon branch for {environment}</DialogTitle>
            <DialogDescription>
              Start binds it; the environment stays on it until you pick another.
            </DialogDescription>
          </DialogHeader>
          <DialogBody>
            <Section label="Project" htmlFor={projectField}>
              <select
                id={projectField}
                aria-label="Project"
                className={fieldClass}
                value={project}
                onChange={(e) => {
                  setProject(e.target.value);
                  setPicked(null);
                }}
                data-testid="neon-target-project"
              >
                <option value={WORKSPACE}>
                  The workspace's
                  {workspaceProject ? ` (${workspaceProject.name})` : ""}
                </option>
                {projects.data?.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.org} / {p.name}
                  </option>
                ))}
              </select>
              {projects.isError && <Failure error={projects.error} />}
            </Section>
            <Section label="Branch">
              <ChoiceList label="Branch">
                <ChoiceRow
                  name="branch"
                  checked={choice === NEW}
                  onPick={() => setPicked(NEW)}
                  testId="neon-target-new"
                  title="A new branch"
                  hint={
                    <>
                      Made at Start from <span className="font-mono">{parent}</span>
                    </>
                  }
                >
                  <LifetimeSelect label="Lifetime" value={ttl} onChange={setTtl} />
                </ChoiceRow>
                {branches.isPending && (
                  <p className="px-3 py-2 text-xs text-muted-foreground">Loading branches…</p>
                )}
                {branches.isError && (
                  <div className="px-3 py-2">
                    <Failure error={branches.error} />
                  </div>
                )}
                {branches.data?.branches.map((b) => (
                  <ChoiceRow
                    key={b.id}
                    name="branch"
                    checked={choice === b.name}
                    onPick={() => setPicked(b.name)}
                    testId={`neon-target-${b.name}`}
                    title={<span className="font-mono">{b.name}</span>}
                    tags={
                      <>
                        {b.default && <Badge>default</Badge>}
                        {b.owned && <Badge variant="outline">trestle</Badge>}
                      </>
                    }
                  />
                ))}
              </ChoiceList>
              {needs?.needed && (
                <p className="text-xs text-muted-foreground">
                  It runs {describe(needs)} on the branch.
                </p>
              )}
            </Section>
            <CreateBranch
              workspaceId={workspaceId}
              project={projectId}
              parent={parent}
              collapsible
              onCreated={(b) => setPicked(b.name)}
            />
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" data-testid="neon-target-save">
              {startAfter ? "Save and start" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** How long a new branch lives, as a select. */
function LifetimeSelect({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (hours: number) => void;
}) {
  return (
    <select
      aria-label={label}
      className={cn(fieldClass, "w-auto shrink-0")}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
    >
      {TTLS.map((t) => (
        <option key={t.hours} value={t.hours}>
          Lives {t.label}
        </option>
      ))}
    </select>
  );
}

/** Make a branch now, under a name of your own, from `parent`, owned by trestle and
 *  expiring after the lifetime picked: it is listed (and picked) right away. Not a
 *  form of its own, so it can sit inside the target dialog's. */
export function CreateBranch({
  workspaceId,
  project = null,
  parent,
  collapsible = false,
  onCreated,
}: {
  workspaceId: string;
  /** The project to make it in: the workspace's when `null`. */
  project?: string | null;
  /** Where it is made from, for the label. */
  parent: string;
  /** Behind a button until asked for, where it is the less common way. */
  collapsible?: boolean;
  onCreated?: (branch: NeonBranch) => void;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(!collapsible);
  const [name, setName] = useState("");
  const [ttl, setTtl] = useState<number>(168);
  const create = useMutation({
    mutationFn: () => shell.neon.createBranch(workspaceId, name, { project, ttlHours: ttl }),
    onSuccess: async (branch) => {
      setName("");
      await queryClient.invalidateQueries({ queryKey: ["neon", workspaceId] });
      onCreated?.(branch);
    },
  });
  const submit = () => {
    if (name.trim() && !create.isPending) create.mutate();
  };
  if (!open) {
    return (
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="-ml-2 h-7 gap-1 text-xs text-muted-foreground"
        onClick={() => setOpen(true)}
        data-testid="neon-create-open"
      >
        <Plus className="h-3.5 w-3.5" />
        Create a named branch now
      </Button>
    );
  }
  return (
    <Section label="Create a branch now">
      <div className="space-y-1.5" data-testid="neon-create-branch">
        <div className="flex items-center gap-2">
          <Input
            aria-label="New branch name"
            placeholder="new-branch"
            className="h-8 flex-1 font-mono"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                submit();
              }
            }}
            data-testid="neon-create-name"
          />
          <LifetimeSelect label="New branch lifetime" value={ttl} onChange={setTtl} />
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8"
            disabled={!name.trim() || create.isPending}
            onClick={submit}
            data-testid="neon-create-submit"
          >
            {create.isPending ? "Creating…" : "Create"}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          Made from <span className="font-mono">{parent}</span>, owned by trestle.
        </p>
        {create.isError && <Failure error={create.error} />}
      </div>
    </Section>
  );
}

/** What it runs on the branch, in a few words. */
export function describe(needs: NeonNeeds): string {
  const parts = [
    needs.functions.length > 0 && `${needs.functions.join(" and ")} as functions`,
    needs.databases.length > 0 && `on its Postgres (${needs.databases.join(", ")})`,
    needs.storage && "on its object storage",
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(", ") : "with the branch's settings";
}
