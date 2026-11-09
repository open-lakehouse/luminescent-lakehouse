// Where an environment runs on Neon: a branch of a project, picked in its header
// and bound by Start (`trestle up --neon-branch <branch|new> [--ttl]`). The project
// is the workspace's unless the environment picks its own, so a dev and a prod
// environment can run side by side on different projects. A new branch is made at
// the next Start, or right here under a name of your own (`CreateBranch`), from the
// workspace's parent (`trestle ws set-neon --parent`) or the project's default
// branch, and owned by trestle.

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@open-lakehouse/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
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
      <DialogContent data-testid="neon-target">
        <form onSubmit={submit} className="space-y-4">
          <DialogHeader>
            <DialogTitle>Where {environment} runs on Neon</DialogTitle>
            <DialogDescription>
              {needs?.needed
                ? `It runs ${describe(needs)} on this branch, and everything else here. `
                : ""}
              Start binds the branch; the instance stays on it until you pick another.
            </DialogDescription>
          </DialogHeader>
          <label className="flex items-center gap-2 text-sm">
            Project
            <select
              aria-label="Project"
              className="flex-1 rounded border bg-background px-1 py-0.5"
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
          </label>
          {projects.isError && <Failure error={projects.error} />}
          {branches.isError && <Failure error={branches.error} />}
          {branches.isPending && <p className="text-sm text-muted-foreground">Loading branches…</p>}
          <fieldset className="space-y-2 text-sm" aria-label="Branch">
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="branch"
                checked={choice === NEW}
                onChange={() => setPicked(NEW)}
                data-testid="neon-target-new"
              />
              A new branch from <span className="font-mono">{parent}</span>, for{" "}
              <select
                aria-label="Lifetime"
                className="rounded border bg-background px-1"
                value={ttl}
                onChange={(e) => {
                  setTtl(Number(e.target.value));
                  setPicked(NEW);
                }}
              >
                {TTLS.map((t) => (
                  <option key={t.hours} value={t.hours}>
                    {t.label}
                  </option>
                ))}
              </select>
            </label>
            {branches.data?.branches.map((b) => (
              <label key={b.id} className="flex items-center gap-2">
                <input
                  type="radio"
                  name="branch"
                  checked={choice === b.name}
                  onChange={() => setPicked(b.name)}
                  data-testid={`neon-target-${b.name}`}
                />
                <span className="font-mono">{b.name}</span>
                {b.default && <span className="text-xs text-muted-foreground">default</span>}
                {b.owned && <span className="text-xs text-muted-foreground">made by trestle</span>}
              </label>
            ))}
          </fieldset>
          <CreateBranch
            workspaceId={workspaceId}
            project={projectId}
            parent={parent}
            onCreated={(b) => setPicked(b.name)}
          />
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

/** Make a branch now, under a name of your own, from `parent`, owned by trestle and
 *  expiring after the lifetime picked: it is listed (and picked) right away. Not a
 *  form of its own, so it can sit inside the target dialog's. */
export function CreateBranch({
  workspaceId,
  project = null,
  parent,
  onCreated,
}: {
  workspaceId: string;
  /** The project to make it in: the workspace's when `null`. */
  project?: string | null;
  /** Where it is made from, for the label. */
  parent: string;
  onCreated?: (branch: NeonBranch) => void;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
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
  return (
    <div className="space-y-1 text-sm" data-testid="neon-create-branch">
      <div className="flex items-center gap-2">
        <input
          aria-label="New branch name"
          placeholder="new-branch"
          className="w-40 rounded border bg-background px-1 py-0.5 font-mono"
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
        <span className="text-muted-foreground">
          from <span className="font-mono">{parent}</span>, for
        </span>
        <select
          aria-label="New branch lifetime"
          className="rounded border bg-background px-1"
          value={ttl}
          onChange={(e) => setTtl(Number(e.target.value))}
        >
          {TTLS.map((t) => (
            <option key={t.hours} value={t.hours}>
              {t.label}
            </option>
          ))}
        </select>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!name.trim() || create.isPending}
          onClick={submit}
          data-testid="neon-create-submit"
        >
          {create.isPending ? "Creating…" : "Create now"}
        </Button>
      </div>
      {create.isError && <Failure error={create.error} />}
    </div>
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
