// The control room: plans on the left with their revision chain, the plan as
// a graph or a diff in the middle, and the selected node's inspector (or its
// diff) on the right. It is the one component a host mounts; it reaches the
// factory through the host's `factory` binding (or a provided client).

import type { DiffEntry, OmnigentSession } from "@open-lakehouse/factory-client";
import { useEnvironmentKey } from "@open-lakehouse/host/react";
import {
  ResizeHandle,
  Switch,
  Tabs,
  TabsList,
  TabsTrigger,
  usePaneSize,
} from "@open-lakehouse/ui-kit";
import type { ColorMode } from "@xyflow/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { PlanCanvas } from "./canvas/PlanCanvas.js";
import { NodeDiffPanel } from "./diff/NodeDiffPanel.js";
import { type DiffMode, PlanDiffView } from "./diff/PlanDiffView.js";
import { GraphList } from "./GraphList.js";
import { NodeInspector } from "./inspector/NodeInspector.js";
import { displayName, latestRuns } from "./model.js";
import {
  useDiff,
  useGraph,
  useGraphMutations,
  useGraphs,
  usePayloadRegistry,
  usePlaces,
  useRevisions,
  useRuns,
  useSchema,
  useWatchGraph,
} from "./queries.js";
import { RevisionTimeline } from "./RevisionTimeline.js";

export interface FactoryControlRoomProps {
  /** Start on this plan; defaults to the first. */
  initialRoot?: string;
  readOnly?: boolean;
  colorMode?: ColorMode;
  /** Opens a run's Omnigent session in the host (the desktop's Agent view). */
  onOpenSession?: (session: OmnigentSession) => void;
}

export function FactoryControlRoom(props: FactoryControlRoomProps) {
  // A new environment is a different factory: start over.
  return <ControlRoom key={useEnvironmentKey()} {...props} />;
}

type View = "plan" | "diff";

function ControlRoom({ initialRoot, readOnly, colorMode, onOpenSession }: FactoryControlRoomProps) {
  const graphs = useGraphs();
  const schema = useSchema();
  const registry = usePayloadRegistry();
  const [root, setRoot] = useState(initialRoot);
  useEffect(() => {
    if (!root && graphs.data?.length) setRoot(graphs.data[0].root?.ref);
  }, [root, graphs.data]);

  const graph = useGraph(root);
  const revisions = useRevisions(root);
  const runsQuery = useRuns(root);
  const runs = useMemo(() => latestRuns(runsQuery.data), [runsQuery.data]);
  const watch = useWatchGraph(root);
  // The line is across graphs, so only fetched while a run here waits.
  const anyWaiting = useMemo(() => [...runs.values()].some((r) => r.waiting), [runs]);
  const places = usePlaces(anyWaiting).data;
  const m = useGraphMutations(root);

  const [view, setView] = useState<View>("plan");
  const [diffMode, setDiffMode] = useState<DiffMode>("tree");
  const [selected, setSelected] = useState<string>();
  const [overlay, setOverlay] = useState(true);
  const [network, setNetwork] = useState(true);
  const [entries, setEntries] = useState<DiffEntry[]>([]);

  // The diff's ends: by default the approved revision (else the previous
  // one) against the current one. A pick sticks until the plan changes.
  const [pickedBase, setPickedBase] = useState<string>();
  const [pickedTarget, setPickedTarget] = useState<string>();
  const current = graph.data?.revision ?? "";
  const approved = graph.data?.approvedRevision ?? "";
  const previous = revisions.data?.[1]?.rootHash ?? "";
  const base = pickedBase ?? (approved && approved !== current ? approved : previous);
  const target = pickedTarget ?? current;
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset on a new plan.
  useEffect(() => {
    setPickedBase(undefined);
    setPickedTarget(undefined);
    setSelected(undefined);
  }, [root]);

  const changes = useMemo(() => new Map(entries.map((e) => [e.key, e])), [entries]);
  const node = graph.data?.graph?.nodes.find((n) => n.ref === selected);

  const onAddChild = useCallback(
    async (parent: string, kind: string) => {
      const res = await m.createNode.mutateAsync({
        parent,
        kind,
        spec: { title: `New ${displayName(schema.data, kind).toLowerCase()}` },
      });
      const ref = (res as { node?: { ref: string } }).node?.ref;
      if (ref) setSelected(ref);
    },
    [m.createNode, schema.data],
  );

  const [left, resizeLeft] = usePaneSize(280, { min: 200, max: 480 });
  const [right, resizeRight] = usePaneSize(380, { min: 280, max: 640, invert: true });

  return (
    <div
      className="flex h-full min-h-0 w-full bg-background text-foreground"
      data-testid="factory-control-room"
    >
      <aside className="flex min-h-0 flex-col" style={{ width: left }}>
        <div className="max-h-[40%] min-h-0 overflow-auto border-b">
          <GraphList selected={root} onSelect={setRoot} />
        </div>
        <div className="min-h-0 flex-1">
          {root ? (
            <RevisionTimeline
              root={root}
              base={base}
              target={target}
              onBaseChange={(h) => {
                setPickedBase(h);
                setView("diff");
              }}
              onTargetChange={(h) => {
                setPickedTarget(h);
                setView("diff");
              }}
              readOnly={readOnly}
            />
          ) : null}
        </div>
      </aside>
      <ResizeHandle orientation="vertical" size={left} onResize={resizeLeft} />

      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-3 border-b px-3 py-2">
          <h2 className="truncate text-sm font-semibold">
            {graph.data?.graph?.nodes.find((n) => n.ref === root)?.spec?.title ?? "Factory"}
          </h2>
          <Tabs value={view} onValueChange={(v) => setView(v as View)}>
            <TabsList>
              <TabsTrigger value="plan">Plan</TabsTrigger>
              <TabsTrigger value="diff">Diff</TabsTrigger>
            </TabsList>
          </Tabs>
          <div className="ml-auto flex items-center gap-3 text-xs">
            {watch === "reconnecting" ? (
              <span role="status" className="text-muted-foreground">
                Reconnecting…
              </span>
            ) : null}
            {view === "plan" ? (
              <>
                <span className="flex items-center gap-1.5">
                  <Switch
                    checked={overlay}
                    onCheckedChange={setOverlay}
                    aria-label="Show changes"
                  />
                  Changes
                </span>
                <span className="flex items-center gap-1.5">
                  <Switch
                    checked={network}
                    onCheckedChange={setNetwork}
                    aria-label="Show cross-references"
                  />
                  Cross-references
                </span>
              </>
            ) : null}
          </div>
        </div>
        <div className="min-h-0 flex-1">
          {!root ? null : view === "plan" ? (
            <PlanCanvas
              graph={graph.data?.graph}
              schema={schema.data}
              registry={registry}
              selected={selected}
              onSelect={setSelected}
              changes={overlay ? changes : undefined}
              runs={runs}
              places={places}
              showNetwork={network}
              readOnly={readOnly}
              colorMode={colorMode}
              onAddChild={onAddChild}
              onMove={(ref, newParent) => m.moveNode.mutate({ ref, newParent })}
              onAddEdge={(edge) => m.addEdge.mutate(edge)}
              onRemoveEdge={(edge) => m.removeEdge.mutate(edge)}
            />
          ) : (
            <PlanDiffView
              root={root}
              base={base}
              target={target}
              selected={selected}
              onSelect={setSelected}
              mode={diffMode}
              onModeChange={setDiffMode}
              onEntries={setEntries}
            />
          )}
          {/* Keeps the overlay's entries loaded while the canvas shows. */}
          {root && view === "plan" && overlay ? (
            <DiffEntriesLoader root={root} base={base} target={target} onEntries={setEntries} />
          ) : null}
        </div>
        {[m.createNode, m.moveNode, m.addEdge, m.removeEdge, m.setPhase, m.approve].map((x) =>
          x.error ? (
            <p
              key={String(x.error)}
              role="alert"
              className="border-t bg-destructive/10 px-3 py-1.5 text-xs text-destructive"
            >
              {x.error.message}
            </p>
          ) : null,
        )}
      </main>

      <ResizeHandle orientation="vertical" size={right} onResize={resizeRight} />
      <aside className="min-h-0 overflow-hidden" style={{ width: right }}>
        {root && node && view === "plan" ? (
          <NodeInspector
            key={node.ref}
            root={root}
            node={node}
            run={runs.get(node.ref)}
            places={places}
            schema={schema.data}
            registry={registry}
            readOnly={readOnly}
            onDeleted={() => setSelected(undefined)}
            onOpenSession={onOpenSession}
          />
        ) : root && selected && view === "diff" ? (
          <NodeDiffPanel root={root} base={base} target={target} nodeKey={selected} />
        ) : (
          <p className="p-3 text-sm text-muted-foreground">
            {view === "plan" ? "Select a node to edit it." : "Select a change to see it in detail."}
          </p>
        )}
      </aside>
    </div>
  );
}

function DiffEntriesLoader(props: {
  root: string;
  base: string;
  target: string;
  onEntries: (e: DiffEntry[]) => void;
}) {
  const { data } = useDiff(props.root, props.base, props.target, false);
  const { onEntries } = props;
  useEffect(() => onEntries(data ?? []), [data, onEntries]);
  return null;
}
