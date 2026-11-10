// headwaters' lineage UI in the shell: its browsers, search and graph, composed
// the way headwaters' own app composes them, on local state instead of routes.
// RPCs go through the host's `headwaters` binding (src/integrations/bind.ts).

import {
  DatasetBrowser,
  DatasetDetailPanel,
  HomeView,
  JobBrowser,
  JobDetailPanel,
  LineageView as LineageGraph,
  type LineageNode,
  ReadClientProvider,
  SearchView,
} from "@open-lakehouse/lineage-ui";
import { cn } from "@open-lakehouse/ui-kit";
import { Cog, Database, GitBranch, Home, Search } from "lucide-react";
import { type ComponentType, useState } from "react";

type Page = "home" | "datasets" | "jobs" | "lineage" | "search";

const PAGES: { id: Page; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "home", label: "Home", icon: Home },
  { id: "datasets", label: "Datasets", icon: Database },
  { id: "jobs", label: "Jobs", icon: Cog },
  { id: "lineage", label: "Lineage", icon: GitBranch },
  { id: "search", label: "Search", icon: Search },
];

interface Selected {
  kind: "dataset" | "job";
  namespace: string;
  name: string;
}

/** `dataset:<ns>:<name>` / `job:<ns>:<name>` of a clicked node, from its data. */
function parseSelection(node: LineageNode): Selected | null {
  const kind = node.id.split(":", 1)[0];
  if (kind !== "dataset" && kind !== "job") return null;
  const id = (node.data as { id?: { namespace?: string; name?: string } } | undefined)?.id;
  return id?.namespace && id.name ? { kind, namespace: id.namespace, name: id.name } : null;
}

/** The lineage browser for the current environment's `headwaters`. */
export function LineageView() {
  const [page, setPage] = useState<Page>("home");
  const [nodeId, setNodeId] = useState<string>();
  const [query, setQuery] = useState("");
  const showLineage = (id: string) => {
    setNodeId(id);
    setPage("lineage");
  };

  return (
    <ReadClientProvider>
      <div className="flex h-full" data-testid="lineage-view">
        <nav
          aria-label="Lineage pages"
          className="flex w-48 shrink-0 flex-col gap-0.5 border-r bg-sidebar p-2 text-sidebar-foreground"
        >
          {PAGES.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              aria-current={page === id ? "page" : undefined}
              data-testid={`lineage-page-${id}`}
              onClick={() => setPage(id)}
              className={cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                page === id && "bg-accent text-accent-foreground",
              )}
            >
              <Icon className="size-4" />
              {label}
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 overflow-hidden">
          {page === "home" && (
            <div className="h-full overflow-auto">
              <HomeView />
            </div>
          )}
          {page === "datasets" && (
            <DatasetBrowser
              onSelect={({ namespace, name }) => showLineage(`dataset:${namespace}:${name}`)}
            />
          )}
          {page === "jobs" && (
            <JobBrowser
              onSelect={({ namespace, name }) => showLineage(`job:${namespace}:${name}`)}
            />
          )}
          {page === "search" && (
            <SearchView
              query={query}
              onQueryChange={setQuery}
              onSelect={({ nodeId }) => showLineage(nodeId)}
            />
          )}
          {page === "lineage" && <LineagePage nodeId={nodeId} onViewLineage={showLineage} />}
        </div>
      </div>
    </ReadClientProvider>
  );
}

function LineagePage({
  nodeId,
  onViewLineage,
}: {
  nodeId: string | undefined;
  onViewLineage: (id: string) => void;
}) {
  const [selected, setSelected] = useState<Selected | null>(null);
  if (!nodeId) {
    return (
      <div className="p-6 text-sm text-muted-foreground">
        Select a dataset or job to view its lineage.
      </div>
    );
  }
  return (
    <div className="flex h-full">
      <div className="min-w-0 flex-1">
        <LineageGraph
          nodeId={nodeId}
          selectedId={
            selected ? `${selected.kind}:${selected.namespace}:${selected.name}` : undefined
          }
          onSelect={(node) => setSelected(parseSelection(node))}
        />
      </div>
      {selected && (
        <aside className="w-96 shrink-0 overflow-auto border-l">
          {selected.kind === "dataset" ? (
            <DatasetDetailPanel
              namespace={selected.namespace}
              name={selected.name}
              onViewLineage={onViewLineage}
              onClose={() => setSelected(null)}
            />
          ) : (
            <JobDetailPanel
              namespace={selected.namespace}
              name={selected.name}
              onViewLineage={onViewLineage}
              onClose={() => setSelected(null)}
            />
          )}
        </aside>
      )}
    </div>
  );
}
