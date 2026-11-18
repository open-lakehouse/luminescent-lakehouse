// The plans across every backend, each with its backend, revision and drift.

import { Badge, cn } from "@open-lakehouse/ui-kit";
import { shortHash } from "./model.js";
import { useGraphs } from "./queries.js";

export interface GraphListProps {
  selected?: string;
  onSelect: (root: string) => void;
}

export function GraphList({ selected, onSelect }: GraphListProps) {
  const graphs = useGraphs();
  if (graphs.isLoading) return <p className="p-3 text-sm text-muted-foreground">Loading plans…</p>;
  if (graphs.error) {
    return (
      <p role="alert" className="p-3 text-sm text-destructive">
        {graphs.error.message}
      </p>
    );
  }
  const list = graphs.data ?? [];
  if (!list.length) return <p className="p-3 text-sm text-muted-foreground">No plans yet.</p>;
  return (
    <ul className="flex flex-col" aria-label="Plans">
      {list.map((g) => {
        const ref = g.root?.ref ?? "";
        return (
          <li key={ref}>
            <button
              type="button"
              onClick={() => onSelect(ref)}
              aria-current={ref === selected}
              className={cn(
                "flex w-full flex-col gap-1 border-b px-3 py-2 text-left text-sm hover:bg-accent/60",
                ref === selected && "bg-accent",
              )}
            >
              <span className="font-medium">{g.root?.spec?.title}</span>
              <span className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                <Badge variant="outline" className="px-1 py-0 text-[10px]">
                  {g.root?.metadata?.backend}
                </Badge>
                {g.nodeCount} nodes
                <code title={g.revision}>{shortHash(g.revision)}</code>
                {g.approvedRevision ? (
                  g.driftedCount ? (
                    <span className="text-amber-600 dark:text-amber-400">
                      {g.driftedCount} drifted
                    </span>
                  ) : (
                    <span className="text-emerald-600 dark:text-emerald-400">approved</span>
                  )
                ) : (
                  <span>not approved</span>
                )}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
