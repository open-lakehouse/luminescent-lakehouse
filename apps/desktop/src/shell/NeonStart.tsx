// Start, for a revision that runs on Neon: which branch of the workspace's Neon
// project to run it on, as `trestle up --neon-branch <branch|new> [--ttl]` asks.
// The branch the environment last ran on comes first; a new branch is made from the
// workspace's parent (`trestle ws set-neon --parent`) and owned by trestle.

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Failure } from "./RunPanel";
import { type BranchChoice, type NeonNeeds, useShellHost } from "./shell-host";

/** How long a new branch lives. */
export const TTLS = [
  { hours: 12, label: "12 hours" },
  { hours: 24, label: "1 day" },
  { hours: 168, label: "7 days" },
] as const;

const NEW = "\0new";

export function NeonStart({
  workspaceId,
  environment,
  revision,
  needs,
  remembered,
  onStart,
  onClose,
}: {
  workspaceId: string;
  environment: string;
  revision: number | null;
  needs: NeonNeeds;
  /** The branch the environment last ran on, offered first. */
  remembered: string | undefined;
  onStart: (choice: BranchChoice) => void;
  onClose: () => void;
}) {
  const shell = useShellHost();
  const branches = useQuery({
    queryKey: ["neon", workspaceId],
    queryFn: () => shell.neon.branches(workspaceId),
    staleTime: 60_000,
  });
  const [picked, setPicked] = useState<string | null>(null);
  const [ttl, setTtl] = useState<number>(168);
  const names = branches.data?.branches.map((b) => b.name) ?? [];
  const choice = picked ?? (remembered && names.includes(remembered) ? remembered : NEW);
  const parent =
    branches.data?.parent ?? branches.data?.branches.find((b) => b.default)?.name ?? "the default";

  const submit = (e: FormEvent) => {
    e.preventDefault();
    onStart(choice === NEW ? { kind: "new", ttlHours: ttl } : { kind: "existing", branch: choice });
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="neon-start">
        <form onSubmit={submit} className="space-y-4">
          <DialogHeader>
            <DialogTitle>
              Start {environment}
              {revision ? ` #${revision}` : ""} on Neon
            </DialogTitle>
            <DialogDescription>
              It runs {describe(needs)} on a branch of{" "}
              <span className="font-mono">{branches.data?.projectId ?? "the Neon project"}</span>,
              and everything else here. The instance stays on the branch until it is deleted.
            </DialogDescription>
          </DialogHeader>
          {branches.isError && <Failure error={branches.error} />}
          {branches.isPending && <p className="text-sm text-muted-foreground">Loading branches…</p>}
          <fieldset className="space-y-2 text-sm" aria-label="Branch">
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="branch"
                checked={choice === NEW}
                onChange={() => setPicked(NEW)}
                data-testid="neon-start-new"
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
                  data-testid={`neon-start-${b.name}`}
                />
                <span className="font-mono">{b.name}</span>
                {b.default && <span className="text-xs text-muted-foreground">default</span>}
                {b.owned && <span className="text-xs text-muted-foreground">made by trestle</span>}
                {b.name === remembered && (
                  <span className="text-xs text-muted-foreground">last used</span>
                )}
              </label>
            ))}
          </fieldset>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" data-testid="neon-start-go">
              Start
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
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
