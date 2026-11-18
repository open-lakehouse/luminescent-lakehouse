// The plan's revision chain, newest first: who changed it, through which
// path, what changed, and which revision is approved. Pick a base (A) and a
// target (B) to diff; approve the current revision.

import { Badge, Button, cn } from "@open-lakehouse/ui-kit";
import { CheckCircle2 } from "lucide-react";
import { shortHash } from "./model.js";
import { useGraphMutations, useRevisions } from "./queries.js";

export interface RevisionTimelineProps {
  root: string;
  base: string;
  target: string;
  onBaseChange: (hash: string) => void;
  onTargetChange: (hash: string) => void;
  readOnly?: boolean;
}

function when(seconds: bigint | undefined): string {
  if (seconds === undefined) return "";
  return new Date(Number(seconds) * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function RevisionTimeline({
  root,
  base,
  target,
  onBaseChange,
  onTargetChange,
  readOnly,
}: RevisionTimelineProps) {
  const revisions = useRevisions(root);
  const m = useGraphMutations(root);
  const list = revisions.data ?? [];
  const current = list[0];

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="revision-timeline">
      <div className="flex items-center gap-2 border-b px-3 py-2">
        <h3 className="text-sm font-medium">Revisions</h3>
        <span className="text-xs text-muted-foreground">{list.length}</span>
        {!readOnly && current && !current.approved ? (
          <Button
            size="sm"
            className="ml-auto"
            disabled={m.approve.isPending}
            onClick={() => m.approve.mutate(current.rootHash)}
          >
            Approve current
          </Button>
        ) : null}
      </div>
      <ol className="min-h-0 flex-1 overflow-auto">
        {list.map((r) => {
          const isBase = r.rootHash === base;
          const isTarget = r.rootHash === target;
          return (
            <li
              key={`${r.sequence}`}
              className={cn(
                "flex items-start gap-2 border-b px-3 py-2 text-xs",
                (isBase || isTarget) && "bg-accent/50",
              )}
              data-testid={`revision-${r.sequence}`}
            >
              <div className="flex flex-col gap-1 pt-0.5">
                <button
                  type="button"
                  aria-label={`Use revision ${r.sequence} as the base`}
                  aria-pressed={isBase}
                  onClick={() => onBaseChange(r.rootHash)}
                  className={cn(
                    "size-5 rounded border text-[10px] font-semibold",
                    isBase && "border-rose-500 bg-rose-500/15",
                  )}
                >
                  A
                </button>
                <button
                  type="button"
                  aria-label={`Use revision ${r.sequence} as the target`}
                  aria-pressed={isTarget}
                  onClick={() => onTargetChange(r.rootHash)}
                  className={cn(
                    "size-5 rounded border text-[10px] font-semibold",
                    isTarget && "border-emerald-500 bg-emerald-500/15",
                  )}
                >
                  B
                </button>
              </div>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <div className="flex items-center gap-1.5">
                  <span className="font-medium">#{String(r.sequence)}</span>
                  <code className="text-muted-foreground" title={r.rootHash}>
                    {shortHash(r.rootHash)}
                  </code>
                  {r.approved ? (
                    <Badge variant="success" className="gap-1 px-1.5 py-0 text-[10px]">
                      <CheckCircle2 className="size-3" /> approved
                    </Badge>
                  ) : null}
                  <span className="ml-auto text-muted-foreground">
                    {when(r.createdAt?.seconds)}
                  </span>
                </div>
                <div className="text-muted-foreground">
                  {r.provenance?.actor} via {r.provenance?.via}
                  {r.provenance?.runRef ? <span title={r.provenance.runRef}> (run)</span> : null}
                </div>
                <div className="flex flex-wrap gap-1">
                  {Object.entries(r.changes).map(([k, n]) => (
                    <span key={k} className="rounded bg-muted px-1.5 py-px">
                      {n} {k}
                    </span>
                  ))}
                </div>
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
